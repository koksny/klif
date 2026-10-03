//! The probe pool: one scheduler thread and `WORKERS` worker threads. Probes are scheduled per watch key with an
//! in-flight guard (one round per key at a time); each round has a deadline (`ROUND_DEADLINE`, every request's
//! timeout is cut to what is left of it); a key whose health check fails backs off 0.5 s -> 5 s (1 s at most
//! while a KLIF-launched server has never answered, so "online" is not detected late). Due keys are dispatched
//! answering ones first; rounds that may hang (an external not probed yet, any failing key) never hold more than
//! `WORKERS - 1` workers together, so a stalled or dead server never delays another key's probes.
//!
//! A round: the health check (the preset's `health`, else the adapter's default: llama.cpp / vLLM `/health`,
//! sd.cpp / generic TCP, OpenAI-compatible `/health` -> `/v1/models` -> TCP), then what the adapter wants while
//! ready (llama.cpp `/slots`; `/v1/models` once; `/metrics` at its own period; vLLM `/version` once). The API
//! key (only present for KLIF-launched llama.cpp / vLLM, SPEC 16.12) goes to `/slots`, `/v1/models` and
//! `/metrics` of the System's own host, never to `/health`, `/version` or a TCP check. A 401 backs off that
//! endpoint for 30 s (each failed call writes a line in the server's log); 404 / 501 / non-Prometheus text
//! disables it for the session. Results are applied only while the key still has the same generation.

use crate::backend::{HealthPlan, ProbeResult, ProbeWants};
use crate::probe::{self, Fail, Get, MetricsResult, Prober, SlotsResult};
use crate::{lock, Health, Shared, WatchedRef};
use klif_common::Secret;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Worker threads.
pub(crate) const WORKERS: usize = 4;
/// One probe round of one key never takes longer than this.
const ROUND_DEADLINE: Duration = Duration::from_millis(2500);
/// Failure back-off: first step and ceiling (seconds).
const BACKOFF_MIN_S: f64 = 0.5;
const BACKOFF_MAX_S: f64 = 5.0;
/// Ceiling while a KLIF-launched server has never answered (it is starting).
const BACKOFF_STARTING_MAX_S: f64 = 1.0;
/// TCP check timeout.
const TCP_TIMEOUT: Duration = Duration::from_millis(300);
/// After a 401 on an authenticated endpoint.
const AUTH_BACKOFF_S: f64 = 30.0;
/// `/v1/models` retry when it did not answer.
const MODELS_RETRY_S: f64 = 5.0;

/// Scheduler-side state of one key.
struct KeyState {
    /// The watch this state belongs to (a re-watch is a new Arc: the state starts over at once).
    w: WatchedRef,
    generation: u64,
    external: bool,
    in_flight: bool,
    /// Rounds run so far.
    rounds: u64,
    due: f64,
    backoff: f64,
    ever_ok: bool,
    slots_until: f64,
    slots_off: bool,
    metrics_due: f64,
    metrics_off: bool,
    /// Consecutive /metrics failures while the server is ready (3 = it has none: off for the session).
    metrics_fails: u32,
    models_due: f64,
    version_tries: u32,
    /// OpenAI-compatible chain: 0 = /health, 1 = /v1/models, 2 = TCP.
    chain_step: u8,
}

impl KeyState {
    /// Dispatch class, lowest first: 0 answering, 1 a launched server not probed yet, 2 an external not probed
    /// yet, 3 failing (backing off). Classes 2 and 3 may hang until the deadline: together they never hold more
    /// than `WORKERS - 1` rounds, so answering keys always find a worker.
    fn class(&self) -> u8 {
        if self.backoff > 0.0 {
            3
        } else if self.ever_ok {
            0
        } else if self.rounds == 0 && !self.external {
            1
        } else if self.rounds == 0 {
            2
        } else {
            3
        }
    }

    fn suspect(&self) -> bool {
        self.class() >= 2
    }

    fn new(w: WatchedRef, generation: u64, external: bool, now: f64) -> Self {
        KeyState {
            w,
            generation,
            external,
            in_flight: false,
            rounds: 0,
            due: now,
            backoff: 0.0,
            ever_ok: false,
            slots_until: 0.0,
            slots_off: false,
            metrics_due: 0.0,
            metrics_off: false,
            metrics_fails: 0,
            models_due: 0.0,
            version_tries: 0,
            chain_step: 0,
        }
    }
}

/// What one round does (decided by the scheduler from the tracker and the key's state).
struct Job {
    key: String,
    generation: u64,
    w: WatchedRef,
    plan: HealthPlan,
    host: String,
    port: u16,
    api_key: Option<Secret>,
    slots: bool,
    models: bool,
    metrics: bool,
    version: bool,
    chain_step: u8,
}

/// What a round found (sent back to the scheduler).
struct Done {
    key: String,
    generation: u64,
    /// The server answered at all (not Down / not timed out).
    answered: bool,
    chain_step: u8,
    slots: Option<SlotsResult>,
    metrics: Option<MetricsResult>,
    models_ok: Option<bool>,
    version_done: bool,
}

/// Start the scheduler (returned: joined on drop, it exits within 50 ms) and the workers (detached: one may be in
/// the middle of a round against a stalled server for up to `ROUND_DEADLINE`; they exit when the scheduler's job
/// channel closes).
pub(crate) fn start(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>) -> Vec<JoinHandle<()>> {
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (done_tx, done_rx) = mpsc::channel::<Done>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let prober = Arc::new(Prober::new());
    for i in 0..WORKERS {
        let (rx, tx, st, p) = (job_rx.clone(), done_tx.clone(), stop.clone(), prober.clone());
        let _detached = std::thread::Builder::new().name(format!("klif-probe-{i}")).spawn(move || worker(rx, tx, st, p));
    }
    drop(done_tx);
    let mut threads = Vec::new();
    if let Ok(h) = std::thread::Builder::new().name("klif-probe".into()).spawn(move || scheduler(shared, stop, job_tx, done_rx)) {
        threads.push(h);
    }
    threads
}

fn scheduler(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>, jobs: Sender<Job>, done: Receiver<Done>) {
    let mut keys: BTreeMap<String, KeyState> = BTreeMap::new();
    while !stop.load(Ordering::SeqCst) {
        // Completed rounds first (wakes up as soon as one finishes, else every 50 ms).
        match done.recv_timeout(Duration::from_millis(50)) {
            Ok(d) => {
                finish(&mut keys, &shared, d);
                while let Ok(d) = done.try_recv() {
                    finish(&mut keys, &shared, d);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = klif_common::now_s();
        let watched: Vec<(String, WatchedRef)> = lock(&shared).sessions.iter().map(|(k, w)| (k.clone(), w.clone())).collect();
        keys.retain(|k, _| watched.iter().any(|(wk, _)| wk == k));
        // Due keys, answering ones first; suspect rounds in flight never take the last worker.
        let mut due: Vec<(u8, f64, String, WatchedRef)> = Vec::new();
        for (key, w) in watched {
            if let Some(ks) = keys.get(&key) {
                if Arc::ptr_eq(&ks.w, &w) && (ks.in_flight || now < ks.due) {
                    continue;
                }
            }
            let (generation, external) = {
                let g = lock(&w);
                (g.generation, g.tracker.spec().external)
            };
            let ks = keys.entry(key.clone()).or_insert_with(|| KeyState::new(w.clone(), generation, external, now));
            if !Arc::ptr_eq(&ks.w, &w) || ks.generation != generation {
                // A re-watch: start over (a round of the old watch may still finish; its result is ignored).
                *ks = KeyState::new(w.clone(), generation, external, now);
            }
            if ks.in_flight || now < ks.due {
                continue;
            }
            due.push((ks.class(), ks.due, key, w));
        }
        due.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut suspect_in_flight = keys.values().filter(|k| k.in_flight && k.suspect()).count();
        for (class, _, key, w) in due {
            if class >= 2 {
                if suspect_in_flight >= WORKERS - 1 {
                    continue;
                }
                suspect_in_flight += 1;
            }
            let (plan, host, port, api_key, wants) = {
                let g = lock(&w);
                let sp = g.tracker.spec();
                (g.tracker.health_plan(), sp.host.clone(), sp.port, sp.api_key.clone(), g.tracker.probe_wants())
            };
            let Some(ks) = keys.get_mut(&key) else { continue };
            if port == 0 {
                // Nothing to probe (a generic server without a port; its adapter reports it ready while the process
                // runs): look again later in case the watch changes.
                ks.due = now + BACKOFF_MAX_S;
                continue;
            }
            let ProbeWants { slots, models, metrics, version } = wants;
            let job = Job {
                key: key.clone(),
                generation: ks.generation,
                w,
                plan,
                host,
                port,
                api_key,
                slots: slots && !ks.slots_off && now >= ks.slots_until,
                models: models && now >= ks.models_due,
                metrics: metrics.is_some() && !ks.metrics_off && now >= ks.metrics_due,
                version: version && ks.version_tries < 3,
                chain_step: ks.chain_step,
            };
            if let Some(m) = metrics {
                if job.metrics {
                    ks.metrics_due = now + m;
                }
            }
            if job.models {
                ks.models_due = now + MODELS_RETRY_S;
            }
            if job.version {
                ks.version_tries += 1;
            }
            ks.in_flight = true;
            ks.rounds += 1;
            if jobs.send(job).is_err() {
                return;
            }
        }
    }
}

fn finish(keys: &mut BTreeMap<String, KeyState>, shared: &Arc<Mutex<Shared>>, d: Done) {
    let Some(ks) = keys.get_mut(&d.key) else { return };
    if ks.generation != d.generation {
        // A round of an older watch of this key: the new watch has its own state (and maybe its own round).
        return;
    }
    ks.in_flight = false;
    let now = klif_common::now_s();
    let external = ks.external;
    let period = {
        let w = lock(shared).sessions.get(&d.key).cloned();
        w.map(|w| lock(&w).tracker.probe_period()).unwrap_or(1.0)
    };
    ks.chain_step = d.chain_step;
    if d.answered {
        ks.ever_ok = true;
        ks.backoff = 0.0;
        ks.due = now + period;
    } else {
        let cap = if !external && !ks.ever_ok { BACKOFF_STARTING_MAX_S } else { BACKOFF_MAX_S };
        ks.backoff = if ks.backoff <= 0.0 { BACKOFF_MIN_S } else { (ks.backoff * 2.0).min(cap) };
        ks.due = now + ks.backoff;
    }
    match d.slots {
        Some(SlotsResult::Unauthorized) => ks.slots_until = now + AUTH_BACKOFF_S,
        Some(SlotsResult::Unavailable) => ks.slots_off = true,
        _ => {}
    }
    match d.metrics {
        Some(MetricsResult::Ok(_)) => ks.metrics_fails = 0,
        Some(MetricsResult::Unauthorized) => ks.metrics_due = now + AUTH_BACKOFF_S,
        Some(MetricsResult::Unavailable) => ks.metrics_off = true,
        Some(MetricsResult::Failed) => {
            // The server answered its health check but not /metrics, three times in a row (e.g. a TCP-only server
            // that closes the connection): it has no metrics endpoint.
            ks.metrics_fails += 1;
            if ks.metrics_fails >= 3 {
                ks.metrics_off = true;
            }
        }
        None => {}
    }
    if d.models_ok == Some(true) {
        ks.models_due = f64::INFINITY;
    }
    if d.version_done {
        ks.version_tries = u32::MAX;
    }
}

fn worker(jobs: Arc<Mutex<Receiver<Job>>>, done: Sender<Done>, stop: Arc<AtomicBool>, p: Arc<Prober>) {
    while !stop.load(Ordering::SeqCst) {
        let job = {
            let rx = lock(&jobs);
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(j) => j,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        let d = run_round(&p, job);
        if done.send(d).is_err() {
            return;
        }
    }
}

/// Apply to the session only while it still has the job's generation.
fn apply(w: &WatchedRef, generation: u64, f: impl FnOnce(&mut crate::SessionTracker)) {
    let mut g = lock(w);
    if g.generation == generation {
        f(&mut g.tracker);
    }
}

fn run_round(p: &Prober, job: Job) -> Done {
    let start = Instant::now();
    let left = || ROUND_DEADLINE.saturating_sub(start.elapsed());
    let req_timeout = || left().min(probe::DEFAULT_TIMEOUT);
    let (host, port) = (job.host.as_str(), job.port);
    let mut chain_step = job.chain_step;

    // Health.
    let (health, answered) = match &job.plan {
        HealthPlan::Http(path) => match p.get(host, port, path, None, req_timeout(), 0) {
            Get::Status(200, _) => (Health::Ready, true),
            Get::Status(..) => (Health::Loading, true),
            Get::Transport(_) => (Health::Down, false),
        },
        HealthPlan::Tcp => {
            let up = probe::tcp_listening(host, port, TCP_TIMEOUT.min(left()));
            (if up { Health::Ready } else { Health::Down }, up)
        }
        HealthPlan::Chain => chain(p, host, port, job.api_key.as_ref(), &mut chain_step, &req_timeout, &left),
    };
    let at = klif_common::now_s();
    apply(&job.w, job.generation, |t| t.apply_health(at, health));

    let mut out = Done {
        key: job.key,
        generation: job.generation,
        answered,
        chain_step,
        slots: None,
        metrics: None,
        models_ok: None,
        version_done: false,
    };
    if health != Health::Ready {
        return out;
    }
    let key = job.api_key.as_ref();
    let enough = |l: Duration| l >= Duration::from_millis(100);
    if job.slots && enough(left()) {
        let r = p.slots_at(host, port, key, req_timeout(), klif_common::now_s);
        if let SlotsResult::Ok(v) = &r {
            let v = v.clone();
            apply(&job.w, job.generation, |t| t.apply_probe(klif_common::now_s(), ProbeResult::Slots(v)));
        }
        out.slots = Some(r);
    }
    if job.models && enough(left()) {
        let m = p.models_at(host, port, key, req_timeout());
        out.models_ok = Some(m.is_some());
        if let Some(m) = m {
            apply(&job.w, job.generation, |t| t.apply_probe(klif_common::now_s(), ProbeResult::Models(m)));
        }
    }
    if job.metrics && enough(left()) {
        let r = p.metrics_at(host, port, key, req_timeout());
        if let MetricsResult::Ok(s) = &r {
            let s = s.clone();
            apply(&job.w, job.generation, |t| t.apply_probe(klif_common::now_s(), ProbeResult::Metrics(s)));
        }
        out.metrics = Some(match r {
            MetricsResult::Ok(_) => MetricsResult::Ok(Vec::new()),
            other => other,
        });
    }
    if job.version && enough(left()) {
        if let Some(v) = p.version_at(host, port, req_timeout()) {
            out.version_done = true;
            apply(&job.w, job.generation, |t| t.apply_probe(klif_common::now_s(), ProbeResult::Version(v)));
        }
    }
    out
}

/// The OpenAI-compatible chain, starting at the step that answered last time: `/health` (200 ready, 503
/// loading, another status -> next step), `/v1/models` (200 ready, 503 loading, another status = an HTTP server
/// that answers: ready), TCP (a listener that does not speak HTTP: ready). No answer at all -> down (the chain
/// starts over next time).
fn chain(
    p: &Prober,
    host: &str,
    port: u16,
    key: Option<&Secret>,
    step: &mut u8,
    req_timeout: &dyn Fn() -> Duration,
    left: &dyn Fn() -> Duration,
) -> (Health, bool) {
    if *step == 0 {
        match p.get(host, port, "/health", None, req_timeout(), 0) {
            Get::Status(200, _) => return (Health::Ready, true),
            Get::Status(503, _) => return (Health::Loading, true),
            Get::Status(..) => *step = 1,
            Get::Transport(Fail::NotHttp) => *step = 2,
            Get::Transport(_) => return (Health::Down, false),
        }
    }
    if *step == 1 {
        match p.get(host, port, "/v1/models", key, req_timeout(), 0) {
            Get::Status(503, _) => return (Health::Loading, true),
            Get::Status(..) => return (Health::Ready, true),
            Get::Transport(Fail::NotHttp) => *step = 2,
            Get::Transport(_) => {
                *step = 0;
                return (Health::Down, false);
            }
        }
    }
    let up = probe::tcp_listening(host, port, TCP_TIMEOUT.min(left()));
    if !up {
        *step = 0;
    }
    (if up { Health::Ready } else { Health::Down }, up)
}
