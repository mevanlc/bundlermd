//! Project state (Rust-owned source of truth) and the Tauri commands the
//! frontend drives it with. Mutating commands return the full ProjectView so
//! the frontend re-renders from scratch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

use crate::bundle::{self, BundleFile};
use crate::project::{
    effective_title, project_dir, resolve_stored, stored_path, PathPresentation, ProjectFile,
    ProjectSettings,
};
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

struct Project {
    files: Vec<PathBuf>,
    settings: ProjectSettings,
    last_export: Option<PathBuf>,
    project_path: Option<PathBuf>,
    dirty: bool,
}

impl Default for Project {
    fn default() -> Self {
        Self::new(ProjectSettings::default())
    }
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

    fn with_default<R>(
        &self,
        label: &str,
        settings: ProjectSettings,
        f: impl FnOnce(&mut Project) -> R,
    ) -> R {
        let mut map = self.0.lock().unwrap();
        f(map
            .entry(label.to_string())
            .or_insert_with(|| Project::new(settings)))
    }
}

/// OS-level "open with BundlerMD" requests can arrive before the frontend has
/// registered listeners. Keep them here until a window explicitly drains them.
#[derive(Default)]
pub struct PendingOpenProjects(Mutex<Vec<String>>);

impl PendingOpenProjects {
    fn push(&self, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        self.0.lock().unwrap().extend(paths);
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

#[derive(Clone, Serialize)]
pub struct FolderPreviewFile {
    pub path: String,
    pub importable: bool,
    pub note: String,
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

#[derive(Serialize)]
pub struct BundleTextResult {
    pub markdown: String,
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

fn screen_import(path: &Path, total: &mut u64, limits: Limits) -> Option<String> {
    // Check size before reading so an oversized file is never decoded.
    let size = std::fs::metadata(path).ok().map(|m| m.len());
    if let Some(size) = size {
        if size > limits.max_file_bytes {
            return Some(format!(
                "exceeds the maximum file size ({})",
                human_bytes(limits.max_file_bytes)
            ));
        }
        if *total + size > limits.max_total_bytes {
            return Some(format!(
                "would push the bundle over the maximum total size ({})",
                human_bytes(limits.max_total_bytes)
            ));
        }
    }
    match reading::read_file(path) {
        Ok(FileContent::Text(_)) => {
            *total += size.unwrap_or(0);
            None
        }
        Ok(FileContent::Binary) => Some("Binary File".into()),
        Err(e) => Some(e.to_string()),
    }
}

fn is_bmd_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bmd"))
}

fn normalize_open_project_path(path: PathBuf, cwd: Option<&Path>) -> Option<String> {
    let path = if path.is_absolute() {
        path
    } else {
        cwd?.join(path)
    };
    if !is_bmd_path(&path) {
        return None;
    }
    Some(path.canonicalize().unwrap_or(path).display().to_string())
}

fn open_project_path_from_arg(arg: &str, cwd: Option<&Path>) -> Option<String> {
    if let Ok(url) = tauri::Url::parse(arg) {
        if let Ok(path) = url.to_file_path() {
            return normalize_open_project_path(path, cwd);
        }
    }
    normalize_open_project_path(PathBuf::from(arg), cwd)
}

pub fn open_project_paths_from_args(args: &[String], cwd: &str) -> Vec<String> {
    let cwd = Path::new(cwd);
    args.iter()
        .filter_map(|arg| open_project_path_from_arg(arg, Some(cwd)))
        .collect()
}

pub fn open_project_paths_from_env_args() -> Vec<String> {
    let cwd = std::env::current_dir().ok();
    std::env::args()
        .skip(1)
        .filter_map(|arg| open_project_path_from_arg(&arg, cwd.as_deref()))
        .collect()
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
pub fn open_project_paths_from_urls(urls: Vec<tauri::Url>) -> Vec<String> {
    urls.into_iter()
        .filter_map(|url| url.to_file_path().ok())
        .filter_map(|path| normalize_open_project_path(path, None))
        .collect()
}

pub fn queue_open_project_paths(app: &tauri::AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    app.state::<PendingOpenProjects>().push(paths);
    let focused = app
        .webview_windows()
        .into_values()
        .find(|w| w.is_focused().unwrap_or(false));
    let window = focused.or_else(|| app.webview_windows().into_values().next());
    if let Some(window) = window {
        let _ = window.set_focus();
        let _ = window.emit_to(window.label(), "open-projects-pending", ());
    }
}

impl Project {
    fn new(settings: ProjectSettings) -> Self {
        Self {
            files: Vec::new(),
            settings,
            last_export: None,
            project_path: None,
            dirty: false,
        }
    }

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
            if let Some(reason) = screen_import(&path, &mut total, limits) {
                skipped.push(Skipped {
                    path: path.display().to_string(),
                    reason,
                });
            } else {
                self.files.push(path);
                added_any = true;
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
    let settings = store.settings();
    let limits = settings.limits();
    state.with_default(
        window.label(),
        settings.default_project_settings,
        |project| {
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
        },
    )
}

/// List regular files under a folder and prescreen them for the add-folder
/// preview dialog. The confirmed list still goes through `add_files`, which
/// repeats screening because files can change between preview and import.
#[tauri::command]
pub fn preview_folder(
    window: tauri::Window,
    state: State<'_, Workareas>,
    store: State<'_, GlobalStore>,
    path: String,
    recursive: bool,
) -> Result<Vec<FolderPreviewFile>, String> {
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
    let settings = store.settings();
    let limits = settings.limits();
    Ok(state.with_default(
        window.label(),
        settings.default_project_settings,
        |project| {
            let mut total: u64 = project
                .files
                .iter()
                .filter_map(|p| std::fs::metadata(p).ok().map(|m| m.len()))
                .sum();
            files
                .iter()
                .map(|path| {
                    let note = if project.files.contains(path) {
                        "Already Added".into()
                    } else {
                        screen_import(path, &mut total, limits).unwrap_or_default()
                    };
                    FolderPreviewFile {
                        path: path.display().to_string(),
                        importable: note.is_empty(),
                        note,
                    }
                })
                .collect()
        },
    ))
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
pub fn get_project(
    window: tauri::Window,
    state: State<'_, Workareas>,
    store: State<'_, GlobalStore>,
) -> ProjectView {
    state.with_default(
        window.label(),
        store.settings().default_project_settings,
        |project| project.view(),
    )
}

#[tauri::command]
pub fn take_pending_open_projects(pending: State<'_, PendingOpenProjects>) -> Vec<String> {
    pending.0.lock().unwrap().drain(..).collect()
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
pub fn new_project(
    window: tauri::Window,
    state: State<'_, Workareas>,
    store: State<'_, GlobalStore>,
) -> ProjectView {
    let default_project_settings = store.settings().default_project_settings;
    state.with(window.label(), |project| {
        *project = Project::new(default_project_settings);
        project.view()
    })
}

/// The OS the app is running on (`std::env::consts::OS`, e.g. "macos",
/// "windows", "linux"). The frontend uses it for platform-conventional UI such
/// as the titlebar text.
#[tauri::command]
pub fn host_os() -> &'static str {
    std::env::consts::OS
}

static WINDOW_SEQ: AtomicUsize = AtomicUsize::new(1);

/// Open a fresh window with an empty Untitled project.
#[tauri::command]
pub fn new_window(app: tauri::AppHandle, store: State<'_, GlobalStore>) -> Result<(), String> {
    let label = format!("main-{}", WINDOW_SEQ.fetch_add(1, Ordering::Relaxed));
    let window = tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::default())
        .title("BundlerMD")
        .inner_size(1024.0, 800.0)
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
        // Path presentation governs storage too: in Smart mode, files under
        // the .bmd's directory are stored relative to it so the project travels
        // with its files; in Absolute mode everything is stored absolute.
        let dir = target.parent();
        let smart = matches!(project.settings.path_presentation, PathPresentation::Smart);
        let stored_files = project
            .files
            .iter()
            .map(|p| match dir {
                Some(d) if smart => stored_path(p, d),
                _ => p.display().to_string(),
            })
            .collect();
        let file = ProjectFile::new(
            stored_files,
            project
                .last_export
                .as_ref()
                .map(|p| p.display().to_string()),
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
    // Relative entries resolve against the .bmd's directory; absolute ones are
    // taken as-is (mirrors `stored_path` at save time).
    let dir = project_path.parent().map(Path::to_path_buf);
    let view = state.with(window.label(), |project| {
        *project = Project {
            files: pf
                .files
                .iter()
                .map(|s| match dir.as_deref() {
                    Some(d) => resolve_stored(s, d),
                    None => PathBuf::from(s),
                })
                .collect(),
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

/// Generate the bundle for clipboard copy. This mirrors export problem handling
/// but intentionally does not touch `last_export` or dirty state.
#[tauri::command]
pub fn render_bundle_for_clipboard(
    window: tauri::Window,
    state: State<'_, Workareas>,
    store: State<'_, GlobalStore>,
    allow_problems: bool,
) -> Result<BundleTextResult, String> {
    let limits = store.settings().limits();
    let (markdown, problems) = state.with(window.label(), |project| {
        let title = effective_title(project.project_path.as_deref(), Path::new("Untitled.md"));
        generate_bundle_with_title(
            &project.files,
            &project.settings,
            project.project_path.as_deref(),
            &title,
            limits,
        )
    });

    if !problems.is_empty() && !allow_problems {
        return Ok(BundleTextResult {
            markdown: String::new(),
            problems,
        });
    }

    Ok(BundleTextResult { markdown, problems })
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
    let title = effective_title(project_path, output);
    generate_bundle_with_title(files, settings, project_path, &title, limits)
}

fn generate_bundle_with_title(
    files: &[PathBuf],
    settings: &ProjectSettings,
    project_path: Option<&Path>,
    title: &str,
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
                let fence_tag = settings
                    .add_detected_language_tag_to_code_fences
                    .then(|| crate::lang::fence_tag(path, &content))
                    .flatten();
                bundle_files.push(BundleFile {
                    display,
                    fence_tag,
                    content,
                });
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

    let description = if settings.include_description_in_export {
        settings.description.as_str()
    } else {
        ""
    };
    let markdown = bundle::assemble(
        title,
        description,
        &bundle_files,
        settings.newlines.resolve(),
        settings.toc_links,
        settings.include_line_ranges_in_headings,
    );
    (markdown, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_project_args_keep_only_bmd_paths() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("Demo.BMD");
        std::fs::write(&project, "{}").unwrap();
        std::fs::write(dir.path().join("note.txt"), "not a project").unwrap();

        let args = vec![
            "--flag".to_string(),
            "note.txt".to_string(),
            "Demo.BMD".to_string(),
        ];

        assert_eq!(
            open_project_paths_from_args(&args, dir.path().to_str().unwrap()),
            vec![project.canonicalize().unwrap().display().to_string()]
        );
    }

    #[test]
    fn open_project_args_accept_file_urls() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("url.bmd");
        std::fs::write(&project, "{}").unwrap();
        let url = tauri::Url::from_file_path(&project).unwrap().to_string();

        assert_eq!(
            open_project_paths_from_args(&[url], dir.path().to_str().unwrap()),
            vec![project.canonicalize().unwrap().display().to_string()]
        );
    }
}
