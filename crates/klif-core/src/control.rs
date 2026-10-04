//! The engine's control servers (`crate::wire` protocol, challenge + HMAC auth):
//! - `serve_local`: TCP 127.0.0.1:0, token = 32 random bytes hex (getrandom), full rights; writes
//!   `control.json {pid, port, token, version}` atomically after binding and deletes it on shutdown (klif-cli).
//! - `serve_network`: only with `[node] listen` and a node token; rights per `[node] allow`, decided per request
//!   with `klif_common::vm::Right` (default-deny; Select and diag never over the network). Hardening (SPEC 16.9):
//!   first line within 3 s, <= 4 unauthenticated connections (<= 2 per peer IP), <= 8 authenticated, idle timeout
//!   30 s, SO_KEEPALIVE 10 s, failed auth = deferred close (never a sleep on the accept loop).
//!
//! Every connection: the server sends a [`wire::Challenge`]; each request must carry a valid mac for that nonce and
//! an id greater than the previous one. `hello` answers the client's `nonce` with [`wire::server_proof`] (required
//! on the network listener), so the client knows it talks to a holder of the token before it trusts anything. A
//! request that fails the mac or the id is answered and closed ~1 s later by the reaper thread (the connection keeps
//! its unauthenticated slot until then). Network peers only see this machine's LOCAL
//! Systems: `snapshot` / `status` drop remote Systems, nodes, recommendations, downloads and config paths; actions,
//! `plan`, `preset` and `command_preview` naming another node (or the selected tab) are refused, so a node never
//! acts on its own nodes for a peer. Owner: package E3.

use anyhow::{bail, Context, Result};
use klif_common::config::{Config, NodeRight, FILE_NAME};
use klif_common::vm::{Action, ConfigInfo, Right, SystemId, ViewModel};
use klif_common::{Secret, KLIF_VERSION};
use serde_json::Value;
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::nodes::node_token;
use crate::wire::{self, code, Challenge, ControlFile, Hello, LineReader, ReadError, Request, Response};
use crate::EngineHandle;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The methods the protocol knows (anything else: `unknown_method`). `bench` (klif-cli's bench window for the
/// records) is local only, like every method `Right::for_method` does not name.
const METHODS: [&str; 10] =
    ["hello", "snapshot", "status", "act", "plan", "preset", "command_preview", "diag", "bench", "records_history"];

/// Connection limits and timeouts of one server.
#[derive(Debug, Clone, Copy)]
struct Limits {
    max_unauth: usize,
    max_unauth_per_ip: usize,
    max_auth: usize,
    /// The first complete line must arrive within this time after connecting.
    first_line: Duration,
    /// An authenticated connection without a request for this long is closed.
    idle: Duration,
    keepalive: Option<Duration>,
    /// Failed auth: answer + close after this delay (reaper thread).
    fail_delay: Duration,
}

const NETWORK_LIMITS: Limits = Limits {
    max_unauth: 4,
    max_unauth_per_ip: 2,
    max_auth: 8,
    first_line: Duration::from_secs(3),
    idle: Duration::from_secs(30),
    keepalive: Some(Duration::from_secs(10)),
    fail_delay: Duration::from_secs(1),
};

/// Loopback only, one user's tools (klif-cli, agents): roomier, same auth.
const LOCAL_LIMITS: Limits = Limits {
    max_unauth: 8,
    max_unauth_per_ip: 8,
    max_auth: 32,
    first_line: Duration::from_secs(3),
    idle: Duration::from_secs(300),
    keepalive: None,
    fail_delay: Duration::from_secs(1),
};

// ------------------------------------------------------------------------------------------ servers

/// The local control server (stops on `shutdown`).
pub struct ControlServer {
    port: u16,
    server: Server,
    control_path: PathBuf,
}

impl ControlServer {
    /// The bound port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stop accepting, close connections, delete control.json.
    pub fn shutdown(&self) {
        if !self.server.stop() {
            return;
        }
        // Delete control.json only while it still describes this server.
        let ours = std::fs::read(&self.control_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<ControlFile>(&b).ok())
            .is_some_and(|c| c.pid == std::process::id() && c.port == self.port);
        if ours {
            if let Err(e) = std::fs::remove_file(&self.control_path) {
                log::warn!("could not delete {}: {e}", self.control_path.display());
            }
        }
    }
}

/// The network listener of this node (stops on `shutdown`).
pub struct NetworkServer {
    addr: String,
    server: Server,
}

impl NetworkServer {
    /// The bound address, e.g. "0.0.0.0:7340".
    pub fn addr(&self) -> &str {
        &self.addr
    }

    pub fn shutdown(&self) {
        self.server.stop();
    }
}

/// Serve `handle` on 127.0.0.1 and publish `control.json` in `state_dir`.
pub fn serve_local(handle: EngineHandle, state_dir: &Path) -> Result<ControlServer> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).context("The local control server could not listen on 127.0.0.1")?;
    let port = listener.local_addr()?.port();
    let token_hex = wire::random_hex(32)?;
    let token = Secret::new(token_hex.clone()).context("empty control token")?;
    let control_path = state_dir.join(wire::CONTROL_FILE);
    let file = ControlFile { pid: std::process::id(), port, token: token_hex, version: KLIF_VERSION.to_string() };
    let mut bytes = serde_json::to_vec_pretty(&file)?;
    drop(file);
    let written = wire::write_atomic(&control_path, &bytes);
    bytes.iter_mut().for_each(|b| *b = 0);
    written.with_context(|| format!("{} could not be written", control_path.display()))?;
    let gate = Gate::Local { token, name: computer_name() };
    let server = match Server::start(listener, handle, gate, LOCAL_LIMITS, state_dir, "klif-control") {
        Ok(s) => s,
        Err(e) => {
            let _ = std::fs::remove_file(&control_path);
            return Err(e);
        }
    };
    log::info!("control server on 127.0.0.1:{port}");
    Ok(ControlServer { port, server, control_path })
}

/// Serve `handle` to other nodes on `[node] listen` with `auth`.
pub fn serve_network(handle: EngineHandle, auth: Arc<NodeAuth>) -> Result<NetworkServer> {
    let Some(addr) = auth.listen_addr() else {
        bail!("[node] listen is not set; this machine does not listen for other nodes.")
    };
    let listener = TcpListener::bind(addr.as_str()).with_context(|| format!("Could not listen on {addr} for other nodes"))?;
    let bound = listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| addr.clone());
    let state_dir = auth.state_dir();
    let server = Server::start(listener, handle, Gate::Network(auth.clone()), NETWORK_LIMITS, &state_dir, "klif-node")?;
    // NodeAuth re-reads [node] allow and node-token.txt every second.
    let stop = server.shared.stop_flag();
    let refresher = std::thread::Builder::new().name("klif-node-auth".into()).spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(1));
            auth.refresh_from_disk();
        }
    });
    if let Err(e) = refresher {
        server.stop();
        bail!("could not start the node auth thread: {e}");
    }
    log::info!("node listener on {bound}");
    Ok(NetworkServer { addr: bound, server })
}

// ------------------------------------------------------------------------------------------ auth

/// Who may do what on the network listener: `[node] allow` + `node-token.txt`, re-read (mtime) every second.
pub struct NodeAuth {
    st: Mutex<AuthState>,
}

struct AuthState {
    allow: Vec<NodeRight>,
    name: String,
    /// The address the listener binds (fixed at load).
    listen: Option<String>,
    /// `[node] listen` is still configured (cleared when it disappears from klif.toml: every request is refused).
    enabled: bool,
    token: Option<Secret>,
    token_path: PathBuf,
    token_stamp: Option<(SystemTime, u64)>,
    cfg_path: Option<PathBuf>,
    cfg_stamp: Option<(SystemTime, u64)>,
    state_dir: PathBuf,
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// This computer's name (the default `[node] name`).
pub fn computer_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "KLIF node".into())
}

impl NodeAuth {
    /// From the config (`[node]`, `<state_dir>\node-token.txt`). Err (a sentence) when there is no token file:
    /// the listener is then not started and the engine reports an Issue.
    pub fn load(cfg: &Config) -> Result<NodeAuth> {
        let Some(node) = cfg.node.as_ref() else { bail!("[node] is not configured; this machine does not listen for other nodes.") };
        let Some(listen) = node.listen_addr() else { bail!("[node] listen is not set; this machine does not listen for other nodes.") };
        let token_path = cfg.state_path(node_token::FILE);
        let token_stamp = stamp(&token_path);
        if token_stamp.is_none() {
            bail!(
                "[node] listen is set but there is no node token yet, so the listener stays off: run `klif-cli node token --create` on this machine (it prints the token once)."
            );
        }
        let Some(token) = node_token::load(cfg) else {
            bail!(
                "{} is not a usable node token (at least {} characters), so the listener stays off: run `klif-cli node token --create`.",
                node_token::FILE,
                node_token::MIN_LEN
            );
        };
        let cfg_path = cfg.source.clone().or_else(|| Some(cfg.state_dir.join(FILE_NAME)));
        let cfg_stamp = cfg_path.as_deref().and_then(stamp);
        Ok(NodeAuth {
            st: Mutex::new(AuthState {
                allow: node.allow.clone(),
                name: node_name(cfg),
                listen: Some(listen),
                enabled: true,
                token: Some(token),
                token_path,
                token_stamp,
                cfg_path,
                cfg_stamp,
                state_dir: cfg.state_dir.clone(),
            }),
        })
    }

    /// Re-read `[node] allow` and the token file when they changed (called every second).
    pub fn refresh(&self, cfg: &Config) {
        let mut st = lock(&self.st);
        apply_cfg(&mut st, cfg);
        reload_token(&mut st);
    }

    /// Re-read klif.toml (`[node]`, when its mtime / size changed; a file that does not parse keeps the last
    /// values) and node-token.txt (mtime / size; a missing or unusable file revokes every peer). The network
    /// server calls this every second.
    pub fn refresh_from_disk(&self) {
        let cfg_path = lock(&self.st).cfg_path.clone();
        if let Some(path) = cfg_path {
            let now = stamp(&path);
            let changed = lock(&self.st).cfg_stamp != now;
            if changed {
                let loaded = klif_common::config::load_from(&path);
                let mut st = lock(&self.st);
                st.cfg_stamp = now;
                if loaded.unreadable() {
                    log::warn!("klif.toml does not parse; the node listener keeps its last rights");
                } else {
                    apply_cfg(&mut st, &loaded.cfg);
                }
            }
        }
        reload_token(&mut lock(&self.st));
    }

    /// Whether a peer may use `right` now (nothing at all while `[node] listen` is gone or the token is missing).
    pub fn allows(&self, right: Right) -> bool {
        let st = lock(&self.st);
        st.enabled && st.token.is_some() && right.granted(&st.allow)
    }

    /// [`wire::server_proof`] with the current token (None while `[node] listen` is gone or there is no token).
    pub fn server_proof(&self, client_nonce: &str, server_nonce: &str, instance_id: &str) -> Option<String> {
        let token = {
            let st = lock(&self.st);
            if !st.enabled {
                return None;
            }
            st.token.clone()
        }?;
        Some(wire::server_proof(token.expose(), client_nonce, server_nonce, instance_id))
    }

    /// Constant-time check of a request mac against the current token.
    pub fn verify(&self, nonce: &str, id: u64, method: &str, params: &serde_json::Value, mac: &str) -> bool {
        let token = {
            let st = lock(&self.st);
            if !st.enabled {
                return false;
            }
            st.token.clone()
        };
        let Some(token) = token else { return false };
        wire::ct_eq(&wire::request_mac(token.expose(), nonce, id, method, params), mac)
    }

    /// The rights besides view, as `Hello.allow` strings ("launch", "edit"; "launch" is listed when "edit" is).
    pub fn allow_list(&self) -> Vec<String> {
        let st = lock(&self.st);
        if !st.enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        if Right::Launch.granted(&st.allow) {
            out.push(NodeRight::Launch.as_str().to_string());
        }
        if Right::Edit.granted(&st.allow) {
            out.push(NodeRight::Edit.as_str().to_string());
        }
        out
    }

    /// The address `[node] listen` named at load (with the default port).
    pub fn listen_addr(&self) -> Option<String> {
        lock(&self.st).listen.clone()
    }

    /// `[node] name`, else the computer name.
    pub fn node_name(&self) -> String {
        lock(&self.st).name.clone()
    }

    /// False once `[node] listen` disappeared from klif.toml (the engine should then stop the listener).
    pub fn enabled(&self) -> bool {
        lock(&self.st).enabled
    }

    fn state_dir(&self) -> PathBuf {
        lock(&self.st).state_dir.clone()
    }
}

fn node_name(cfg: &Config) -> String {
    cfg.node.as_ref().and_then(|n| n.name.as_deref()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(computer_name)
}

fn apply_cfg(st: &mut AuthState, cfg: &Config) {
    let node = cfg.node.as_ref();
    st.enabled = node.and_then(|n| n.listen_addr()).is_some();
    st.allow = node.map(|n| n.allow.clone()).unwrap_or_default();
    st.name = node_name(cfg);
}

fn reload_token(st: &mut AuthState) {
    let now = stamp(&st.token_path);
    if now == st.token_stamp {
        return;
    }
    st.token_stamp = now;
    st.token = std::fs::read_to_string(&st.token_path).ok().and_then(node_token::parse);
    if st.token.is_none() {
        log::warn!("{} is missing or unusable: every node peer is refused until it is recreated", node_token::FILE);
    } else {
        log::info!("{} changed: peers must use the new token", node_token::FILE);
    }
}

/// Who checks requests on a server.
enum Gate {
    /// The local control server: the control.json token, every right.
    Local { token: Secret, name: String },
    /// The network listener.
    Network(Arc<NodeAuth>),
}

impl Gate {
    fn is_network(&self) -> bool {
        matches!(self, Gate::Network(_))
    }

    fn verify(&self, nonce: &str, req: &Request) -> bool {
        match self {
            Gate::Local { token, .. } => {
                wire::ct_eq(&wire::request_mac(token.expose(), nonce, req.id, &req.method, &req.params), &req.mac)
            }
            Gate::Network(a) => a.verify(nonce, req.id, &req.method, &req.params, &req.mac),
        }
    }

    fn allows(&self, right: Right) -> bool {
        match self {
            Gate::Local { .. } => true,
            Gate::Network(a) => a.allows(right),
        }
    }

    /// The `hello` proof for a client nonce, with the token the request mac was just checked against.
    fn server_proof(&self, client_nonce: &str, server_nonce: &str, instance_id: &str) -> Option<String> {
        match self {
            Gate::Local { token, .. } => Some(wire::server_proof(token.expose(), client_nonce, server_nonce, instance_id)),
            Gate::Network(a) => a.server_proof(client_nonce, server_nonce, instance_id),
        }
    }

    fn allow_list(&self) -> Vec<String> {
        match self {
            Gate::Local { .. } => vec![NodeRight::Launch.as_str().into(), NodeRight::Edit.as_str().into()],
            Gate::Network(a) => a.allow_list(),
        }
    }

    fn node_name(&self) -> String {
        match self {
            Gate::Local { name, .. } => name.clone(),
            Gate::Network(a) => a.node_name(),
        }
    }

    fn open(&self) -> bool {
        match self {
            Gate::Local { .. } => true,
            Gate::Network(a) => a.enabled(),
        }
    }
}

// ------------------------------------------------------------------------------------------ server core

struct Counts {
    unauth: usize,
    per_ip: BTreeMap<IpAddr, usize>,
    auth: usize,
}

struct Shared {
    handle: EngineHandle,
    gate: Gate,
    limits: Limits,
    instance_id: String,
    stop: Arc<AtomicBool>,
    counts: Mutex<Counts>,
    /// Socket clones of open connections (closed on stop).
    conns: Mutex<BTreeMap<u64, TcpStream>>,
    next_conn: AtomicU64,
    reaper: Mutex<Sender<Reap>>,
    name: &'static str,
}

impl Shared {
    fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Admit a new connection as unauthenticated (None: over the limits; the caller drops it).
    fn admit(self: &Arc<Self>, ip: IpAddr, stream: &TcpStream) -> Option<Slot> {
        {
            let mut c = lock(&self.counts);
            let on_ip = c.per_ip.get(&ip).copied().unwrap_or(0);
            if c.unauth >= self.limits.max_unauth || on_ip >= self.limits.max_unauth_per_ip {
                return None;
            }
            c.unauth += 1;
            *c.per_ip.entry(ip).or_insert(0) += 1;
        }
        let id = self.next_conn.fetch_add(1, Ordering::SeqCst);
        if let Ok(clone) = stream.try_clone() {
            lock(&self.conns).insert(id, clone);
        }
        Some(Slot { shared: self.clone(), ip, authed: false, id })
    }
}

/// A connection's place in the counts (released on drop).
struct Slot {
    shared: Arc<Shared>,
    ip: IpAddr,
    authed: bool,
    id: u64,
}

impl Slot {
    /// Move from unauthenticated to authenticated; false when the authenticated cap is reached.
    fn promote(&mut self) -> bool {
        if self.authed {
            return true;
        }
        let mut c = lock(&self.shared.counts);
        if c.auth >= self.shared.limits.max_auth {
            return false;
        }
        c.auth += 1;
        release_unauth(&mut c, self.ip);
        self.authed = true;
        true
    }
}

fn release_unauth(c: &mut Counts, ip: IpAddr) {
    c.unauth = c.unauth.saturating_sub(1);
    if let Some(n) = c.per_ip.get_mut(&ip) {
        *n = n.saturating_sub(1);
        if *n == 0 {
            c.per_ip.remove(&ip);
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        {
            let mut c = lock(&self.shared.counts);
            if self.authed {
                c.auth = c.auth.saturating_sub(1);
            } else {
                release_unauth(&mut c, self.ip);
            }
        }
        if let Some(s) = lock(&self.shared.conns).remove(&self.id) {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// A deferred close: answer (if any) and close at `at`; the slot is held until then.
enum Reap {
    Close { at: Instant, stream: TcpStream, answer: Option<Vec<u8>>, slot: Slot },
    Stop,
}

fn finish(stream: TcpStream, answer: Option<Vec<u8>>, slot: Slot) {
    use std::io::Write;
    let mut stream = stream;
    if let Some(a) = answer {
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = stream.write_all(&a);
        let _ = stream.flush();
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    drop(slot);
}

fn reaper(rx: mpsc::Receiver<Reap>) {
    let mut pending: Vec<(Instant, TcpStream, Option<Vec<u8>>, Slot)> = Vec::new();
    loop {
        let wait = pending
            .iter()
            .map(|p| p.0)
            .min()
            .map(|t| t.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));
        match rx.recv_timeout(wait) {
            Ok(Reap::Close { at, stream, answer, slot }) => pending.push((at, stream, answer, slot)),
            Ok(Reap::Stop) | Err(RecvTimeoutError::Disconnected) => {
                for (_, s, a, slot) in pending.drain(..) {
                    finish(s, a, slot);
                }
                return;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        let mut i = 0;
        while i < pending.len() {
            if pending[i].0 <= now {
                let (_, s, a, slot) = pending.swap_remove(i);
                finish(s, a, slot);
            } else {
                i += 1;
            }
        }
    }
}

/// One listening server (accept thread + a thread per connection + the reaper).
struct Server {
    shared: Arc<Shared>,
    wake_addr: SocketAddr,
    accept: Mutex<Option<JoinHandle<()>>>,
}

impl Server {
    fn start(listener: TcpListener, handle: EngineHandle, gate: Gate, limits: Limits, state_dir: &Path, name: &'static str) -> Result<Server> {
        let local = listener.local_addr()?;
        let wake_ip = match local.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
            ip => ip,
        };
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new().name(format!("{name}-reaper")).spawn(move || reaper(rx)).context("could not start a server thread")?;
        let shared = Arc::new(Shared {
            handle,
            gate,
            limits,
            instance_id: wire::instance_id(state_dir),
            stop: Arc::new(AtomicBool::new(false)),
            counts: Mutex::new(Counts { unauth: 0, per_ip: BTreeMap::new(), auth: 0 }),
            conns: Mutex::new(BTreeMap::new()),
            next_conn: AtomicU64::new(1),
            reaper: Mutex::new(tx),
            name,
        });
        let s2 = shared.clone();
        let accept = std::thread::Builder::new().name(format!("{name}-accept")).spawn(move || accept_loop(listener, s2));
        let accept = match accept {
            Ok(h) => h,
            Err(e) => {
                let _ = lock(&shared.reaper).send(Reap::Stop);
                bail!("could not start a server thread: {e}");
            }
        };
        Ok(Server { shared, wake_addr: SocketAddr::new(wake_ip, local.port()), accept: Mutex::new(Some(accept)) })
    }

    /// Stop everything; false when it was already stopped.
    fn stop(&self) -> bool {
        if self.shared.stop.swap(true, Ordering::SeqCst) {
            return false;
        }
        // Unblock accept() with a connection of our own.
        let _ = TcpStream::connect_timeout(&self.wake_addr, Duration::from_millis(500));
        if let Some(h) = lock(&self.accept).take() {
            let _ = h.join();
        }
        for (_, s) in std::mem::take(&mut *lock(&self.shared.conns)) {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        let _ = lock(&self.shared.reaper).send(Reap::Stop);
        log::info!("{} stopped", self.shared.name);
        true
    }
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut errors = 0u32;
    loop {
        let accepted = listener.accept();
        if shared.stopped() {
            return;
        }
        match accepted {
            Ok((stream, peer)) => {
                errors = 0;
                let Some(slot) = shared.admit(peer.ip(), &stream) else {
                    // Over the unauthenticated limits: close at once (no thread, no answer).
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    continue;
                };
                let s2 = shared.clone();
                let spawned = std::thread::Builder::new()
                    .name(format!("{}-conn", shared.name))
                    .spawn(move || serve_connection(s2, stream, slot));
                if let Err(e) = spawned {
                    log::warn!("{}: could not start a connection thread: {e}", shared.name);
                }
            }
            Err(e) => {
                // Persistent accept errors (out of handles...): do not spin.
                errors += 1;
                if errors % 100 == 1 {
                    log::warn!("{}: accept failed: {e}", shared.name);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// Answer once, then hand the connection to the reaper (closed after the failed-auth delay).
fn defer_close(shared: &Shared, stream: TcpStream, resp: Response, slot: Slot) {
    let mut answer = serde_json::to_vec(&resp).unwrap_or_default();
    answer.push(b'\n');
    let at = Instant::now() + shared.limits.fail_delay;
    if let Err(mpsc::SendError(Reap::Close { stream, answer, slot, .. })) =
        lock(&shared.reaper).send(Reap::Close { at, stream, answer: Some(answer), slot })
    {
        finish(stream, answer, slot);
    }
}

fn serve_connection(shared: Arc<Shared>, stream: TcpStream, mut slot: Slot) {
    let started = Instant::now();
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    if let Some(k) = shared.limits.keepalive {
        keepalive::enable(&stream, k);
    }
    let (Ok(mut writer), Ok(reaper_handle)) = (stream.try_clone(), stream.try_clone()) else { return };
    let Ok(nonce) = wire::random_hex(16) else { return };
    let challenge = Challenge { nonce: nonce.clone(), instance_id: shared.instance_id.clone() };
    if wire::write_line(&mut writer, &challenge).is_err() {
        return;
    }
    let mut reader = LineReader::new(stream);
    let mut last_id = 0u64;
    loop {
        if shared.stopped() {
            return;
        }
        let deadline = if slot.authed { Instant::now() + shared.limits.idle } else { started + shared.limits.first_line };
        let line = match reader.read_line(deadline, wire::MAX_LINE_BYTES) {
            Ok(l) => l,
            Err(ReadError::TooLong) => {
                let resp = Response::failure(0, code::TOO_LARGE, "The request exceeds 1 MiB.");
                return defer_close(&shared, reaper_handle, resp, slot);
            }
            // Timeout (first line / idle), EOF, IO error: close now.
            Err(_) => return,
        };
        if line.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        let req: Request = match serde_json::from_slice(&line) {
            Ok(r) => r,
            Err(_) => {
                let resp = Response::failure(0, code::BAD_REQUEST, "The line is not a request.");
                return defer_close(&shared, reaper_handle, resp, slot);
            }
        };
        if !shared.gate.verify(&nonce, &req) {
            let resp = Response::failure(req.id, code::UNAUTHORIZED, "The token was not accepted.");
            return defer_close(&shared, reaper_handle, resp, slot);
        }
        if req.id <= last_id {
            let resp = Response::failure(req.id, code::BAD_ID, "Request ids must increase on a connection.");
            return defer_close(&shared, reaper_handle, resp, slot);
        }
        last_id = req.id;
        if !slot.promote() {
            let resp = Response::failure(req.id, code::BUSY, "Too many connections; try again later.");
            let _ = wire::write_line(&mut writer, &resp);
            return;
        }
        if !shared.gate.open() {
            let resp = Response::failure(req.id, code::FORBIDDEN, "This machine no longer accepts other nodes.");
            let _ = wire::write_line(&mut writer, &resp);
            return;
        }
        let resp = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(&shared, &req, &nonce))) {
            Ok(r) => r,
            Err(_) => {
                log::error!("{}: method {} panicked", shared.name, req.method);
                Response::failure(req.id, code::REFUSED, "The engine failed while answering; see the KLIF log.")
            }
        };
        if wire::write_line(&mut writer, &resp).is_err() {
            return;
        }
    }
}

// ------------------------------------------------------------------------------------------ dispatch

fn forbidden_sentence(right: Right, method: &str, action: Option<&Action>) -> String {
    match right {
        Right::Launch => {
            "This node does not allow launching or stopping from other machines (add \"launch\" to [node] allow on that machine).".into()
        }
        Right::Edit => {
            "This node does not allow editing from other machines (add \"edit\" to [node] allow on that machine; it means arbitrary commands run there).".into()
        }
        Right::View => "This node does not accept other machines right now.".into(),
        Right::LocalOnly => match action {
            Some(Action::Select { .. }) => "Selecting a tab is local to each machine.".into(),
            _ => format!("\"{method}\" is only available on this machine."),
        },
    }
}

fn bad(id: u64, msg: impl Into<String>) -> Response {
    Response::failure(id, code::BAD_PARAMS, msg)
}

/// A local System id from params (network peers may not name another node's Systems).
fn param_system(req: &Request, key: &str, net: bool) -> Result<Option<SystemId>, Response> {
    match req.params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if net && s.contains('/') => {
            Err(Response::failure(req.id, code::FORBIDDEN, "Systems of other machines are not served over the network."))
        }
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(SystemId::new(s.trim()))),
        Some(_) => Err(bad(req.id, format!("\"{key}\" must be a System id."))),
    }
}

fn to_value<T: serde::Serialize>(id: u64, v: &T) -> Response {
    match serde_json::to_value(v) {
        Ok(v) => Response::success(id, v),
        Err(e) => Response::failure(id, code::REFUSED, format!("The answer could not be encoded: {e}.")),
    }
}

fn refused(id: u64, e: anyhow::Error) -> Response {
    Response::failure(id, code::REFUSED, format!("{e:#}"))
}

/// Actions whose `system` may be absent (= the selected tab): a network peer must name the System.
fn needs_named_system(a: &Action) -> bool {
    matches!(
        a,
        Action::Launch { system: None, .. }
            | Action::Stop { system: None }
            | Action::Restart { system: None }
            | Action::Dismiss { system: None }
            | Action::AdoptRecommendation { system: None, .. }
    )
}

/// `nonce` = this connection's challenge nonce (bound into the `hello` proof).
fn dispatch(shared: &Shared, req: &Request, nonce: &str) -> Response {
    let id = req.id;
    let net = shared.gate.is_network();
    let method = req.method.as_str();
    if !METHODS.contains(&method) {
        return Response::failure(id, code::UNKNOWN_METHOD, format!("Unknown method \"{method}\"."));
    }
    let action = if method == "act" {
        match serde_json::from_value::<Action>(req.params.clone()) {
            Ok(a) => Some(a),
            Err(e) => return bad(id, format!("The action could not be read: {e}.")),
        }
    } else {
        None
    };
    let right = action.as_ref().map(Action::required_right).unwrap_or_else(|| Right::for_method(method));
    if !shared.gate.allows(right) {
        return Response::failure(id, code::FORBIDDEN, forbidden_sentence(right, method, action.as_ref()));
    }
    let h = &shared.handle;
    match method {
        "hello" => {
            // The client's nonce: the answer proves this server holds the token (wire::server_proof). Required on
            // the network: a hub must never take a listener that does not know the token for its node.
            let client_nonce = match req.params.get("nonce") {
                None | Some(Value::Null) => None,
                Some(Value::String(n)) if wire::is_hex_id(n, 32) => Some(n.as_str()),
                Some(_) => return bad(id, "\"nonce\" must be 16 random bytes as hex (32 characters)."),
            };
            if net && client_nonce.is_none() {
                return bad(id, "hello needs {\"nonce\": 16 random bytes hex}; use the same KLIF version on both machines.");
            }
            let proof = client_nonce.and_then(|cn| shared.gate.server_proof(cn, nonce, &shared.instance_id)).unwrap_or_default();
            to_value(
                id,
                &Hello {
                    schema_version: wire::SCHEMA_VERSION,
                    klif_version: KLIF_VERSION.to_string(),
                    node_name: shared.gate.node_name(),
                    allow: shared.gate.allow_list(),
                    instance_id: shared.instance_id.clone(),
                    proof,
                },
            )
        }
        "snapshot" => {
            let focus = match param_system(req, "focus", net) {
                Ok(f) => f,
                Err(r) => return r,
            };
            let vm = h.snapshot_focus(focus.as_ref());
            if net {
                // The records travel only when they changed since the revision the hub holds.
                let have = req.params.get("recordsRev").and_then(Value::as_u64);
                to_value(id, &network_view(vm, focus.as_ref(), have))
            } else {
                to_value(id, &vm)
            }
        }
        "status" => {
            let vm = h.snapshot();
            let vm = if net { network_view(vm, None, None) } else { vm };
            to_value(id, &wire::status_of(&vm))
        }
        "act" => {
            let Some(action) = action else { return bad(id, "Missing action.") };
            if net {
                match action.route() {
                    Ok(None) => {}
                    Ok(Some(_)) => {
                        return Response::failure(id, code::FORBIDDEN, "Actions are not forwarded to other machines from here.")
                    }
                    Err(e) => return bad(id, e),
                }
                if needs_named_system(&action) {
                    return bad(id, "Name the System: other machines cannot act on the selected tab.");
                }
            }
            match h.act(action) {
                Ok(()) => Response::success(id, Value::Null),
                Err(e) => refused(id, e),
            }
        }
        "plan" => match param_system(req, "system", net) {
            Ok(Some(system)) => match h.plan(&system) {
                Ok(c) => to_value(id, &c),
                Err(e) => refused(id, e),
            },
            Ok(None) => bad(id, "plan needs {\"system\": id}."),
            Err(r) => r,
        },
        "preset" => {
            let Some(pid) = req.params.get("id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else {
                return bad(id, "preset needs {\"id\": preset id}.");
            };
            let node = match req.params.get("node") {
                None | Some(Value::Null) => None,
                Some(Value::String(n)) if !n.trim().is_empty() => Some(n.trim().to_string()),
                Some(_) => return bad(id, "\"node\" must be a node id."),
            };
            if net && node.is_some() {
                return Response::failure(id, code::FORBIDDEN, "Presets of other machines are not served over the network.");
            }
            match h.try_preset(pid, node.as_deref()) {
                Ok(d) => to_value(id, &d),
                Err(e) => refused(id, e),
            }
        }
        "command_preview" => {
            let spec = match req.params.get("spec").cloned().map(serde_json::from_value::<klif_common::config::PresetCfg>) {
                Some(Ok(s)) => s,
                Some(Err(e)) => return bad(id, format!("The preset could not be read: {e}.")),
                None => return bad(id, "command_preview needs {\"spec\": preset}."),
            };
            match param_system(req, "system", net) {
                Ok(system) => to_value(id, &h.command_preview(&spec, system.as_ref())),
                Err(r) => r,
            }
        }
        "diag" => Response::success(id, h.diag()),
        "records_history" => {
            let Some(key) = req.params.get("key").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else {
                return bad(id, "records_history needs {\"key\": record key, \"metric\"?: metric}.");
            };
            let metric = match req.params.get("metric") {
                None | Some(Value::Null) => None,
                Some(v) => match serde_json::from_value::<klif_common::vm::RecordMetric>(v.clone()) {
                    Ok(m) => Some(m),
                    Err(_) => return bad(id, "\"metric\" must be a record metric (decodeTps, prefillTps, ttftS, imageS, ttsRtf, sttRtf, videoS, musicRtf)."),
                },
            };
            // A network peer sees this machine's records only; the local channel may name a node's key.
            let points = if net { h.records_history_local(key, metric) } else { h.records_history(key, metric) };
            match points {
                Ok(p) => to_value(id, &p),
                Err(e) => refused(id, e),
            }
        }
        "bench" => {
            let system = match param_system(req, "system", net) {
                Ok(Some(s)) => s,
                Ok(None) => return bad(id, "bench needs {\"system\": id, \"phase\": \"start\" | \"end\"}."),
                Err(r) => return r,
            };
            let start = match req.params.get("phase").and_then(Value::as_str) {
                Some("start") => true,
                Some("end") => false,
                _ => return bad(id, "\"phase\" must be \"start\" or \"end\"."),
            };
            let record = match req.params.get("record") {
                None | Some(Value::Null) => None,
                Some(v) => match serde_json::from_value::<crate::bench::BenchRecord>(v.clone()) {
                    Ok(r) => Some(r),
                    Err(e) => return bad(id, format!("The bench result could not be read: {e}.")),
                },
            };
            match h.bench_mark(&system, start, record.as_ref()) {
                Ok(()) => Response::success(id, Value::Null),
                Err(e) => refused(id, e),
            }
        }
        _ => Response::failure(id, code::UNKNOWN_METHOD, format!("Unknown method \"{method}\".")),
    }
}

/// What a network peer sees (SPEC 16.14): this machine's LOCAL Systems only, no nodes / recommendations /
/// downloads, config blanked (no paths), and the session / console conveniences only for a focused local System.
/// Records: this machine's only, and none at all when the peer already holds `records_rev` (`have`).
fn network_view(mut vm: ViewModel, focus: Option<&SystemId>, have: Option<u64>) -> ViewModel {
    vm.records.retain(|r| r.node.is_none());
    let local: std::collections::BTreeSet<&str> = vm.records.iter().map(|r| r.key.as_str()).collect();
    vm.record_events.retain(|e| local.contains(e.key.as_str()));
    if vm.records_rev != 0 && have == Some(vm.records_rev) {
        vm.records.clear();
        vm.record_events.clear();
    }
    vm.systems.retain(|s| s.node.is_none() && !s.id.is_remote());
    for s in &mut vm.systems {
        s.conflicts.retain(|c| !c.is_remote());
    }
    if vm.selected.as_ref().is_some_and(|s| s.is_remote() || !vm.systems.iter().any(|x| &x.id == s)) {
        vm.selected = None;
    }
    let focused_local = focus.is_some_and(|f| vm.systems.iter().any(|s| &s.id == f));
    let session_local = vm.session.as_ref().is_none_or(|s| !s.system.is_remote())
        && vm.last_session.as_ref().is_none_or(|s| !s.system.is_remote());
    if !focused_local || !session_local {
        vm.session = None;
        vm.last_session = None;
        vm.console.clear();
    }
    vm.nodes.clear();
    vm.recommendations.clear();
    vm.downloads.clear();
    vm.config = ConfigInfo { on_conflict: vm.config.on_conflict, ..ConfigInfo::default() };
    for p in &mut vm.presets {
        p.node = None;
    }
    vm
}

// ------------------------------------------------------------------------------------------ keepalive

/// SO_KEEPALIVE with a 10 s idle time / interval (std has no API; plain setsockopt, no new crate).
mod keepalive {
    use std::net::TcpStream;
    use std::time::Duration;

    #[cfg(windows)]
    pub fn enable(stream: &TcpStream, every: Duration) {
        use std::os::windows::io::AsRawSocket;
        #[link(name = "ws2_32")]
        extern "system" {
            fn setsockopt(s: usize, level: i32, optname: i32, optval: *const u8, optlen: i32) -> i32;
        }
        const SOL_SOCKET: i32 = 0xffff;
        const SO_KEEPALIVE: i32 = 0x0008;
        const IPPROTO_TCP: i32 = 6;
        const TCP_KEEPIDLE: i32 = 3;
        const TCP_KEEPCNT: i32 = 16;
        const TCP_KEEPINTVL: i32 = 17;
        let sock = stream.as_raw_socket() as usize;
        let on: u32 = 1;
        let secs: u32 = every.as_secs().clamp(1, 3600) as u32;
        let count: u32 = 3;
        // SAFETY: valid socket handle owned by `stream`; each option value is a 4-byte DWORD that outlives the call.
        unsafe {
            setsockopt(sock, SOL_SOCKET, SO_KEEPALIVE, (&on as *const u32).cast(), 4);
            setsockopt(sock, IPPROTO_TCP, TCP_KEEPIDLE, (&secs as *const u32).cast(), 4);
            setsockopt(sock, IPPROTO_TCP, TCP_KEEPINTVL, (&secs as *const u32).cast(), 4);
            setsockopt(sock, IPPROTO_TCP, TCP_KEEPCNT, (&count as *const u32).cast(), 4);
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn enable(stream: &TcpStream, every: Duration) {
        use std::os::fd::AsRawFd;
        extern "C" {
            fn setsockopt(fd: i32, level: i32, name: i32, val: *const u8, len: u32) -> i32;
        }
        #[cfg(target_os = "linux")]
        const OPTS: (i32, i32, i32, i32) = (1, 9, 4, 5); // SOL_SOCKET, SO_KEEPALIVE, TCP_KEEPIDLE, TCP_KEEPINTVL
        #[cfg(target_os = "macos")]
        const OPTS: (i32, i32, i32, i32) = (0xffff, 0x0008, 0x10, 0x101); // ..., TCP_KEEPALIVE, TCP_KEEPINTVL
        const IPPROTO_TCP: i32 = 6;
        let fd = stream.as_raw_fd();
        let on: i32 = 1;
        let secs: i32 = every.as_secs().clamp(1, 3600) as i32;
        // SAFETY: valid fd owned by `stream`; each option value is a 4-byte int that outlives the call.
        unsafe {
            setsockopt(fd, OPTS.0, OPTS.1, (&on as *const i32).cast(), 4);
            setsockopt(fd, IPPROTO_TCP, OPTS.2, (&secs as *const i32).cast(), 4);
            setsockopt(fd, IPPROTO_TCP, OPTS.3, (&secs as *const i32).cast(), 4);
        }
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    pub fn enable(_stream: &TcpStream, _every: Duration) {}
}
