pub mod bundle;
pub mod reading;
pub mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::Workarea::default())
        .invoke_handler(tauri::generate_handler![
            state::add_files,
            state::add_folder,
            state::remove_file,
            state::move_file,
            state::set_order,
            state::get_files,
            state::export_bundle,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
