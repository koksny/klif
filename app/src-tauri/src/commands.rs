//! IPC commands of the native transport (app/ui/src/lib/transport/tauri.ts). Errors are user-facing
//! sentences (the UI shows them as toasts). The API key is copied natively and never sent to the page.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use klif_common::vm::{Action, SlotKind, ViewModel};
use tauri::{AppHandle, Runtime, State};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::clipboard;
use crate::engine::EngineHandle;
use crate::panel;
use crate::shell::{main_window, Shell};

type ShellState<'a> = State<'a, Arc<Shell>>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("Internal error: {e}"))?
}

/// Run `f` on the main (UI) thread and wait for its result.
async fn on_main<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    f: impl FnOnce(&AppHandle<R>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    let h = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&h));
    })
    .map_err(|e| format!("Internal error: {e}"))?;
    blocking(move || rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "Internal error: the window did not answer.".to_string())?).await
}

fn engine(shell: &ShellState<'_>) -> Result<EngineHandle, String> {
    shell.engine()
}

/// The first view model (waits for the core to finish starting).
#[tauri::command]
pub async fn klif_snapshot(shell: ShellState<'_>) -> Result<ViewModel, String> {
    let s = shell.inner().clone();
    let e = blocking(move || s.wait_engine(Duration::from_secs(120))).await?;
    Ok(e.snapshot())
}

/// One entry of the UI's skin registry, for the tray's Skin submenu.
#[derive(serde::Deserialize)]
pub struct SkinEntry {
    id: String,
    name: String,
}

/// The UI reports its skin list (built-in plus local-only skins); the tray's Skin submenu follows it.
#[tauri::command]
pub async fn klif_skins(app: AppHandle<tauri::Wry>, skins: Vec<SkinEntry>) -> Result<(), String> {
    let list: Vec<(String, String)> = skins.into_iter().map(|s| (s.id, s.name)).collect();
    on_main(&app, move |app| crate::tray::set_skins(app, &list).map_err(|e| format!("The tray menu could not be updated: {e}"))).await
}

#[tauri::command]
pub async fn klif_act(shell: ShellState<'_>, action: Action) -> Result<(), String> {
    let e = engine(&shell)?;
    log::info!("act {action:?}");
    let r = blocking(move || e.act(action).map_err(|err| err.to_string())).await;
    if let Err(msg) = &r {
        log::info!("act refused: {msg}");
    }
    r
}

fn endpoint(shell: &ShellState<'_>) -> Result<(EngineHandle, String), String> {
    let e = engine(shell)?;
    let url = e.endpoint_url().ok_or_else(|| "No session is running.".to_string())?;
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("Not a web address: {url}"));
    }
    Ok((e, url))
}

/// Open the session endpoint in the default browser.
#[tauri::command]
pub async fn klif_open_endpoint<R: Runtime>(app: AppHandle<R>, shell: ShellState<'_>) -> Result<(), String> {
    let (_, url) = endpoint(&shell)?;
    log::info!("open endpoint {url}");
    on_main(&app, move |_| {
        let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(url.as_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        // ShellExecute returns a value > 32 on success.
        if (r.0 as isize) > 32 { Ok(()) } else { Err(format!("Could not open {url} (error {}).", r.0 as isize)) }
    })
    .await
}

/// Copy the endpoint. LLM sessions copy the OpenAI-compatible base URL (".../v1").
#[tauri::command]
pub async fn klif_copy_endpoint<R: Runtime>(app: AppHandle<R>, shell: ShellState<'_>) -> Result<(), String> {
    let (e, url) = endpoint(&shell)?;
    let llm = e.snapshot().session.map(|s| s.slot.kind() == SlotKind::Llm).unwrap_or(false);
    let text = if llm && !url.trim_end_matches('/').ends_with("/v1") { format!("{}/v1", url.trim_end_matches('/')) } else { url };
    log::info!("copy endpoint {text}");
    on_main(&app, move |h| {
        let w = main_window(h).ok_or("The window is gone.")?;
        let hwnd = w.hwnd().map_err(|e| e.to_string())?;
        clipboard::copy_text(hwnd, &text, false)
    })
    .await
}

/// Copy the API key natively (excluded from clipboard history). The key never reaches the page.
#[tauri::command]
pub async fn klif_copy_api_key<R: Runtime>(app: AppHandle<R>, shell: ShellState<'_>) -> Result<(), String> {
    let e = engine(&shell)?;
    let key = e.api_key().ok_or_else(|| "No API key is set.".to_string())?;
    log::info!("copy API key ({} chars)", key.expose().chars().count());
    on_main(&app, move |h| {
        let w = main_window(h).ok_or("The window is gone.")?;
        let hwnd = w.hwnd().map_err(|e| e.to_string())?;
        clipboard::copy_text(hwnd, key.expose(), true)
    })
    .await
}

/// Panel mode on / off: move the window onto the small status screen and fill it, or bring the previous
/// window back. The UI follows through the view model (HostInfo.panel), not through this result.
#[tauri::command]
pub async fn klif_toggle_panel<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let r = blocking(move || panel::toggle(&app).map(|_| ())).await;
    if let Err(msg) = &r {
        log::info!("panel toggle refused: {msg}");
    }
    r
}

/// Diagnostics from the page into the shell log (bounded; never carries secrets).
#[tauri::command]
pub fn klif_ui_log(line: String) {
    let mut line = line.replace(['\r', '\n'], " ");
    if line.len() > 400 {
        let mut cut = 400;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
    }
    log::info!("ui: {line}");
}
