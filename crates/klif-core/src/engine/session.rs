//! One System's session: the phase machine (starting -> loading -> live -> stopping / fault), fault building,
//! adoption, the stop / cleanup threads, dormancy narration and the last-session summary. Ported per session from
//! the 0.2 single-session engine.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::{anyhow, Result};
use klif_common::config::{normalize_gpu_id, Config};
use klif_common::now_s;
use klif_common::vm::{
    AdapterId, Ended, Endpoint, Fault, GenericLive, ImageActivity, ImageLive, LastSession, LlmActivity, LlmLive, LoadProgress, LoadStep,
    LoadStepId, Phase, Session, StepState, SystemId, SystemKind, VramLayer, VramLayerId,
};
use klif_supervisor::{Owned, ProcState, ProcessHost, Supervisor};
use klif_telemetry::{GpuSnapshot, Health, SessionSignals, TelemetrySnapshot, WatchSpec};

use super::{r1, Inner, State, CONSOLE_LEN};
use crate::keys;
use crate::narrate::{dormant_line, wake_line};
use crate::state::{PersistedLast, PersistedSession, MAX_LAYERS};

/// A fatal log line while the process stays up and the server is not ready: wait this long for it to exit.
const FATAL_GRACE_S: f64 = 10.0;
const LOG_TAIL_LEN: usize = 12;

pub(crate) struct FaultRec {
    pub(crate) title: String,
    pub(crate) exit_code: Option<i64>,
    pub(crate) exit_code_hex: Option<String>,
    pub(crate) log_tail: Vec<String>,
    pub(crate) at: f64,
    pub(crate) steps: Option<Vec<LoadStep>>,
}

/// A session KLIF owns (launched, or adopted after a restart).
pub(crate) struct SessionCtx {
    pub(crate) p: PersistedSession,
    pub(crate) owned: Arc<Owned>,
    pub(crate) phase: Phase,
    pub(crate) fault: Option<FaultRec>,
    /// When Stop was requested (uptime ends there).
    pub(crate) stop_at: Option<f64>,
    pub(crate) stopper: Option<JoinHandle<Result<()>>>,
    /// Cleanup of what is left of a faulted session's tree.
    pub(crate) cleanup: Option<JoinHandle<Result<()>>>,
    /// Dismiss was asked while processes were left: the session ends once the cleanup is done.
    pub(crate) dismissing: bool,
    pub(crate) fatal_since: Option<f64>,
    /// KLIF's own lines shown before the logs (launch facts) and after them (stop / fault notes).
    pub(crate) header: Vec<String>,
    pub(crate) tail: Vec<String>,
    /// The session's console as telemetry last reported it (kept: `unwatch` drops telemetry's copy).
    pub(crate) console: Vec<String>,
    pub(crate) been_live: bool,
    /// Every PID seen in the session's tree: a dying member still listed in the TCP table is not foreign.
    pub(crate) known_pids: BTreeSet<u32>,
    /// The session's live PIDs now (empty: nothing of it is left).
    pub(crate) pids: Vec<u32>,
    pub(crate) last_llm: Option<LlmLive>,
    pub(crate) last_image: Option<ImageLive>,
    pub(crate) last_generic: Option<GenericLive>,
    pub(crate) median_tps: Option<f64>,
    /// PDH Total Committed / Dedicated Usage over the session's pids (GiB), and the peak of committed.
    pub(crate) committed: f64,
    pub(crate) resident: f64,
    pub(crate) peak_committed: f64,
    /// The same per GPU: (GPU id, resident GiB, committed GiB), for the per-GPU VRAM rule (SPEC 16.6 / 16.22).
    pub(crate) per_gpu: Vec<(String, f64, f64)>,
    /// This session's VRAM layers now, and the last non-empty ones while live (persisted at the end).
    pub(crate) layers: Vec<VramLayer>,
    pub(crate) live_layers: Vec<VramLayer>,
    pub(crate) busy: bool,
    pub(crate) activity: f64,
    /// Dormancy episodes already narrated (entry, wake); None until the first look at its GPU.
    pub(crate) dormant_seen: Option<(u64, u64)>,
}

impl SessionCtx {
    pub(crate) fn new(p: PersistedSession, owned: Owned, phase: Phase, header: Vec<String>) -> SessionCtx {
        SessionCtx {
            p,
            owned: Arc::new(owned),
            phase,
            fault: None,
            stop_at: None,
            stopper: None,
            cleanup: None,
            dismissing: false,
            fatal_since: None,
            header,
            tail: Vec::new(),
            console: Vec::new(),
            been_live: false,
            known_pids: BTreeSet::new(),
            pids: Vec::new(),
            last_llm: None,
            last_image: None,
            last_generic: None,
            median_tps: None,
            committed: 0.0,
            resident: 0.0,
            peak_committed: 0.0,
            per_gpu: Vec::new(),
            layers: Vec::new(),
            live_layers: Vec::new(),
            busy: false,
            activity: 0.0,
            dormant_seen: None,
        }
    }

    pub(crate) fn started_at(&self) -> f64 {
        self.p.record.started_at
    }

    /// Starting or loading (the reservation then covers the expected VRAM).
    pub(crate) fn loading(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Loading)
    }

    /// The VRAM this session holds or will hold (SPEC 16.5).
    pub(crate) fn reservation(&self) -> f64 {
        if self.loading() {
            self.committed.max(self.p.expected_gib.unwrap_or(0.0))
        } else {
            self.committed
        }
    }

    /// Header + logs + KLIF lines, newest last, capped.
    pub(crate) fn console_lines(&self) -> Vec<String> {
        let mut c: Vec<String> = Vec::with_capacity(self.header.len() + self.console.len() + self.tail.len());
        c.extend(self.header.iter().cloned());
        c.extend(self.console.iter().cloned());
        c.extend(self.tail.iter().cloned());
        cap(&mut c);
        c
    }
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub(crate) fn cap(c: &mut Vec<String>) {
    if c.len() > CONSOLE_LEN {
        c.drain(..c.len() - CONSOLE_LEN);
    }
}

// ------------------------------------------------------------------------------------------ helpers

pub(crate) fn display_host(host: &str) -> &str {
    match host.trim() {
        "" | "0.0.0.0" | "*" => "127.0.0.1",
        "::" | "[::]" => "::1",
        h => h,
    }
}

/// `http://host:port` (+ `/v1` for an LLM): the base URL clients use.
pub(crate) fn base_url(kind: SystemKind, host: &str, port: u16) -> String {
    let h = display_host(host);
    let h = if h.contains(':') && !h.starts_with('[') { format!("[{h}]") } else { h.to_string() };
    let v1 = if kind == SystemKind::Llm { "/v1" } else { "" };
    format!("http://{h}:{port}{v1}")
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.retain(|x| x.is_finite() && *x > 0.0);
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

fn step_failed_detail(id: LoadStepId) -> &'static str {
    match id {
        LoadStepId::Process => "process did not start",
        LoadStepId::Device => "device listing failed",
        LoadStepId::Weights => "weights not loaded",
        LoadStepId::Kv => "allocation failed",
        LoadStepId::Warmup => "warm-up failed",
        LoadStepId::Ready => "never became ready",
    }
}

/// The load steps at the moment the session died: the running step is marked failed.
fn fail_steps(mut steps: Vec<LoadStep>) -> Vec<LoadStep> {
    if steps.iter().any(|s| s.state == StepState::Failed) {
        return steps;
    }
    let idx = steps.iter().rposition(|s| s.state == StepState::Active).or_else(|| steps.iter().position(|s| s.state == StepState::Pending));
    if let Some(i) = idx {
        steps[i].state = StepState::Failed;
        steps[i].detail = Some(step_failed_detail(steps[i].id).to_string());
    }
    steps
}

/// Did the server get past "process spawned" (log lines, a device, an answering socket)?
fn progressed(sig: &SessionSignals) -> bool {
    sig.health != Health::Down
        || sig.load_steps.first().map(|s| s.state == StepState::Done).unwrap_or(false)
        || sig.load_steps.iter().skip(1).any(|s| s.state != StepState::Pending)
}

/// 0..1, how hard a live server works right now (tab pulse / hero drive).
pub(crate) fn activity_of(sig: &SessionSignals) -> f64 {
    let a: f64 = if let Some(l) = &sig.llm {
        match l.activity {
            LlmActivity::Decode => 1.0,
            LlmActivity::Prefill => 0.7,
            LlmActivity::Idle => 0.0,
        }
    } else if let Some(i) = &sig.image {
        if i.activity == ImageActivity::Generating {
            1.0
        } else {
            0.0
        }
    } else if sig.generic.as_ref().and_then(|g| g.requests_in_flight).unwrap_or(0) > 0 {
        1.0
    } else {
        0.0
    };
    if sig.busy {
        a.max(0.6)
    } else {
        a
    }
}

pub(crate) enum Cause {
    Exited(u32),
    Gone,
    Fatal,
}

pub(crate) fn build_fault(
    kind: SystemKind,
    adapter: AdapterId,
    phase: Phase,
    cause: &Cause,
    sig: Option<&SessionSignals>,
    console: &[String],
    now: f64,
) -> FaultRec {
    let loading = matches!(phase, Phase::Starting | Phase::Loading);
    let base = match cause {
        Cause::Exited(_) if loading => "The server exited while loading",
        Cause::Exited(_) => "The server exited unexpectedly",
        Cause::Gone if loading => "The server process disappeared while loading",
        Cause::Gone => "The server process disappeared",
        Cause::Fatal => "The server reported a fatal error and did not recover",
    };
    let hint = sig.and_then(|s| s.fatal_hint.clone());
    let title = match &hint {
        Some(h) => format!("{base}: {}.", h.trim().trim_end_matches('.')),
        None => format!("{base}."),
    };
    // Starter scripts print the server's own exit code; it says more than the starter's.
    let starter = sig.and_then(|s| s.starter_exit).filter(|c| *c != 0);
    let code: Option<i64> = starter.or(match cause {
        Cause::Exited(c) => Some(*c as i32 as i64),
        _ => None,
    });
    let exit_code_hex = code.map(|c| c as i32 as u32).filter(|u| u & 0x8000_0000 != 0).map(|u| format!("0x{u:08X}"));
    let err_tail: Vec<String> = sig.map(|s| s.error_tail.clone()).unwrap_or_default();
    let src: &[String] = if err_tail.len() >= 3 { &err_tail } else { console };
    let log_tail = src[src.len().saturating_sub(LOG_TAIL_LEN)..].to_vec();
    let steps = if loading {
        let steps =
            sig.map(|s| s.load_steps.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| klif_telemetry::steps::initial_steps(adapter, kind));
        Some(fail_steps(steps))
    } else {
        None
    };
    FaultRec { title, exit_code: code, exit_code_hex, log_tail, at: now, steps }
}

pub(crate) fn spawn_stop(owned: Arc<Owned>, what: &'static str) -> Option<JoinHandle<Result<()>>> {
    std::thread::Builder::new()
        .name(format!("klif-{what}"))
        .spawn(move || Supervisor::new().stop(&owned))
        .map_err(|e| log::error!("could not start the {what} thread: {e}"))
        .ok()
}

fn join_result(h: JoinHandle<Result<()>>) -> Result<()> {
    h.join().unwrap_or_else(|_| Err(anyhow!("the stop thread panicked")))
}

/// The GPU snapshot a session's GPU id refers to (exact id, else the same PCI id with `#0` / no index).
pub(crate) fn gpu_snap<'a>(snap: &'a TelemetrySnapshot, gpu: Option<&str>) -> Option<&'a GpuSnapshot> {
    match gpu {
        Some(g) => snap.gpus.iter().find(|s| super::conflicts::gpu_eq(&s.memory.id, g)),
        None => snap.gpus.first(),
    }
}

enum Outcome {
    Nothing,
    End(Ended),
    Fault(FaultRec, bool),
}

/// The model key arches are learned under: the preset's model path (lower case), else `preset:<id>`.
pub(crate) fn model_key(cfg: &Config, preset: Option<&str>) -> Option<String> {
    let pid = preset?;
    let spec = cfg.presets.get(pid);
    Some(match spec.and_then(|s| s.model.as_deref()).map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => m.replace('/', "\\").to_ascii_lowercase(),
        None => format!("preset:{pid}"),
    })
}

// ------------------------------------------------------------------------------------------ engine

impl Inner {
    /// What telemetry watches for a session: its PERSISTED kind / adapter / port (never the current config).
    /// The API key only for llama.cpp / vllm sessions KLIF launched with the key (SPEC 16.12).
    pub(super) fn session_watch(&self, cfg: &Config, p: &PersistedSession) -> WatchSpec {
        let adapter = p.adapter();
        let api_key = if p.api_key_set && matches!(adapter, AdapterId::LlamaCpp | AdapterId::Vllm) { keys::load(cfg) } else { None };
        WatchSpec {
            kind: p.kind,
            adapter,
            external: false,
            out_log: Some(p.record.out_log.clone()),
            err_log: Some(p.record.err_log.clone()),
            host: display_host(&p.host).to_string(),
            port: p.record.port,
            api_key,
            started_at: p.record.started_at,
            ctx_tokens: p.ctx_tokens,
            spec_mode: p.spec_mode.clone(),
            health: p.health.clone().unwrap_or_default(),
            metrics: p.metrics,
            expect_device: Some(p.model.device.trim().to_string()).filter(|d| !d.is_empty()),
            // Every GPU of a multi-GPU preset (comma list): telemetry attributes the session's memory on each.
            gpu: Some(p.all_gpus().join(",")).filter(|g| !g.is_empty()),
        }
    }

    pub(super) fn begin_session(&self, st: &mut State, cfg: &Config, id: SystemId, ctx: SessionCtx, from_start: bool) {
        let spec = self.session_watch(cfg, &ctx.p);
        self.tel(|t| t.watch(id.as_str(), spec, from_start));
        st.externals.remove(&id);
        st.idle_tails.remove(&id);
        st.sessions.insert(id, ctx);
        st.session_seq += 1;
        st.dirty = true;
    }

    /// Re-attach to a persisted session (job by name, else root PID + creation time). False when it is gone.
    pub(super) fn adopt_persisted(&self, st: &mut State, cfg: &Config, id: SystemId, mut p: PersistedSession) -> bool {
        if st.sessions.contains_key(&id) {
            return false;
        }
        let owned = match self.sup.adopt(&p.record) {
            Ok(o) => o,
            Err(e) => {
                log::info!("the persisted session {} of {id} is gone ({e:#}); forgetting it", p.record.session_name);
                return false;
            }
        };
        log::info!(
            "adopted session {} of {id} (root pid {}, job {})",
            p.record.session_name,
            p.record.root_pid,
            if owned.has_job() { "open" } else { "none" }
        );
        p.system = id.clone();
        if p.gpu.is_none() && p.gpus.is_empty() {
            // 0.2 sessions (and some ghosts) record no GPU. A preset without `gpu` runs on [gpu] inference (SPEC 3),
            // and 0.2 always ran on the inference card: without this the exclusive / VRAM rules never pair the
            // adopted session with a System on that GPU (an unknown GPU only matches an unknown GPU).
            let spec_gpus = p
                .preset
                .as_deref()
                .or(Some(p.card_id.as_str()))
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .and_then(|k| cfg.presets.get(k))
                .map(|s| cfg.preset_gpus(s))
                .unwrap_or_default();
            let gpus: Vec<String> = if spec_gpus.is_empty() {
                cfg.gpu.inference.as_deref().and_then(normalize_gpu_id).into_iter().collect()
            } else {
                spec_gpus
            };
            if !gpus.is_empty() {
                log::info!("{id}: the persisted session records no GPU; it is counted on {}", gpus.join(", "));
            }
            p.gpu = gpus.first().cloned();
            p.gpus = gpus;
        }
        let mut header = vec![
            format!("[KLIF] adopted {} (pid {}) after a KLIF restart", p.record.session_name, p.record.root_pid),
            format!("[KLIF] stdout: {}", p.record.out_log.display()),
            format!("[KLIF] stderr: {}", p.record.err_log.display()),
        ];
        if !owned.has_job() {
            header.push("[KLIF] warning: its job could not be opened; KLIF can watch this session but not stop it".into());
        }
        if cfg.system(id.as_str()).is_none() {
            header.push(format!("[KLIF] {id} is not in klif.toml; it can only be stopped"));
        }
        let ctx = SessionCtx::new(p, owned, Phase::Loading, header);
        self.begin_session(st, cfg, id, ctx, true);
        true
    }

    pub(super) fn begin_stop(&self, st: &mut State, id: &SystemId) {
        let Some(ctx) = st.sessions.get_mut(id) else { return };
        if ctx.phase == Phase::Stopping {
            return;
        }
        log::info!("stopping {} ({id})", ctx.p.record.session_name);
        ctx.phase = Phase::Stopping;
        ctx.stop_at = Some(now_s());
        ctx.tail.push(format!("[KLIF] stopping {}", ctx.p.record.session_name));
        ctx.stopper = spawn_stop(ctx.owned.clone(), "stop");
        st.dirty = true;
    }

    /// End a faulted session (it becomes the last session). What is left of its tree is stopped first, on the
    /// cleanup thread; the session then ends on a later tick.
    pub(super) fn dismiss(&self, st: &mut State, id: &SystemId) {
        let Some(ctx) = st.sessions.get_mut(id) else { return };
        if ctx.phase != Phase::Fault {
            return;
        }
        if ctx.cleanup.is_none() && ctx.pids.is_empty() && self.sup.tree_pids(&ctx.owned).is_empty() {
            self.end_session(st, id, Ended::Fault, now_s());
            return;
        }
        ctx.dismissing = true;
        if ctx.cleanup.is_none() {
            ctx.tail.push("[KLIF] stopping what is left of the session's processes".into());
            ctx.cleanup = spawn_stop(ctx.owned.clone(), "cleanup");
        }
    }

    pub(super) fn end_session(&self, st: &mut State, id: &SystemId, ended: Ended, now: f64) {
        let Some(ctx) = st.sessions.remove(id) else { return };
        st.session_seq += 1;
        let (up_end, ended_at) = match (ended, &ctx.fault) {
            (Ended::Fault, Some(f)) => (f.at, f.at),
            _ => (ctx.stop_at.unwrap_or(now), now),
        };
        let llm = ctx.last_llm.as_ref();
        let img = ctx.last_image.as_ref();
        let decode = ctx.median_tps.or_else(|| {
            median(llm.map(|l| l.requests.iter().filter(|r| r.decode_s > 0.0).map(|r| r.generated_tokens as f64 / r.decode_s).collect()).unwrap_or_default())
        });
        let summary = LastSession {
            system: id.clone(),
            model: ctx.p.model.clone(),
            uptime_s: (up_end - ctx.started_at()).max(0.0).round(),
            ended_ago_s: 0.0,
            ended,
            requests: llm.map(|l| l.totals.requests).or_else(|| ctx.last_generic.as_ref().and_then(|g| g.requests_total)),
            generated_tokens: llm.map(|l| l.totals.generated_tokens),
            decode_tps: decode.map(r1),
            images: img.map(|i| i.images_this_session),
            seconds_per_image: img.and_then(|i| median(i.recent.iter().map(|j| j.seconds).collect())).map(r1),
        };
        log::info!("session {} of {id} ended ({:?}) after {} s", ctx.p.record.session_name, ended, summary.uptime_s);
        st.last.insert(id.clone(), PersistedLast { summary, ended_at });

        // Measured VRAM of this exact command (SPEC 16.23): llama.cpp / vllm layers from the logs, else the peak.
        let hash = ctx.p.command.as_ref().map(|c| c.hash.clone()).unwrap_or_default();
        if ctx.been_live && !hash.is_empty() {
            let parsed = matches!(ctx.p.adapter(), AdapterId::LlamaCpp | AdapterId::Vllm) && ctx.p.kind == SystemKind::Llm;
            let layers = if parsed && !ctx.live_layers.is_empty() {
                Some(ctx.live_layers.clone())
            } else if ctx.peak_committed > 0.05 {
                Some(vec![VramLayer { id: VramLayerId::Weights, label: "Measured peak".into(), gib: r2(ctx.peak_committed) }])
            } else {
                None
            };
            if let Some(l) = layers {
                st.layers.insert(hash.clone(), l);
                while st.layers.len() > MAX_LAYERS {
                    let Some(k) = st.layers.keys().find(|k| **k != hash).cloned() else { break };
                    st.layers.remove(&k);
                }
            }
        }

        let mut tail = ctx.console_lines();
        tail.push(format!(
            "[KLIF] {} {}",
            ctx.p.record.session_name,
            if ended == Ended::Stopped { "stopped" } else { "ended with a fault" }
        ));
        cap(&mut tail);
        st.idle_tails.insert(id.clone(), tail);
        self.tel(|t| t.unwatch(id.as_str()));
        st.dirty = true;
    }

    /// One step of every session: process state, telemetry signals, phases, faults, ends.
    pub(super) fn advance(&self, st: &mut State, cfg: &Config, snap: Option<&TelemetrySnapshot>, now: f64) {
        let ids: Vec<SystemId> = st.sessions.keys().cloned().collect();
        let mut outcomes: Vec<(SystemId, Outcome)> = Vec::new();
        let mut finished: Vec<SystemId> = Vec::new();
        for id in ids {
            let State { sessions, arches, dirty, .. } = &mut *st;
            let Some(ctx) = sessions.get_mut(&id) else { continue };
            let ps = self.sup.state(&ctx.owned);
            let pids = match &ps {
                ProcState::Running { pids } => pids.clone(),
                _ => self.sup.tree_pids(&ctx.owned),
            };
            ctx.known_pids.extend(pids.iter().copied());
            ctx.pids = pids.clone();
            self.tel(|t| t.set_session_pids(id.as_str(), pids));
            let sig = snap.and_then(|s| s.sessions.get(id.as_str()));
            if let Some(sig) = sig {
                if !sig.console.is_empty() || ctx.console.is_empty() {
                    ctx.console = sig.console.clone();
                }
                if ctx.phase != Phase::Fault {
                    ctx.committed = sig.committed_gib;
                    ctx.resident = sig.resident_gib;
                    ctx.per_gpu = sig.per_gpu.iter().map(|g| (g.id.clone(), g.resident_gib, g.committed_gib)).collect();
                    ctx.peak_committed = ctx.peak_committed.max(sig.committed_gib);
                    ctx.layers = sig.layers.clone();
                }
                if matches!(ctx.phase, Phase::Live | Phase::Stopping) {
                    if sig.llm.is_some() {
                        ctx.last_llm = sig.llm.clone();
                    }
                    if sig.image.is_some() {
                        ctx.last_image = sig.image.clone();
                    }
                    if sig.generic.is_some() {
                        ctx.last_generic = sig.generic.clone();
                    }
                    if sig.median_decode_tps.is_some() {
                        ctx.median_tps = sig.median_decode_tps;
                    }
                }
                if ctx.phase == Phase::Live {
                    if !sig.layers.is_empty() {
                        ctx.live_layers = sig.layers.clone();
                    }
                    ctx.activity = activity_of(sig);
                    ctx.busy = sig.busy || ctx.activity > 0.0;
                } else {
                    ctx.activity = 0.0;
                    ctx.busy = false;
                }
                if let Some(a) = sig.arch.as_ref().filter(|a| ctx.p.model.arch.as_ref() != Some(*a)) {
                    ctx.p.model.arch = Some(a.clone());
                    if let Some(k) = model_key(cfg, ctx.p.preset.as_deref()) {
                        arches.insert(k, a.clone());
                    }
                    *dirty = true;
                }
            }
            if let Some(t) = snap {
                self.narrate_dormancy(&id, ctx, t);
            }
            let console = ctx.console.clone();
            let out = match ctx.phase {
                Phase::Stopping => {
                    if ctx.stopper.as_ref().map(|h| h.is_finished()).unwrap_or(true) {
                        match ctx.stopper.take().map(join_result).unwrap_or(Ok(())) {
                            Ok(()) => Outcome::End(Ended::Stopped),
                            Err(e) => {
                                log::error!("stop of {id} failed: {e:#}");
                                let mut f = build_fault(ctx.p.kind, ctx.p.adapter(), Phase::Live, &Cause::Gone, sig, &console, now);
                                f.title = format!("KLIF could not stop the session: {}.", format!("{e:#}").trim_end_matches('.'));
                                f.steps = None;
                                Outcome::Fault(f, false)
                            }
                        }
                    } else {
                        Outcome::Nothing
                    }
                }
                Phase::Fault => {
                    if ctx.cleanup.as_ref().map(|h| h.is_finished()).unwrap_or(false) {
                        if let Some(h) = ctx.cleanup.take() {
                            match join_result(h) {
                                Ok(()) => ctx.tail.push("[KLIF] the rest of the session's processes were stopped".into()),
                                Err(e) => {
                                    ctx.tail.push(format!("[KLIF] cleanup failed: {e:#}"));
                                    ctx.dismissing = false;
                                }
                            }
                        }
                    }
                    let ghost = cfg.system(id.as_str()).is_none();
                    if (ctx.dismissing || ghost) && ctx.cleanup.is_none() && ctx.pids.is_empty() {
                        finished.push(id.clone());
                    }
                    Outcome::Nothing
                }
                Phase::Starting | Phase::Loading | Phase::Live => match ps {
                    ProcState::Exited { code } => {
                        let left = !ctx.pids.is_empty();
                        Outcome::Fault(build_fault(ctx.p.kind, ctx.p.adapter(), ctx.phase, &Cause::Exited(code), sig, &console, now), left)
                    }
                    ProcState::Gone => {
                        Outcome::Fault(build_fault(ctx.p.kind, ctx.p.adapter(), ctx.phase, &Cause::Gone, sig, &console, now), false)
                    }
                    ProcState::Running { .. } => {
                        let health = sig.map(|s| s.health).unwrap_or(Health::Down);
                        let fatal = sig.map(|s| s.fatal_hint.is_some()).unwrap_or(false);
                        let mut out = Outcome::Nothing;
                        if fatal && health != Health::Ready {
                            let since = *ctx.fatal_since.get_or_insert(now);
                            if now - since >= FATAL_GRACE_S {
                                out = Outcome::Fault(
                                    build_fault(ctx.p.kind, ctx.p.adapter(), ctx.phase, &Cause::Fatal, sig, &console, now),
                                    true,
                                );
                            }
                        } else {
                            ctx.fatal_since = None;
                        }
                        if matches!(out, Outcome::Nothing) {
                            if health == Health::Ready && ctx.phase != Phase::Live {
                                log::info!("{} ({id}) is live", ctx.p.record.session_name);
                                ctx.phase = Phase::Live;
                                ctx.been_live = true;
                            } else if ctx.phase == Phase::Starting && sig.map(progressed).unwrap_or(false) {
                                ctx.phase = Phase::Loading;
                            }
                        }
                        out
                    }
                },
            };
            outcomes.push((id, out));
        }
        for (id, out) in outcomes {
            match out {
                Outcome::Nothing => {}
                Outcome::End(ended) => self.end_session(st, &id, ended, now),
                Outcome::Fault(f, cleanup) => {
                    let ghost = cfg.system(id.as_str()).is_none();
                    let Some(ctx) = st.sessions.get_mut(&id) else { continue };
                    log::warn!("{} ({id}) faulted: {}", ctx.p.record.session_name, f.title);
                    ctx.tail.push(format!("[KLIF] fault: {}", f.title));
                    ctx.phase = Phase::Fault;
                    ctx.fault = Some(f);
                    ctx.stopper = None;
                    ctx.busy = false;
                    ctx.activity = 0.0;
                    if cleanup {
                        ctx.tail.push("[KLIF] stopping what is left of the session's processes".into());
                        ctx.cleanup = spawn_stop(ctx.owned.clone(), "cleanup");
                    }
                    st.dirty = true;
                    // A ghost (not in klif.toml) disappears when its process is gone.
                    if ghost && !cleanup {
                        finished.push(id.clone());
                    }
                }
            }
        }
        for id in finished {
            if st.sessions.get(&id).is_some_and(|c| c.phase == Phase::Fault && c.cleanup.is_none() && c.pids.is_empty()) {
                self.end_session(st, &id, Ended::Fault, now);
            }
        }
    }

    /// Console lines when the session's GPU goes dormant with the session loaded, and when it is back.
    fn narrate_dormancy(&self, id: &SystemId, ctx: &mut SessionCtx, snap: &TelemetrySnapshot) {
        let Some(g) = gpu_snap(snap, ctx.p.gpu.as_deref()) else { return };
        let f = &g.dormancy;
        let entry_ep = f.last_entry.as_ref().map(|e| e.episode).unwrap_or(0);
        let wake_ep = f.last_wake.as_ref().map(|w| w.episode).unwrap_or(0);
        let Some((seen_entry, seen_wake)) = ctx.dormant_seen else {
            // First look: older episodes of this GPU belong to someone else.
            ctx.dormant_seen = Some((entry_ep, wake_ep));
            return;
        };
        let dev = match g.memory.name.trim() {
            "" => "The GPU".to_string(),
            d => d.to_string(),
        };
        let mut seen = (seen_entry, seen_wake);
        if let Some(e) = f.last_entry.as_ref().filter(|e| e.episode > seen_entry) {
            seen.0 = e.episode;
            // Only a live session sleeps; anything seen while stopping is the teardown.
            if ctx.phase == Phase::Live {
                let ulps = self.ulps_for(&g.memory.id);
                log::info!(
                    "{} dormant (episode {}): {:.2} GiB paged out, power {:?}, EnableUlps {:?}",
                    g.memory.id,
                    e.episode,
                    e.paged_out_gib,
                    e.power,
                    ulps.as_ref().and_then(|u| u.enable_ulps)
                );
                let line = dormant_line(&dev, e, ulps.as_ref().map(|u| u.is_on()).unwrap_or(false));
                self.tel(|t| t.note(id.as_str(), line));
            } else {
                seen.1 = e.episode;
            }
        }
        if let Some(w) = f.last_wake.as_ref().filter(|w| w.episode > seen.1) {
            seen.1 = w.episode;
            log::info!("{} awake (episode {}): {:.1} s, {:.2} GiB resident", g.memory.id, w.episode, w.seconds, w.resident_gib);
            let line = wake_line(&dev, w);
            self.tel(|t| t.note(id.as_str(), line));
        }
        ctx.dormant_seen = Some(seen);
    }

    /// The view-model Session of a session.
    pub(super) fn session_vm(&self, id: &SystemId, s: &SessionCtx, sig: Option<&SessionSignals>, now: f64) -> Session {
        let end = match (&s.fault, s.phase) {
            (Some(f), Phase::Fault) => f.at,
            _ => now,
        };
        let uptime = r1((end - s.started_at()).max(0.0));
        let loading = matches!(s.phase, Phase::Starting | Phase::Loading).then(|| {
            let (steps, fraction) = match sig {
                Some(g) if !g.load_steps.is_empty() => (g.load_steps.clone(), g.load_fraction),
                _ => (klif_telemetry::steps::initial_steps(s.p.adapter(), s.p.kind), 0.0),
            };
            LoadProgress { steps, fraction: (fraction * 100.0).round() / 100.0, elapsed_s: uptime }
        });
        let fault = s.fault.as_ref().filter(|_| s.phase == Phase::Fault).map(|f| Fault {
            title: f.title.clone(),
            exit_code: f.exit_code,
            exit_code_hex: f.exit_code_hex.clone(),
            log_tail: f.log_tail.clone(),
            since_s: r1((now - f.at).max(0.0)),
            steps: f.steps.clone(),
        });
        let show_live = match s.phase {
            Phase::Live | Phase::Stopping => true,
            Phase::Fault => s.been_live,
            _ => false,
        };
        let (llm, image, generic) = if show_live {
            match s.phase {
                // A faulted server says nothing new: show what it last said.
                Phase::Fault => (s.last_llm.clone(), s.last_image.clone(), s.last_generic.clone()),
                _ => (
                    sig.and_then(|g| g.llm.clone()).or_else(|| s.last_llm.clone()),
                    sig.and_then(|g| g.image.clone()).or_else(|| s.last_image.clone()),
                    sig.and_then(|g| g.generic.clone()).or_else(|| s.last_generic.clone()),
                ),
            }
        } else {
            (None, None, None)
        };
        let (llm, image, generic) = live_parts(s.p.kind, show_live, llm, image, generic);
        Session {
            system: id.clone(),
            model: s.p.model.clone(),
            phase: s.phase,
            uptime_s: uptime,
            endpoint: Endpoint { host: display_host(&s.p.host).to_string(), port: s.p.record.port },
            api_key_set: s.p.api_key_set,
            loading,
            fault,
            llm,
            image,
            generic,
            preset: s.p.preset.clone(),
            command: s.p.command.clone(),
            gpu: s.p.gpu.clone(),
            vram_gib: (s.committed > 0.0).then_some(r2(s.committed)),
        }
    }
}

/// Exactly one live part by kind while live: llm for kind llm, image for kind image, generic otherwise (and for
/// a kind whose server gives no dedicated signals).
pub(crate) fn live_parts(
    kind: SystemKind,
    live: bool,
    llm: Option<LlmLive>,
    image: Option<ImageLive>,
    generic: Option<GenericLive>,
) -> (Option<LlmLive>, Option<ImageLive>, Option<GenericLive>) {
    if !live {
        return (None, None, None);
    }
    match kind {
        SystemKind::Llm if llm.is_some() => (llm, None, None),
        SystemKind::Image if image.is_some() => (None, image, None),
        _ => (None, None, Some(generic.unwrap_or_default())),
    }
}
