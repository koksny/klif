//! klif-core: the engine. Owns the catalog, supervisor, telemetry and the remote-node clients; runs one session
//! state machine per System (concurrently); publishes a `ViewModel` at 2 Hz; executes `Action`s.
//!
//! Per System:
//!   offline --launch--> starting (process spawned) --log/health--> loading --health ready--> online / busy
//!   online --stop--> stopping --tree gone--> offline (lastSession recorded)
//!   any --process exited unexpectedly / fatal log--> fault (exit code, log tail, failed load step)
//!   fault --dismiss--> offline;  fault/online --restart--> stop + launch the System's active preset
//! External Systems (preset `endpoint`) are only watched; remote Systems (`"<node>/<id>"`) come from `nodes`.
//! Conflicts (port, exclusive GPU, VRAM) are computed every tick; Launch stops them only when asked.
//! Startup: adopt every persisted session; never auto-start anything. One engine per state dir (`engine.lock`);
//! klif-cli and other nodes talk to it over the control protocol (`control`, `wire`, `link`).
//! Persistence (state dir, `state.v3.json`; 0.2's `state.json` is only read once): selected tab, running session
//! records, last sessions, learned model shapes, measured VRAM layers. Never the API key (`keys`) or node tokens.
//! The remote nodes' last-known Systems live in `<data_dir>\node-cache.json` (`nodes`).
//!
//! Layout: `engine` (state machine, tick thread, view model), `state` (state.json), `timefmt`, `narrate` (E1);
//! `wire`, `control`, `link`, `nodes` (E3); `keys`, `bench`, `download` (E2).
//! Frozen API: SPEC section 4 (klif-core).

pub mod bench;
pub mod control;
pub mod download;
mod engine;
pub mod keys;
pub mod link;
pub mod narrate;
pub mod nodes;
mod state;
mod timefmt;
pub mod wire;

use anyhow::Result;
use klif_common::config::{LoadedConfig, PresetCfg};
use klif_common::vm::{Action, CommandView, HostInfo, PresetDetail, SystemId, ViewModel};
use klif_common::Secret;
use std::path::PathBuf;
use std::sync::Arc;

pub use klif_catalog;
pub use klif_common;
pub use klif_supervisor;
pub use klif_telemetry;

/// Why the engine did not start.
pub enum StartError {
    /// Another process (klif-cli, a second KLIF) holds `engine.lock` in this state dir.
    Busy { pid: u32 },
    Other(anyhow::Error),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Busy { pid } => write!(f, "Another KLIF process (pid {pid}) holds the engine."),
            StartError::Other(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::fmt::Debug for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Busy { pid } => f.debug_struct("Busy").field("pid", pid).finish(),
            StartError::Other(e) => f.debug_tuple("Other").field(e).finish(),
        }
    }
}

impl std::error::Error for StartError {}

impl From<anyhow::Error> for StartError {
    fn from(e: anyhow::Error) -> Self {
        StartError::Other(e)
    }
}

pub struct Engine {
    _private: (),
}

/// Thread-safe handle used by the host (Tauri), the control servers and klif-cli.
#[derive(Clone)]
pub struct EngineHandle {
    inner: Arc<engine::Inner>,
}

impl Engine {
    /// Take `engine.lock`, build the catalog, adopt the persisted sessions, start telemetry, the node clients,
    /// the 2 Hz tick thread and the control servers. `Busy` when another process holds the lock.
    pub fn start(loaded: LoadedConfig, host: HostInfo) -> Result<EngineHandle, StartError> {
        let handle = EngineHandle { inner: engine::start(loaded, host)? };
        // control::serve_local always (klif-cli), control::serve_network when `[node] listen` is set (and a node
        // token exists; otherwise an Issue says why it does not listen).
        handle.inner.start_servers();
        Ok(handle)
    }
}

impl EngineHandle {
    /// The latest view model (conveniences from the selected System).
    pub fn snapshot(&self) -> ViewModel {
        self.inner.snapshot()
    }

    /// The latest view model with its conveniences (session / last_session / console / vram) derived from
    /// `focus` instead of the selected System (control protocol `snapshot{focus}`, remote nodes).
    pub fn snapshot_focus(&self, focus: Option<&SystemId>) -> ViewModel {
        self.inner.snapshot_focus(focus)
    }

    /// Register a callback invoked (on the engine thread) with every new view model (2 Hz).
    pub fn subscribe(&self, f: Box<dyn Fn(&ViewModel) + Send + Sync>) {
        self.inner.subscribe(f)
    }

    /// Execute an action. Errors are user-facing sentences (shown as toasts / inline).
    pub fn act(&self, action: Action) -> Result<()> {
        self.inner.act(action)
    }

    /// Update host facts (e.g. maximized) that the view model reports.
    pub fn set_host(&self, host: HostInfo) {
        self.inner.set_host(host)
    }

    /// Stop background threads and servers of the protocol (does NOT stop model servers: they survive KLIF).
    pub fn shutdown(&self) {
        self.inner.shutdown()
    }

    /// The state directory (state.json, engine.lock, control.json, api-key.txt, node-token.txt).
    pub fn state_dir(&self) -> PathBuf {
        self.inner.state_dir()
    }

    /// The data directory (logs, bench, webview-data, downloads).
    pub fn data_dir(&self) -> PathBuf {
        self.inner.data_dir()
    }

    /// The inference card's AMD ULPS setting (EnableUlps in its driver key; read-only registry check).
    pub fn ulps_setting(&self) -> Option<klif_telemetry::UlpsSetting> {
        self.inner.ulps()
    }

    /// The base URL of a System's session (None = the selected System; None when it does not run).
    pub fn endpoint_url(&self, system: Option<&SystemId>) -> Option<String> {
        self.inner.endpoint_url(system)
    }

    /// The API key for "copy API key" (host copies it to the clipboard natively; never sent to the UI).
    pub fn api_key(&self) -> Option<Secret> {
        self.inner.api_key()
    }

    /// One preset in full for the editor (secrets masked); `node` = a remote node's preset. None also when a
    /// remote node could not be asked (logged): prefer [`EngineHandle::try_preset`], which says why.
    pub fn preset(&self, id: &str, node: Option<&str>) -> Option<PresetDetail> {
        self.inner.preset(id, node).unwrap_or_else(|e| {
            log::info!("preset {id}: {e:#}");
            None
        })
    }

    /// One preset in full for the editor (secrets masked); `node` = a remote node's preset. Ok(None): there is no
    /// such preset. Err: a remote node could not be asked, refused, or answered something unreadable (the
    /// sentence says which), so a caller never mistakes an unreachable node for a missing preset.
    pub fn try_preset(&self, id: &str, node: Option<&str>) -> Result<Option<PresetDetail>> {
        self.inner.preset(id, node)
    }

    /// The command an unsaved preset spec would run for `system` (Tune preview), with issues.
    pub fn command_preview(&self, spec: &PresetCfg, system: Option<&SystemId>) -> CommandView {
        self.inner.command_preview(spec, system)
    }

    /// The masked command a System would launch now (`klif-cli plan`, protocol method `plan`).
    pub fn plan(&self, system: &SystemId) -> Result<CommandView> {
        self.inner.plan(system)
    }

    /// The klif.toml in use (None: defaults, no file yet).
    pub fn config_path(&self) -> Option<PathBuf> {
        self.inner.config_path()
    }

    /// Make sure klif.toml exists (creates a starter file) and return its path (Open klif.toml).
    pub fn ensure_config(&self) -> Result<PathBuf> {
        self.inner.ensure_config()
    }

    /// Set or clear the API key (`[security] api_key = "file"` only). The key is never echoed.
    pub fn set_api_key(&self, key: Option<Secret>) -> Result<()> {
        self.inner.set_api_key(key)
    }

    /// Diagnostics for `klif-cli diag` (no secrets).
    pub fn diag(&self) -> serde_json::Value {
        self.inner.diag()
    }

    // ---- extras ---------------------------------------------------------------------------------

    /// Run one tick now (recompute and publish the view model). Tools use it to avoid waiting.
    pub fn refresh(&self) {
        self.inner.tick()
    }
}
