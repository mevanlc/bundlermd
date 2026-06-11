//! Native menu built from the shared single-source-of-truth definition in
//! `src/menu.json`. The frontend reads the same file when it renders an
//! in-window menubar (App Settings preference). The macOS app menu
//! (About/Settings/Hide/Quit) is platform boilerplate and stays in code.

use serde::Deserialize;
use tauri::menu::{
    Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, Submenu, SubmenuBuilder,
};
use tauri::{App, AppHandle, Emitter, Manager, Wry};

use crate::store::GlobalStore;

const MENU_JSON: &str = include_str!("../../src/menu.json");

#[derive(Deserialize)]
struct MenuDef {
    menus: Vec<SubmenuDef>,
}

#[derive(Deserialize)]
struct SubmenuDef {
    label: String,
    items: Vec<MenuNode>,
}

/// One entry in a menu. Untagged: variants are distinguished by their fields,
/// so order matters (Submenu requires `items`, Action requires `id`).
#[derive(Deserialize)]
#[serde(untagged)]
enum MenuNode {
    Separator {
        #[allow(dead_code)]
        separator: bool,
    },
    Predefined {
        predefined: String,
    },
    /// Placeholder filled with the global Recents list at build time.
    Recents {
        label: String,
        #[allow(dead_code)]
        recents: bool,
    },
    Submenu(SubmenuDef),
    Action {
        id: String,
        label: String,
        accelerator: Option<String>,
    },
}

/// Home-relative display for recents entries ("~/p/foo.bmd").
fn abbreviate_home(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

fn build_recents_submenu(app: &AppHandle, label: &str) -> tauri::Result<Submenu<Wry>> {
    let recents = app.state::<GlobalStore>().recents();
    let mut builder = SubmenuBuilder::new(app, label);
    for path in &recents {
        let item =
            MenuItemBuilder::with_id(format!("recent:{path}"), abbreviate_home(path)).build(app)?;
        builder = builder.item(&item);
    }
    if !recents.is_empty() {
        let clear = MenuItemBuilder::with_id("clear_recents", "Clear Menu").build(app)?;
        builder = builder.separator().item(&clear);
    }
    let submenu = builder.build()?;
    if recents.is_empty() {
        submenu.set_enabled(false)?;
    }
    Ok(submenu)
}

fn build_submenu(app: &AppHandle, def: &SubmenuDef) -> tauri::Result<Submenu<Wry>> {
    let mut builder = SubmenuBuilder::new(app, &def.label);
    for node in &def.items {
        match node {
            MenuNode::Separator { .. } => builder = builder.separator(),
            MenuNode::Predefined { predefined } => {
                let item = match predefined.as_str() {
                    "undo" => PredefinedMenuItem::undo(app, None)?,
                    "redo" => PredefinedMenuItem::redo(app, None)?,
                    "cut" => PredefinedMenuItem::cut(app, None)?,
                    "copy" => PredefinedMenuItem::copy(app, None)?,
                    "paste" => PredefinedMenuItem::paste(app, None)?,
                    "select_all" => PredefinedMenuItem::select_all(app, None)?,
                    other => panic!("menu.json: unknown predefined item {other:?}"),
                };
                builder = builder.item(&item);
            }
            MenuNode::Recents { label, .. } => {
                builder = builder.item(&build_recents_submenu(app, label)?);
            }
            MenuNode::Submenu(sub) => builder = builder.item(&build_submenu(app, sub)?),
            MenuNode::Action {
                id,
                label,
                accelerator,
            } => {
                let mut item = MenuItemBuilder::with_id(id.clone(), label);
                if let Some(acc) = accelerator {
                    item = item.accelerator(acc);
                }
                builder = builder.item(&item.build(app)?);
            }
        }
    }
    builder.build()
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let def: MenuDef = serde_json::from_str(MENU_JSON).expect("menu.json is invalid");

    let mut menu = MenuBuilder::new(app);
    #[cfg(target_os = "macos")]
    {
        let settings = MenuItemBuilder::with_id("app_settings", "Settings…")
            .accelerator("Cmd+,")
            .build(app)?;
        let app_menu = SubmenuBuilder::new(app, "BundlerMD")
            .about(None)
            .separator()
            .item(&settings)
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .quit()
            .build()?;
        menu = menu.item(&app_menu);
    }
    for submenu in &def.menus {
        menu = menu.item(&build_submenu(app, submenu)?);
    }
    menu.build()
}

/// Build and install the native menu, and forward menu events to the focused
/// window as a `"menu"` event carrying the action id.
pub fn install(app: &App) -> tauri::Result<()> {
    app.set_menu(build_menu(app.handle())?)?;
    app.on_menu_event(|app, event| {
        // The menu is app-wide; deliver the action to the focused window
        // only, so e.g. Cmd+N doesn't fire in every open window.
        let focused = app
            .webview_windows()
            .into_values()
            .find(|w| w.is_focused().unwrap_or(false));
        if let Some(window) = focused {
            let _ = window.emit_to(window.label(), "menu", event.id().as_ref());
        }
    });
    Ok(())
}

/// Rebuild the native menu (e.g. after the Recents list changes). Menus must
/// be touched from the main thread; the event handler installed by
/// `install()` is unaffected.
pub fn refresh(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Ok(menu) = build_menu(&app) {
            let _ = app.set_menu(menu);
        }
    });
}
