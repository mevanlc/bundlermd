//! Project settings and the `.bmd` on-disk format (versioned JSON).

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bundle::Newline;

const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/mevanlc/bundlermd/refs/heads/main/schemas/project-v1.json";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NewlineSetting {
    #[default]
    Unix,
    Windows,
    Platform,
}

impl NewlineSetting {
    pub fn resolve(self) -> Newline {
        match self {
            NewlineSetting::Unix => Newline::Unix,
            NewlineSetting::Windows => Newline::Windows,
            NewlineSetting::Platform => {
                if cfg!(windows) {
                    Newline::Windows
                } else {
                    Newline::Unix
                }
            }
        }
    }
}

/// How file paths are both stored in the `.bmd` and rendered in the bundle.
/// - `Smart`: files under the project file's directory are stored/shown
///   relative to it; everything else is absolute (rendered as the shortest
///   unambiguous name).
/// - `Absolute`: always store and show full absolute paths.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, Eq, PartialEq)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum PathPresentation {
    #[default]
    Smart,
    Absolute,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HeaderStyle {
    #[default]
    Filename,
    None,
    Custom,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(default)]
pub struct FileOptions {
    pub include_code_fence: bool,
    pub include_in_toc: bool,
    pub header_style: HeaderStyle,
    pub custom_header: String,
}

impl Default for FileOptions {
    fn default() -> Self {
        Self {
            include_code_fence: true,
            include_in_toc: true,
            header_style: HeaderStyle::Filename,
            custom_header: String::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectEntry {
    pub path: PathBuf,
    pub options: FileOptions,
}

impl ProjectEntry {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            options: FileOptions::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct ProjectFileEntry {
    pub path: String,
    pub options: FileOptions,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(default)]
pub struct ProjectSettings {
    pub add_detected_language_tag_to_code_fences: bool,
    pub description: String,
    pub include_description_in_export: bool,
    pub include_line_ranges_in_headings: bool,
    pub newlines: NewlineSetting,
    pub path_presentation: PathPresentation,
    pub toc_links: bool,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            add_detected_language_tag_to_code_fences: true,
            description: String::new(),
            include_description_in_export: true,
            include_line_ranges_in_headings: false,
            newlines: NewlineSetting::Unix,
            path_presentation: PathPresentation::Smart,
            toc_links: false,
        }
    }
}

/// The `.bmd` file contents. The `$schema` field identifies the format version
/// and enables editor validation against the published JSON Schema.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectFile {
    #[serde(rename = "$schema", default)]
    schema: String,
    pub files: Vec<ProjectFileEntry>,
    pub last_export: Option<String>,
    #[serde(default)]
    pub settings: ProjectSettings,
}

impl ProjectFile {
    pub fn new(
        files: Vec<ProjectFileEntry>,
        last_export: Option<String>,
        settings: ProjectSettings,
    ) -> Self {
        Self {
            schema: SCHEMA_URL.into(),
            files,
            last_export,
            settings,
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read project file: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("invalid project file: {e}"))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("could not serialize project: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("could not write project file: {e}"))
    }
}

/// Bundle title, derived from the filename: `.bmd` stem → export output stem →
/// "Bundle". (There is no title setting; the project's name is its filename.)
pub fn effective_title(project_path: Option<&Path>, output: &Path) -> String {
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned());
    project_path
        .and_then(stem)
        .or_else(|| stem(output))
        .unwrap_or_else(|| "Bundle".into())
}

pub fn project_dir(project_path: Option<&Path>) -> Option<PathBuf> {
    project_path.and_then(Path::parent).map(Path::to_path_buf)
}

/// On-disk form of a workarea path. Files living under `project_dir` (the
/// `.bmd` file's directory) are stored relative to it, forward-slashed so the
/// project stays portable across platforms; files elsewhere keep their
/// absolute path.
pub fn stored_path(path: &Path, project_dir: &Path) -> String {
    if project_dir.as_os_str().is_empty() {
        return path.display().to_string();
    }
    match path.strip_prefix(project_dir) {
        Ok(rel) => rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.display().to_string(),
    }
}

/// Resolve a stored workarea path back to absolute. Relative entries are joined
/// to `project_dir`; absolute entries are returned unchanged.
pub fn resolve_stored(stored: &str, project_dir: &Path) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_dir.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bmd");
        let original = ProjectFile::new(
            vec![
                ProjectFileEntry {
                    path: "/a/b.txt".into(),
                    options: FileOptions::default(),
                },
                ProjectFileEntry {
                    path: "/c/d.txt".into(),
                    options: FileOptions {
                        include_code_fence: false,
                        include_in_toc: false,
                        header_style: HeaderStyle::Custom,
                        custom_header: "Custom heading".into(),
                    },
                },
            ],
            Some("/out.md".into()),
            ProjectSettings {
                add_detected_language_tag_to_code_fences: false,
                description: "Description".into(),
                include_description_in_export: true,
                include_line_ranges_in_headings: true,
                newlines: NewlineSetting::Platform,
                path_presentation: PathPresentation::Absolute,
                toc_links: true,
            },
        );
        original.save(&path).unwrap();
        let loaded = ProjectFile::load(&path).unwrap();
        assert_eq!(loaded.files, original.files);
        assert_eq!(loaded.last_export, original.last_export);
        assert_eq!(loaded.settings, original.settings);
    }

    #[test]
    fn schema_url_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schema.bmd");
        ProjectFile::new(vec![], None, ProjectSettings::default())
            .save(&path)
            .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#""$schema""#));
        assert!(text.contains(SCHEMA_URL));
    }

    #[test]
    fn missing_settings_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("min.bmd");
        std::fs::write(
            &path,
            format!(
                r#"{{
                    "$schema": "{SCHEMA_URL}",
                    "files": [{{ "path": "/x.txt", "options": {{}} }}],
                    "last_export": null
                }}"#
            ),
        )
        .unwrap();
        let loaded = ProjectFile::load(&path).unwrap();
        assert_eq!(loaded.settings, ProjectSettings::default());
        assert_eq!(loaded.files[0].options, FileOptions::default());
    }

    #[test]
    fn missing_setting_fields_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old-settings.bmd");
        std::fs::write(
            &path,
            format!(
                r#"{{
                    "$schema": "{SCHEMA_URL}",
                    "files": [{{ "path": "/x.txt", "options": {{}} }}],
                    "last_export": null,
                    "settings": {{
                        "description": "Old project",
                        "include_description_in_export": true,
                        "newlines": "unix",
                        "path_presentation": {{ "mode": "smart" }},
                        "toc_links": false
                    }}
                }}"#
            ),
        )
        .unwrap();

        let loaded = ProjectFile::load(&path).unwrap();
        assert_eq!(loaded.settings.description, "Old project");
        assert!(loaded.settings.add_detected_language_tag_to_code_fences);
        assert!(!loaded.settings.include_line_ranges_in_headings);
    }

    #[test]
    fn stored_path_relativizes_under_project_dir() {
        let dir = Path::new("/proj");
        // Under the project dir (directly and nested): stored relative.
        assert_eq!(stored_path(Path::new("/proj/a.txt"), dir), "a.txt");
        assert_eq!(
            stored_path(Path::new("/proj/src/main.rs"), dir),
            "src/main.rs"
        );
        // Outside the project dir: stored absolute.
        assert_eq!(
            stored_path(Path::new("/elsewhere/x.txt"), dir),
            "/elsewhere/x.txt"
        );
        // A sibling sharing a name prefix is not "under" the dir.
        assert_eq!(
            stored_path(Path::new("/project-x/y.txt"), dir),
            "/project-x/y.txt"
        );
    }

    #[test]
    fn resolve_stored_round_trip() {
        let dir = Path::new("/proj");
        for abs in ["/proj/a.txt", "/proj/src/main.rs", "/elsewhere/x.txt"] {
            let stored = stored_path(Path::new(abs), dir);
            assert_eq!(resolve_stored(&stored, dir), PathBuf::from(abs));
        }
    }

    #[test]
    fn effective_title_fallbacks() {
        let out = Path::new("/tmp/Out File.md");
        let proj = PathBuf::from("/p/My Proj.bmd");
        // .bmd stem wins; without a project, the output stem; "Bundle" as last resort.
        assert_eq!(effective_title(Some(&proj), out), "My Proj");
        assert_eq!(effective_title(None, out), "Out File");
        assert_eq!(effective_title(None, Path::new("/")), "Bundle");
    }
}
