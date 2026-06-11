//! Project state (Rust-owned source of truth) and the Tauri commands the
//! frontend drives it with. Mutating commands return the full ProjectView so
//! the frontend re-renders from scratch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::bundle::{self, BundleFile};
use crate::project::{effective_title, project_dir, ProjectFile, ProjectSettings};
use crate::reading::{self, FileContent};
use crate::smartpath::presented_paths;
use crate::store::GlobalStore;

/// Size limits enforced at add and export time. Defaults per the PRD;
/// actual values come from App Settings (`GlobalStore`).
#[derive(Clone, Copy)]
pub struct Limits {
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_bytes: 200_000_000,
            max_total_bytes: 250_000_000,
        }
    }
}

#[derive(Default)]
struct Project {
    files: Vec<PathBuf>,
    settings: ProjectSettings,
    last_export: Option<PathBuf>,
    project_path: Option<PathBuf>,
    dirty: bool,
}

/// One project per window, keyed by window label. A label with no entry is
/// an empty Untitled project (created on first touch).
#[derive(Default)]
pub struct Workareas(Mutex<HashMap<String, Project>>);

impl Workareas {
    pub fn is_dirty(&self, label: &str) -> bool {
        self.0.lock().unwrap().get(label).is_some_and(|p| p.dirty)
    }

    /// Drop a destroyed window's project.
    pub fn remove(&self, label: &str) {
        self.0.lock().unwrap().remove(label);
    }

    fn with<R>(&self, label: &str, f: impl FnOnce(&mut Project) -> R) -> R {
        let mut map = self.0.lock().unwrap();
        f(map.entry(label.to_string()).or_default())
    }
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

/// Everything the frontend needs to render the workarea.
#[derive(Serialize)]
pub struct ProjectView {
    pub files: Vec<FileRow>,
    pub settings: ProjectSettings,
    pub project_path: Option<String>,
    pub dirty: bool,
}

#[derive(Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct AddResult {
    pub project: ProjectView,
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

/// Round-figure rendering for limit values in user-facing messages
/// (limits are set in round decimal bytes, e.g. 200,000,000 → "200 MB").
fn human_bytes(bytes: u64) -> String {
    const UNITS: [(&str, u64); 3] = [("GB", 1_000_000_000), ("MB", 1_000_000), ("KB", 1_000)];
    for (unit, factor) in UNITS {
        if bytes >= factor {
            return format!("{} {unit}", bytes / factor);
        }
    }
    format!("{bytes} B")
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl Project {
    fn view(&self) -> ProjectView {
        ProjectView {
            files: self
                .files
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
                .collect(),
            settings: self.settings.clone(),
            project_path: self.project_path.as_ref().map(|p| p.display().to_string()),
            dirty: self.dirty,
        }
    }

    /// Screen `paths` for text-ness and size limits, append the survivors,
    /// and collect skip reasons; paths already present are a no-op.
    fn screen_and_add(&mut self, paths: Vec<PathBuf>, limits: Limits, skipped: &mut Vec<Skipped>) {
        // Running total starts from the files already in the workarea.
        let mut total: u64 = self
            .files
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
            .sum();
        let mut added_any = false;
        for path in paths {
            if self.files.contains(&path) {
                continue;
            }
            // Check size before reading so an oversized file is never decoded.
            let size = std::fs::metadata(&path).ok().map(|m| m.len());
            if let Some(size) = size {
                if size > limits.max_file_bytes {
                    skipped.push(Skipped {
                        path: path.display().to_string(),
                        reason: format!(
                            "exceeds the maximum file size ({})",
                            human_bytes(limits.max_file_bytes)
                        ),
                    });
                    continue;
                }
                if total + size > limits.max_total_bytes {
                    skipped.push(Skipped {
                        path: path.display().to_string(),
                        reason: format!(
                            "would push the bundle over the maximum total size ({})",
                            human_bytes(limits.max_total_bytes)
                        ),
                    });
                    continue;
                }
            }
            match reading::read_file(&path) {
                Ok(FileContent::Text(_)) => {
                    total += size.unwrap_or(0);
                    self.files.push(path);
                    added_any = true;
                }
                Ok(FileContent::Binary) => skipped.push(Skipped {
                    path: path.display().to_string(),
                    reason: "binary file".into(),
                }),
                Err(e) => skipped.push(Skipped {
                    path: path.display().to_string(),
                    reason: e.to_string(),
                }),
            }
        }
        if added_any {
            self.dirty = true;
        }
    }
}

/// Add files with text/binary screening. Binary and unreadable files are
/// skipped (reported batched).
#[tauri::command]
pub fn add_files(
    window: tauri::Window,
    state: State<'_, Workareas>,
    store: State<'_, GlobalStore>,
    paths: Vec<String>,
) -> AddResult {
    let limits = store.settings().limits();
    state.with(window.label(), |project| {
        let mut skipped = Vec::new();
        project.screen_and_add(
            paths.iter().map(PathBuf::from).collect(),
            limits,
            &mut skipped,
        );
        AddResult {
            project: project.view(),
            skipped,
        }
    })
}

/// List the regular files under a folder (sorted by path) for the add-folder
/// preview dialog. No screening here — the confirmed list goes through
/// `add_files`, which screens and batches the warnings.
#[tauri::command]
pub fn preview_folder(path: String, recursive: bool) -> Result<Vec<String>, String> {
    let root = PathBuf::from(&path);
    // Error only if the chosen folder itself is unreadable; unreadable
    // subfolders are skipped (their files will simply be absent).
    std::fs::read_dir(&root).map_err(|e| format!("could not read folder: {e}"))?;

    let mut files = Vec::new();
    let mut dirs = vec![root];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // file_type() doesn't follow symlinks, so symlinked dirs can't
            // cause cycles; symlinks to files still count as files.
            if file_type.is_dir() {
                if recursive {
                    dirs.push(path);
                }
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files.iter().map(|p| p.display().to_string()).collect())
}

#[tauri::command]
pub fn remove_file(
    window: tauri::Window,
    state: State<'_, Workareas>,
    path: String,
) -> ProjectView {
    state.with(window.label(), |project| {
        let before = project.files.len();
        project.files.retain(|p| *p != Path::new(&path));
        if project.files.len() != before {
            project.dirty = true;
        }
        project.view()
    })
}

#[tauri::command]
pub fn move_file(
    window: tauri::Window,
    state: State<'_, Workareas>,
    path: String,
    op: MoveOp,
) -> ProjectView {
    state.with(window.label(), |project| {
        if let Some(i) = project.files.iter().position(|p| *p == Path::new(&path)) {
            let last = project.files.len() - 1;
            let moved = match op {
                MoveOp::Up if i > 0 => {
                    project.files.swap(i, i - 1);
                    true
                }
                MoveOp::Down if i < last => {
                    project.files.swap(i, i + 1);
                    true
                }
                MoveOp::Top if i > 0 => {
                    project.files[..=i].rotate_right(1);
                    true
                }
                MoveOp::Bottom if i < last => {
                    project.files[i..].rotate_left(1);
                    true
                }
                _ => false,
            };
            if moved {
                project.dirty = true;
            }
        }
        project.view()
    })
}

/// Replace the ordering wholesale (drag-reorder). Rejected unless `paths` is
/// a permutation of the current list.
#[tauri::command]
pub fn set_order(
    window: tauri::Window,
    state: State<'_, Workareas>,
    paths: Vec<String>,
) -> Result<ProjectView, String> {
    state.with(window.label(), |project| {
        let new: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let mut sorted_old = project.files.clone();
        let mut sorted_new = new.clone();
        sorted_old.sort();
        sorted_new.sort();
        if sorted_old != sorted_new {
            return Err("reorder does not match current file set".into());
        }
        if project.files != new {
            project.files = new;
            project.dirty = true;
        }
        Ok(project.view())
    })
}

#[tauri::command]
pub fn get_project(window: tauri::Window, state: State<'_, Workareas>) -> ProjectView {
    state.with(window.label(), |project| project.view())
}

#[tauri::command]
pub fn update_settings(
    window: tauri::Window,
    state: State<'_, Workareas>,
    settings: ProjectSettings,
) -> ProjectView {
    state.with(window.label(), |project| {
        if project.settings != settings {
            project.settings = settings;
            project.dirty = true;
        }
        project.view()
    })
}

#[tauri::command]
pub fn new_project(window: tauri::Window, state: State<'_, Workareas>) -> ProjectView {
    state.with(window.label(), |project| {
        *project = Project::default();
        project.view()
    })
}

static WINDOW_SEQ: AtomicUsize = AtomicUsize::new(1);

/// Open a fresh window with an empty Untitled project.
#[tauri::command]
pub fn new_window(app: tauri::AppHandle, store: State<'_, GlobalStore>) -> Result<(), String> {
    let label = format!("main-{}", WINDOW_SEQ.fetch_add(1, Ordering::Relaxed));
    let window = tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::default())
        .title("BundlerMD")
        .inner_size(800.0, 600.0)
        // Same rationale as dragDropEnabled: false in tauri.conf.json — the
        // OS drop handler fights HTML5 row drag-reorder.
        .disable_drag_drop_handler()
        .build()
        .map_err(|e| format!("could not open window: {e}"))?;
    let _ = window.set_theme(store.settings().theme.native());
    Ok(())
}

/// Save to the current .bmd, or to `path` (Save / Save As).
#[tauri::command]
pub fn save_project(
    window: tauri::Window,
    app: tauri::AppHandle,
    state: State<'_, Workareas>,
    path: Option<String>,
) -> Result<ProjectView, String> {
    let view = state.with(window.label(), |project| -> Result<ProjectView, String> {
        let target = path
            .map(PathBuf::from)
            .or_else(|| project.project_path.clone())
            .ok_or("no project file path; use Save As")?;
        let file = ProjectFile::new(
            project.files.iter().map(|p| p.display().to_string()).collect(),
            project.last_export.as_ref().map(|p| p.display().to_string()),
            project.settings.clone(),
        );
        file.save(&target)?;
        project.project_path = Some(target);
        project.dirty = false;
        Ok(project.view())
    })?;
    if let Some(saved) = &view.project_path {
        crate::store::note_recent(&app, saved);
    }
    Ok(view)
}

/// Open a project into this window. One window per project: if another
/// window already has it open, that window is focused instead and `None`
/// is returned.
#[tauri::command]
pub fn open_project(
    window: tauri::Window,
    app: tauri::AppHandle,
    state: State<'_, Workareas>,
    path: String,
) -> Result<Option<ProjectView>, String> {
    let project_path = PathBuf::from(&path);
    let elsewhere = {
        let map = state.0.lock().unwrap();
        map.iter()
            .find(|(label, p)| {
                label.as_str() != window.label()
                    && p.project_path.as_deref() == Some(project_path.as_path())
            })
            .map(|(label, _)| label.clone())
    };
    if let Some(label) = elsewhere {
        if let Some(other) = app.get_webview_window(&label) {
            let _ = other.set_focus();
        }
        return Ok(None);
    }

    let pf = ProjectFile::load(&project_path)?;
    let view = state.with(window.label(), |project| {
        *project = Project {
            files: pf.files.iter().map(PathBuf::from).collect(),
            settings: pf.settings,
            last_export: pf.last_export.map(PathBuf::from),
            project_path: Some(project_path),
            dirty: false,
        };
        project.view()
    });
    crate::store::note_recent(&app, &path);
    Ok(Some(view))
}

/// Generate the bundle in memory, best-effort. If problems occurred and
/// `allow_problems` is false, nothing is written and the problems are
/// returned for the Save-anyway/Cancel dialog; a second call with
/// `allow_problems: true` writes the export minus the problem files.
#[tauri::command]
pub fn export_bundle(
    window: tauri::Window,
    state: State<'_, Workareas>,
    store: State<'_, GlobalStore>,
    output_path: String,
    allow_problems: bool,
) -> Result<ExportResult, String> {
    let output = PathBuf::from(&output_path);
    let limits = store.settings().limits();
    let (markdown, problems) = state.with(window.label(), |project| {
        generate_bundle(
            &project.files,
            &project.settings,
            project.project_path.as_deref(),
            &output,
            limits,
        )
    });

    if !problems.is_empty() && !allow_problems {
        return Ok(ExportResult {
            written: false,
            problems,
        });
    }

    std::fs::write(&output, markdown).map_err(|e| format!("could not write export: {e}"))?;

    state.with(window.label(), |project| {
        if project.last_export.as_deref() != Some(output.as_path()) {
            project.last_export = Some(output);
            project.dirty = true;
        }
    });
    Ok(ExportResult {
        written: true,
        problems,
    })
}

/// Best-effort bundle generation: problem files are reported and omitted.
/// Every check is re-done here regardless of what add-time screening saw —
/// files can be deleted, grow, lose permissions, or turn binary in between.
pub fn generate_bundle(
    files: &[PathBuf],
    settings: &ProjectSettings,
    project_path: Option<&Path>,
    output: &Path,
    limits: Limits,
) -> (String, Vec<Problem>) {
    let dir = project_dir(project_path);
    let displays = presented_paths(files, &settings.path_presentation, dir.as_deref());

    let mut bundle_files = Vec::new();
    let mut problems = Vec::new();
    let mut total: u64 = 0;
    for (path, display) in files.iter().zip(displays) {
        // Size gate first so an oversized file is never read into memory.
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if size > limits.max_file_bytes {
            problems.push(Problem {
                path: path.display().to_string(),
                reason: format!(
                    "exceeds the maximum file size ({})",
                    human_bytes(limits.max_file_bytes)
                ),
            });
            continue;
        }
        if total + size > limits.max_total_bytes {
            problems.push(Problem {
                path: path.display().to_string(),
                reason: format!(
                    "would push the bundle over the maximum total size ({})",
                    human_bytes(limits.max_total_bytes)
                ),
            });
            continue;
        }
        match reading::read_file(path) {
            Ok(FileContent::Text(content)) => {
                // Only included files count toward the total.
                total += size;
                bundle_files.push(BundleFile { display, content });
            }
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

    let title = effective_title(settings, project_path, output);
    let markdown = bundle::assemble(
        &title,
        &settings.introduction,
        &bundle_files,
        settings.newlines.resolve(),
        settings.toc_links,
    );
    (markdown, problems)
}
