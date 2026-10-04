//! One interface to an engine, in-process (`EngineHandle`) or in another process over the local control channel
//! (`ControlClient`), so every klif-cli command is written once. Owner: package E3.

use anyhow::{anyhow, bail, Result};
use klif_common::config::{Config, LoadedConfig, PresetCfg};
use klif_common::vm::{Action, CommandView, PresetDetail, RecordEvent, RecordMetric, SystemId, ViewModel};
use klif_common::Secret;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::bench::BenchRecord;
use crate::wire::{self, CallError, Connection, ControlFile, Hello};
use crate::EngineHandle;

/// What klif-cli needs from an engine.
pub trait EngineLink {
    /// The view model with its conveniences (session / console / vram) derived from `focus` (None = selected).
    fn snapshot(&self, focus: Option<&SystemId>) -> Result<ViewModel>;
    fn act(&self, action: Action) -> Result<()>;
    /// The masked command a System would launch now.
    fn plan(&self, system: &SystemId) -> Result<CommandView>;
    /// A preset in full (masked); `node` = a remote node's preset.
    fn preset(&self, id: &str, node: Option<&str>) -> Result<Option<PresetDetail>>;
    fn command_preview(&self, spec: &PresetCfg, system: Option<&SystemId>) -> Result<CommandView>;
    /// The climb of a record key (a key of `ViewModel.records`), oldest first; a node's key is asked of that node.
    fn records_history(&self, key: &str, metric: Option<RecordMetric>) -> Result<Vec<RecordEvent>>;
    fn diag(&self) -> Result<Value>;
    /// The API key (read locally; never sent over the control channel).
    fn api_key(&self) -> Option<Secret>;
    fn state_dir(&self) -> PathBuf;
    fn data_dir(&self) -> PathBuf;
    fn config_path(&self) -> Option<PathBuf>;
    /// Open (`start`) or close a bench window on a local System for the records; the end carries the result.
    fn bench_mark(&self, system: &SystemId, start: bool, record: Option<&BenchRecord>) -> Result<()>;
}

impl EngineLink for EngineHandle {
    fn snapshot(&self, focus: Option<&SystemId>) -> Result<ViewModel> {
        Ok(EngineHandle::snapshot_focus(self, focus))
    }
    fn act(&self, action: Action) -> Result<()> {
        EngineHandle::act(self, action)
    }
    fn plan(&self, system: &SystemId) -> Result<CommandView> {
        EngineHandle::plan(self, system)
    }
    fn preset(&self, id: &str, node: Option<&str>) -> Result<Option<PresetDetail>> {
        EngineHandle::try_preset(self, id, node)
    }
    fn command_preview(&self, spec: &PresetCfg, system: Option<&SystemId>) -> Result<CommandView> {
        Ok(EngineHandle::command_preview(self, spec, system))
    }
    fn records_history(&self, key: &str, metric: Option<RecordMetric>) -> Result<Vec<RecordEvent>> {
        EngineHandle::records_history(self, key, metric)
    }
    fn diag(&self) -> Result<Value> {
        Ok(EngineHandle::diag(self))
    }
    fn api_key(&self) -> Option<Secret> {
        EngineHandle::api_key(self)
    }
    fn state_dir(&self) -> PathBuf {
        EngineHandle::state_dir(self)
    }
    fn data_dir(&self) -> PathBuf {
        EngineHandle::data_dir(self)
    }
    fn config_path(&self) -> Option<PathBuf> {
        EngineHandle::config_path(self)
    }
    fn bench_mark(&self, system: &SystemId, start: bool, record: Option<&BenchRecord>) -> Result<()> {
        EngineHandle::bench_mark(self, system, start, record)
    }
}

/// How long `connect` waits for control.json / a reachable engine.
const CONNECT_WAIT: Duration = Duration::from_secs(5);
/// Answer time for `act` (a launch with `stop_others` waits for the stopped Systems to exit).
const ACT_TIMEOUT: Duration = Duration::from_secs(120);
/// Answer time for every other method.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Reconnect before a request when the connection was idle this long (the server closes idle ones).
const MAX_IDLE: Duration = Duration::from_secs(60);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A connection to the engine another process runs on this machine (the GUI, or `klif-cli serve`).
pub struct ControlClient {
    cfg: Config,
    conn: Mutex<Option<Connection>>,
    /// The engine's KLIF version (from control.json / hello).
    version: String,
    pid: u32,
}

impl ControlClient {
    /// `try_lock` engine.lock: success = no engine runs (Ok(None); the caller runs an in-process engine while
    /// holding the lock); failure = read control.json (retry ~5 s while missing / pid dead) and connect.
    pub fn connect(loaded: &LoadedConfig) -> Result<Option<Self>> {
        let cfg = loaded.cfg.clone();
        if !engine_running(&cfg)? {
            return Ok(None);
        }
        let deadline = Instant::now() + CONNECT_WAIT;
        loop {
            match open(&cfg) {
                Ok((conn, file, hello)) => {
                    return Ok(Some(ControlClient { cfg, conn: Mutex::new(Some(conn)), version: hello.klif_version, pid: file.pid }));
                }
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(e);
                    }
                    // The engine may have exited meanwhile: then run in-process after all.
                    if !engine_running(&cfg)? {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(150));
                }
            }
        }
    }

    /// The engine's KLIF version.
    pub fn engine_version(&self) -> &str {
        &self.version
    }

    /// The engine's process id (from control.json).
    pub fn engine_pid(&self) -> u32 {
        self.pid
    }

    /// One request; a stale or broken connection is reopened once (`act` is never re-sent after a broken
    /// connection: it may have been executed).
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let timeout = if method == "act" { ACT_TIMEOUT } else { CALL_TIMEOUT };
        let mut g = lock(&self.conn);
        for attempt in 0..2 {
            if g.as_ref().is_none_or(|c| c.idle_for() >= MAX_IDLE) {
                *g = Some(open(&self.cfg)?.0);
            }
            let conn = g.as_mut().expect("connection just opened");
            match conn.call(method, params.clone(), timeout) {
                Ok(v) => return Ok(v),
                Err(CallError::Remote(e)) => bail!("{}", e.message),
                Err(CallError::Io(e)) if attempt == 0 && method != "act" => {
                    log::debug!("control connection broke ({e}); reconnecting");
                    *g = None;
                }
                Err(e) => {
                    *g = None;
                    bail!("The KLIF engine did not answer: {e}");
                }
            }
        }
        bail!("The KLIF engine did not answer.")
    }

    fn call_as<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T> {
        let v = self.call(method, params)?;
        serde_json::from_value(v).map_err(|e| anyhow!("The engine's answer to \"{method}\" could not be read ({e}); is it the same KLIF version?"))
    }
}

/// Whether another process holds engine.lock (an engine runs for this state dir).
fn engine_running(cfg: &Config) -> Result<bool> {
    let path = cfg.state_path(wire::LOCK_FILE);
    if let Err(e) = std::fs::create_dir_all(&cfg.state_dir) {
        bail!("The state folder {} could not be created: {e}.", cfg.state_dir.display());
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| anyhow!("{} could not be opened: {e}.", path.display()))?;
    match f.try_lock() {
        // Nobody holds it: no engine runs. Dropping `f` releases our lock again.
        Ok(()) => Ok(false),
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(e)) => bail!("{} could not be locked: {e}.", path.display()),
    }
}

/// Read control.json, connect, `hello`.
fn open(cfg: &Config) -> Result<(Connection, ControlFile, Hello)> {
    let path = cfg.state_path(wire::CONTROL_FILE);
    let bytes = std::fs::read(&path).map_err(|_| {
        anyhow!("Another KLIF process holds the engine but has not published {} yet; try again in a moment.", wire::CONTROL_FILE)
    })?;
    let file: ControlFile = serde_json::from_slice(&bytes).map_err(|e| anyhow!("{} could not be read: {e}.", path.display()))?;
    let token = Secret::new(file.token.clone()).ok_or_else(|| anyhow!("{} holds no token.", path.display()))?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, file.port));
    let pid = file.pid;
    let mut conn = Connection::open(&[addr], token, Duration::from_millis(1500), Duration::from_secs(3)).map_err(|e| {
        let alive = klif_supervisor::image_name(pid).is_some();
        if alive {
            anyhow!("The KLIF engine (pid {pid}) does not answer on its control channel: {e}")
        } else {
            anyhow!("{} names pid {pid}, which is not running ({e}).", wire::CONTROL_FILE)
        }
    })?;
    let cn = wire::random_hex(16)?;
    let hello: Hello = match conn.call("hello", json!({ "nonce": &cn }), CALL_TIMEOUT) {
        Ok(v) => serde_json::from_value(v).map_err(|e| anyhow!("The engine's greeting could not be read: {e}."))?,
        Err(e) => bail!("The KLIF engine (pid {pid}) refused the control connection: {e}"),
    };
    if hello.schema_version != wire::SCHEMA_VERSION {
        bail!(
            "The running KLIF {} speaks control protocol {}; this klif-cli speaks {}. Use matching versions.",
            hello.klif_version,
            hello.schema_version,
            wire::SCHEMA_VERSION
        );
    }
    // Whatever answers on that port must hold control.json's token (a stale file can name a port another program
    // took over since).
    if !conn.verify_hello(&cn, &hello) {
        bail!(
            "The program on 127.0.0.1:{} did not prove it holds the token in {}; it is not the KLIF engine (pid {pid}).",
            file.port,
            wire::CONTROL_FILE
        );
    }
    Ok((conn, file, hello))
}

impl EngineLink for ControlClient {
    fn snapshot(&self, focus: Option<&SystemId>) -> Result<ViewModel> {
        let params = match focus {
            Some(f) => json!({ "focus": f }),
            None => json!({}),
        };
        self.call_as("snapshot", params)
    }
    fn act(&self, action: Action) -> Result<()> {
        let params = serde_json::to_value(&action).map_err(|e| anyhow!("The action could not be encoded: {e}."))?;
        self.call("act", params).map(|_| ())
    }
    fn plan(&self, system: &SystemId) -> Result<CommandView> {
        self.call_as("plan", json!({ "system": system }))
    }
    fn preset(&self, id: &str, node: Option<&str>) -> Result<Option<PresetDetail>> {
        let params = match node {
            Some(n) => json!({ "id": id, "node": n }),
            None => json!({ "id": id }),
        };
        self.call_as("preset", params)
    }
    fn command_preview(&self, spec: &PresetCfg, system: Option<&SystemId>) -> Result<CommandView> {
        let spec = serde_json::to_value(spec).map_err(|e| anyhow!("The preset could not be encoded: {e}."))?;
        let params = match system {
            Some(s) => json!({ "spec": spec, "system": s }),
            None => json!({ "spec": spec }),
        };
        self.call_as("command_preview", params)
    }
    fn records_history(&self, key: &str, metric: Option<RecordMetric>) -> Result<Vec<RecordEvent>> {
        self.call_as("records_history", json!({ "key": key, "metric": metric }))
    }
    fn diag(&self) -> Result<Value> {
        self.call("diag", json!({}))
    }
    fn api_key(&self) -> Option<Secret> {
        crate::keys::load(&self.cfg)
    }
    fn state_dir(&self) -> PathBuf {
        self.cfg.state_dir.clone()
    }
    fn data_dir(&self) -> PathBuf {
        self.cfg.data_dir.clone()
    }
    fn config_path(&self) -> Option<PathBuf> {
        self.cfg.source.clone()
    }
    fn bench_mark(&self, system: &SystemId, start: bool, record: Option<&BenchRecord>) -> Result<()> {
        let params = json!({ "system": system, "phase": if start { "start" } else { "end" }, "record": record });
        self.call("bench", params).map(|_| ())
    }
}
