//! The control protocol's wire format: newline-delimited JSON over TCP. On connect the server sends one
//! [`Challenge`] line; then one `Request` per line, one `Response` per line (requests <= 1 MiB, responses <= 16 MiB
//! on the client). Every request carries `mac = hex(HMAC-SHA256(token, nonce || id || method ||
//! canonical-params-json))` instead of the token; ids strictly increase per connection; the token never crosses
//! the wire (payloads are cleartext until TLS). Served locally (127.0.0.1, token in control.json, full rights) and,
//! when `[node] listen` is set, on the network (node token, rights per `[node] allow`, see `klif_common::vm::Right`).
//! Spoken by klif-cli (`link::ControlClient`) and other KLIF nodes (`nodes::NodeHub`).
//! Methods: `hello`, `snapshot` {focus?, recordsRev?}, `status`, `act` (params = the Action itself), `plan` {system},
//! `preset` {id, node?}, `command_preview` {spec, system?}, `diag`, `bench` {system, phase, record?} (local only),
//! `records_history` {key, metric?} (the broken records of one record key from `records-history.jsonl`, oldest
//! first, as `RecordEvent`s; view right; a network peer gets this machine's keys only).
//! A network `snapshot` that names the `recordsRev` the caller holds comes without records (unchanged). Owner:
//! package E3.
//!
//! MAC input (exact bytes): `nonce "\n" id "\n" method "\n" canonical-params`, where `id` is decimal and
//! `canonical-params` is [`canonical_json`] of the params (object keys sorted, no whitespace; absent params =
//! `null`). The key is the token's UTF-8 bytes. HMAC-SHA256 is implemented here over `sha2`.
//!
//! The server proves it holds the token too (protocol 2): `hello` carries `{"nonce": <16 random bytes hex>}` from
//! the client (required on the network listener) and the answer's `proof` is
//! `hex(HMAC-SHA256(token, "klif-server\n" client-nonce "\n" server-nonce "\n" instance-id))` ([`server_proof`]).
//! A client trusts nothing the server says (instance id, name, rights, view) before the proof checks out, so a
//! listener that does not know the token (another machine on a node's old address) cannot pose as that node. An
//! on-path relay can still read and alter cleartext payloads: that needs TLS (future work).

use klif_common::secret::MASK;
use klif_common::vm::{LlmClass, NodeState, SystemId, SystemKind, SystemStatus, ViewModel};
use klif_common::Secret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The protocol version (`Hello.schema_version`); a node with another version is "incompatible". 2: the server
/// proves it holds the token in `hello` (`Hello.proof`).
pub const SCHEMA_VERSION: u32 = 2;
/// `engine.lock` in the state dir: held (File::try_lock) by the one running engine for its lifetime.
pub const LOCK_FILE: &str = "engine.lock";
/// `control.json` in the state dir: written atomically after the local server binds, deleted on shutdown.
pub const CONTROL_FILE: &str = "control.json";
/// `<state_dir>\instance-id`: 16 random bytes hex identifying this KLIF installation (`Hello.instance_id`).
pub const INSTANCE_ID_FILE: &str = "instance-id";
/// Longest accepted request line (server side).
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Longest accepted response line (client side).
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Error codes of [`ErrorBody::code`].
pub mod code {
    /// The line is not a request.
    pub const BAD_REQUEST: &str = "bad_request";
    /// The mac does not match the token (or the token was rotated). The server closes the connection ~1 s later.
    pub const UNAUTHORIZED: &str = "unauthorized";
    /// The id is not greater than the previous one on this connection.
    pub const BAD_ID: &str = "bad_id";
    /// The connection may not use this method / action (rights, or local-only).
    pub const FORBIDDEN: &str = "forbidden";
    pub const UNKNOWN_METHOD: &str = "unknown_method";
    /// The params do not fit the method.
    pub const BAD_PARAMS: &str = "bad_params";
    /// The engine refused (the message is its sentence).
    pub const REFUSED: &str = "refused";
    /// The request line exceeds [`super::MAX_LINE_BYTES`].
    pub const TOO_LARGE: &str = "too_large";
    /// Too many connections.
    pub const BUSY: &str = "busy";
}

/// The first line a server sends on every connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Challenge {
    /// 16 random bytes hex, fresh per connection.
    pub nonce: String,
    pub instance_id: String,
}

/// `control.json` (Debug masks the token).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlFile {
    pub pid: u32,
    pub port: u16,
    /// 32 random bytes, hex.
    pub token: String,
    /// The engine's KLIF version.
    pub version: String,
}

impl fmt::Debug for ControlFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControlFile")
            .field("pid", &self.pid)
            .field("port", &self.port)
            .field("token", &MASK)
            .field("version", &self.version)
            .finish()
    }
}

/// The answer to `hello`: who the server is and what the caller may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub schema_version: u32,
    pub klif_version: String,
    pub node_name: String,
    /// Rights granted to this connection besides view: "launch", "edit".
    pub allow: Vec<String>,
    /// The server's installation id (a node equal to ourselves or to another connected node is refused).
    pub instance_id: String,
    /// [`server_proof`] of the client's `hello` nonce (empty when the client sent none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proof: String,
}

/// One request line (Debug masks the mac).
#[derive(Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    /// hex(HMAC-SHA256(token, nonce || id || method || canonical-params-json)).
    pub mac: String,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request").field("id", &self.id).field("method", &self.method).field("mac", &MASK).finish_non_exhaustive()
    }
}

/// The request mac (see the module docs).
pub fn request_mac(token: &str, nonce: &str, id: u64, method: &str, params: &Value) -> String {
    let id = id.to_string();
    let params = canonical_json(params);
    let mac = hmac_sha256(
        token.as_bytes(),
        &[nonce.as_bytes(), b"\n", id.as_bytes(), b"\n", method.as_bytes(), b"\n", params.as_bytes()],
    );
    hex(&mac)
}

/// The server's answer to a client's `hello` nonce: proof that it holds the token, bound to this connection's
/// challenge nonce and the instance id it claims (see the module docs).
pub fn server_proof(token: &str, client_nonce: &str, server_nonce: &str, instance_id: &str) -> String {
    hex(&hmac_sha256(
        token.as_bytes(),
        &[b"klif-server\n", client_nonce.as_bytes(), b"\n", server_nonce.as_bytes(), b"\n", instance_id.as_bytes()],
    ))
}

/// HMAC-SHA256 (RFC 2104) of the concatenated `parts` with `key`.
pub(crate) fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    for p in parts {
        inner.update(p);
    }
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner);
    outer.finalize().into()
}

/// The canonical JSON text of a value: object keys sorted (byte order), no whitespace, scalars as serde_json
/// writes them. Independent of serde_json's `preserve_order` feature, so both ends agree.
pub fn canonical_json(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                write_canonical(&m[k.as_str()], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap_or_else(|_| "null".into())),
    }
}

/// Constant-time equality of two strings (length is not secret: macs and tokens have fixed lengths).
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b) {
        d |= x ^ y;
    }
    // Keep the compiler from short-circuiting the loop above.
    std::hint::black_box(d) == 0
}

/// Lower-case hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

/// `n` random bytes (OS generator) as hex. Err only when the OS generator fails.
pub fn random_hex(n: usize) -> anyhow::Result<String> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("the system random number generator failed: {e}"))?;
    let s = hex(&buf);
    buf.iter_mut().for_each(|b| *b = 0);
    Ok(s)
}

pub(crate) fn is_hex_id(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// This installation's id: `<state_dir>\instance-id` (16 random bytes hex), created on first use. When the file
/// cannot be written the id lives for this process only (cached, so the server and the node hub agree).
pub fn instance_id(state_dir: &Path) -> String {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(id) = cache.get(state_dir) {
        return id.clone();
    }
    let path = state_dir.join(INSTANCE_ID_FILE);
    let read = |p: &Path| std::fs::read_to_string(p).ok().map(|s| s.trim().to_ascii_lowercase()).filter(|s| is_hex_id(s, 32));
    let id = match read(&path) {
        Some(id) => id,
        None => {
            let fresh = random_hex(16).unwrap_or_else(|_| {
                // Last resort: time + pid (still unique enough to tell installations apart).
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
                format!("{:016x}{:08x}{:08x}", t as u64, std::process::id(), (t >> 64) as u32)
            });
            let _ = std::fs::create_dir_all(state_dir);
            match write_new(&path, &fresh) {
                Ok(()) => fresh,
                // Another process created it first: use theirs.
                Err(_) => read(&path).unwrap_or_else(|| {
                    log::warn!("{} could not be written; this KLIF's instance id lasts for this process only", path.display());
                    fresh
                }),
            }
        }
    };
    cache.insert(state_dir.to_path_buf(), id.clone());
    id
}

/// Create `path` with `text` only if it does not exist (an existing file wins). A file holding an invalid id is
/// replaced.
fn write_new(path: &Path, text: &str) -> io::Result<()> {
    let attempt = || -> io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()
    };
    match attempt() {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let current = std::fs::read_to_string(path).unwrap_or_default();
            if is_hex_id(current.trim(), 32) {
                Err(e)
            } else {
                write_atomic(path, text.as_bytes())
            }
        }
        other => other,
    }
}

/// Write a file atomically: a temp file next to it, then rename (retried briefly: Defender / indexers may hold
/// the target for a moment).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let tmp = path.with_file_name(format!("{name}.{}.tmp", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    let mut last = None;
    for i in 0..10 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(20 * (i + 1)));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last.unwrap_or_else(|| io::Error::other("rename failed")))
}

/// One response line (`ok` with `result`, or not `ok` with `error`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn success(id: u64, result: Value) -> Response {
        Response { id, ok: true, result: Some(result), error: None }
    }
    pub fn failure(id: u64, code: &str, message: impl Into<String>) -> Response {
        Response { id, ok: false, result: None, error: Some(ErrorBody { code: code.to_string(), message: message.into() }) }
    }
}

/// A failure: a stable code (see [`code`]: "unauthorized", "forbidden", "unknown_method", "refused"...) and one
/// sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

/// One System in the compact status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatusSystem {
    pub id: SystemId,
    pub label: String,
    pub kind: SystemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LlmClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub status: SystemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Base URL clients use (LLM: ".../v1").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(rename = "vramGiB", default, skip_serializing_if = "Option::is_none")]
    pub vram_gib: Option<f64>,
    /// The fault title, when faulted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<String>,
}

/// One GPU in the compact status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatusGpu {
    pub id: String,
    pub name: String,
    #[serde(rename = "usedGiB")]
    pub used_gib: f64,
    #[serde(rename = "totalGiB")]
    pub total_gib: f64,
}

/// One remote node in the compact status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatusNode {
    pub id: String,
    pub state: NodeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
}

/// The compact status an agent polls (`klif-cli --json status` = `{schemaVersion:1, ...StatusJson}`; method
/// `status`). `status <system>` returns one `systems` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatusJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<SystemId>,
    pub systems: Vec<StatusSystem>,
    pub gpus: Vec<StatusGpu>,
    pub nodes: Vec<StatusNode>,
}

fn finite(x: f64) -> Option<f64> {
    x.is_finite().then_some(x)
}

/// The compact status of a view model.
pub fn status_of(vm: &ViewModel) -> StatusJson {
    let systems = vm
        .systems
        .iter()
        .map(|s| {
            let session = s.session.as_ref();
            let model = Some(s.model.name.trim()).filter(|n| !n.is_empty()).map(str::to_string);
            StatusSystem {
                id: s.id.clone(),
                label: s.label.clone(),
                kind: s.kind,
                class: s.class,
                node: s.node.clone(),
                status: s.status,
                reason: s.reason.clone(),
                preset: s.preset.clone(),
                base_url: s.endpoint.clone(),
                model,
                decode_tps: session.and_then(|x| x.llm.as_ref()).and_then(|l| finite(l.decode_tps)),
                vram_gib: session.and_then(|x| x.vram_gib).and_then(finite),
                fault: session.and_then(|x| x.fault.as_ref()).map(|f| f.title.clone()),
            }
        })
        .collect();
    let gpus = vm
        .gpus
        .iter()
        .map(|g| StatusGpu {
            id: g.id.clone(),
            name: if g.name.trim().is_empty() { g.device.clone() } else { g.name.clone() },
            used_gib: finite(g.used_gib).unwrap_or(0.0),
            total_gib: finite(g.total_gib).unwrap_or(0.0),
        })
        .collect();
    let nodes = vm
        .nodes
        .iter()
        .map(|n| StatusNode { id: n.id.clone(), state: n.state, latency_ms: n.latency_ms.and_then(finite) })
        .collect();
    StatusJson { selected: vm.selected.clone(), systems, gpus, nodes }
}

// ----------------------------------------------------------------------------------------- framing

/// Why a line could not be read.
#[derive(Debug)]
pub(crate) enum ReadError {
    /// The peer closed the connection.
    Eof,
    /// Nothing complete before the deadline.
    TimedOut,
    /// The line exceeds the limit.
    TooLong,
    Io(io::Error),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Eof => f.write_str("the connection was closed"),
            ReadError::TimedOut => f.write_str("no answer in time"),
            ReadError::TooLong => f.write_str("the message is too large"),
            ReadError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Reads newline-terminated lines with a total deadline per line (a peer trickling bytes cannot extend it).
pub(crate) struct LineReader {
    stream: TcpStream,
    buf: Vec<u8>,
    /// Bytes of `buf` already searched for a newline.
    scanned: usize,
    chunk: Box<[u8]>,
}

impl LineReader {
    pub(crate) fn new(stream: TcpStream) -> LineReader {
        LineReader { stream, buf: Vec::new(), scanned: 0, chunk: vec![0u8; 64 * 1024].into_boxed_slice() }
    }

    /// The next line without its terminator (`\n` or `\r\n`).
    pub(crate) fn read_line(&mut self, deadline: Instant, max: usize) -> Result<Vec<u8>, ReadError> {
        loop {
            if let Some(i) = self.buf[self.scanned..].iter().position(|&b| b == b'\n') {
                let end = self.scanned + i;
                let mut line: Vec<u8> = self.buf.drain(..=end).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                self.scanned = 0;
                if line.len() > max {
                    return Err(ReadError::TooLong);
                }
                return Ok(line);
            }
            self.scanned = self.buf.len();
            if self.buf.len() > max {
                return Err(ReadError::TooLong);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(ReadError::TimedOut);
            }
            let left = (deadline - now).max(Duration::from_millis(1));
            self.stream.set_read_timeout(Some(left)).map_err(ReadError::Io)?;
            match self.stream.read(&mut self.chunk) {
                Ok(0) => return Err(ReadError::Eof),
                Ok(n) => self.buf.extend_from_slice(&self.chunk[..n]),
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Err(ReadError::TimedOut),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(ReadError::Io(e)),
            }
        }
    }
}

/// Write one JSON line.
pub(crate) fn write_line<T: Serialize>(stream: &mut TcpStream, value: &T) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    stream.flush()
}

// ------------------------------------------------------------------------------------------- client

/// Why a call failed (client side).
#[derive(Debug, Clone)]
pub(crate) enum CallError {
    /// No connection, or it broke (refused, timeout, closed): reconnect.
    Io(String),
    /// The peer does not speak this protocol (the challenge or a response did not decode).
    Protocol(String),
    /// The server answered with an error.
    Remote(ErrorBody),
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Io(s) | CallError::Protocol(s) => f.write_str(s),
            CallError::Remote(e) => f.write_str(&e.message),
        }
    }
}

/// An authenticated client connection (klif-cli to the local engine, a hub to a node).
pub(crate) struct Connection {
    reader: LineReader,
    writer: TcpStream,
    nonce: String,
    instance_id: String,
    token: Secret,
    next_id: u64,
    last_used: Instant,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection").field("instance_id", &self.instance_id).field("next_id", &self.next_id).finish_non_exhaustive()
    }
}

/// Resolve "host:port" (DNS may block briefly).
pub(crate) fn resolve(addr: &str) -> Result<Vec<SocketAddr>, CallError> {
    let addrs: Vec<SocketAddr> =
        addr.to_socket_addrs().map_err(|e| CallError::Io(format!("{addr} could not be resolved: {e}")))?.collect();
    if addrs.is_empty() {
        return Err(CallError::Io(format!("{addr} could not be resolved.")));
    }
    Ok(addrs)
}

impl Connection {
    /// Connect (each resolved address, `connect_timeout` each) and read the challenge (within `wait`).
    pub(crate) fn open(addrs: &[SocketAddr], token: Secret, connect_timeout: Duration, wait: Duration) -> Result<Connection, CallError> {
        let mut last = String::from("no address");
        let mut stream = None;
        for a in addrs {
            match TcpStream::connect_timeout(a, connect_timeout) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last = format!("{a}: {e}"),
            }
        }
        let stream = stream.ok_or_else(|| CallError::Io(format!("Could not connect ({last}).")))?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
        let writer = stream.try_clone().map_err(|e| CallError::Io(e.to_string()))?;
        let mut reader = LineReader::new(stream);
        let line = match reader.read_line(Instant::now() + wait, 4096) {
            Ok(l) => l,
            Err(ReadError::TooLong) => return Err(CallError::Protocol("The address does not answer like a KLIF node.".into())),
            Err(e) => return Err(CallError::Io(format!("No greeting from the server: {e}."))),
        };
        let ch: Challenge = serde_json::from_slice(&line)
            .map_err(|_| CallError::Protocol("The address does not answer like a KLIF node (no challenge).".into()))?;
        if !is_hex_id(&ch.nonce, 32) {
            return Err(CallError::Protocol("The server sent an invalid challenge.".into()));
        }
        Ok(Connection { reader, writer, nonce: ch.nonce, instance_id: ch.instance_id, token, next_id: 1, last_used: Instant::now() })
    }

    /// The server's installation id from its challenge (trust it only after [`Connection::verify_hello`]).
    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Did the server prove it holds the token? `client_nonce` is what this client sent in `hello`; the answer
    /// must carry the challenge's instance id and its [`server_proof`] (constant-time compare). An empty proof (an
    /// older server, or one that does not know the token) fails.
    pub(crate) fn verify_hello(&self, client_nonce: &str, hello: &Hello) -> bool {
        if hello.proof.is_empty() || hello.instance_id != self.instance_id {
            return false;
        }
        ct_eq(&server_proof(self.token.expose(), client_nonce, &self.nonce, &hello.instance_id), &hello.proof)
    }

    /// Time since the last request.
    pub(crate) fn idle_for(&self) -> Duration {
        self.last_used.elapsed()
    }

    /// A handle that can `shutdown` the socket from another thread (unblocks a pending read).
    pub(crate) fn shutdown_handle(&self) -> Option<TcpStream> {
        self.writer.try_clone().ok()
    }

    /// One request, its result (`timeout` for the whole answer).
    pub(crate) fn call(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, CallError> {
        let id = self.next_id;
        self.next_id += 1;
        self.last_used = Instant::now();
        let mac = request_mac(self.token.expose(), &self.nonce, id, method, &params);
        let req = Request { id, method: method.to_string(), params, mac };
        write_line(&mut self.writer, &req).map_err(|e| CallError::Io(format!("The connection broke: {e}.")))?;
        let line = match self.reader.read_line(Instant::now() + timeout, MAX_RESPONSE_BYTES) {
            Ok(l) => l,
            Err(ReadError::TooLong) => return Err(CallError::Protocol("The answer exceeds 16 MiB.".into())),
            Err(e) => return Err(CallError::Io(format!("The connection broke: {e}."))),
        };
        self.last_used = Instant::now();
        let resp: Response =
            serde_json::from_slice(&line).map_err(|e| CallError::Protocol(format!("The answer could not be read: {e}.")))?;
        if resp.ok {
            if resp.id != id {
                return Err(CallError::Protocol(format!("The answer belongs to request {} instead of {id}.", resp.id)));
            }
            Ok(resp.result.unwrap_or(Value::Null))
        } else {
            Err(CallError::Remote(
                resp.error.unwrap_or_else(|| ErrorBody { code: code::REFUSED.into(), message: "The server refused the request.".into() }),
            ))
        }
    }
}
