//! The engine klif-cli talks to: the one another process runs (the GUI or `klif-cli serve`), over the local
//! control channel (`ControlClient`), else an in-process engine that holds `engine.lock` while klif-cli runs.
//! Model servers are never stopped when klif-cli exits (they survive KLIF by design; the next engine adopts them).

use crate::out::{note, CliError, CliResult};
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::{HostInfo, HostKind, NodeState, Phase, SystemId, ViewModel};
use klif_core::klif_common::KLIF_VERSION;
use klif_core::link::{ControlClient, EngineLink};
use klif_core::wire::{ControlFile, CONTROL_FILE};
use klif_core::{Engine, EngineHandle, StartError};
use std::time::{Duration, Instant};

/// How long klif-cli keeps trying when the lock changes hands between the check and the start.
const CONNECT_PATIENCE: Duration = Duration::from_secs(10);
/// An in-process engine gets at most this long for its first health results before the first snapshot.
const SETTLE_MAX: Duration = Duration::from_secs(3);
const SETTLE_MIN: Duration = Duration::from_millis(500);

pub enum Conn {
    /// klif-cli runs the engine itself (holding engine.lock until it exits).
    Local(EngineHandle),
    /// Connected to the engine of another process.
    Client { client: Box<ControlClient>, pid: Option<u32> },
}

impl Conn {
    pub fn link(&self) -> &dyn EngineLink {
        match self {
            Conn::Local(h) => h,
            Conn::Client { client, .. } => client.as_ref(),
        }
    }

    /// "in-process" / "pid 1234".
    pub fn describe(&self) -> String {
        match self {
            Conn::Local(_) => "in-process (this klif-cli holds the engine)".into(),
            Conn::Client { pid: Some(p), .. } => format!("the running KLIF engine (pid {p})"),
            Conn::Client { pid: None, .. } => "the running KLIF engine".into(),
        }
    }

    pub fn mode(&self) -> &'static str {
        match self {
            Conn::Local(_) => "in-process",
            Conn::Client { .. } => "connected",
        }
    }

    pub fn engine_pid(&self) -> Option<u32> {
        match self {
            Conn::Local(_) => Some(std::process::id()),
            Conn::Client { pid, .. } => *pid,
        }
    }

    /// A fresh view model (an in-process engine runs one tick first).
    pub fn snapshot(&self, focus: Option<&SystemId>) -> CliResult<ViewModel> {
        if let Conn::Local(h) = self {
            h.refresh();
        }
        self.link().snapshot(focus).map_err(|e| CliError::new("control", format!("The engine did not answer: {e:#}")))
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        if let Conn::Local(h) = self {
            // Stops threads and servers of the protocol; model servers keep running.
            h.shutdown();
        }
    }
}

pub fn host_info() -> HostInfo {
    HostInfo { kind: HostKind::Browser, frameless: false, maximized: false, app_version: KLIF_VERSION.into(), panel: Default::default() }
}

fn control_pid(loaded: &LoadedConfig) -> Option<u32> {
    let bytes = std::fs::read(loaded.cfg.state_path(CONTROL_FILE)).ok()?;
    serde_json::from_slice::<ControlFile>(&bytes).ok().map(|c| c.pid)
}

/// Connect to the running engine, else start one in-process (and let it settle).
pub fn connect(loaded: &LoadedConfig) -> CliResult<Conn> {
    let t0 = Instant::now();
    loop {
        match ControlClient::connect(loaded) {
            Ok(Some(client)) => return Ok(Conn::Client { client: Box::new(client), pid: control_pid(loaded) }),
            Ok(None) => match Engine::start(loaded.clone(), host_info()) {
                Ok(h) => {
                    settle(&h);
                    return Ok(Conn::Local(h));
                }
                Err(StartError::Busy { pid }) => {
                    // Another process took the lock between the check and the start: connect to it instead.
                    if t0.elapsed() > CONNECT_PATIENCE {
                        return Err(CliError::new(
                            "engine_busy",
                            format!("Another KLIF process (pid {pid}) holds the engine and does not answer on the control channel."),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
                Err(StartError::Other(e)) => return Err(CliError::new("engine", format!("The engine could not start: {e:#}"))),
            },
            Err(e) => {
                return Err(CliError::new("control", format!("The running KLIF engine could not be reached: {e:#}")));
            }
        }
    }
}

/// Start an engine for `serve`: refused when one already runs.
pub fn start_owned(loaded: &LoadedConfig) -> CliResult<EngineHandle> {
    match ControlClient::connect(loaded) {
        Ok(Some(_)) => Err(CliError::new(
            "engine_busy",
            match control_pid(loaded) {
                Some(p) => format!("A KLIF engine already runs for this configuration (pid {p})."),
                None => "A KLIF engine already runs for this configuration.".into(),
            },
        )),
        Ok(None) => match Engine::start(loaded.clone(), host_info()) {
            Ok(h) => {
                dev_web_assets(&h);
                Ok(h)
            }
            Err(StartError::Busy { pid }) => Err(CliError::new("engine_busy", format!("A KLIF engine already runs for this configuration (pid {pid})."))),
            Err(StartError::Other(e)) => Err(CliError::new("engine", format!("The engine could not start: {e:#}"))),
        },
        Err(e) => Err(CliError::new("control", format!("The control channel could not be checked: {e:#}"))),
    }
}

/// Debug builds only: `KLIF_WEBUI_DIR=<app/ui/dist>` lets `serve` answer klif-webui from a UI build on disk, so the
/// page can be checked without the window app. Release builds serve no page from klif-cli.
#[cfg(debug_assertions)]
fn dev_web_assets(h: &EngineHandle) {
    let Some(dir) = std::env::var_os("KLIF_WEBUI_DIR").map(std::path::PathBuf::from) else { return };
    note(&format!("note: klif-webui files from {} (debug build)", dir.display()));
    h.set_web_assets(std::sync::Arc::new(move |path: &str| {
        if path.split('/').any(|p| p.is_empty() || p == "..") {
            return None;
        }
        let bytes = std::fs::read(dir.join(path)).ok()?;
        let mime = match path.rsplit('.').next().unwrap_or_default() {
            "html" => "text/html",
            "js" => "text/javascript",
            "css" => "text/css",
            "woff2" => "font/woff2",
            "png" => "image/png",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        };
        Some((bytes, mime.to_string()))
    }));
}

#[cfg(not(debug_assertions))]
fn dev_web_assets(_h: &EngineHandle) {}

/// Give a fresh in-process engine up to 3 s for its first health results: adopted sessions show "starting"
/// until the first probe answers, remote nodes "connecting" until their first hello.
fn settle(h: &EngineHandle) {
    let t0 = Instant::now();
    loop {
        h.refresh();
        let vm = h.snapshot();
        let pending_session = vm
            .systems
            .iter()
            .filter(|s| s.node.is_none())
            .any(|s| s.session.as_ref().is_some_and(|x| matches!(x.phase, Phase::Starting | Phase::Loading)));
        let pending_node = vm.nodes.iter().any(|n| n.state == NodeState::Connecting);
        if t0.elapsed() >= SETTLE_MIN && !pending_session && !pending_node {
            return;
        }
        if t0.elapsed() >= SETTLE_MAX {
            if pending_session {
                note("note: some sessions are still starting or loading.");
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}
