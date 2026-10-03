//! Remote KLIF nodes (`[nodes.<id>]`): one client thread per node — `hello` (schema check -> incompatible), then
//! `snapshot{focus}` at 1 Hz (2 Hz for the focused node), latency measured, reconnect with backoff <= 10 s,
//! states connecting / online / offline / unauthorized / incompatible. The engine merges each node's LOCAL Systems
//! into its view model as `"<node>/<id>"` and forwards actions on them (`act`). The node token is only the HMAC key
//! of each request (`crate::wire`); it never crosses the wire, and API keys never do. Owner: package E3.
//!
//! - Limits (SPEC 16.18): connect timeout 1.5 s; `hello` / `snapshot` 5 s and `act` / `call` 10 s per answer (any
//!   timeout drops the connection and reconnects); answers up to 16 MiB; a view model that does not decode makes
//!   the node `incompatible` with the serde message.
//! - Authenticity: `hello` carries a fresh nonce and the node must answer it with `wire::server_proof` (it holds
//!   the token). Until it has, nothing it says counts: its instance id, name, rights and view are not used, the
//!   instance registry and the node cache are not touched, and the node shows `unauthorized`. The 5 s `hello`
//!   refresh proves it again (a failed proof drops the connection).
//! - Identity (16.19): a node whose (proven) instance id is our own ("This is this machine") or another connected
//!   node's ("Duplicate of ...") is `offline` and contributes no Systems (not even cached ones).
//! - Node cache (16.16): `<data_dir>\node-cache.json` keeps each node's last-known local Systems
//!   ([`CachedSystem`] + `at`), written when that subset changes, at most every 30 s (and on shutdown). It is
//!   display data only (never used for adoption); [`RemoteState::cached`] carries it to the engine.
//! - `act` / `call` block the calling thread until the node answered (<= 10 s): never call them while holding a
//!   lock the tick needs.

use anyhow::{anyhow, bail, Result};
use klif_common::config::{Config, RemoteNodeCfg, TokenRef};
use klif_common::vm::{Action, LlmClass, NodeState, NodeView, Right, SystemKind, ViewModel};
use klif_common::{now_s, Secret};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::wire::{self, code, CallError, Connection, Hello};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
/// The server's challenge must arrive within this time after connecting.
const GREETING_TIMEOUT: Duration = Duration::from_secs(3);
/// `hello` / `snapshot` answer time; a longer silence drops the connection (SPEC 16.9: "> 5 s").
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
/// Forwarded `act` / `call` answer time.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_EVERY: Duration = Duration::from_secs(1);
const POLL_FOCUSED: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(10);
/// `hello` is repeated this often while connected (name / version / rights may change on the node).
const HELLO_EVERY: Duration = Duration::from_secs(5);
/// node-cache.json is written at most this often.
const CACHE_EVERY: Duration = Duration::from_secs(30);
/// How long `shutdown` waits for client threads before leaving them to finish on their own.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// The node cache file in the data dir: per node, its last-known local Systems (display only, never used for
/// adoption).
pub const NODE_CACHE_FILE: &str = "node-cache.json";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One last-known System of a remote node (`<data_dir>\node-cache.json`). Shown with status unreachable, no
/// session, activity 0 and no command while its node is down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedSystem {
    /// The node-local id (without `"<node>/"`).
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub kind: SystemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LlmClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
}

/// What the hub knows about one remote node.
#[derive(Debug, Clone)]
pub struct RemoteState {
    pub view: NodeView,
    /// The node's last view model (its local Systems are in `vm.systems`), while it is online. Served by the node
    /// with LOCAL Systems only, conveniences (session / console) for the focused System only.
    pub vm: Option<ViewModel>,
    /// The node's last-known local Systems (from `node-cache.json`, then from every snapshot); the engine shows
    /// them as unreachable while `vm` is None. Empty for a node refused as "this machine" / a duplicate.
    pub cached: Vec<CachedSystem>,
    /// Epoch seconds of the snapshot `cached` comes from.
    pub cached_at: Option<f64>,
}

// ------------------------------------------------------------------------------------------ cache

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct CachedNode {
    /// Epoch seconds of the snapshot the Systems came from.
    at: f64,
    systems: Vec<CachedSystem>,
}

struct CacheStore {
    path: PathBuf,
    nodes: BTreeMap<String, CachedNode>,
    dirty: bool,
    last_write: Option<Instant>,
}

impl CacheStore {
    /// Read node-cache.json leniently: an unreadable file is ignored, an unreadable System is skipped.
    fn load(data_dir: &Path) -> CacheStore {
        let path = data_dir.join(NODE_CACHE_FILE);
        let mut nodes = BTreeMap::new();
        if let Ok(bytes) = std::fs::read(&path) {
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(v) => {
                    if let Some(map) = v.get("nodes").and_then(Value::as_object) {
                        for (id, n) in map {
                            let at = n.get("at").and_then(Value::as_f64).unwrap_or(0.0);
                            let systems = n
                                .get("systems")
                                .and_then(Value::as_array)
                                .map(|a| a.iter().filter_map(|s| serde_json::from_value::<CachedSystem>(s.clone()).ok()).collect())
                                .unwrap_or_default();
                            nodes.insert(id.clone(), CachedNode { at, systems });
                        }
                    }
                }
                Err(e) => log::warn!("{} is not readable ({e}); remote nodes start without cached Systems", path.display()),
            }
        }
        CacheStore { path, nodes, dirty: false, last_write: None }
    }

    fn get(&self, id: &str) -> (Vec<CachedSystem>, Option<f64>) {
        self.nodes.get(id).map(|n| (n.systems.clone(), Some(n.at))).unwrap_or_default()
    }

    fn update(&mut self, id: &str, systems: &[CachedSystem], at: f64) {
        match self.nodes.get_mut(id) {
            Some(n) if n.systems == systems => n.at = at,
            _ => {
                self.nodes.insert(id.to_string(), CachedNode { at, systems: systems.to_vec() });
                self.dirty = true;
            }
        }
    }

    fn remove(&mut self, id: &str) {
        if self.nodes.remove(id).is_some() {
            self.dirty = true;
        }
    }

    fn retain(&mut self, ids: &[String]) {
        let before = self.nodes.len();
        self.nodes.retain(|k, _| ids.contains(k));
        if self.nodes.len() != before {
            self.dirty = true;
        }
    }

    fn maybe_write(&mut self) {
        if self.dirty && self.last_write.is_none_or(|t| t.elapsed() >= CACHE_EVERY) {
            self.write();
        }
    }

    fn flush(&mut self) {
        if self.dirty {
            self.write();
        }
    }

    fn write(&mut self) {
        self.dirty = false;
        self.last_write = Some(Instant::now());
        let doc = json!({ "version": 1, "nodes": &self.nodes });
        let bytes = serde_json::to_vec_pretty(&doc).unwrap_or_default();
        if let Err(e) = wire::write_atomic(&self.path, &bytes) {
            log::warn!("could not write {}: {e}", self.path.display());
        }
    }
}

/// The local Systems of a node's view model, as cached.
fn cached_subset(vm: &ViewModel) -> Vec<CachedSystem> {
    vm.systems
        .iter()
        .filter(|s| s.node.is_none() && !s.id.is_remote())
        .map(|s| CachedSystem {
            id: s.id.as_str().to_string(),
            label: s.label.clone(),
            kind: s.kind,
            class: s.class,
            preset: s.preset.clone(),
            model_name: Some(s.model.name.trim()).filter(|n| !n.is_empty()).map(str::to_string),
        })
        .collect()
}

// ------------------------------------------------------------------------------------------ hub

/// State shared by the hub and its client threads.
struct HubShared {
    state_dir: PathBuf,
    our_instance: OnceLock<String>,
    /// Who holds each connected instance id: instance id -> owner. A duplicate entry earlier in klif.toml takes
    /// the instance over from a later one (deterministic, whatever connects first).
    instances: Mutex<BTreeMap<String, Owner>>,
    cache: Mutex<CacheStore>,
}

impl HubShared {
    fn our_instance(&self) -> &str {
        self.our_instance.get_or_init(|| wire::instance_id(&self.state_dir))
    }

    /// Drop `client`'s claim (only its own: a replaced client with the same node id never touches its successor's).
    fn unregister(&self, client: &Client) {
        lock(&self.instances).retain(|_, o| !Weak::ptr_eq(&o.client, &client.me));
    }

    /// Register `node` (at file position `order`) as the holder of `instance`; Err(name of the holder) when an
    /// entry earlier in the file holds it.
    fn claim(&self, instance: &str, client: &Client, name: &str) -> Result<(), String> {
        let order = client.order.load(Ordering::SeqCst);
        let displaced = {
            let mut reg = lock(&self.instances);
            let mut displaced = None;
            if let Some(o) = reg.get(instance) {
                if o.node != client.id {
                    if o.order < order {
                        return Err(o.name.clone());
                    }
                    displaced = Some(o.client.clone());
                }
            }
            reg.retain(|_, o| o.node != client.id);
            reg.insert(
                instance.to_string(),
                Owner { node: client.id.clone(), order, name: name.to_string(), client: client.me.clone() },
            );
            displaced
        };
        // The later entry that held the node notices at once (not at its next poll).
        if let Some(c) = displaced.and_then(|w| w.upgrade()) {
            c.wake();
        }
        Ok(())
    }

    /// The holder's name when `node` lost `instance` to an earlier entry.
    fn displaced(&self, instance: &str, node: &str) -> Option<String> {
        lock(&self.instances).get(instance).filter(|o| o.node != node).map(|o| o.name.clone())
    }
}

struct Owner {
    node: String,
    /// Position of the `[nodes.*]` entry in klif.toml.
    order: usize,
    name: String,
    client: Weak<Client>,
}

/// The remote-node clients.
pub struct NodeHub {
    shared: Arc<HubShared>,
    clients: Mutex<Vec<Arc<Client>>>,
    /// The focused (node, node-local System).
    focus: Mutex<Option<(String, String)>>,
}

impl NodeHub {
    /// Start a client per `[nodes.*]` entry (no threads when there are none).
    pub fn start(cfg: &Config) -> NodeHub {
        let hub = NodeHub {
            shared: Arc::new(HubShared {
                state_dir: cfg.state_dir.clone(),
                our_instance: OnceLock::new(),
                instances: Mutex::new(BTreeMap::new()),
                cache: Mutex::new(CacheStore::load(&cfg.data_dir)),
            }),
            clients: Mutex::new(Vec::new()),
            focus: Mutex::new(None),
        };
        hub.reconfigure(cfg);
        hub
    }

    /// Follow a changed `[nodes.*]` list (start / stop / re-point clients). Unchanged entries keep their
    /// connection; the order of `remote()` follows the file.
    pub fn reconfigure(&self, cfg: &Config) {
        let focus = lock(&self.focus).clone();
        let mut clients = lock(&self.clients);
        let mut next: Vec<Arc<Client>> = Vec::with_capacity(cfg.nodes.len());
        for (order, (id, ncfg)) in cfg.nodes.iter().enumerate() {
            if let Some(pos) = clients.iter().position(|c| &c.id == id && same_cfg(&c.cfg, ncfg)) {
                let c = clients.remove(pos);
                c.order.store(order, Ordering::SeqCst);
                next.push(c);
                continue;
            }
            let (cached, cached_at) = lock(&self.shared.cache).get(id);
            let client = Arc::new_cyclic(|me| Client::new(me.clone(), id, ncfg, cached, cached_at));
            client.order.store(order, Ordering::SeqCst);
            client.set_focus(focus.as_ref().filter(|(n, _)| n == id).map(|(_, s)| s.clone()));
            let (c2, s2) = (client.clone(), self.shared.clone());
            match std::thread::Builder::new().name(format!("klif-node-{id}")).spawn(move || run(c2, s2)) {
                Ok(h) => *lock(&client.thread) = Some(h),
                Err(e) => client.go_down(&self.shared, Down::new(NodeState::Offline, format!("Could not start the client thread: {e}."))),
            }
            next.push(client);
        }
        for old in clients.drain(..) {
            old.signal_stop();
        }
        *clients = next;
        let ids: Vec<String> = cfg.nodes.iter().map(|(id, _)| id.clone()).collect();
        lock(&self.shared.cache).retain(&ids);
    }

    /// The System the user looks at on `node` (that node then returns its console / session), None = none.
    /// Focusing one node unfocuses every other one.
    pub fn set_focus(&self, node: &str, system: Option<&str>) {
        let focus = {
            let mut f = lock(&self.focus);
            match system {
                Some(s) => *f = Some((node.to_string(), s.to_string())),
                None => {
                    if f.as_ref().is_some_and(|(n, _)| n == node) {
                        *f = None;
                    }
                }
            }
            f.clone()
        };
        for c in lock(&self.clients).iter() {
            c.set_focus(focus.as_ref().filter(|(n, _)| n == &c.id).map(|(_, s)| s.clone()));
        }
    }

    /// Every configured node with its last state and view model.
    pub fn remote(&self) -> Vec<RemoteState> {
        lock(&self.clients).iter().map(|c| c.remote_state()).collect()
    }

    /// Forward an action (its System id already stripped of `"<node>/"`; stripped again here) to a node. Returns
    /// when the node answered (<= 10 s). Only local-only actions (Select) are refused here; the node checks the
    /// launch / edit rights of every request against its current `[node] allow`.
    pub fn act(&self, node: &str, action: Action) -> Result<()> {
        let c = self.client(node)?;
        let action = action.for_forwarding();
        let (name, state, _) = c.brief();
        // The node decides launch / edit rights per request (its [node] allow may have changed); only what never
        // crosses the network is refused here.
        if action.required_right() == Right::LocalOnly {
            bail!("\"{}\" is local to each machine; it is not sent to {name}.", action.kind());
        }
        let params = serde_json::to_value(&action).map_err(|e| anyhow!("The action could not be encoded: {e}."))?;
        c.request("act", params, CALL_TIMEOUT).map_err(|e| anyhow!(c.sentence(&name, state, &e)))?;
        // Fetch a fresh view right away so the change shows without waiting for the next poll.
        c.wake();
        Ok(())
    }

    /// Call any protocol method on a node (preset, command_preview, plan...). The node decides the rights.
    pub fn call(&self, node: &str, method: &str, params: Value) -> Result<Value> {
        let c = self.client(node)?;
        let (name, state, _) = c.brief();
        c.request(method, params, CALL_TIMEOUT).map_err(|e| anyhow!(c.sentence(&name, state, &e)))
    }

    /// Stop the client threads (waits up to ~2 s) and write a pending node cache.
    pub fn shutdown(&self) {
        let clients = std::mem::take(&mut *lock(&self.clients));
        for c in &clients {
            c.signal_stop();
        }
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        for c in &clients {
            while !c.exited.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if c.exited.load(Ordering::SeqCst) {
                if let Some(h) = lock(&c.thread).take() {
                    let _ = h.join();
                }
            }
        }
        lock(&self.shared.cache).flush();
    }

    fn client(&self, node: &str) -> Result<Arc<Client>> {
        lock(&self.clients).iter().find(|c| c.id == node).cloned().ok_or_else(|| anyhow!("There is no node \"{node}\" in klif.toml."))
    }
}

fn same_cfg(a: &RemoteNodeCfg, b: &RemoteNodeCfg) -> bool {
    a.address.trim() == b.address.trim() && a.token.trim() == b.token.trim() && a.name == b.name
}

// ------------------------------------------------------------------------------------------ client

/// Why a node is not online.
struct Down {
    state: NodeState,
    error: String,
    /// "This is this machine" / duplicate: take no Systems at all (not even cached ones).
    refused: bool,
}

impl Down {
    fn new(state: NodeState, error: impl Into<String>) -> Down {
        Down { state, error: error.into(), refused: false }
    }

    fn refused(error: impl Into<String>) -> Down {
        Down { state: NodeState::Offline, error: error.into(), refused: true }
    }

    fn from_call(e: CallError) -> Down {
        match e {
            CallError::Io(s) => Down::new(NodeState::Offline, s),
            CallError::Protocol(s) => Down::new(NodeState::Incompatible, s),
            CallError::Remote(b) if b.code == code::UNAUTHORIZED => Down::new(
                NodeState::Unauthorized,
                "The node did not accept the token: compare this machine's [nodes] token with node-token.txt on that machine.",
            ),
            CallError::Remote(b) => Down::new(NodeState::Offline, b.message),
        }
    }
}

struct Status {
    view: NodeView,
    vm: Option<ViewModel>,
    cached: Vec<CachedSystem>,
    cached_at: Option<f64>,
}

struct Client {
    /// Itself (for the instance registry).
    me: Weak<Client>,
    id: String,
    cfg: RemoteNodeCfg,
    status: Mutex<Status>,
    conn: Mutex<Option<Connection>>,
    /// A clone of the connection's socket, to unblock a pending read on stop.
    sock: Mutex<Option<TcpStream>>,
    /// The focused System on this node (Some only for the focused node).
    focus: Mutex<Option<String>>,
    /// Position of the entry in klif.toml (duplicate resolution: the earlier entry keeps the node).
    order: AtomicUsize,
    /// The instance id of the node we are connected to.
    instance: Mutex<Option<String>>,
    stop: AtomicBool,
    exited: AtomicBool,
    wake: Mutex<bool>,
    cv: Condvar,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Client {
    fn new(me: Weak<Client>, id: &str, cfg: &RemoteNodeCfg, cached: Vec<CachedSystem>, cached_at: Option<f64>) -> Client {
        let view = NodeView {
            id: id.to_string(),
            name: display_name(cfg, None, id),
            address: cfg.address_with_port(),
            state: NodeState::Connecting,
            version: None,
            latency_ms: None,
            allow: Vec::new(),
            gpus: Vec::new(),
            machine: None,
            presets: Vec::new(),
            error: None,
        };
        Client {
            me,
            id: id.to_string(),
            cfg: cfg.clone(),
            status: Mutex::new(Status { view, vm: None, cached, cached_at }),
            conn: Mutex::new(None),
            sock: Mutex::new(None),
            focus: Mutex::new(None),
            order: AtomicUsize::new(0),
            instance: Mutex::new(None),
            stop: AtomicBool::new(false),
            exited: AtomicBool::new(false),
            wake: Mutex::new(false),
            cv: Condvar::new(),
            thread: Mutex::new(None),
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn signal_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(s) = lock(&self.sock).take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        self.wake();
    }

    fn wake(&self) {
        *lock(&self.wake) = true;
        self.cv.notify_all();
    }

    /// Sleep until `deadline`, a wake-up or stop.
    fn sleep_until(&self, deadline: Instant) {
        let mut w = lock(&self.wake);
        while !*w && !self.stopped() {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            w = self.cv.wait_timeout(w, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
        *w = false;
    }

    fn set_focus(&self, system: Option<String>) {
        let changed = {
            let mut f = lock(&self.focus);
            let changed = *f != system;
            *f = system;
            changed
        };
        if changed {
            self.wake();
        }
    }

    fn remote_state(&self) -> RemoteState {
        let st = lock(&self.status);
        RemoteState { view: st.view.clone(), vm: st.vm.clone(), cached: st.cached.clone(), cached_at: st.cached_at }
    }

    fn brief(&self) -> (String, NodeState, Vec<String>) {
        let st = lock(&self.status);
        (st.view.name.clone(), st.view.state, st.view.allow.clone())
    }

    /// A sentence for a failed forwarded call.
    fn sentence(&self, name: &str, state: NodeState, e: &CallError) -> String {
        match e {
            CallError::Remote(b) => format!("{name}: {}", b.message),
            CallError::Io(_) if state != NodeState::Online => {
                let why = lock(&self.status).view.error.clone().unwrap_or_else(|| "not connected yet".into());
                format!("{name} is not reachable ({}).", why.trim_end_matches('.'))
            }
            CallError::Io(s) => format!("{name} did not answer: {s}"),
            CallError::Protocol(s) => format!("{name}: {s}"),
        }
    }

    fn drop_conn(&self) {
        *lock(&self.conn) = None;
        if let Some(s) = lock(&self.sock).take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }

    /// One request on the shared connection. A broken connection is dropped (the client thread reconnects).
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, CallError> {
        self.with_conn(|conn| conn.call(method, params, timeout))
    }

    /// `hello` with a fresh nonce; the node must prove it holds the token (else the connection is dropped and the
    /// next round reconnects, where `establish` says why).
    fn proven_hello(&self) -> Result<Hello, CallError> {
        let cn = wire::random_hex(16).map_err(|e| CallError::Io(format!("{e:#}")))?;
        self.with_conn(|conn| {
            let v = conn.call("hello", json!({ "nonce": &cn }), POLL_TIMEOUT)?;
            let hello: Hello =
                serde_json::from_value(v).map_err(|e| CallError::Protocol(format!("The node's greeting could not be read ({e}).")))?;
            if !conn.verify_hello(&cn, &hello) {
                return Err(CallError::Protocol("The node did not prove it holds the node token.".into()));
            }
            Ok(hello)
        })
    }

    /// Run `f` on the shared connection. A broken connection is dropped (the client thread reconnects).
    fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T, CallError>) -> Result<T, CallError> {
        let mut g = lock(&self.conn);
        let Some(conn) = g.as_mut() else { return Err(CallError::Io("not connected".into())) };
        let r = f(conn);
        let broken = match &r {
            Ok(_) => false,
            Err(CallError::Io(_) | CallError::Protocol(_)) => true,
            // The server closes the connection after these.
            Err(CallError::Remote(b)) => {
                [code::UNAUTHORIZED, code::BAD_ID, code::BAD_REQUEST, code::TOO_LARGE, code::BUSY].contains(&b.code.as_str())
            }
        };
        if broken {
            *g = None;
            drop(g);
            if let Some(s) = lock(&self.sock).take() {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            self.wake();
        }
        r
    }

    /// Connect, `hello` (the node proves it holds the token), then check the instance id.
    fn establish(&self, hub: &HubShared) -> Result<(), Down> {
        let token = remote_token(&self.cfg, &hub.state_dir).map_err(|e| Down::new(NodeState::Unauthorized, e))?;
        let addr = self.cfg.address_with_port();
        let addrs = wire::resolve(&addr).map_err(|e| Down::new(NodeState::Offline, e.to_string()))?;
        let mut conn = Connection::open(&addrs, token, CONNECT_TIMEOUT, GREETING_TIMEOUT).map_err(Down::from_call)?;
        if self.stopped() {
            return Err(Down::new(NodeState::Offline, "Stopped."));
        }
        *lock(&self.sock) = conn.shutdown_handle();
        let cn = wire::random_hex(16).map_err(|e| Down::new(NodeState::Offline, format!("{e:#}")))?;
        let hello = conn.call("hello", json!({ "nonce": &cn }), POLL_TIMEOUT).map_err(Down::from_call)?;
        let hello: Hello = serde_json::from_value(hello)
            .map_err(|e| Down::new(NodeState::Incompatible, format!("The node's greeting could not be read ({e}).")))?;
        if hello.schema_version != wire::SCHEMA_VERSION {
            return Err(Down::new(
                NodeState::Incompatible,
                format!(
                    "That node runs KLIF {} (protocol {}); this KLIF speaks protocol {}. Use the same KLIF version on both machines.",
                    hello.klif_version,
                    hello.schema_version,
                    wire::SCHEMA_VERSION
                ),
            ));
        }
        // Nothing the node says counts before it proved it holds the token: a listener that does not know it (another
        // machine on the node's old address) must not show its Systems, claim rights, or take an instance id over
        // (its entry is not marked refused, so the instance registry and the node cache stay untouched).
        if !conn.verify_hello(&cn, &hello) {
            return Err(Down::new(
                NodeState::Unauthorized,
                format!(
                    "{} did not prove it holds the node token; it may not be your node (check [nodes.{}] address and token).",
                    self.cfg.address_with_port(),
                    self.id
                ),
            ));
        }
        let instance = conn.instance_id().to_string();
        if instance == hub.our_instance() {
            return Err(Down::refused(format!("This is this machine: [nodes.{}] points at this KLIF itself.", self.id)));
        }
        let name = display_name(&self.cfg, Some(&hello.node_name), &self.id);
        if let Err(holder) = hub.claim(&instance, self, &name) {
            return Err(duplicate(&holder));
        }
        *lock(&self.instance) = Some(instance);
        self.note_hello(hello);
        *lock(&self.conn) = Some(conn);
        Ok(())
    }

    /// Name, version and rights from a `hello` (also refreshed every few seconds: `[node] allow` may change).
    fn note_hello(&self, hello: Hello) {
        let mut st = lock(&self.status);
        st.view.name = display_name(&self.cfg, Some(&hello.node_name), &self.id);
        st.view.version = Some(hello.klif_version);
        st.view.allow = hello.allow;
    }

    /// An entry earlier in klif.toml took over the KLIF this client is connected to: go down as its duplicate.
    fn lost_instance(&self, hub: &HubShared) -> bool {
        let instance = lock(&self.instance).clone();
        let Some(holder) = instance.and_then(|i| hub.displaced(&i, &self.id)) else { return false };
        self.drop_conn();
        *lock(&self.instance) = None;
        self.go_down(hub, duplicate(&holder));
        true
    }

    fn go_online(&self, hub: &HubShared, vm: ViewModel, latency: Duration) {
        let subset = cached_subset(&vm);
        let at = now_s();
        {
            let mut st = lock(&self.status);
            st.view.state = NodeState::Online;
            st.view.error = None;
            st.view.latency_ms = Some((latency.as_secs_f64() * 10_000.0).round() / 10.0);
            st.view.gpus = vm.gpus.clone();
            st.view.machine = Some(vm.machine.clone());
            st.view.presets = vm
                .presets
                .iter()
                .cloned()
                .map(|mut p| {
                    p.node = Some(self.id.clone());
                    p
                })
                .collect();
            st.cached = subset.clone();
            st.cached_at = Some(at);
            st.vm = Some(vm);
        }
        if !self.stopped() {
            lock(&hub.cache).update(&self.id, &subset, at);
        }
    }

    fn go_down(&self, hub: &HubShared, down: Down) {
        hub.unregister(self);
        {
            let mut st = lock(&self.status);
            st.view.state = down.state;
            st.view.error = Some(down.error);
            st.view.latency_ms = None;
            st.view.gpus.clear();
            st.view.machine = None;
            st.view.presets.clear();
            st.vm = None;
            if down.refused {
                st.cached.clear();
                st.cached_at = None;
                st.view.allow.clear();
            }
        }
        if down.refused && !self.stopped() {
            // Rare (a misconfiguration): written at once so a displaced duplicate never lingers in the file.
            let mut cache = lock(&hub.cache);
            cache.remove(&self.id);
            cache.flush();
        }
    }
}

fn duplicate(holder: &str) -> Down {
    Down::refused(format!("Duplicate of {holder}: both entries point at the same KLIF."))
}

fn display_name(cfg: &RemoteNodeCfg, reported: Option<&str>, id: &str) -> String {
    [cfg.name.as_deref(), reported]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| id.to_string())
}

/// The token for a `[nodes.X]` entry (`file:` relative to the state dir, or `env:`), read on every connect.
fn remote_token(cfg: &RemoteNodeCfg, state_dir: &Path) -> Result<Secret, String> {
    match cfg.token_ref()?.resolve(state_dir) {
        TokenRef::File(p) => match std::fs::read_to_string(&p) {
            Ok(s) => Secret::new(s).ok_or_else(|| format!("The token file {} is empty.", p.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!(
                "The token file {} does not exist: put the node's token there (`klif-cli node token --create` on that machine prints it).",
                p.display()
            )),
            Err(e) => Err(format!("The token file {} could not be read: {e}.", p.display())),
        },
        TokenRef::Env(n) => {
            std::env::var(&n).ok().and_then(Secret::new).ok_or_else(|| format!("The environment variable {n} (the node's token) is not set."))
        }
    }
}

fn next_backoff(b: Duration) -> Duration {
    if b.is_zero() { Duration::from_millis(500) } else { (b * 2).min(MAX_BACKOFF) }
}

/// The client thread of one node.
fn run(c: Arc<Client>, hub: Arc<HubShared>) {
    let mut backoff = Duration::ZERO;
    // Consecutive transport failures while connected (one immediate reconnect before going offline).
    let mut fails = 0u32;
    let mut hello_at = Instant::now();
    while !c.stopped() {
        if lock(&c.conn).is_none() {
            hello_at = Instant::now();
            if let Err(down) = c.establish(&hub) {
                c.drop_conn();
                fails = 0;
                if c.stopped() {
                    break;
                }
                c.go_down(&hub, down);
                lock(&hub.cache).maybe_write();
                backoff = next_backoff(backoff);
                c.sleep_until(Instant::now() + backoff);
                continue;
            }
        }
        if c.lost_instance(&hub) {
            backoff = MAX_BACKOFF;
            c.sleep_until(Instant::now() + backoff);
            continue;
        }
        let t0 = Instant::now();
        let focus = lock(&c.focus).clone();
        let params = match &focus {
            Some(f) => json!({ "focus": f }),
            None => json!({}),
        };
        match c.request("snapshot", params, POLL_TIMEOUT) {
            Ok(v) => match serde_json::from_value::<ViewModel>(v) {
                Ok(vm) => {
                    if c.lost_instance(&hub) {
                        backoff = MAX_BACKOFF;
                        c.sleep_until(Instant::now() + backoff);
                        continue;
                    }
                    c.go_online(&hub, vm, t0.elapsed());
                    backoff = Duration::ZERO;
                    fails = 0;
                }
                Err(e) => {
                    c.drop_conn();
                    if c.stopped() {
                        break;
                    }
                    c.go_down(&hub, Down::new(NodeState::Incompatible, format!("The node's view could not be read ({e}). Use the same KLIF version on both machines.")));
                    backoff = next_backoff(backoff);
                    c.sleep_until(Instant::now() + backoff);
                    continue;
                }
            },
            Err(e) => {
                let transport = matches!(e, CallError::Io(_));
                fails += 1;
                if transport && fails < 2 && !c.stopped() {
                    // Reconnect once right away before showing the node offline.
                    c.drop_conn();
                    continue;
                }
                c.drop_conn();
                if c.stopped() {
                    break;
                }
                c.go_down(&hub, Down::from_call(e));
                backoff = next_backoff(backoff);
                c.sleep_until(Instant::now() + backoff);
                continue;
            }
        }
        if hello_at.elapsed() >= HELLO_EVERY {
            hello_at = Instant::now();
            // Proven again each time (a failed proof drops the connection; `establish` then says why).
            match c.proven_hello() {
                Ok(h) if h.schema_version == wire::SCHEMA_VERSION => c.note_hello(h),
                Ok(_) => {}
                Err(e) => log::debug!("node {}: hello refresh failed: {e}", c.id),
            }
        }
        lock(&hub.cache).maybe_write();
        let every = if focus.is_some() { POLL_FOCUSED } else { POLL_EVERY };
        c.sleep_until(t0 + every);
    }
    c.drop_conn();
    hub.unregister(&c);
    c.exited.store(true, Ordering::SeqCst);
}

/// This machine's node token: `<state_dir>\node-token.txt` (32 random bytes hex), created by
/// `klif-cli node token --create`, which prints it once.
pub mod node_token {
    use anyhow::{Context, Result};
    use klif_common::config::Config;
    use klif_common::Secret;

    /// The token file's name in the state dir.
    pub const FILE: &str = "node-token.txt";
    /// Shortest token accepted from the file (a created token has 64 hex characters).
    pub const MIN_LEN: usize = 32;

    /// A token from the file's text: trimmed, at least [`MIN_LEN`] characters, no inner whitespace.
    pub fn parse(text: String) -> Option<Secret> {
        let mut bytes = text.into_bytes();
        let out = std::str::from_utf8(&bytes)
            .ok()
            .map(str::trim)
            .filter(|t| t.len() >= MIN_LEN && !t.chars().any(char::is_whitespace))
            .and_then(Secret::new);
        bytes.iter_mut().for_each(|b| *b = 0);
        out
    }

    /// The token, if one exists.
    pub fn load(cfg: &Config) -> Option<Secret> {
        std::fs::read_to_string(cfg.state_path(FILE)).ok().and_then(parse)
    }

    /// Create (or replace) the token and return it. Replacing it disconnects every peer at its next request.
    pub fn create(cfg: &Config) -> Result<Secret> {
        let path = cfg.state_path(FILE);
        let hex = crate::wire::random_hex(32)?;
        let mut text = format!("{hex}\n");
        let written = crate::wire::write_atomic(&path, text.as_bytes()).with_context(|| format!("{} could not be written", path.display()));
        // SAFETY: zero bytes are valid UTF-8; the strings are not used afterwards.
        unsafe {
            text.as_bytes_mut().iter_mut().for_each(|b| *b = 0);
        }
        let secret = Secret::new(hex);
        written?;
        secret.context("empty token")
    }
}
