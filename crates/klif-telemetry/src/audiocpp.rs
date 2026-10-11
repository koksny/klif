//! audio.cpp `audiocpp_server` log lines. Without `--log` the server prints only `audiocpp_server listening on
//! http://H:P`, `audiocpp_server stopped`, a fatal `audiocpp_server failed: <message>` (stderr, exit code 1) and a few
//! stderr notices (`[server] idle N ms: unloaded K model(s)`). With `--log` (stdout; `--log-file` writes a file KLIF
//! does not read) it adds one `[SERVER_HTTP_DEBUG] http.headers method=M path=P ...` line per HTTP request, which
//! gives the request count and the activity; KLIF's own probes (`GET /health`, `GET /v1/models`) are told apart by
//! their path. Per-request timing is only in the responses (headers / `timing`), so TTS and music records come from
//! `klif-cli bench`.
//!
//! With `--log` the server also prints the model's own `[TRACE ts=..] key value` / `[TIMING ts=..] key value` lines
//! while it works. KLIF follows a job from its POST to `session.wall_ms` (every music family and most speech ones
//! print it when the request is done) or an `http.error` line, so a song that takes minutes counts as in flight the
//! whole time. YuE2 also names its stages: `yue2.request` (cot=off|melody|full), `yue2.semantic.abc_generate_ms`
//! (the score is written), `yue2.semantic_ms` (the codes), `yue2.nar_ms` (the render), `yue2.vae_decode_ms`.

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
/// `[TIMING ts=20261011-041523] yue2.semantic_ms 64868.7` / `[TRACE ts=..] yue2.request seed=1234 cot=off ...`.
static TRACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[(?:TIMING|TRACE)(?:\s+ts=\S+)?\]\s+(\S+)(?:\s+(.*))?$").expect("audiocpp trace"));

/// Finished jobs kept for the live view.
const RECENT_JOBS: usize = 24;

/// A request the server is working on (from its POST to `session.wall_ms` or an error).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioJob {
    /// The POST line's arrival (epoch s).
    pub started: f64,
    /// `load` until the model names the request, then its stage (`score`, `codes`, `render`, `decode`) or `work`.
    pub phase: &'static str,
    /// The stage in flight (1-based) / all stages (0 / 0 for a family whose stages KLIF does not know).
    pub step: u32,
    pub steps: u32,
}

/// A finished job: when it ended and how long the model said it took.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioJobDone {
    pub at: f64,
    pub seconds: f64,
}

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
    /// The job in flight.
    pub job: Option<AudioJob>,
    /// Finished jobs, oldest first (up to 24).
    pub done: std::collections::VecDeque<AudioJobDone>,
    /// Jobs finished this session.
    pub jobs_done: u64,
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
                // A model-management call is not a job.
                if !path.contains("unload") && !path.contains("load_model") {
                    self.job = Some(AudioJob { started: at, phase: "load", step: 0, steps: 0 });
                }
                return AudioLine::Request(path);
            }
            if &c[1] == "error" {
                self.job = None;
            }
            return AudioLine::Other;
        }
        if let Some(c) = TRACE.captures(l) {
            self.trace(&c[1], c.get(2).map_or("", |m| m.as_str()), at);
            return AudioLine::Other;
        }
        if let Some(c) = LISTENING.captures(l) {
            self.listening = Some(c[1].to_string());
            self.stopped = false;
            self.job = None;
            return AudioLine::Listening;
        }
        if STOPPED.is_match(l) {
            self.stopped = true;
            self.job = None;
            return AudioLine::Stopped;
        }
        if let Some(c) = FAILED.captures(l) {
            let msg = c[1].to_string();
            self.fatal.get_or_insert_with(|| msg.clone());
            self.job = None;
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

    /// A `[TRACE]` / `[TIMING]` line of the model: the job's stages and its end.
    fn trace(&mut self, key: &str, value: &str, at: f64) {
        if key == "session.wall_ms" {
            if let Some(j) = self.job.take() {
                let seconds = value.trim().parse::<f64>().map(|ms| ms / 1000.0).unwrap_or(at - j.started);
                self.done.push_back(AudioJobDone { at, seconds: (seconds * 10.0).round() / 10.0 });
                while self.done.len() > RECENT_JOBS {
                    self.done.pop_front();
                }
                self.jobs_done += 1;
            }
            return;
        }
        let Some(j) = self.job.as_mut() else { return };
        // YuE2: the score (unless cot=off), the codes, the render, the VAE; `step` = the stage in flight, 1-based like a
        // sampling step. Each line ends a stage; a given score (cot=melody|full with `abc`) writes no score line, so the
        // stage numbers are set from the end, not counted.
        let (phase, step) = match key {
            "yue2.request" => {
                let score = !value.split_whitespace().any(|w| w == "cot=off");
                j.steps = if score { 4 } else { 3 };
                (if score { "score" } else { "codes" }, 1)
            }
            "yue2.semantic.abc_generate_ms" => ("codes", 2),
            "yue2.semantic_ms" => ("render", j.steps.saturating_sub(1)),
            "yue2.nar_ms" | "yue2.vae_decode_ms" => ("decode", j.steps),
            // Another family's own line: it is past loading, its stages are unknown.
            _ if j.phase == "load" && !key.starts_with("server.") && !key.starts_with("runtime.") => ("work", 0),
            _ => return,
        };
        j.phase = phase;
        j.step = step.min(j.steps);
    }
}

/// The line is a debug line of KLIF's own probes (hidden from the console).
pub fn is_probe_line(line: &str) -> bool {
    HTTP_DEBUG.captures(line.trim()).is_some_and(|c| {
        let method = c.get(2).map(|m| m.as_str().to_ascii_uppercase());
        c.get(3).is_some_and(|p| probe_path(p.as_str())) && method.as_deref().is_none_or(|m| m == "GET" || m == "HEAD")
    })
}
