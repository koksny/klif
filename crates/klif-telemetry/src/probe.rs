//! HTTP / socket probes against the running server. Short timeouts; called only from the probe
//! thread (never the UI thread). The API key goes into the Authorization header and nowhere else.

use crate::Health;
use klif_common::Secret;
use serde_json::Value;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

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
    pub n_ctx: Option<u64>,
    pub size_bytes: Option<u64>,
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
    if h.contains(':') && !h.starts_with('[') {
        format!("http://[{h}]:{port}{path}")
    } else {
        format!("http://{h}:{port}{path}")
    }
}

fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i.max(0) as u64)))
}

impl Prober {
    pub fn new() -> Self {
        let cfg = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_millis(1500)))
            .timeout_connect(Some(Duration::from_millis(400)))
            .http_status_as_error(false)
            .proxy(None)
            .max_idle_connections_per_host(2)
            .build();
        Self { agent: ureq::Agent::new_with_config(cfg) }
    }

    /// llama-server `/health`: 200 ready, 503 loading, no answer down.
    pub fn health(&self, host: &str, port: u16) -> Health {
        match self.agent.get(&url(host, port, "/health")).call() {
            Ok(r) if r.status().as_u16() == 200 => Health::Ready,
            Ok(_) => Health::Loading,
            Err(_) => Health::Down,
        }
    }

    pub fn slots(&self, host: &str, port: u16, key: Option<&Secret>, at_fn: impl Fn() -> f64) -> SlotsResult {
        let mut req = self.agent.get(&url(host, port, "/slots"));
        if let Some(k) = key {
            req = req.header("Authorization", format!("Bearer {}", k.expose()));
        }
        let mut resp = match req.call() {
            Ok(r) => r,
            Err(_) => return SlotsResult::Failed,
        };
        match resp.status().as_u16() {
            200 => {}
            401 | 403 => return SlotsResult::Unauthorized,
            404 | 501 => return SlotsResult::Unavailable,
            _ => return SlotsResult::Failed,
        }
        let at = at_fn();
        let body: Value = match resp.body_mut().with_config().limit(4 * 1024 * 1024).read_json() {
            Ok(v) => v,
            Err(_) => return SlotsResult::Failed,
        };
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

    /// `/v1/models` (once per session): model id, per-slot context, weights size.
    pub fn models(&self, host: &str, port: u16, key: Option<&Secret>) -> Option<ModelsInfo> {
        let mut req = self.agent.get(&url(host, port, "/v1/models"));
        if let Some(k) = key {
            req = req.header("Authorization", format!("Bearer {}", k.expose()));
        }
        let mut resp = req.call().ok()?;
        if resp.status().as_u16() != 200 {
            return None;
        }
        let body: Value = resp.body_mut().with_config().limit(1024 * 1024).read_json().ok()?;
        let d0 = body.get("data").and_then(|d| d.get(0))?;
        let meta = d0.get("meta");
        Some(ModelsInfo {
            id: d0.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
            n_ctx: meta.and_then(|m| u(m, "n_ctx")),
            size_bytes: meta.and_then(|m| u(m, "size")),
        })
    }
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
