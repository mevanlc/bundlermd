//! Global, cross-window data: App Settings and the Recents list, persisted
//! as one JSON file in the platform config directory.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

use crate::project::ProjectSettings;
use crate::state::Limits;

pub const MAX_RECENTS: usize = 12;

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// `None` means "follow the OS" (Tauri's default).
    pub fn native(self) -> Option<tauri::Theme> {
        match self {
            Theme::System => None,
            Theme::Light => Some(tauri::Theme::Light),
            Theme::Dark => Some(tauri::Theme::Dark),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuRendering {
    /// Native menu bar only.
    #[default]
    Native,
    /// Native menu bar plus an in-window menubar (both render src/menu.json).
    Both,
}

fn default_max_file() -> u64 {
    Limits::default().max_file_bytes
}

fn default_max_total() -> u64 {
    Limits::default().max_total_bytes
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub theme: Theme,
    #[serde(default = "default_max_file")]
    pub max_file_bytes: u64,
    #[serde(default = "default_max_total")]
    pub max_total_bytes: u64,
    #[serde(default)]
    pub menu_rendering: MenuRendering,
    #[serde(default)]
    pub default_project_settings: ProjectSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            max_file_bytes: default_max_file(),
            max_total_bytes: default_max_total(),
            menu_rendering: MenuRendering::default(),
            default_project_settings: ProjectSettings::default(),
        }
    }
}

impl AppSettings {
    pub fn limits(&self) -> Limits {
        Limits {
            max_file_bytes: self.max_file_bytes,
            max_total_bytes: self.max_total_bytes,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct GlobalData {
    #[serde(default)]
    settings: AppSettings,
    #[serde(default)]
    recents: Vec<String>,
}

pub struct GlobalStore {
    path: PathBuf,
    data: Mutex<GlobalData>,
}

impl GlobalStore {
    /// Load from `path`, falling back to defaults if absent or unparsable —
    /// a corrupt settings file must not brick the app.
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            data: Mutex::new(data),
        }
    }

    fn persist(&self, data: &GlobalData) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    pub fn settings(&self) -> AppSettings {
        self.data.lock().unwrap().settings.clone()
    }

    pub fn set_settings(&self, settings: AppSettings) {
        let mut data = self.data.lock().unwrap();
        data.settings = settings;
        self.persist(&data);
    }

    pub fn recents(&self) -> Vec<String> {
        self.data.lock().unwrap().recents.clone()
    }

    /// MRU: move (or insert) `path` to the front, capped at MAX_RECENTS.
    pub fn touch_recent(&self, path: &str) {
        let mut data = self.data.lock().unwrap();
        data.recents.retain(|p| p != path);
        data.recents.insert(0, path.to_string());
        data.recents.truncate(MAX_RECENTS);
        self.persist(&data);
    }

    pub fn clear_recents(&self) {
        let mut data = self.data.lock().unwrap();
        data.recents.clear();
        self.persist(&data);
    }
}

/// Apply the theme preference to every open window.
pub fn apply_theme(app: &tauri::AppHandle, theme: Theme) {
    for window in app.webview_windows().values() {
        let _ = window.set_theme(theme.native());
    }
}

/// MRU-touch a project path and refresh the native Open Recent menu.
pub fn note_recent(app: &tauri::AppHandle, path: &str) {
    app.state::<GlobalStore>().touch_recent(path);
    crate::menudef::refresh(app);
}

#[tauri::command]
pub fn get_app_settings(store: State<'_, GlobalStore>) -> AppSettings {
    store.settings()
}

#[tauri::command]
pub fn set_app_settings(
    app: tauri::AppHandle,
    store: State<'_, GlobalStore>,
    settings: AppSettings,
) {
    store.set_settings(settings.clone());
    apply_theme(&app, settings.theme);
    // Broadcast so every window picks up e.g. the menubar preference.
    let _ = app.emit("app-settings-changed", settings);
}

#[tauri::command]
pub fn get_recents(store: State<'_, GlobalStore>) -> Vec<String> {
    store.recents()
}

#[tauri::command]
pub fn clear_recents(app: tauri::AppHandle, store: State<'_, GlobalStore>) {
    store.clear_recents();
    crate::menudef::refresh(&app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::NewlineSetting;

    #[test]
    fn app_settings_default_project_settings_falls_back() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();

        assert_eq!(
            settings.default_project_settings,
            ProjectSettings::default()
        );
    }

    #[test]
    fn app_settings_default_project_settings_round_trips() {
        let settings = AppSettings {
            default_project_settings: ProjectSettings {
                description: "Starter description".into(),
                include_description_in_export: false,
                newlines: NewlineSetting::Platform,
                ..Default::default()
            },
            ..Default::default()
        };

        let json = serde_json::to_string(&settings).unwrap();
        let loaded: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(
            loaded.default_project_settings,
            settings.default_project_settings
        );
    }
}
