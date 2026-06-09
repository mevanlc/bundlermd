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

/// One workarea row as the frontend's table displays it.
#[derive(Serialize)]
pub struct FileRow {
    pub path: String,
    pub name: String,
    pub folder: String,
    /// `None` when the file is currently missing/unreadable.
    pub size: Option<u64>,
}

#[derive(Serialize)]
pub struct AddResult {
    pub files: Vec<FileRow>,
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

fn rows(paths: &[PathBuf]) -> Vec<FileRow> {
    paths
        .iter()
        .map(|p| FileRow {
            path: p.display().to_string(),
            name: basename(p),
            folder: p
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
            size: std::fs::metadata(p).ok().map(|m| m.len()),
        })
        .collect()
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Screen `paths` for text-ness and append the survivors, collecting skip
/// reasons; paths already present are a no-op.
fn screen_and_add(files: &mut Vec<PathBuf>, paths: Vec<PathBuf>, skipped: &mut Vec<Skipped>) {
    for path in paths {
        if files.contains(&path) {
            continue;
        }
        match reading::read_file(&path) {
            Ok(FileContent::Text(_)) => files.push(path),
            Ok(FileContent::Binary) => skipped.push(Skipped {
                path: path.display().to_string(),
                reason: "binary file (contains NUL bytes)".into(),
            }),
            Err(e) => skipped.push(Skipped {
                path: path.display().to_string(),
                reason: e.to_string(),
            }),
        }
    }
}

/// Add files with text/binary screening. Binary and unreadable files are
/// skipped (reported batched).
#[tauri::command]
pub fn add_files(state: State<'_, Workarea>, paths: Vec<String>) -> AddResult {
    let mut files = state.0.lock().unwrap();
    let mut skipped = Vec::new();
    screen_and_add(
        &mut files,
        paths.into_iter().map(PathBuf::from).collect(),
        &mut skipped,
    );
    AddResult {
        files: rows(&files),
        skipped,
    }
}

/// Add a folder's immediate-children regular files (sorted by name) with the
/// same screening as `add_files`. Recursive import with a preview dialog is
/// Phase 3.
#[tauri::command]
pub fn add_folder(state: State<'_, Workarea>, path: String) -> Result<AddResult, String> {
    let entries = std::fs::read_dir(&path).map_err(|e| format!("could not read folder: {e}"))?;
    let mut children: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    children.sort();

    let mut files = state.0.lock().unwrap();
    let mut skipped = Vec::new();
    screen_and_add(&mut files, children, &mut skipped);
    Ok(AddResult {
        files: rows(&files),
        skipped,
    })
}

#[tauri::command]
pub fn remove_file(state: State<'_, Workarea>, path: String) -> Vec<FileRow> {
    let mut files = state.0.lock().unwrap();
    files.retain(|p| *p != Path::new(&path));
    rows(&files)
}

#[tauri::command]
pub fn move_file(state: State<'_, Workarea>, path: String, op: MoveOp) -> Vec<FileRow> {
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
    rows(&files)
}

/// Replace the ordering wholesale (drag-reorder). Rejected unless `paths` is
/// a permutation of the current list.
#[tauri::command]
pub fn set_order(state: State<'_, Workarea>, paths: Vec<String>) -> Result<Vec<FileRow>, String> {
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
    Ok(rows(&files))
}

#[tauri::command]
pub fn get_files(state: State<'_, Workarea>) -> Vec<FileRow> {
    rows(&state.0.lock().unwrap())
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
