//! Tray icon: Show / Hide, a "Panel mode" check item, a Skin submenu (emits `klif://skin` to the UI), Quit.
//! The Skin submenu starts with the built-in skins and is replaced by the UI's own list (`klif_skins`), which
//! also holds local-only skins.
//! Left click shows KLIF.

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};

use crate::{panel, shell};

/// The tray items that follow the application's state (managed by Tauri).
pub struct TrayItems {
    panel: CheckMenuItem<Wry>,
    skins: Submenu<Wry>,
}

/// Replace the Skin submenu with the UI registry's list (id, name), in its order.
pub fn set_skins(app: &AppHandle<Wry>, list: &[(String, String)]) -> tauri::Result<()> {
    let Some(t) = app.try_state::<TrayItems>() else { return Ok(()) };
    for item in t.skins.items()? {
        t.skins.remove(&item)?;
    }
    for (id, name) in list {
        let item = MenuItem::with_id(app, format!("skin:{id}"), name, true, None::<&str>)?;
        t.skins.append(&item)?;
    }
    Ok(())
}

/// Keep the "Panel mode" item in step: enabled when a small screen exists, checked while panel mode is on.
pub fn sync_panel<R: Runtime>(app: &AppHandle<R>, available: bool, active: bool) {
    if let Some(t) = app.try_state::<TrayItems>() {
        let _ = t.panel.set_enabled(available || active);
        let _ = t.panel.set_checked(active);
    }
}

/// Skin ids and names, in the UI registry's order (app/ui/src/skins/registry.ts).
const SKINS: &[(&str, &str)] = &[
    ("cliff", "Cliff"),
    ("silicon", "Silicon"),
    ("instrument", "Instrument"),
    ("phosphor", "Phosphor"),
    ("decode", "Decode"),
    ("loom", "Loom"),
    ("ether", "Ether"),
    ("rings", "Rings"),
    ("spirit", "Spirit"),
];

pub fn build(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", true, None::<&str>)?;
    let (available, active) = {
        let h = shell::shell(app).host.lock().unwrap().panel.clone();
        (h.available, h.active)
    };
    let panel_item = CheckMenuItem::with_id(app, "panel", "Panel mode", available || active, active, None::<&str>)?;
    let skin_items: Vec<MenuItem<Wry>> = SKINS
        .iter()
        .map(|(id, name)| MenuItem::with_id(app, format!("skin:{id}"), *name, true, None::<&str>))
        .collect::<tauri::Result<_>>()?;
    let skin_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = skin_items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<Wry>).collect();
    let skins = Submenu::with_id_and_items(app, "skin", "Skin", true, &skin_refs)?;
    app.manage(TrayItems { panel: panel_item.clone(), skins: skins.clone() });
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit KLIF", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &panel_item, &skins, &sep, &quit])?;
    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    TrayIconBuilder::with_id("klif")
        .icon(icon)
        .tooltip("KLIF")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| {
            let id = e.id.as_ref();
            match id {
                "toggle" => shell::toggle_main(app),
                "panel" => {
                    // The check box flips by itself when clicked: toggle for real, then show the true state.
                    let app = app.clone();
                    let _ = std::thread::Builder::new().name("klif-panel".into()).spawn(move || {
                        match panel::toggle(&app) {
                            Ok(on) => log::info!("tray: panel mode {}", if on { "on" } else { "off" }),
                            Err(e) => log::warn!("tray: panel mode: {e}"),
                        }
                        let h = shell::shell(&app).host.lock().unwrap().panel.clone();
                        sync_panel(&app, h.available, h.active);
                    });
                }
                "quit" => {
                    log::info!("tray: quit");
                    app.exit(0);
                }
                _ => {
                    if let Some(skin) = id.strip_prefix("skin:") {
                        log::info!("tray: skin {skin}");
                        let _ = app.emit("klif://skin", skin);
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                shell::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    log::info!("tray built");
    Ok(())
}
