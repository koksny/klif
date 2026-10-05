//! klif-webui: a small control page for a phone or a browser on the LAN (`[webui]`, off by default). The engine
//! serves the page (its files come from the host: the window app passes its embedded UI build, see
//! [`crate::EngineHandle::set_web_assets`]) and a JSON API:
//!
//! | Request | Auth | |
//! | --- | --- | --- |
//! | `GET /`, `GET /assets/<file>` | none | the page and its scripts, styles and fonts |
//! | `POST /api/pair` `{secret, name}` | none | an open pairing from Tune -> `{token, device}` |
//! | `GET /api/state` | Bearer | machines, GPUs and Systems ([`web_state`]) |
//! | `GET /api/console?system=<id>` | Bearer | the System's last console lines |
//! | `POST /api/act` `{type, system, ...}` | Bearer | launch, stop, restart, dismiss, usePreset, setParam |
//! | `POST /api/forget` | Bearer | this device unpairs itself |
//!
//! - **Devices.** A paired device holds a 32-byte token (the page keeps it in localStorage and sends it as
//!   `Authorization: Bearer`; no cookie, so nothing ambient for another site to ride on). The engine keeps only the
//!   token's SHA-256 in `webui-devices.json` (state folder), with a name and dates. Tune lists and removes them.
//! - **Pairing.** Tune opens one (`PairWebDevice`): a 32-byte secret for the QR code (in the URL's `#` part, so it is
//!   never sent in a request line or logged) and a 6-digit code for typing. Valid 5 minutes, closed by one success or
//!   5 wrong attempts; a wrong attempt also costs the caller half a second.
//! - **Rights.** What a node's `launch` right allows (launch, stop, restart, dismiss, preset, params), on every System
//!   the engine shows; a remote System still needs its node's `launch` (`controllable`). Never Select, never edits,
//!   downloads or settings.
//! - **Hardening.** Plain HTTP: authenticated, not encrypted (like nodes). The Host header must be an IP address,
//!   localhost or this computer's name (DNS rebinding); POST bodies must be `application/json` (a cross-site form
//!   cannot send that); at most 16 connections, 8 per peer address; 10 s to send a request; 16 KiB of headers and
//!   64 KiB of body; one request per connection; nosniff, no-referrer and no framing on every answer, a CSP on the
//!   page.
//! - **What it shows.** The state carries no commands, no config paths and no secrets; a `reason` sentence may name
//!   a missing file. The console is the server's own output (which can name its model file) without KLIF's lines
//!   that spell out the command and the log paths.

use anyhow::{Context, Result};
use klif_common::config::Config;
use klif_common::vm::{
    Action, GpuMemory, LlmClass, NodeState, ParamView, PresetInfo, StepState, System, SystemId, SystemKind, SystemStatus, ViewModel,
    WebUiDevice,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::wire::{ct_eq, hex, random_hex, write_atomic};
use crate::EngineHandle;

/// Paired devices, next to klif.toml.
pub const DEVICES_FILE: &str = "webui-devices.json";
/// How long an open pairing lasts.
pub const PAIR_TTL_S: f64 = 300.0;
const PAIR_MAX_FAILS: u32 = 5;
const MAX_DEVICES: usize = 32;
/// `last_seen` is written to disk at most this often.
const SEEN_SAVE_S: f64 = 60.0;

const MAX_CONNS: usize = 16;
const MAX_CONNS_PER_IP: usize = 8;
const REQUEST_TIME: Duration = Duration::from_secs(10);
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;
const CONSOLE_LINES: usize = 80;
/// How often the accept loop looks for connections and for the stop flag.
const ACCEPT_POLL: Duration = Duration::from_millis(50);

/// The page's files: `path` ("webui.html", "assets/index-1a2b.js") -> (bytes, MIME type). None: no such file.
pub type WebAssets = Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn unix_now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

// ------------------------------------------------------------------------------------------------ devices

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredDevice {
    id: String,
    name: String,
    token_sha256: String,
    paired_at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_seen: Option<f64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DevicesFile {
    version: u32,
    devices: Vec<StoredDevice>,
}

struct Pairing {
    secret: String,
    code: String,
    expires: f64,
    fails: u32,
}

#[derive(Default)]
struct AuthState {
    devices: Vec<StoredDevice>,
    pairing: Option<Pairing>,
    /// `last_seen` changed since the last write.
    seen_dirty: bool,
    last_save: f64,
}

/// Why a pairing attempt failed: one sentence for the page.
#[derive(Debug, PartialEq)]
pub enum PairError {
    /// No pairing is open (or it expired).
    Closed,
    Wrong { closed: bool },
    Full,
    Io(String),
}

impl PairError {
    pub fn sentence(&self) -> String {
        match self {
            PairError::Closed => "No pairing is open. In KLIF open Tune, then Web UI, and choose Pair a device.".into(),
            PairError::Wrong { closed: false } => "That code is wrong or has expired. Check it in KLIF (Tune, Web UI).".into(),
            PairError::Wrong { closed: true } => {
                "Too many wrong codes, so KLIF closed this pairing. Open a new one in Tune, Web UI.".into()
            }
            PairError::Full => format!("{MAX_DEVICES} devices are paired already. Remove one in Tune, Web UI."),
            PairError::Io(e) => format!("KLIF could not save the device: {e}."),
        }
    }
}

/// Paired devices and the open pairing (the engine owns one; the web server checks tokens against it).
pub struct WebAuth {
    path: PathBuf,
    st: Mutex<AuthState>,
}

impl WebAuth {
    /// `<state_dir>\webui-devices.json`; a missing file is no devices, an unreadable one is logged and treated so.
    pub fn load(state_dir: &Path) -> WebAuth {
        let path = state_dir.join(DEVICES_FILE);
        let devices = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<DevicesFile>(&bytes) {
                Ok(f) => f.devices.into_iter().filter(|d| crate::wire::is_hex_id(&d.token_sha256, 64)).collect(),
                Err(e) => {
                    log::warn!("{} could not be read ({e}); no klif-webui device is paired until it is fixed or removed", path.display());
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        };
        WebAuth { path, st: Mutex::new(AuthState { devices, ..AuthState::default() }) }
    }

    /// Open a pairing (replacing an open one).
    pub fn pair_begin(&self, now: f64) -> Result<()> {
        let secret = random_hex(32)?;
        let code = six_digits()?;
        lock(&self.st).pairing = Some(Pairing { secret, code, expires: now + PAIR_TTL_S, fails: 0 });
        Ok(())
    }

    pub fn pair_cancel(&self) {
        lock(&self.st).pairing = None;
    }

    /// The open pairing: (secret, code, expires); an expired one is dropped.
    pub fn pairing(&self, now: f64) -> Option<(String, String, f64)> {
        let mut st = lock(&self.st);
        if st.pairing.as_ref().is_some_and(|p| p.expires <= now) {
            st.pairing = None;
        }
        st.pairing.as_ref().map(|p| (p.secret.clone(), p.code.clone(), p.expires))
    }

    /// Pair with the QR secret or the typed code. Ok((token, device id)).
    pub fn pair_complete(&self, given: &str, name: &str, now: f64) -> Result<(String, String), PairError> {
        let mut st = lock(&self.st);
        let Some(p) = st.pairing.as_mut().filter(|p| p.expires > now) else {
            st.pairing = None;
            return Err(PairError::Closed);
        };
        let given = given.trim();
        let digits: String = given.chars().filter(char::is_ascii_digit).collect();
        let by_secret = given.len() == 64 && ct_eq(&given.to_ascii_lowercase(), &p.secret);
        let by_code = digits.len() == 6 && given.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '-') && ct_eq(&digits, &p.code);
        if !(by_secret || by_code) {
            p.fails += 1;
            let closed = p.fails >= PAIR_MAX_FAILS;
            if closed {
                st.pairing = None;
            }
            return Err(PairError::Wrong { closed });
        }
        if st.devices.len() >= MAX_DEVICES {
            return Err(PairError::Full);
        }
        st.pairing = None;
        let token = random_hex(32).map_err(|e| PairError::Io(format!("{e:#}")))?;
        let id = loop {
            let id = random_hex(4).map_err(|e| PairError::Io(format!("{e:#}")))?;
            if !st.devices.iter().any(|d| d.id == id) {
                break id;
            }
        };
        st.devices.push(StoredDevice {
            id: id.clone(),
            name: clean_name(name),
            token_sha256: sha256_hex(&token),
            paired_at: now,
            last_seen: Some(now),
        });
        self.save(&mut st, now).map_err(|e| {
            st.devices.retain(|d| d.id != id);
            PairError::Io(format!("{e:#}"))
        })?;
        Ok((token, id))
    }

    /// The device a bearer token belongs to (its id and name); notes when it was seen.
    pub fn check(&self, token: &str, now: f64) -> Option<(String, String)> {
        if !crate::wire::is_hex_id(token, 64) {
            return None;
        }
        let want = sha256_hex(&token.to_ascii_lowercase());
        let mut st = lock(&self.st);
        let mut found = None;
        for (i, d) in st.devices.iter().enumerate() {
            if ct_eq(&d.token_sha256, &want) {
                found = Some(i);
            }
        }
        let i = found?;
        let d = &mut st.devices[i];
        d.last_seen = Some(now);
        let out = (d.id.clone(), d.name.clone());
        st.seen_dirty = true;
        if now - st.last_save >= SEEN_SAVE_S {
            // Written outside the lock (a failing write retries for about a second), and tried again a minute later
            // at the earliest, whatever the outcome.
            st.last_save = now;
            st.seen_dirty = false;
            let file = DevicesFile { version: 1, devices: st.devices.clone() };
            drop(st);
            let written = serde_json::to_vec_pretty(&file).map_err(anyhow::Error::from).and_then(|b| write_atomic(&self.path, &b).map_err(Into::into));
            if let Err(e) = written {
                log::warn!("{} could not be written: {e:#}", self.path.display());
                lock(&self.st).seen_dirty = true;
            }
        }
        Some(out)
    }

    /// Remove one device (None: all). Ok(false) when there was no such device.
    pub fn forget(&self, id: Option<&str>) -> Result<bool> {
        let mut st = lock(&self.st);
        let before = st.devices.len();
        match id {
            Some(id) => st.devices.retain(|d| d.id != id),
            None => st.devices.clear(),
        }
        if st.devices.len() == before {
            return Ok(id.is_none());
        }
        self.save(&mut st, unix_now())?;
        Ok(true)
    }

    pub fn devices(&self) -> Vec<WebUiDevice> {
        lock(&self.st)
            .devices
            .iter()
            .map(|d| WebUiDevice { id: d.id.clone(), name: d.name.clone(), paired_at: d.paired_at, last_seen: d.last_seen })
            .collect()
    }

    /// Write `last_seen` that is still only in memory (engine shutdown).
    pub fn flush(&self) {
        let mut st = lock(&self.st);
        if st.seen_dirty {
            if let Err(e) = self.save(&mut st, unix_now()) {
                log::warn!("{} could not be written: {e:#}", self.path.display());
            }
        }
    }

    fn save(&self, st: &mut AuthState, now: f64) -> Result<()> {
        let file = DevicesFile { version: 1, devices: st.devices.clone() };
        let bytes = serde_json::to_vec_pretty(&file)?;
        write_atomic(&self.path, &bytes).with_context(|| format!("{} could not be written", self.path.display()))?;
        st.seen_dirty = false;
        st.last_save = now;
        Ok(())
    }
}

/// Six uniformly random digits.
fn six_digits() -> Result<String> {
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("the system random number generator failed: {e}"))?;
        let n = u32::from_le_bytes(b);
        // Reject the top sliver so every code is equally likely.
        if n < u32::MAX - (u32::MAX % 1_000_000) {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

/// A device name as the page sent it: printable, at most 48 characters, "Browser" when empty.
fn clean_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control()).take(48).collect();
    let s = s.trim();
    if s.is_empty() { "Browser".into() } else { s.to_string() }
}

// ------------------------------------------------------------------------------------------------ addresses

/// The address a phone opens for a server listening on `host:port`: the given IP, or for 0.0.0.0 / :: the address
/// this machine reaches its LAN with (the route of a UDP socket; nothing is sent). None when there is no route.
pub fn page_urls(host: &str, port: u16) -> Vec<String> {
    let ip: Option<IpAddr> = match host.parse::<IpAddr>() {
        Ok(ip) if !ip.is_unspecified() => Some(ip),
        Ok(IpAddr::V6(_)) => route_ip(true).or_else(|| route_ip(false)),
        _ => route_ip(false),
    };
    match ip {
        Some(IpAddr::V6(v6)) => vec![format!("http://[{v6}]:{port}/")],
        Some(ip) => vec![format!("http://{ip}:{port}/")],
        None => Vec::new(),
    }
}

fn route_ip(v6: bool) -> Option<IpAddr> {
    let (bind, target): (SocketAddr, SocketAddr) = if v6 {
        ((Ipv6Addr::UNSPECIFIED, 0).into(), "[2001:db8::1]:9".parse().ok()?)
    } else {
        ((Ipv4Addr::UNSPECIFIED, 0).into(), "192.0.2.1:9".parse().ok()?)
    };
    let s = UdpSocket::bind(bind).ok()?;
    s.connect(target).ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

// ------------------------------------------------------------------------------------------------ server

/// A running klif-webui listener.
pub struct WebServer {
    addr: String,
    stop: Arc<AtomicBool>,
    accept: Mutex<Option<JoinHandle<()>>>,
}

impl WebServer {
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Stop listening: the accept loop sees the flag within one poll (connections in flight finish on their own).
    pub fn shutdown(&self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(h) = lock(&self.accept).take() {
            let _ = h.join();
        }
        log::info!("klif-webui on {} stopped", self.addr);
    }
}

struct Shared {
    handle: EngineHandle,
    auth: Arc<WebAuth>,
    assets: WebAssets,
    stop: Arc<AtomicBool>,
    conns: Mutex<BTreeMap<IpAddr, usize>>,
    /// Names the Host header may carry besides IP addresses and localhost (lower case).
    names: Vec<String>,
}

/// One admitted connection; frees its slot when dropped.
struct Slot {
    shared: Arc<Shared>,
    ip: IpAddr,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut c = lock(&self.shared.conns);
        if let Some(n) = c.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                c.remove(&self.ip);
            }
        }
    }
}

/// Listen on `addr` ("0.0.0.0:7341").
pub fn serve(handle: EngineHandle, auth: Arc<WebAuth>, assets: WebAssets, addr: &str) -> Result<WebServer> {
    let listener = TcpListener::bind(addr).with_context(|| format!("klif-webui could not listen on {addr}"))?;
    // Polled, not blocking: stopping needs no connection to our own address (which may be gone by then).
    listener.set_nonblocking(true).context("klif-webui could not configure its listener")?;
    let computer = crate::control::computer_name().to_ascii_lowercase();
    let names = vec![computer.clone(), format!("{computer}.local")];
    let stop = Arc::new(AtomicBool::new(false));
    let shared = Arc::new(Shared { handle, auth, assets, stop: stop.clone(), conns: Mutex::new(BTreeMap::new()), names });
    let accept = std::thread::Builder::new()
        .name("klif-webui-accept".into())
        .spawn(move || accept_loop(listener, shared))
        .context("could not start the klif-webui thread")?;
    Ok(WebServer { addr: addr.to_string(), stop, accept: Mutex::new(Some(accept)) })
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut errors = 0u32;
    loop {
        let accepted = listener.accept();
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        match accepted {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL);
            }
            Ok((stream, peer)) => {
                errors = 0;
                // Accepted sockets may inherit non-blocking mode; the connection code reads with timeouts.
                let _ = stream.set_nonblocking(false);
                let ip = peer.ip();
                {
                    let mut c = lock(&shared.conns);
                    let total: usize = c.values().sum();
                    let mine = c.get(&ip).copied().unwrap_or(0);
                    if total >= MAX_CONNS || mine >= MAX_CONNS_PER_IP {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        continue;
                    }
                    *c.entry(ip).or_insert(0) += 1;
                }
                let slot = Slot { shared: shared.clone(), ip };
                let spawned = std::thread::Builder::new().name("klif-webui-conn".into()).spawn(move || {
                    let s = slot.shared.clone();
                    serve_connection(&s, stream, ip);
                    drop(slot);
                });
                if let Err(e) = spawned {
                    log::warn!("klif-webui: could not start a connection thread: {e}");
                }
            }
            Err(e) => {
                errors += 1;
                if errors % 100 == 1 {
                    log::warn!("klif-webui: accept failed: {e}");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ HTTP

struct Request {
    /// The peer, for the log.
    ip: IpAddr,
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn query_param(&self, name: &str) -> Option<String> {
        self.query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k) == name).then(|| percent_decode(v))
        })
    }
}

struct Response {
    status: u16,
    content_type: String,
    body: Vec<u8>,
    cache: &'static str,
    html: bool,
}

impl Response {
    fn json(status: u16, v: &Value) -> Response {
        Response { status, content_type: "application/json".into(), body: serde_json::to_vec(v).unwrap_or_default(), cache: "no-store", html: false }
    }

    fn error(status: u16, code: &str, message: impl Into<String>) -> Response {
        Response::json(status, &json!({ "error": { "code": code, "message": message.into() } }))
    }

    fn text(status: u16, msg: &str) -> Response {
        Response { status, content_type: "text/plain; charset=utf-8".into(), body: msg.as_bytes().to_vec(), cache: "no-store", html: false }
    }
}

enum ReadFail {
    /// Closed or timed out: no answer.
    Gone,
    Bad(u16, &'static str),
}

fn read_request(stream: &mut TcpStream, ip: IpAddr) -> Result<Request, ReadFail> {
    let deadline = Instant::now() + REQUEST_TIME;
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(ReadFail::Bad(431, "The request headers are too large."));
        }
        let n = read_some(stream, &mut chunk, deadline)?;
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| ReadFail::Bad(400, "The request is not text."))?;
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split(' ');
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ReadFail::Bad(400, "The request line is not HTTP."));
    };
    if parts.next().is_some() || !version.starts_with("HTTP/1.") {
        return Err(ReadFail::Bad(400, "Only HTTP/1.x is served."));
    }
    let mut headers = Vec::new();
    for line in lines {
        let Some((k, v)) = line.split_once(':') else { return Err(ReadFail::Bad(400, "A header line is not \"name: value\".")) };
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    let mut req = Request {
        ip,
        method: method.to_string(),
        path: String::new(),
        query: String::new(),
        headers,
        body: Vec::new(),
    };
    if req.header("transfer-encoding").is_some() {
        return Err(ReadFail::Bad(411, "Send a Content-Length, not a chunked body."));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    req.path = path.to_string();
    req.query = query.to_string();
    let len: usize = match req.header("content-length") {
        Some(v) => v.parse().map_err(|_| ReadFail::Bad(400, "Content-Length is not a number."))?,
        None => 0,
    };
    if len > MAX_BODY {
        return Err(ReadFail::Bad(413, "The request body is too large."));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = read_some(stream, &mut chunk, deadline)?;
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    req.body = body;
    Ok(req)
}

fn read_some(stream: &mut TcpStream, chunk: &mut [u8], deadline: Instant) -> Result<usize, ReadFail> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(ReadFail::Gone);
    }
    let _ = stream.set_read_timeout(Some(left));
    match stream.read(chunk) {
        Ok(0) | Err(_) => Err(ReadFail::Gone),
        Ok(n) => Ok(n),
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn percent_decode(s: &str) -> String {
    let hexval = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hexval(b[i + 1]), hexval(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// Every answer's security headers; the page also gets a CSP.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data:; \
connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn write_response(stream: &mut TcpStream, r: &Response, head_only: bool) {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\nX-Content-Type-Options: nosniff\r\n\
Referrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nCross-Origin-Resource-Policy: same-origin\r\nConnection: close\r\n",
        r.status,
        status_text(r.status),
        r.content_type,
        r.body.len(),
        r.cache
    );
    if r.html {
        head.push_str(&format!("Content-Security-Policy: {CSP}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let _ = stream.write_all(head.as_bytes());
    if !head_only {
        let _ = stream.write_all(&r.body);
    }
    let _ = stream.flush();
}

fn serve_connection(shared: &Shared, mut stream: TcpStream, ip: IpAddr) {
    let _ = stream.set_nodelay(true);
    let req = match read_request(&mut stream, ip) {
        Ok(r) => r,
        Err(ReadFail::Gone) => return,
        Err(ReadFail::Bad(code, msg)) => {
            write_response(&mut stream, &Response::text(code, msg), false);
            return;
        }
    };
    let resp = route(shared, &req);
    write_response(&mut stream, &resp, req.method == "HEAD");
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

/// The Host header names this machine: an IP address, localhost, or the computer's name (`.local` too).
fn host_ok(host: Option<&str>, names: &[String]) -> bool {
    let Some(h) = host.map(str::trim).filter(|h| !h.is_empty()) else { return false };
    let name = if let Some(rest) = h.strip_prefix('[') {
        match rest.split_once(']') {
            Some((ip, _)) => return ip.parse::<Ipv6Addr>().is_ok(),
            None => return false,
        }
    } else {
        h.rsplit_once(':').map_or(h, |(n, port)| if port.bytes().all(|b| b.is_ascii_digit()) { n } else { h })
    };
    let name = name.to_ascii_lowercase();
    name.parse::<IpAddr>().is_ok() || name == "localhost" || names.iter().any(|n| *n == name)
}

fn route(shared: &Shared, req: &Request) -> Response {
    if !host_ok(req.header("host"), &shared.names) {
        return Response::text(421, "klif-webui answers only to this machine's address or name.");
    }
    let get = req.method == "GET" || req.method == "HEAD";
    match (req.path.as_str(), get) {
        ("/" | "/index.html" | "/webui.html", true) => page(shared),
        (p, true) if p.starts_with("/assets/") => asset(shared, &p[1..]),
        ("/koksny-mark.png" | "/favicon.ico", true) => match (shared.assets)(&req.path[1..]) {
            Some((bytes, mime)) => Response { status: 200, content_type: mime, body: bytes, cache: "public, max-age=86400", html: false },
            None => Response { status: 204, content_type: "text/plain".into(), body: Vec::new(), cache: "public, max-age=86400", html: false },
        },
        ("/api/pair", false) if req.method == "POST" => with_json(req, |v| pair(shared, v, req.ip)),
        ("/api/state", true) => with_device(shared, req, |dev| state(shared, dev)),
        ("/api/console", true) => with_device(shared, req, |_| console(shared, req.query_param("system").unwrap_or_default())),
        ("/api/act", false) if req.method == "POST" => with_device(shared, req, |dev| with_json(req, |v| act(shared, v, &dev.0))),
        ("/api/forget", false) if req.method == "POST" => with_device(shared, req, |dev| with_json(req, |_| forget_self(shared, &dev.0))),
        (p, _) if p.starts_with("/api/") => Response::error(405, "method", "This request is not served."),
        _ => Response::text(404, "Not found."),
    }
}

fn page(shared: &Shared) -> Response {
    match (shared.assets)("webui.html") {
        Some((bytes, _)) => Response { status: 200, content_type: "text/html; charset=utf-8".into(), body: bytes, cache: "no-cache", html: true },
        None => Response::text(503, "This KLIF has no klif-webui page built in."),
    }
}

/// `assets/<name>`: one plain file name, nothing that climbs.
fn asset(shared: &Shared, path: &str) -> Response {
    let name = &path["assets/".len()..];
    let plain = !name.is_empty() && name.len() <= 128 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) && !name.starts_with('.');
    if !plain {
        return Response::text(404, "Not found.");
    }
    match (shared.assets)(path) {
        Some((bytes, mime)) => Response { status: 200, content_type: mime, body: bytes, cache: "public, max-age=31536000, immutable", html: false },
        None => Response::text(404, "Not found."),
    }
}

fn with_json(req: &Request, f: impl FnOnce(Value) -> Response) -> Response {
    let json_type = req.header("content-type").is_some_and(|t| t.split(';').next().is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json")));
    if !json_type {
        return Response::error(415, "bad_request", "Send JSON (Content-Type: application/json).");
    }
    match serde_json::from_slice::<Value>(&req.body) {
        Ok(v) if v.is_object() => f(v),
        _ => Response::error(400, "bad_request", "The request body is not a JSON object."),
    }
}

fn with_device(shared: &Shared, req: &Request, f: impl FnOnce((String, String)) -> Response) -> Response {
    let token = req.header("authorization").and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))).map(str::trim);
    match token.and_then(|t| shared.auth.check(t, unix_now())) {
        Some(dev) => f(dev),
        None => Response::error(401, "unpaired", "This browser is not paired with KLIF. Pair it from Tune, Web UI."),
    }
}

fn pair(shared: &Shared, v: Value, ip: IpAddr) -> Response {
    let given = v.get("secret").and_then(Value::as_str).unwrap_or_default();
    let name = v.get("name").and_then(Value::as_str).unwrap_or_default();
    match shared.auth.pair_complete(given, name, unix_now()) {
        Ok((token, id)) => {
            log::info!("klif-webui: device {id} paired from {ip}");
            // Tune shows the new device and the closed pairing at once, not at the next tick.
            shared.handle.refresh();
            Response::json(200, &json!({ "token": token, "device": { "id": id, "name": clean_name(name) } }))
        }
        Err(e) => {
            match e {
                PairError::Wrong { closed: true } => log::warn!("klif-webui: a wrong pairing code from {ip}; 5 wrong codes closed the pairing"),
                PairError::Wrong { closed: false } => log::info!("klif-webui: a wrong pairing code from {ip}"),
                _ => {}
            }
            if matches!(e, PairError::Wrong { .. }) {
                // Slow down guessing a little more than the attempt limit already does.
                std::thread::sleep(Duration::from_millis(500));
            }
            let status = match e {
                PairError::Io(_) => 500,
                PairError::Full => 409,
                _ => 403,
            };
            Response::error(status, "pair", e.sentence())
        }
    }
}

fn forget_self(shared: &Shared, id: &str) -> Response {
    match shared.auth.forget(Some(id)) {
        Ok(_) => {
            log::info!("klif-webui: device {id} unpaired itself");
            shared.handle.refresh();
            Response::json(200, &json!({ "ok": true }))
        }
        Err(e) => Response::error(500, "io", format!("{e:#}")),
    }
}

fn state(shared: &Shared, dev: (String, String)) -> Response {
    let vm = shared.handle.snapshot();
    let cfg = shared.handle.inner.cfg();
    let mut v = web_state(&vm, &cfg);
    v["device"] = json!({ "id": dev.0, "name": dev.1 });
    drop_nulls(&mut v);
    Response::json(200, &v)
}

fn console(shared: &Shared, system: String) -> Response {
    let id = SystemId::new(system.trim());
    let vm = shared.handle.snapshot();
    if !vm.systems.iter().any(|s| s.id == id) {
        return Response::error(404, "not_found", format!("There is no System \"{id}\"."));
    }
    let focused = shared.handle.snapshot_focus(Some(&id));
    // KLIF's own launch lines spell out the command and the log paths: not for the page.
    let hidden = ["[KLIF] command:", "[KLIF] stdout:", "[KLIF] stderr:"];
    let lines: Vec<&String> = focused
        .console
        .iter()
        .filter(|l| !hidden.iter().any(|h| l.starts_with(h)))
        .rev()
        .take(CONSOLE_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Response::json(200, &json!({ "system": id, "lines": lines }))
}

/// The actions the page may send: what a node's `launch` right allows, always naming a System.
fn act(shared: &Shared, v: Value, device: &str) -> Response {
    let action = match web_action(&v) {
        Ok(a) => a,
        Err(msg) => return Response::error(400, "bad_request", msg),
    };
    let vm = shared.handle.snapshot();
    let Some(sys) = action.system().and_then(|id| vm.systems.iter().find(|s| &s.id == id)) else {
        return Response::error(404, "not_found", "That System is not in KLIF (any more).");
    };
    if !sys.controllable {
        return Response::error(403, "forbidden", format!("{} cannot be controlled from here: its machine does not grant launch.", sys.label));
    }
    if sys.external && matches!(action, Action::Launch { .. } | Action::Stop { .. } | Action::Restart { .. }) {
        return Response::error(409, "refused", format!("{} is an external server: KLIF only watches it.", sys.label));
    }
    log::info!("klif-webui: device {device}: {}", action.summary());
    match shared.handle.act(action) {
        Ok(()) => Response::json(200, &json!({ "ok": true })),
        Err(e) => Response::error(409, "refused", format!("{e:#}")),
    }
}

/// Parse `{type, system, ...}` into an Action the page may send.
fn web_action(v: &Value) -> Result<Action, String> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string);
    let system = s("system").map(SystemId::new).ok_or("The action names no System.")?;
    let kind = s("type").ok_or("The action has no type.")?;
    Ok(match kind.as_str() {
        "launch" => Action::Launch { system: Some(system), stop_others: v.get("stopOthers").and_then(Value::as_bool).unwrap_or(false) },
        "stop" => Action::Stop { system: Some(system) },
        "restart" => Action::Restart { system: Some(system) },
        "dismiss" => Action::Dismiss { system: Some(system) },
        "usePreset" => Action::UsePreset { system, preset: s("preset").ok_or("Choose a preset.")? },
        "setParam" => Action::SetParam { system, name: s("name").ok_or("The param has no name.")?, value: s("value").ok_or("Choose a value.")? },
        other => return Err(format!("\"{other}\" cannot be sent from klif-webui.")),
    })
}

// ------------------------------------------------------------------------------------------------ state

/// What the page shows: this machine and every node, their GPUs (who holds which memory) and every System, in a
/// shape made for a phone. Built from the view model: no commands, no config paths, no secrets (a `reason` sentence
/// may name a missing file).
pub fn web_state(vm: &ViewModel, cfg: &Config) -> Value {
    let local_name = cfg.node.as_ref().and_then(|n| n.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(crate::control::computer_name);
    let label_of = |id: &SystemId| vm.systems.iter().find(|s| &s.id == id).map_or_else(|| id.to_string(), |s| s.label.clone());
    let mut machines = vec![json!({
        "id": "local",
        "name": local_name,
        "local": true,
        "os": os_name(),
        "state": "online",
        "gpus": gpus_view(&vm.gpus, vm.systems.iter().filter(|s| s.node.is_none()), unified_ids(vm)),
    })];
    for n in &vm.nodes {
        machines.push(json!({
            "id": n.id,
            "name": n.name,
            "local": false,
            "state": node_state(n.state),
            "latencyMs": n.latency_ms.map(|l| (l * 10.0).round() / 10.0),
            "error": n.error,
            "gpus": gpus_view(&n.gpus, vm.systems.iter().filter(|s| s.node.as_deref() == Some(n.id.as_str())), Vec::new()),
        }));
    }
    let systems: Vec<Value> = vm
        .systems
        .iter()
        .map(|s| {
            let presets: &[PresetInfo] = match &s.node {
                None => &vm.presets,
                Some(node) => vm.nodes.iter().find(|n| &n.id == node).map_or(&[][..], |n| &n.presets[..]),
            };
            let gpu_names: Vec<String> = s.gpus.iter().map(|g| gpu_name(vm, s.node.as_deref(), g)).collect();
            system_view(s, presets, &gpu_names, s.conflicts.iter().map(&label_of).collect())
        })
        .collect();
    json!({
        "v": 1,
        "now": vm.now,
        "skin": cfg.ui.skin,
        "onConflict": vm.config.on_conflict,
        "machines": machines,
        "systems": systems,
    })
}

/// Absent is absent: object fields that are null are left out (the page tests for undefined).
fn drop_nulls(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.retain(|_, x| !x.is_null());
            m.values_mut().for_each(drop_nulls);
        }
        Value::Array(a) => a.iter_mut().for_each(drop_nulls),
        _ => {}
    }
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    }
}

fn node_state(s: NodeState) -> &'static str {
    match s {
        NodeState::Connecting => "connecting",
        NodeState::Online => "online",
        NodeState::Offline => "offline",
        NodeState::Unauthorized => "unauthorized",
        NodeState::Incompatible => "incompatible",
    }
}

/// GPU ids of this machine that are the unified memory of an APU / Apple Silicon.
fn unified_ids(vm: &ViewModel) -> Vec<String> {
    if !vm.hardware.unified {
        return Vec::new();
    }
    vm.hardware.gpus.iter().filter(|g| g.counted && g.integrated).map(|g| g.id.clone()).collect()
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Each GPU with the memory each running System holds on it (Systems on one GPU only; the rest is `other`).
fn gpus_view<'a>(gpus: &[GpuMemory], systems: impl Iterator<Item = &'a System>, unified: Vec<String>) -> Vec<Value> {
    let systems: Vec<&System> = systems.collect();
    gpus.iter()
        .filter(|g| g.id != "cpu")
        .map(|g| {
            let mut parts = Vec::new();
            let mut held = 0.0;
            for s in &systems {
                let Some(sess) = &s.session else { continue };
                let on_g = s.gpus.len() <= 1 && sess.gpu.as_deref().or(s.gpu.as_deref()) == Some(g.id.as_str());
                if let (true, Some(gib)) = (on_g, sess.vram_gib.filter(|v| *v > 0.0)) {
                    held += gib;
                    parts.push(json!({ "system": s.id, "label": s.label, "gib": round1(gib) }));
                }
            }
            let other = (g.used_gib - held).max(0.0);
            json!({
                "id": g.id,
                "name": if g.name.is_empty() { g.device.clone() } else { g.name.clone() },
                "totalGiB": round1(g.total_gib),
                "usedGiB": round1(g.used_gib.max(held)),
                "otherGiB": round1(other),
                "unified": unified.iter().any(|u| u == &g.id),
                "parts": parts,
            })
        })
        .collect()
}

fn gpu_name(vm: &ViewModel, node: Option<&str>, id: &str) -> String {
    if id.eq_ignore_ascii_case("cpu") {
        return "CPU".into();
    }
    let list: &[GpuMemory] = match node {
        None => &vm.gpus,
        Some(n) => vm.nodes.iter().find(|x| x.id == n).map_or(&[][..], |x| &x.gpus[..]),
    };
    list.iter().find(|g| g.id == id).map(|g| if g.name.is_empty() { g.device.clone() } else { g.name.clone() }).unwrap_or_else(|| id.to_string())
}

fn kind_str(k: SystemKind) -> String {
    serde_json::to_value(k).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn status_str(s: SystemStatus) -> String {
    serde_json::to_value(s).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn class_str(c: Option<LlmClass>) -> Option<String> {
    c.and_then(|c| serde_json::to_value(c).ok()).and_then(|v| v.as_str().map(str::to_string))
}

fn ago(s: f64) -> String {
    let s = s.max(0.0);
    if s < 90.0 {
        "just now".into()
    } else if s < 3600.0 {
        format!("{} min ago", (s / 60.0).round())
    } else if s < 48.0 * 3600.0 {
        format!("{} h ago", (s / 3600.0).round())
    } else {
        format!("{} days ago", (s / 86400.0).round())
    }
}

fn system_view(s: &System, presets: &[PresetInfo], gpu_names: &[String], conflicts: Vec<String>) -> Value {
    let sess = s.session.as_ref();
    let running = matches!(s.status, SystemStatus::Online | SystemStatus::Busy | SystemStatus::Starting | SystemStatus::Stopping);
    let metric = sess.and_then(|x| {
        if let Some(l) = &x.llm {
            let tps = if l.decode_tps > 0.0 { l.decode_tps } else { l.decode_history.iter().rev().copied().find(|v| *v > 0.0).unwrap_or(0.0) };
            (tps > 0.0).then(|| json!({ "v": round1(tps), "u": "tok/s" }))
        } else if let Some(i) = &x.image {
            i.recent.last().map(|j| json!({ "v": round1(j.seconds), "u": "s per image" }))
        } else {
            None
        }
    });
    let last = s.last_session.as_ref().and_then(|l| {
        let what = l.decode_tps.map(|t| format!("{} tok/s", round1(t))).or_else(|| l.seconds_per_image.map(|t| format!("{} s per image", round1(t))));
        what.map(|w| format!("{w} · {}", ago(l.ended_ago_s)))
    });
    let load = sess.and_then(|x| x.loading.as_ref()).map(|p| {
        let step = p.steps.iter().find(|st| st.state == StepState::Active).map(|st| st.label.clone()).unwrap_or_else(|| "Starting".into());
        json!({ "step": step, "pct": (p.fraction.clamp(0.0, 1.0) * 100.0).round() })
    });
    let expected: Option<f64> = s.expected_vram.as_ref().map(|l| l.iter().map(|x| x.gib).sum::<f64>()).filter(|v| *v > 0.0);
    let vram = if running { sess.and_then(|x| x.vram_gib) } else { expected };
    let fault = sess.and_then(|x| x.fault.as_ref()).map(|f| f.title.clone());
    let mut kind_presets: Vec<Value> = presets
        .iter()
        .filter(|p| p.kind == s.kind)
        .map(|p| json!({ "id": p.id, "name": if p.name.is_empty() { p.id.clone() } else { p.name.clone() }, "ready": p.availability == klif_common::vm::Availability::Ready }))
        .collect();
    kind_presets.truncate(64);
    let params: Vec<&ParamView> = s.params.iter().collect();
    json!({
        "id": s.id,
        "label": s.label,
        "machine": s.node.clone().unwrap_or_else(|| "local".into()),
        "kind": kind_str(s.kind),
        "class": class_str(s.class),
        "status": status_str(s.status),
        "reason": s.reason.clone().or(fault),
        "model": Some(s.model.name.trim()).filter(|n| !n.is_empty()),
        "quant": Some(s.model.quant.trim()).filter(|n| !n.is_empty()),
        "preset": s.preset,
        "presets": kind_presets,
        "params": params,
        "gpus": s.gpus,
        "gpuNames": gpu_names,
        "vramGiB": vram.map(round1),
        "metric": metric,
        "last": last,
        "load": load,
        "uptimeS": sess.map(|x| x.uptime_s.round()),
        "conflicts": conflicts,
        "controllable": s.controllable,
        "external": s.external,
    })
}
