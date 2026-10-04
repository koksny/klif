//! Records wiring (SPEC section 3, store in `crate::records`): a live session's finished requests (llama.cpp log
//! timings) and image / video jobs (sd.cpp) become record samples with the session's conditions, each exactly once;
//! bench windows mark what `klif-cli bench` produced; the view model gets this machine's records plus every online
//! node's; ForgetRecord removes a junk entry.
//!
//! Only per-request timings count: vLLM / OpenAI-compatible servers only give busy-period aggregates (`/metrics`),
//! and TTS / STT / music servers (audio.cpp, whisper-server, generic) log no timing at all, so their records come from
//! `klif-cli bench`, whose numbers the bench hands over at the end of its window.
//! Externally started servers have no model file KLIF knows, so they keep no records.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use klif_common::config::Config;
use klif_common::now_s;
use klif_common::vm::{AdapterId, GpuMemory, NodeState, RecordEntry, RecordEvent, RecordMetric, RecordSource, SystemId, SystemKind};
use klif_telemetry::{SessionSignals, TelemetrySnapshot};

use super::conflicts::gpu_eq;
use super::session::SessionCtx;
use super::{lock, Inner};
use crate::bench::BenchRecord;
use crate::nodes::RemoteState;
use crate::records::{self, Conditions, ModelFile, Sample, MAX_EVENTS};
use crate::state::PersistedSession;

/// A bench window lasts at most this long (a klif-cli that died mid-bench does not mark samples forever).
const BENCH_WINDOW_S: f64 = 4.0 * 3600.0;
/// After the bench says it is done, its last request may still be on its way through the log.
const BENCH_GRACE_S: f64 = 5.0;
/// An adopted sd.cpp session's log is read again from the start right after adoption (8 MiB per 0.5 s poll); its
/// lines carry no time, so the jobs it replays arrive "now". Jobs that arrive this soon after adoption are taken as
/// replayed and not counted (a job that really finished in those seconds is lost; they were counted before).
const REPLAY_S: f64 = 5.0;

/// What a session already handed to the records.
#[derive(Debug, Default)]
pub(crate) struct RecordTrack {
    registered: bool,
    /// When KLIF adopted the session after a restart (None: launched by this engine).
    adopted_at: Option<f64>,
    /// Request ids (llama.cpp task ids) in the telemetry window that were already counted.
    requests: BTreeSet<u64>,
    /// Finished image jobs (finish time, seconds) in the telemetry window that were already counted.
    images: BTreeSet<(i64, u64)>,
}

impl RecordTrack {
    /// A session adopted after a KLIF restart at `at` (its log is replayed from the start).
    pub(crate) fn adopted(at: f64) -> RecordTrack {
        RecordTrack { adopted_at: Some(at), ..RecordTrack::default() }
    }
}

/// The model file a session's server opens: `-m` / `--model` / sd.cpp's `--diffusion-model` (or their env forms)
/// in what ran, relative to its working folder; else the preset's `model`; else only the model's name.
fn model_file(cfg: &Config, p: &PersistedSession) -> ModelFile {
    let cmd = p.command.as_ref();
    let cwd = cmd.map(|c| c.cwd.trim()).filter(|c| !c.is_empty()).map(PathBuf::from);
    let from_args = cmd.and_then(|c| {
        let env: BTreeMap<String, String> =
            c.env.iter().filter(|e| !e.removed).filter_map(|e| e.value.clone().map(|v| (e.name.clone(), v))).collect();
        klif_catalog::facts::from_args(p.adapter(), &c.args, &env).model
    });
    let from_preset = p.preset.as_deref().and_then(|id| cfg.presets.get(id)).and_then(|s| s.model.clone());
    let raw = from_args.or(from_preset).map(|m| m.trim().trim_matches('"').to_string()).filter(|m| !m.is_empty());
    let Some(raw) = raw else {
        let name = Some(p.model.name.trim()).filter(|n| !n.is_empty()).or(p.preset.as_deref()).unwrap_or("model");
        return ModelFile { path: None, file: name.to_string() };
    };
    let file = raw.rsplit(['\\', '/']).next().unwrap_or(&raw).to_string();
    // An unexpanded placeholder (`{env:X}` is shown as %X%) or a repo id is no file KLIF can read.
    if raw.contains(['{', '%']) {
        return ModelFile { path: None, file };
    }
    let path = Path::new(&raw);
    let path = match (&cwd, path.is_absolute()) {
        (Some(c), false) => c.join(path),
        _ => path.to_path_buf(),
    };
    ModelFile { path: Some(path), file }
}

/// The conditions of a session's samples (`sig` = its live signals, when there are any).
fn conditions(p: &PersistedSession, sig: Option<&SessionSignals>, gpus: &[GpuMemory], cpu_name: &str, tflops: Option<f64>) -> Conditions {
    let gpu_names: Vec<String> = p
        .all_gpus()
        .iter()
        .map(|g| {
            if g.trim().eq_ignore_ascii_case("cpu") {
                return Some(cpu_name.trim()).filter(|c| !c.is_empty()).unwrap_or("CPU").to_string();
            }
            gpus.iter()
                .find(|m| gpu_eq(&m.id, g))
                .and_then(|m| [&m.name, &m.device].into_iter().map(|n| n.trim()).find(|n| !n.is_empty()).map(str::to_string))
                .unwrap_or_else(|| g.clone())
        })
        .collect();
    let gpu_names = if gpu_names.is_empty() {
        Some(p.model.device.trim()).filter(|d| !d.is_empty()).map(str::to_string).into_iter().collect()
    } else {
        gpu_names
    };
    let live_ctx = sig.and_then(|s| s.llm.as_ref()).map(|l| l.context.total_tokens).filter(|t| *t > 0).map(|t| t.min(u32::MAX as u64) as u32);
    Conditions {
        kind: p.kind,
        name: p.model.name.clone(),
        quant: Some(p.model.quant.trim()).filter(|q| !q.is_empty()).map(str::to_string),
        backend: sig
            .and_then(|s| s.compute_backend.clone())
            .or_else(|| Some(p.model.backend.trim().to_string()).filter(|b| !b.is_empty()))
            .unwrap_or_default(),
        ctx: p.ctx_tokens.or(p.model.ctx_tokens).or(live_ctx),
        kv: p.model.kv_type.clone().filter(|k| !k.trim().is_empty()),
        gpus: gpu_names,
        backend_build: sig.and_then(|s| s.backend_build.clone()),
        preset: p.preset.clone(),
        tflops_fp32: tflops,
    }
}

/// "1024x768" -> (1024, 768).
fn size_of(s: Option<&str>) -> (Option<u32>, Option<u32>) {
    let Some((w, h)) = s.and_then(|s| s.trim().split_once(['x', 'X'])) else { return (None, None) };
    (w.trim().parse().ok(), h.trim().parse().ok())
}

/// The samples of a bench result, under the same rules as live ones (bench prompts are never cached).
fn bench_samples(rec: &BenchRecord, image_size: Option<&str>) -> Vec<Sample> {
    let (width, height) = size_of(image_size);
    let mut out = Vec::new();
    let mk = |metric, value: f64| Sample {
        metric,
        value,
        at: rec.at,
        source: RecordSource::Bench,
        prompt_tokens: None,
        cached_tokens: None,
        gen_tokens: None,
        width: None,
        height: None,
        steps: None,
        frames: None,
    };
    for r in &rec.runs {
        let tokens = |mut s: Sample| {
            s.prompt_tokens = r.prompt_tokens;
            s.cached_tokens = r.prompt_tokens.map(|_| 0);
            s.gen_tokens = r.gen_tokens;
            s
        };
        if let (Some(v), Some(g)) = (r.decode_tps, r.gen_tokens) {
            if g >= records::MIN_DECODE_TOKENS {
                out.push(tokens(mk(RecordMetric::DecodeTps, v)));
            }
        }
        if let (Some(v), Some(p)) = (r.prefill_tps, r.prompt_tokens) {
            if p >= records::MIN_PREFILL_TOKENS {
                out.push(tokens(mk(RecordMetric::PrefillTps, v)));
            }
        }
        if let Some(v) = r.ttft_s {
            out.push(tokens(mk(RecordMetric::TtftS, v)));
        }
        if let Some(v) = r.seconds_per_image {
            let mut s = mk(RecordMetric::ImageS, v);
            s.width = width;
            s.height = height;
            out.push(s);
        }
        if let Some(v) = r.tts_rtf {
            out.push(mk(RecordMetric::TtsRtf, v));
        }
        if let Some(v) = r.stt_rtf {
            out.push(mk(RecordMetric::SttRtf, v));
        }
        if let Some(v) = r.music_rtf {
            out.push(mk(RecordMetric::MusicRtf, v));
        }
    }
    out
}

impl Inner {
    /// The machine's FP32 TFLOPS total as the last view model showed it (package A's hardware inventory).
    fn machine_tflops(&self) -> Option<f64> {
        let t = lock(&self.published).vm.hardware.tflops_fp32;
        (t.is_finite() && t > 0.0).then_some(t)
    }

    /// A live or stopping session's new finished requests / image jobs as record samples. Called by `advance` under
    /// the state lock: memory only (the records are written by the tick's `flush`).
    pub(super) fn capture_records(&self, id: &SystemId, ctx: &mut SessionCtx, sig: &SessionSignals, snap: &TelemetrySnapshot, cfg: &Config, now: f64) {
        let session = ctx.p.record.session_name.clone();
        if !ctx.rec.registered {
            ctx.rec.registered = true;
            self.records.register(&session, model_file(cfg, &ctx.p));
        }
        let mut fresh: Vec<Sample> = Vec::new();
        let source = || self.records.source_for(id.as_str(), now);
        match ctx.p.adapter() {
            AdapterId::LlamaCpp => {
                if let Some(l) = sig.llm.as_ref() {
                    let src = if l.requests.iter().any(|r| !ctx.rec.requests.contains(&r.id)) { source() } else { RecordSource::Live };
                    for r in l.requests.iter().filter(|r| !ctx.rec.requests.contains(&r.id)) {
                        fresh.extend(records::llm_samples(r, src));
                    }
                    ctx.rec.requests = l.requests.iter().map(|r| r.id).collect();
                }
            }
            AdapterId::SdCpp => {
                if let Some(i) = sig.image.as_ref() {
                    let key = |j: &klif_common::vm::ImageJob| (j.at as i64, j.seconds.to_bits());
                    let replayed = |j: &klif_common::vm::ImageJob| ctx.rec.adopted_at.is_some_and(|a| j.at <= a + REPLAY_S);
                    let new: Vec<&klif_common::vm::ImageJob> =
                        i.recent.iter().filter(|j| !ctx.rec.images.contains(&key(j)) && !replayed(j)).collect();
                    let src = if new.is_empty() { RecordSource::Live } else { source() };
                    for j in new {
                        // A video job (sd.cpp `generate_video WxHxT`) carries its frame count.
                        if j.frames.is_some() || ctx.p.kind == SystemKind::Video {
                            fresh.extend(records::video_sample(j, src));
                        } else {
                            fresh.extend(records::image_sample(j, src));
                        }
                    }
                    ctx.rec.images = i.recent.iter().map(key).collect();
                }
            }
            // Per-request timings exist only in the llama.cpp / sd.cpp logs (see the module docs).
            _ => {}
        }
        if fresh.is_empty() {
            return;
        }
        let gpus: Vec<GpuMemory> = snap.gpus.iter().map(|g| g.memory.clone()).collect();
        let cond = conditions(&ctx.p, Some(sig), &gpus, &snap.machine.cpu_name, self.machine_tflops());
        self.records.observe(&session, cond, fresh);
    }

    /// The view model's records: this machine's, then every online node's (key `"<node>/<key>"`, `node` set,
    /// `machine` = the node's name), the events newest last, and the revision of the whole.
    pub(super) fn records_view(&self, cfg: &Config, remote: &[RemoteState]) -> (Vec<RecordEntry>, Vec<RecordEvent>, u64) {
        self.records.set_machine(&records::machine_name(cfg));
        let (rev, mut entries, mut events) = self.records.view();
        let mut parts: Vec<(String, u64)> = vec![(String::new(), rev)];
        let since = self.records.opened_at();
        for r in remote.iter().filter(|r| r.view.state == NodeState::Online) {
            let Some(rvm) = r.vm.as_ref() else { continue };
            let node = r.view.id.as_str();
            parts.push((node.to_string(), rvm.records_rev));
            let mut keys: BTreeSet<&str> = BTreeSet::new();
            for e in rvm.records.iter().filter(|e| e.node.is_none()) {
                keys.insert(e.key.as_str());
                let mut e = e.clone();
                e.key = format!("{node}/{}", e.key);
                e.node = Some(node.to_string());
                // The node published "This machine" or its `[node] name`; never its host name (r.view.name may fall back
                // to it), so an unnamed node shows as its id from this machine's [nodes.<id>].
                if e.machine.is_empty() || e.machine == crate::records::THIS_MACHINE {
                    e.machine = node.to_string();
                }
                entries.push(e);
            }
            // A node's earlier events (before this engine started) are not news here.
            for ev in rvm.record_events.iter().filter(|ev| keys.contains(ev.key.as_str()) && ev.at >= since) {
                let mut ev = ev.clone();
                ev.key = format!("{node}/{}", ev.key);
                events.push(ev);
            }
        }
        if parts.len() > 1 {
            events.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
            let n = events.len();
            if n > MAX_EVENTS {
                events.drain(..n - MAX_EVENTS);
            }
        }
        // `parts` includes the node list itself, so a node going away changes the revision too.
        let rev = self.records.combined_rev(parts);
        (entries, events, rev)
    }

    /// Action ForgetRecord on this machine.
    pub(super) fn forget_record(&self, key: &str) -> Result<()> {
        self.records.forget(key)
    }

    /// The climb of one record key, oldest first (control method `records_history`). A node's entry
    /// (`"<node>/<key>"`) is asked of that node when `forward` is set (never for a network peer: a node does not act
    /// on its own nodes for others).
    pub(crate) fn records_history(&self, key: &str, metric: Option<RecordMetric>, forward: bool) -> Result<Vec<RecordEvent>> {
        let key = key.trim();
        let node = key.split_once('/').map(|(n, _)| n).filter(|n| forward && self.cfg().nodes.iter().any(|(id, _)| id == n));
        if let Some(node) = node {
            let rest = key.strip_prefix(node).and_then(|r| r.strip_prefix('/')).unwrap_or(key);
            let params = serde_json::json!({ "key": rest, "metric": metric });
            let v = self.nodes.call(node, "records_history", params).map_err(|e| {
                let text = format!("{e:#}");
                if text.contains("Unknown method") {
                    anyhow::anyhow!("Node \"{node}\" does not serve record history; use KLIF 0.3.1 or newer on it.")
                } else {
                    e
                }
            })?;
            let mut points: Vec<RecordEvent> = serde_json::from_value(v)
                .map_err(|e| anyhow::anyhow!("Node \"{node}\": the record history could not be read ({e}). Use the same KLIF version on both machines."))?;
            for p in &mut points {
                p.key = format!("{node}/{}", p.key);
            }
            return Ok(points);
        }
        self.records.history(key, metric)
    }

    /// `klif-cli bench` on a local System: `start` opens its bench window (its captured samples are bench samples),
    /// the end closes it after a short grace and hands over the bench's own numbers for a server whose requests
    /// the log does not show.
    pub(crate) fn bench_mark(&self, system: &SystemId, start: bool, rec: Option<&BenchRecord>) -> Result<()> {
        if system.is_remote() {
            bail!("Bench records of another machine are kept by that machine.");
        }
        let now = now_s();
        self.records.bench_until(system.as_str(), now + if start { BENCH_WINDOW_S } else { BENCH_GRACE_S });
        let Some(rec) = rec.filter(|_| !start) else { return Ok(()) };
        let cfg = self.cfg();
        let tflops = self.machine_tflops();
        let found = {
            let st = lock(&self.st);
            match st.sessions.get(system) {
                // llama.cpp / sd.cpp requests are captured from the log (as bench samples while the window was open).
                Some(ctx) if !matches!(ctx.p.adapter(), AdapterId::LlamaCpp | AdapterId::SdCpp) => {
                    let mut cond = conditions(&ctx.p, None, &st.gpus_mem, &st.machine.cpu_name, tflops);
                    if cond.backend_build.is_none() {
                        cond.backend_build = rec.backend_build.clone();
                    }
                    let samples = bench_samples(rec, ctx.p.model.image_size.as_deref());
                    Some((ctx.p.record.session_name.clone(), model_file(&cfg, &ctx.p), cond, samples))
                }
                _ => None,
            }
        };
        if let Some((session, model, cond, samples)) = found {
            self.records.register(&session, model);
            self.records.observe(&session, cond, samples);
            self.records.flush();
        }
        Ok(())
    }
}
