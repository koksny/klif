//! Tray icon: Show / Hide, a "Panel mode" check item, a Skin submenu (emits `klif://skin` to the UI), Stop all
//! Systems (after a confirmation), Quit. Quitting KLIF never stops a model server.
//! The Skin submenu starts with the built-in skins and is replaced by the UI's own list (`klif_skins`), which
//! also holds local-only skins.
//! Left click shows KLIF (macOS: a click opens the menu, as menu bar items do; the icon is a template image that
//! follows the menu bar's look). The tooltip says so while the core waits for another process to release the
//! engine.

use klif_common::vm::Action;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};

use crate::{opener, panel, shell};

/// The tray icon's id.
const TRAY_ID: &str = "klif";

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

/// Tooltip text; None = just "KLIF".
pub fn set_tooltip<R: Runtime>(app: &AppHandle<R>, text: Option<&str>) {
    if let Some(t) = app.tray_by_id(TRAY_ID) {
        let _ = t.set_tooltip(Some(text.unwrap_or("KLIF")));
    }
}

/// "Stop all Systems": ask first (a loaded model costs minutes to bring back), then stop every local System
/// that runs. Works on a worker thread; the dialog blocks it, not the tray.
fn stop_all<R: Runtime>(app: &AppHandle<R>) {
    let Some(e) = shell::shell(app).engine_if_ready() else {
        log::info!("tray: stop all: the core is not ready");
        return;
    };
    let running: Vec<String> = e.snapshot().systems.into_iter().filter(|s| s.node.is_none() && s.session.is_some()).map(|s| s.label).collect();
    if running.is_empty() {
        log::info!("tray: stop all: nothing is running");
        return;
    }
    let question = format!("Stop {}?\n\nThe loaded models are unloaded and have to be loaded again.", running.join(", "));
    if !opener::confirm("KLIF - Stop all Systems", &question) {
        log::info!("tray: stop all: cancelled");
        return;
    }
    log::info!("tray: stop all ({} Systems)", running.len());
    if let Err(err) = e.act(Action::StopAll) {
        let msg = format!("{err:#}");
        log::warn!("tray: stop all failed: {msg}");
        opener::message("KLIF - Stop all Systems", &msg);
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
    let stop_item = MenuItem::with_id(app, "stop-all", "Stop all Systems", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit KLIF", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &panel_item, &skins, &sep, &stop_item, &quit])?;
    #[cfg(target_os = "macos")]
    let icon = Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    #[cfg(not(target_os = "macos"))]
    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    const MAC: bool = cfg!(target_os = "macos");
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(MAC)
        .tooltip("KLIF")
        .menu(&menu)
        .show_menu_on_left_click(MAC)
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
                "stop-all" => {
                    let app = app.clone();
                    let _ = std::thread::Builder::new().name("klif-stop-all".into()).spawn(move || stop_all(&app));
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
                if !MAC {
                    shell::show_main(tray.app_handle());
                }
            }
        })
        .build(app)?;
    log::info!("tray built");
    Ok(())
}
