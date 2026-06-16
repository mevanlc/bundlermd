pub mod anchors;
pub mod bundle;
pub mod lang;
pub mod menudef;
pub mod project;
pub mod reading;
pub mod smartpath;
pub mod state;
pub mod store;

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: second launches forward here and exit.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.webview_windows().values().next() {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state::Workareas::default())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            app.manage(store::GlobalStore::load(config_dir.join("global.json")));
            // The menu build reads the store (Open Recent), so order matters.
            menudef::install(app)?;
            let theme = app.state::<store::GlobalStore>().settings().theme;
            store::apply_theme(app.handle(), theme);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let workareas = window.state::<state::Workareas>();
                if workareas.is_dirty(window.label()) {
                    // Hold the window open; the frontend shows the
                    // save/discard/cancel prompt and destroys it explicitly.
                    api.prevent_close();
                    let _ = window.emit_to(window.label(), "close-requested", ());
                }
            }
            tauri::WindowEvent::Destroyed => {
                window.state::<state::Workareas>().remove(window.label());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            state::add_files,
            state::preview_folder,
            state::new_window,
            state::remove_file,
            state::move_file,
            state::set_order,
            state::get_project,
            state::update_settings,
            state::new_project,
            state::save_project,
            state::open_project,
            state::export_bundle,
            state::render_bundle_for_clipboard,
            state::host_os,
            store::get_app_settings,
            store::set_app_settings,
            store::get_recents,
            store::clear_recents,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
