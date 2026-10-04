//! audio.cpp `audiocpp_server` log lines. Without `--log` the server prints only `audiocpp_server listening on
//! http://H:P`, `audiocpp_server stopped`, a fatal `audiocpp_server failed: <message>` (stderr, exit code 1) and a few
//! stderr notices (`[server] idle N ms: unloaded K model(s)`). With `--log` (stdout; `--log-file` writes a file KLIF
//! does not read) it adds one `[SERVER_HTTP_DEBUG] http.headers method=M path=P ...` line per HTTP request, which
//! gives the request count and the activity; KLIF's own probes (`GET /health`, `GET /v1/models`) are told apart by
//! their path. Per-request timing is only in the responses (headers / `timing`), so TTS records come from
//! `klif-cli bench`.

use regex::Regex;
use std::sync::LazyLock;

static LISTENING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^audiocpp_server listening on (https?://\S+)").expect("audiocpp listening"));
static STOPPED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^audiocpp_server stopped\b").expect("audiocpp stopped"));
static FAILED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^audiocpp_server failed: (.+?)\s*$").expect("audiocpp failed"));
static HTTP_DEBUG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[SERVER_HTTP_DEBUG\]\s+http\.(\w+)\b(?:.*?\bmethod=(\w+))?(?:.*?\bpath=(\S+))?").expect("audiocpp http debug")
});
static IDLE_UNLOAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[server\] idle \d+ ms: unloaded (\d+) model").expect("audiocpp idle unload"));

/// Paths KLIF's probes ask for (their debug lines are KLIF's own traffic, not the user's).
const PROBE_PATHS: [&str; 4] = ["/health", "/v1/models", "/metrics", "/sdcpp/v1/capabilities"];

fn probe_path(path: &str) -> bool {
    let p = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    PROBE_PATHS.contains(&p)
}

/// What one log line was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioLine {
    /// A debug line of one of KLIF's own probes.
    Probe,
    /// A new API request (POST).
    Request(String),
    Listening,
    Stopped,
    /// `audiocpp_server failed: <message>`.
    Fatal(String),
    /// The idle timer unloaded the models.
    Unloaded,
    Other,
}

/// What the audio.cpp log said so far.
#[derive(Debug, Clone, Default)]
pub struct AudioCppLog {
    pub listening: Option<String>,
    pub stopped: bool,
    pub fatal: Option<String>,
    /// POST requests seen in the debug lines; None until the first debug line (no `--log`: unknown).
    pub requests: Option<u64>,
    /// Arrival of the last request line (epoch s).
    pub last_request: Option<f64>,
    /// When the idle timer last unloaded the models (epoch s).
    pub unloaded_at: Option<f64>,
}

impl AudioCppLog {
    /// One log segment.
    pub fn feed(&mut self, line: &str, at: f64) -> AudioLine {
        let l = line.trim();
        if let Some(c) = HTTP_DEBUG.captures(l) {
            self.requests.get_or_insert(0);
            let method = c.get(2).map(|m| m.as_str().to_ascii_uppercase());
            let path = c.get(3).map(|m| m.as_str().to_string()).unwrap_or_default();
            if probe_path(&path) && method.as_deref().is_none_or(|m| m == "GET" || m == "HEAD") {
                return AudioLine::Probe;
            }
            if &c[1] == "headers" && method.as_deref() == Some("POST") {
                self.requests = Some(self.requests.unwrap_or(0) + 1);
                self.last_request = Some(at);
                return AudioLine::Request(path);
            }
            return AudioLine::Other;
        }
        if let Some(c) = LISTENING.captures(l) {
            self.listening = Some(c[1].to_string());
            self.stopped = false;
            return AudioLine::Listening;
        }
        if STOPPED.is_match(l) {
            self.stopped = true;
            return AudioLine::Stopped;
        }
        if let Some(c) = FAILED.captures(l) {
            let msg = c[1].to_string();
            self.fatal.get_or_insert_with(|| msg.clone());
            return AudioLine::Fatal(msg);
        }
        if let Some(c) = IDLE_UNLOAD.captures(l) {
            if c[1].parse::<u64>().unwrap_or(0) > 0 {
                self.unloaded_at = Some(at);
            }
            return AudioLine::Unloaded;
        }
        AudioLine::Other
    }
}

/// The line is a debug line of KLIF's own probes (hidden from the console).
pub fn is_probe_line(line: &str) -> bool {
    HTTP_DEBUG.captures(line.trim()).is_some_and(|c| {
        let method = c.get(2).map(|m| m.as_str().to_ascii_uppercase());
        c.get(3).is_some_and(|p| probe_path(p.as_str())) && method.as_deref().is_none_or(|m| m == "GET" || m == "HEAD")
    })
}
