//! HTTP / socket probes against a running server. Short timeouts; called only from the probe workers (never
//! the UI thread). The API key goes into the Authorization header and nowhere else, only when the watch has
//! one (`WatchSpec.api_key`: KLIF-launched llama.cpp / vLLM with `api_key = true`), and only to the System's own
//! host (`WatchSpec.host`). `/health` and TCP checks never carry it.

use crate::prom;
use crate::Health;
use klif_common::Secret;
use serde_json::Value;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Default per-request time budget (connect + response).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1500);
/// Connect timeout (a closed localhost port on Windows retries SYN for ~2 s: never wait that long).
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(400);

/// One slot of `GET /slots` (only the fields KLIF reads; shapes per server-context.cpp `to_json`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlotSample {
    /// Epoch seconds when the response arrived.
    pub at: f64,
    pub id: u64,
    pub n_ctx: Option<u64>,
    pub is_processing: bool,
    pub id_task: Option<u64>,
    /// Tokens in the slot's context right now (prompt + generated so far).
    pub n_prompt_tokens: Option<u64>,
    pub n_prompt_tokens_processed: Option<u64>,
    pub n_prompt_tokens_cache: Option<u64>,
    pub n_decoded: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SlotsResult {
    Ok(Vec<SlotSample>),
    /// 401: wrong or missing key. Each failed poll writes a log line on the server: back off.
    Unauthorized,
    /// 404/501: the endpoint is disabled.
    Unavailable,
    /// Timeout, connection error, unparsable body.
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelsInfo {
    pub id: Option<String>,
    /// Context window: llama.cpp `meta.n_ctx`, vLLM `max_model_len`, others `context_length` / `context_window`.
    pub n_ctx: Option<u64>,
    pub size_bytes: Option<u64>,
    /// audio.cpp: some entry is resident (`loaded: true`); None when no entry carries a boolean `loaded`.
    pub loaded: Option<bool>,
    /// audio.cpp: the id of the first resident entry.
    pub loaded_id: Option<String>,
}

/// A `/metrics` answer.
#[derive(Debug, Clone, PartialEq)]
pub enum MetricsResult {
    /// Prometheus text, parsed.
    Ok(Vec<prom::Sample>),
    /// 401/403 (back off).
    Unauthorized,
    /// 404/405/501, or the body is not Prometheus text: the server has no metrics endpoint.
    Unavailable,
    /// Timeout, connection error.
    Failed,
}

/// The outcome of one GET.
#[derive(Debug, Clone, PartialEq)]
pub enum Get {
    /// The server answered with this status (body read only for 200 and when asked for).
    Status(u16, Option<String>),
    /// No HTTP answer.
    Transport(Fail),
}

/// Why a GET got no HTTP answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    /// Connected (or tried) but nothing came back in time.
    Timeout,
    /// Something answered, but not HTTP.
    NotHttp,
    /// Refused, reset, unreachable, no such host.
    Down,
}

fn fail_of(e: &ureq::Error) -> Fail {
    match e {
        ureq::Error::Timeout(_) => Fail::Timeout,
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => Fail::Timeout,
        ureq::Error::Protocol(_) => Fail::NotHttp,
        _ => Fail::Down,
    }
}

pub struct Prober {
    agent: ureq::Agent,
}

impl Default for Prober {
    fn default() -> Self {
        Self::new()
    }
}

/// `0.0.0.0` / `::` binds are probed on loopback.
pub fn probe_host(host: &str) -> &str {
    match host.trim() {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        h => h,
    }
}

fn url(host: &str, port: u16, path: &str) -> String {
    let h = probe_host(host);
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    if h.contains(':') && !h.starts_with('[') {
        format!("http://[{h}]:{port}{path}")
    } else {
        format!("http://{h}:{port}{path}")
    }
}

fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i.max(0) as u64)).or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)))
}

impl Prober {
    pub fn new() -> Self {
        let cfg = ureq::Agent::config_builder()
            .timeout_global(Some(DEFAULT_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .http_status_as_error(false)
            .proxy(None)
            .max_idle_connections_per_host(2)
            .build();
        Self { agent: ureq::Agent::new_with_config(cfg) }
    }

    /// GET `path` within `timeout` (the connect part capped at 0.4 s). `body_limit` > 0 reads a 200 body.
    pub fn get(&self, host: &str, port: u16, path: &str, key: Option<&Secret>, timeout: Duration, body_limit: u64) -> Get {
        let timeout = timeout.max(Duration::from_millis(50));
        let mut req = self
            .agent
            .get(&url(host, port, path))
            .config()
            .timeout_global(Some(timeout))
            .timeout_connect(Some(CONNECT_TIMEOUT.min(timeout)))
            .build();
        if let Some(k) = key {
            req = req.header("Authorization", format!("Bearer {}", k.expose()));
        }
        let mut resp = match req.call() {
            Ok(r) => r,
            Err(e) => return Get::Transport(fail_of(&e)),
        };
        let status = resp.status().as_u16();
        if status != 200 || body_limit == 0 {
            return Get::Status(status, None);
        }
        match resp.body_mut().with_config().limit(body_limit).read_to_string() {
            Ok(s) => Get::Status(200, Some(s)),
            Err(e) => Get::Transport(fail_of(&e)),
        }
    }

    /// GET `path`: 200 ready, any other answer loading, no answer down.
    pub fn health_at(&self, host: &str, port: u16, path: &str, timeout: Duration) -> Health {
        match self.get(host, port, path, None, timeout, 0) {
            Get::Status(200, _) => Health::Ready,
            Get::Status(..) => Health::Loading,
            Get::Transport(_) => Health::Down,
        }
    }

    /// llama-server `/health`: 200 ready, 503 loading, no answer down.
    pub fn health(&self, host: &str, port: u16) -> Health {
        self.health_at(host, port, "/health", DEFAULT_TIMEOUT)
    }

    pub fn slots(&self, host: &str, port: u16, key: Option<&Secret>, at_fn: impl Fn() -> f64) -> SlotsResult {
        self.slots_at(host, port, key, DEFAULT_TIMEOUT, at_fn)
    }

    pub fn slots_at(&self, host: &str, port: u16, key: Option<&Secret>, timeout: Duration, at_fn: impl Fn() -> f64) -> SlotsResult {
        let body = match self.get(host, port, "/slots", key, timeout, 4 * 1024 * 1024) {
            Get::Status(200, Some(b)) => b,
            Get::Status(401 | 403, _) => return SlotsResult::Unauthorized,
            Get::Status(404 | 501, _) => return SlotsResult::Unavailable,
            _ => return SlotsResult::Failed,
        };
        let at = at_fn();
        let Ok(body) = serde_json::from_str::<Value>(&body) else { return SlotsResult::Failed };
        let Some(arr) = body.as_array() else { return SlotsResult::Failed };
        let out = arr
            .iter()
            .map(|s| {
                let nt = s.get("next_token");
                let nt0 = nt.and_then(|v| if v.is_array() { v.get(0) } else { Some(v) });
                SlotSample {
                    at,
                    id: u(s, "id").unwrap_or(0),
                    n_ctx: u(s, "n_ctx"),
                    is_processing: s.get("is_processing").and_then(|v| v.as_bool()).unwrap_or(false),
                    id_task: u(s, "id_task"),
                    n_prompt_tokens: u(s, "n_prompt_tokens"),
                    n_prompt_tokens_processed: u(s, "n_prompt_tokens_processed"),
                    n_prompt_tokens_cache: u(s, "n_prompt_tokens_cache"),
                    n_decoded: nt0.and_then(|v| u(v, "n_decoded")),
                }
            })
            .collect();
        SlotsResult::Ok(out)
    }

    /// `/v1/models` (once per session): model id, context window, weights size.
    pub fn models(&self, host: &str, port: u16, key: Option<&Secret>) -> Option<ModelsInfo> {
        self.models_at(host, port, key, DEFAULT_TIMEOUT)
    }

    pub fn models_at(&self, host: &str, port: u16, key: Option<&Secret>, timeout: Duration) -> Option<ModelsInfo> {
        let Get::Status(200, Some(body)) = self.get(host, port, "/v1/models", key, timeout, 1024 * 1024) else { return None };
        parse_models(&body)
    }

    /// `/metrics` as Prometheus text.
    pub fn metrics_at(&self, host: &str, port: u16, key: Option<&Secret>, timeout: Duration) -> MetricsResult {
        match self.get(host, port, "/metrics", key, timeout, 4 * 1024 * 1024) {
            Get::Status(200, Some(b)) => {
                if prom::looks_like(&b) {
                    MetricsResult::Ok(prom::parse(&b))
                } else {
                    MetricsResult::Unavailable
                }
            }
            Get::Status(401 | 403, _) => MetricsResult::Unauthorized,
            Get::Status(404 | 405 | 501, _) | Get::Transport(Fail::NotHttp) => MetricsResult::Unavailable,
            Get::Status(..) | Get::Transport(_) => MetricsResult::Failed,
        }
    }

    /// vLLM `/version` -> `{"version": "0.6.3"}`.
    pub fn version_at(&self, host: &str, port: u16, timeout: Duration) -> Option<String> {
        let Get::Status(200, Some(body)) = self.get(host, port, "/version", None, timeout, 64 * 1024) else { return None };
        let v: Value = serde_json::from_str(&body).ok()?;
        v.get("version").and_then(|x| x.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s.len() < 64)
    }
}

/// An OpenAI-style `/v1/models` body: `data[0]` with llama.cpp `meta.n_ctx` / `meta.size`, vLLM `max_model_len`,
/// or `context_length` / `context_window`.
pub fn parse_models(body: &str) -> Option<ModelsInfo> {
    let body: Value = serde_json::from_str(body).ok()?;
    let d0 = body.get("data").and_then(|d| d.get(0)).or_else(|| body.get("models").and_then(|d| d.get(0)))?;
    let meta = d0.get("meta");
    let n_ctx = meta
        .and_then(|m| u(m, "n_ctx"))
        .or_else(|| u(d0, "max_model_len"))
        .or_else(|| u(d0, "context_length"))
        .or_else(|| u(d0, "context_window"));
    // audio.cpp lists every configured model with `loaded` (weights resident or not).
    let all: &[Value] = body.get("data").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let flags: Vec<(bool, Option<&str>)> =
        all.iter().filter_map(|m| m.get("loaded").and_then(Value::as_bool).map(|l| (l, m.get("id").and_then(Value::as_str)))).collect();
    Some(ModelsInfo {
        id: d0.get("id").or_else(|| d0.get("name")).and_then(|v| v.as_str()).map(model_id_display),
        n_ctx,
        size_bytes: meta.and_then(|m| u(m, "size")),
        loaded: (!flags.is_empty()).then(|| flags.iter().any(|(l, _)| *l)),
        loaded_id: flags.iter().find(|(l, _)| *l).and_then(|(_, id)| *id).map(model_id_display),
    })
}

/// A model id for display: a filesystem path (llama-server reports `-m` as given) becomes its file name, never a
/// machine path; repository ids ("Qwen/Qwen3-8B") stay as they are. At most 200 characters.
pub fn model_id_display(id: &str) -> String {
    let t = id.trim();
    let b = t.as_bytes();
    let pathy = t.contains('\\') || t.starts_with('/') || t.starts_with('~') || (b.len() > 2 && b[1] == b':' && b[0].is_ascii_alphabetic());
    let s = if pathy { crate::text::file_name(t) } else { t };
    s.chars().take(200).collect()
}

/// Is anything accepting TCP connections on host:port (sd-server has no /health)?
pub fn tcp_listening(host: &str, port: u16, timeout: Duration) -> bool {
    let h = probe_host(host);
    let addrs: Vec<SocketAddr> = match (h, port).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(_) => return false,
    };
    addrs.iter().any(|a| TcpStream::connect_timeout(a, timeout).is_ok())
}
