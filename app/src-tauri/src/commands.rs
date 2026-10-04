//! IPC commands of the native transport (app/ui/src/lib/transport/tauri.ts). Errors are user-facing
//! sentences (the UI shows them as toasts). The API key is copied natively and never sent to the page; a key
//! the user types arrives once, goes straight to the core and is never echoed or logged.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use klif_common::config::PresetCfg;
use klif_common::vm::{Action, CommandView, PresetDetail, RecordEvent, RecordMetric, SystemId, SystemKind, SystemStatus, ViewModel};
use klif_common::Secret;
use tauri::{AppHandle, Runtime, State};

use crate::engine::EngineHandle;
use crate::shell::{EngineStatus, Shell};
use crate::{clipboard, endpoint, opener, panel};

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

/// The first view model (waits for the core to finish starting; while another process holds the engine it
/// keeps waiting, because the shell keeps retrying).
#[tauri::command]
pub async fn klif_snapshot(shell: ShellState<'_>) -> Result<ViewModel, String> {
    let s = shell.inner().clone();
    let e = blocking(move || s.wait_engine(Duration::from_secs(120))).await?;
    Ok(e.snapshot())
}

/// Where the core start stands: "starting", "waiting" (another process holds the engine; the message says
/// which, and the shell retries every 2 s), "ready" or "failed". For the UI's Connecting screen.
#[tauri::command]
pub fn klif_engine_status(shell: ShellState<'_>) -> EngineStatus {
    shell.engine_status()
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

/// Run an action. Only its summary is logged (type, System, id), never a preset body.
#[tauri::command]
pub async fn klif_act(shell: ShellState<'_>, action: Action) -> Result<(), String> {
    let e = engine(&shell)?;
    log::info!("act {}", action.summary());
    let r = blocking(move || e.act(action).map_err(|err| format!("{err:#}"))).await;
    if let Err(msg) = &r {
        log::info!("act refused: {msg}");
    }
    r
}

/// One preset in full for the editor (secrets masked); `node` = a preset of a remote node. `null` = no such preset;
/// a node that cannot answer rejects with its sentence (Tune shows it instead of "not in klif.toml").
#[tauri::command]
pub async fn klif_preset_get(shell: ShellState<'_>, id: String, node: Option<String>) -> Result<Option<PresetDetail>, String> {
    let e = engine(&shell)?;
    blocking(move || e.try_preset(&id, node.as_deref()).map_err(|err| format!("{err:#}"))).await
}

/// The climb of one record (the Records screen's progress chart): every broken record of `key` (a key of
/// `ViewModel.records`; a node's `"<node>/<key>"` is asked of that node), oldest first, optionally of one metric.
/// Rejects with the engine's sentence (no such record, or the node could not answer).
#[tauri::command]
pub async fn klif_records_history(shell: ShellState<'_>, key: String, metric: Option<RecordMetric>) -> Result<Vec<RecordEvent>, String> {
    let e = engine(&shell)?;
    blocking(move || e.records_history(&key, metric).map_err(|err| format!("{err:#}"))).await
}

/// Save the Records export card: `data` is the base64 of a PNG or GIF, `name` the file name the UI suggests. It
/// lands in the user's Pictures folder, subfolder KLIF, under a safe name and never over an existing file; the
/// result is the full path of the file.
#[tauri::command]
pub async fn klif_save_image(name: String, data: String) -> Result<String, String> {
    let r = blocking(move || crate::images::save(&name, &data)).await;
    match &r {
        Ok(path) => log::info!("saved image {}", std::path::Path::new(path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
        Err(msg) => log::info!("image not saved: {msg}"),
    }
    r
}

/// The command an unsaved preset would run for `system` (the Tune drawer's live preview), with its issues.
/// The spec can carry secret env values: it is neither logged nor printed.
#[tauri::command]
pub async fn klif_command_preview(shell: ShellState<'_>, spec: PresetCfg, system: Option<SystemId>) -> Result<CommandView, String> {
    let e = engine(&shell)?;
    blocking(move || Ok(e.command_preview(&spec, system.as_ref()))).await
}

/// Set (a string) or clear (`null`) the API key. The key goes to the core as received and is wiped from this
/// side; it is never echoed back and never logged (not even its length).
#[tauri::command]
pub async fn klif_set_api_key(shell: ShellState<'_>, key: Option<String>) -> Result<(), String> {
    let e = engine(&shell)?;
    let secret = match key {
        None => None,
        Some(mut k) => {
            let s = Secret::new(k.as_str());
            wipe(&mut k);
            Some(s.ok_or_else(|| "The API key is empty. Use Clear to remove the key.".to_string())?)
        }
    };
    log::info!("{} the API key", if secret.is_some() { "set" } else { "clear" });
    let r = blocking(move || e.set_api_key(secret).map_err(|err| format!("{err:#}"))).await;
    if let Err(msg) = &r {
        log::info!("API key refused: {msg}");
    }
    r
}

/// Overwrite a String's bytes before it is freed (zero bytes are valid UTF-8).
fn wipe(s: &mut String) {
    // SAFETY: only zero bytes are written, so the String stays valid UTF-8.
    unsafe {
        for b in s.as_bytes_mut() {
            std::ptr::write_volatile(b, 0);
        }
    }
    s.clear();
}

/// Open klif.toml in the default editor (the core creates a starter file first when there is none); Notepad
/// when `.toml` has no handler.
#[tauri::command]
pub async fn klif_open_config<R: Runtime>(app: AppHandle<R>, shell: ShellState<'_>) -> Result<(), String> {
    let e = engine(&shell)?;
    let path = blocking(move || e.ensure_config().map_err(|err| format!("{err:#}"))).await?;
    log::info!("open config {}", path.display());
    on_main(&app, move |_| opener::open_file(&path)).await
}

/// Open the logs folder (`[paths] logs_dir`: session logs and the shell log) in the file manager.
#[tauri::command]
pub async fn klif_open_logs<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    // Read from the file, not from the engine: it works while the core is still starting or failed.
    let dir = blocking(|| {
        let dir = klif_common::config::load().cfg.logs_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create the logs folder {}: {e}", dir.display()))?;
        Ok(dir)
    })
    .await?;
    log::info!("open logs {}", dir.display());
    on_main(&app, move |_| opener::open_folder(&dir)).await
}

/// A System's endpoint as the shell needs it.
struct Found {
    label: String,
    status: SystemStatus,
    endpoint: endpoint::Endpoint,
}

/// The endpoint of `system` (None = the selected one), from what the view model publishes.
fn find_endpoint(e: &EngineHandle, system: Option<SystemId>) -> Result<Found, String> {
    let vm = e.snapshot();
    let id = system.or_else(|| vm.selected.clone()).ok_or_else(|| "No System is selected.".to_string())?;
    let sys = vm.systems.iter().find(|s| s.id == id).ok_or_else(|| format!("There is no System {id}."))?;
    let published = sys
        .endpoint
        .clone()
        .or_else(|| e.endpoint_url(Some(&id)))
        .ok_or_else(|| format!("{} is not running, so it has no endpoint yet.", sys.label))?;
    // A remote System is reached through its node: loopback / wildcard hosts mean the node's, not ours.
    let node_host = sys
        .node
        .as_ref()
        .and_then(|n| vm.nodes.iter().find(|x| &x.id == n))
        .map(|n| endpoint::host_of_address(&n.address));
    let endpoint = endpoint::normalize(&published, sys.kind == SystemKind::Llm, node_host.as_deref())?;
    Ok(Found { label: sys.label.clone(), status: sys.status, endpoint })
}

/// Open a System's endpoint in the default browser (None = the selected System).
#[tauri::command]
pub async fn klif_open_endpoint<R: Runtime>(app: AppHandle<R>, shell: ShellState<'_>, system: Option<SystemId>) -> Result<(), String> {
    let e = engine(&shell)?;
    let found = find_endpoint(&e, system)?;
    match found.status {
        SystemStatus::Online | SystemStatus::Busy => {}
        SystemStatus::Starting => return Err(format!("{} is still starting.", found.label)),
        _ => return Err(format!("{} is not online.", found.label)),
    }
    let url = found.endpoint.open;
    log::info!("open endpoint {url}");
    on_main(&app, move |_| opener::open_url(&url)).await
}

/// Copy a System's endpoint (None = the selected System). An LLM's is the OpenAI-compatible base URL
/// (".../v1"); a remote System's address uses its node's host.
#[tauri::command]
pub async fn klif_copy_endpoint<R: Runtime>(app: AppHandle<R>, shell: ShellState<'_>, system: Option<SystemId>) -> Result<(), String> {
    let e = engine(&shell)?;
    let text = find_endpoint(&e, system)?.endpoint.copy;
    log::info!("copy endpoint {text}");
    on_main(&app, move |h| clipboard::copy(h, &text, false)).await
}

/// Copy the API key natively (excluded from clipboard history). The key never reaches the page. Only for
/// local, KLIF-managed Systems: a remote System's key lives on its node, an external server has its own.
#[tauri::command]
pub async fn klif_copy_api_key<R: Runtime>(app: AppHandle<R>, shell: ShellState<'_>, system: Option<SystemId>) -> Result<(), String> {
    let e = engine(&shell)?;
    let vm = e.snapshot();
    if let Some(sys) = system.or_else(|| vm.selected.clone()).and_then(|id| vm.systems.into_iter().find(|s| s.id == id)) {
        if sys.node.is_some() || sys.id.is_remote() {
            return Err(format!("{} runs on another node; its API key lives on that machine.", sys.label));
        }
        if sys.external {
            return Err(format!("{} is an external server; KLIF does not manage its API key.", sys.label));
        }
    }
    let key = e.api_key().ok_or_else(|| "No API key is set.".to_string())?;
    log::info!("copy the API key");
    on_main(&app, move |h| clipboard::copy(h, key.expose(), true)).await
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
