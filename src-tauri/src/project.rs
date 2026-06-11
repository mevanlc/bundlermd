//! Project settings and the `.bmd` on-disk format (versioned JSON).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bundle::Newline;

/// Current `.bmd` format version. Bump on breaking changes.
pub const PROJECT_VERSION: u32 = 1;

const FORMAT_NAME: &str = "BundlerMD Project";

#[derive(Serialize, Deserialize, Clone, Debug)]
struct FormatTag {
    name: String,
    version: u32,
}

impl FormatTag {
    fn current() -> Self {
        Self { name: FORMAT_NAME.into(), version: PROJECT_VERSION }
    }
}

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

#[derive(Serialize, Deserialize, Clone, Debug, Default, Eq, PartialEq)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum PathPresentation {
    #[default]
    Smart,
    Absolute,
    Fixed {
        location: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(default)]
pub struct ProjectSettings {
    pub title: String,
    pub introduction: String,
    pub newlines: NewlineSetting,
    pub path_presentation: PathPresentation,
    pub toc_links: bool,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            title: String::new(),
            introduction: String::new(),
            newlines: NewlineSetting::Unix,
            path_presentation: PathPresentation::Smart,
            toc_links: false,
        }
    }
}

/// The `.bmd` file contents.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectFile {
    #[serde(rename = "__format__")]
    format: FormatTag,
    pub files: Vec<String>,
    pub last_export: Option<String>,
    #[serde(default)]
    pub settings: ProjectSettings,
}

impl ProjectFile {
    pub fn new(
        files: Vec<String>,
        last_export: Option<String>,
        settings: ProjectSettings,
    ) -> Self {
        Self { format: FormatTag::current(), files, last_export, settings }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read project file: {e}"))?;
        let file: ProjectFile =
            serde_json::from_str(&text).map_err(|e| format!("invalid project file: {e}"))?;
        if file.format.version > PROJECT_VERSION {
            return Err(format!(
                "project file version {} is newer than this BundlerMD understands (max {})",
                file.format.version, PROJECT_VERSION
            ));
        }
        Ok(file)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("could not serialize project: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("could not write project file: {e}"))
    }
}

/// Title fallback chain from the PRD: project title, else .bmd basename
/// (stem), else the export output's stem.
pub fn effective_title(settings: &ProjectSettings, project_path: Option<&Path>, output: &Path) -> String {
    if !settings.title.is_empty() {
        return settings.title.clone();
    }
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned());
    project_path.and_then(stem)
        .or_else(|| stem(output))
        .unwrap_or_else(|| "Bundle".into())
}

pub fn project_dir(project_path: Option<&Path>) -> Option<PathBuf> {
    project_path.and_then(Path::parent).map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bmd");
        let original = ProjectFile::new(
            vec!["/a/b.txt".into(), "/c/d.txt".into()],
            Some("/out.md".into()),
            ProjectSettings {
                title: "T".into(),
                introduction: "Intro".into(),
                newlines: NewlineSetting::Platform,
                path_presentation: PathPresentation::Fixed {
                    location: "/base".into(),
                },
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
    fn magic_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("magic.bmd");
        ProjectFile::new(vec![], None, ProjectSettings::default())
            .save(&path)
            .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#""__format__""#));
        assert!(text.contains(r#""name": "BundlerMD Project""#));
        assert!(text.contains(r#""version": 1"#));
    }

    #[test]
    fn future_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.bmd");
        std::fs::write(
            &path,
            r#"{"__format__": {"name": "BundlerMD Project", "version": 999}, "files": [], "last_export": null}"#,
        )
        .unwrap();
        let err = ProjectFile::load(&path).unwrap_err();
        assert!(err.contains("newer"));
    }

    #[test]
    fn missing_settings_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("min.bmd");
        std::fs::write(
            &path,
            r#"{"__format__": {"name": "BundlerMD Project", "version": 1}, "files": ["/x.txt"], "last_export": null}"#,
        )
        .unwrap();
        let loaded = ProjectFile::load(&path).unwrap();
        assert_eq!(loaded.settings, ProjectSettings::default());
    }

    #[test]
    fn effective_title_fallbacks() {
        let s = |t: &str| ProjectSettings {
            title: t.into(),
            ..Default::default()
        };
        let out = Path::new("/tmp/Out File.md");
        let proj = PathBuf::from("/p/My Proj.bmd");
        assert_eq!(effective_title(&s("Custom"), Some(&proj), out), "Custom");
        assert_eq!(effective_title(&s(""), Some(&proj), out), "My Proj");
        assert_eq!(effective_title(&s(""), None, out), "Out File");
    }
}
