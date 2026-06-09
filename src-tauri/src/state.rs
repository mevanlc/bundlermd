//! Workarea state (Rust-owned source of truth) and the Tauri commands the
//! frontend drives it with. Commands return the full updated file list so the
//! frontend can re-render from scratch.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::bundle::{self, BundleFile, Newline};
use crate::reading::{self, FileContent};

#[derive(Default)]
pub struct Workarea(Mutex<Vec<PathBuf>>);

#[derive(Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct AddResult {
    pub files: Vec<String>,
    pub skipped: Vec<Skipped>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MoveOp {
    Up,
    Down,
    Top,
    Bottom,
}

#[derive(Serialize)]
pub struct Problem {
    pub path: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct ExportResult {
    pub written: bool,
    pub problems: Vec<Problem>,
}

fn paths_as_strings(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Add files with text/binary screening. Binary and unreadable files are
/// skipped (reported batched); paths already present are a no-op.
#[tauri::command]
pub fn add_files(state: State<'_, Workarea>, paths: Vec<String>) -> AddResult {
    let mut files = state.0.lock().unwrap();
    let mut skipped = Vec::new();
    for raw in paths {
        let path = PathBuf::from(&raw);
        if files.contains(&path) {
            continue;
        }
        match reading::read_file(&path) {
            Ok(FileContent::Text(_)) => files.push(path),
            Ok(FileContent::Binary) => skipped.push(Skipped {
                path: raw,
                reason: "binary file (contains NUL bytes)".into(),
            }),
            Err(e) => skipped.push(Skipped {
                path: raw,
                reason: e.to_string(),
            }),
        }
    }
    AddResult {
        files: paths_as_strings(&files),
        skipped,
    }
}

#[tauri::command]
pub fn remove_file(state: State<'_, Workarea>, path: String) -> Vec<String> {
    let mut files = state.0.lock().unwrap();
    files.retain(|p| *p != Path::new(&path));
    paths_as_strings(&files)
}

#[tauri::command]
pub fn move_file(state: State<'_, Workarea>, path: String, op: MoveOp) -> Vec<String> {
    let mut files = state.0.lock().unwrap();
    if let Some(i) = files.iter().position(|p| *p == Path::new(&path)) {
        let last = files.len() - 1;
        match op {
            MoveOp::Up if i > 0 => files.swap(i, i - 1),
            MoveOp::Down if i < last => files.swap(i, i + 1),
            MoveOp::Top => files[..=i].rotate_right(1),
            MoveOp::Bottom => files[i..].rotate_left(1),
            _ => {}
        }
    }
    paths_as_strings(&files)
}

/// Replace the ordering wholesale (drag-reorder). Rejected unless `paths` is
/// a permutation of the current list.
#[tauri::command]
pub fn set_order(state: State<'_, Workarea>, paths: Vec<String>) -> Result<Vec<String>, String> {
    let mut files = state.0.lock().unwrap();
    let new: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let mut sorted_old = files.clone();
    let mut sorted_new = new.clone();
    sorted_old.sort();
    sorted_new.sort();
    if sorted_old != sorted_new {
        return Err("reorder does not match current file set".into());
    }
    *files = new;
    Ok(paths_as_strings(&files))
}

#[tauri::command]
pub fn get_files(state: State<'_, Workarea>) -> Vec<String> {
    paths_as_strings(&state.0.lock().unwrap())
}

/// Generate the bundle in memory, best-effort. If problems occurred and
/// `allow_problems` is false, nothing is written and the problems are
/// returned for the Save-anyway/Cancel dialog; a second call with
/// `allow_problems: true` writes the export minus the problem files.
#[tauri::command]
pub fn export_bundle(
    state: State<'_, Workarea>,
    output_path: String,
    allow_problems: bool,
) -> Result<ExportResult, String> {
    let files = state.0.lock().unwrap().clone();
    let output = PathBuf::from(&output_path);

    let (markdown, problems) = generate_bundle(&files, &output);
    if !problems.is_empty() && !allow_problems {
        return Ok(ExportResult {
            written: false,
            problems,
        });
    }

    std::fs::write(&output, markdown).map_err(|e| format!("could not write export: {e}"))?;
    Ok(ExportResult {
        written: true,
        problems,
    })
}

/// Best-effort bundle generation: problem files are reported and omitted.
/// Phase 1 has no project, so the title falls back to the output file's stem
/// and the introduction is empty.
pub fn generate_bundle(files: &[PathBuf], output: &Path) -> (String, Vec<Problem>) {
    let mut bundle_files = Vec::new();
    let mut problems = Vec::new();
    for path in files {
        match reading::read_file(path) {
            Ok(FileContent::Text(content)) => bundle_files.push(BundleFile {
                display: basename(path),
                content,
            }),
            Ok(FileContent::Binary) => problems.push(Problem {
                path: path.display().to_string(),
                reason: "binary file (contains NUL bytes)".into(),
            }),
            Err(e) => problems.push(Problem {
                path: path.display().to_string(),
                reason: e.to_string(),
            }),
        }
    }

    let title = output
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Bundle".into());
    let markdown = bundle::assemble(&title, "", &bundle_files, Newline::Unix);
    (markdown, problems)
}
