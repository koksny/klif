//! The sampler thread: logs at 2 Hz (every watched session, each under its own lock), and at 1 Hz one platform
//! reading (adapter memory, per-process memory of every watched PID, CPU) plus RAM and the power state of AMD
//! GPUs that run a session. From one reading it attributes memory per session and per GPU:
//!
//! - a session's GPUs = its `WatchSpec.gpu` list matched against the measured GPUs ("cpu" = none; no list or no
//!   match = every measured GPU); `resident` / `committed` / `spill` = its PIDs' sums on those GPUs;
//! - its layers = the log composition (else one "model" layer) scaled to its VRAM allocations
//!   (`max(resident, committed - shared)`), plus an "other" layer for what the log does not explain; without a
//!   per-process measurement (PIDs not known yet) the log layers are capped at `used - baseline` of its GPU;
//! - per GPU: `other = max(0, used - every session's resident memory there)`, `baseline` = used while no
//!   session runs there (else kept), the 1 Hz history (used + per-layer: other plus the sessions' layers), the
//!   spill of spill-reporting sessions, and the dormant detector (AMD only) over every session PID's memory there.

use crate::platform::{self, Frame, PowerHandle, Sampler};
use crate::text::{round_to, GIB, MIB};
use crate::vram::{order_of, LAYER_ORDER};
use crate::{dormant, gpu_id_eq, lock, machine_stats, GpuReading, SessionGpu, SessionVram, Shared, WatchedRef};
use klif_common::vm::{VramLayer, VramLayerId};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Power handle per GPU, rebuilt when the GPU list changes.
struct PowerSlot {
    luid_key: String,
    dev: Option<Box<dyn PowerHandle>>,
    retry_at: f64,
}

impl PowerSlot {
    fn read(&mut self, adapter: &crate::Adapter, nth: usize, now: f64) -> Option<crate::PowerState> {
        if self.dev.is_none() {
            if now < self.retry_at {
                return None;
            }
            self.dev = platform::gpu().power_handle(adapter, nth);
        }
        match self.dev.as_mut().and_then(|d| d.read()) {
            Some(p) => Some(p),
            None => {
                self.dev = None;
                self.retry_at = now + 30.0;
                None
            }
        }
    }
}

/// Which measured GPUs a session's spec names: Some(list) (maybe empty for "cpu"), None = every GPU.
fn declared(spec_gpu: Option<&str>, ids: &[String]) -> Option<Vec<usize>> {
    let list = spec_gpu.map(str::trim).filter(|s| !s.is_empty())?;
    let parts: Vec<&str> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.iter().all(|p| p.eq_ignore_ascii_case("cpu")) {
        return Some(Vec::new());
    }
    let idx: Vec<usize> = (0..ids.len()).filter(|i| parts.iter().any(|p| gpu_id_eq(p, &ids[*i]))).collect();
    if idx.is_empty() { None } else { Some(idx) }
}

/// One session's facts for the attribution pass.
struct Row {
    w: WatchedRef,
    external: bool,
    spill: bool,
    ready: bool,
    busy_since: Option<f64>,
    log_layers: Option<Vec<VramLayer>>,
    /// Per GPU index: (dedicated, shared, committed) bytes and whether any instance was found.
    per: Vec<(f64, f64, f64, bool)>,
    /// The session's GPUs (indexes).
    mine: Vec<usize>,
    /// Declared on these GPUs (for "runs on GPU g").
    declared: Option<Vec<usize>>,
}

/// The part of a session's shared (GPU-mapped system) memory that is spill: memory the server wanted on the card and
/// did not get there. Every GPU process holds shared memory by design (pinned transfer and output buffers, the
/// runtime's staging: about 0.5-0.7 GiB for a llama.cpp server on ROCm), so the shared counter alone is not spill.
/// - With the device buffers from the server's log (`logged`): what of them the card does not hold (`on_card` = the
///   session's commit minus its shared memory), at most the shared memory; 0 until the server is ready (its buffers
///   are still being filled while it loads). Weights placed in RAM on purpose (`--cpu-moe`, `-ngl`) are host buffers
///   in the log, so they are not spill.
/// - Without them: the shared memory, but only while the card is full (only then does the driver put device
///   allocations into system memory).
pub(crate) fn spill_gib(shared: f64, on_card: f64, logged: Option<f64>, ready: bool, card_full: bool) -> f64 {
    let s = match logged {
        Some(dev) if ready => (dev - on_card).min(shared),
        Some(_) => 0.0,
        None if card_full => shared,
        None => 0.0,
    };
    s.max(0.0)
}

/// Free dedicated memory below which a card counts as full: 2 % of it, at least 256 MiB.
fn full_margin(total: f64) -> f64 {
    (total * 0.02).max(256.0 * MIB)
}

/// Scale `log` to `claim` (None: no platform numbers, the log layers as logged): down when they exceed it; with
/// `fill_rest`, what they do not explain becomes an "other" layer (never smeared over the named layers). Without
/// log layers: one "model" layer = claim.
pub(crate) fn compose_session(log: Option<&[VramLayer]>, claim: Option<f64>, fill_rest: bool) -> Vec<VramLayer> {
    let mut out: Vec<VramLayer> = match (log.filter(|l| !l.is_empty()), claim.map(|c| c.max(0.0))) {
        (Some(l), None) => l.to_vec(),
        (Some(l), Some(c)) => {
            let sum: f64 = l.iter().map(|x| x.gib).sum();
            let k = if sum > c && sum > 0.0 { c / sum } else { 1.0 };
            let mut v: Vec<VramLayer> = l.iter().map(|x| VramLayer { gib: x.gib * k, ..x.clone() }).collect();
            if fill_rest && c - sum > 0.0005 {
                v.push(VramLayer { id: VramLayerId::Other, label: "other".into(), gib: c - sum });
            }
            v
        }
        (None, Some(c)) if c > 0.0005 => vec![VramLayer { id: VramLayerId::Weights, label: "model".into(), gib: c }],
        (None, _) => Vec::new(),
    };
    out.sort_by_key(|l| order_of(l.id));
    for l in out.iter_mut() {
        l.gib = round_to(l.gib, 3);
    }
    out.retain(|l| l.gib > 0.0005);
    out
}

pub(crate) fn run(shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>) {
    let name = crate::cpu_name();
    {
        let mut s = lock(&shared);
        s.machine = machine_stats(0.0, name.clone(), None);
    }
    // The first PdhAddEnglishCounterW loads the provider (~265 ms): do it here, not on the UI thread.
    let mut sampler: Option<Box<dyn Sampler>> = platform::gpu().sampler();
    let mut power: Vec<PowerSlot> = Vec::new();
    let mut gpus_gen_seen = u64::MAX;
    let mut pids_gen_seen = u64::MAX;
    let period = Duration::from_millis(500);
    let mut next = Instant::now();
    let mut tick: u64 = 0;
    while !stop.load(Ordering::SeqCst) {
        let now = klif_common::now_s();
        // Logs at 2 Hz, each session under its own lock (never the shared lock during file IO).
        let watched: Vec<WatchedRef> = lock(&shared).sessions.values().cloned().collect();
        for w in &watched {
            lock(w).tracker.poll(now);
        }
        // Memory / CPU / RAM / power at 1 Hz.
        if tick.is_multiple_of(2) {
            let (pids, pids_gen, gpus_gen) = {
                let s = lock(&shared);
                let mut pids: Vec<u32> = Vec::new();
                for w in s.sessions.values() {
                    pids.extend(lock(w).pids.iter().copied());
                }
                pids.sort_unstable();
                pids.dedup();
                (pids, s.pids_gen, s.gpus_gen)
            };
            if gpus_gen != gpus_gen_seen {
                gpus_gen_seen = gpus_gen;
                let s = lock(&shared);
                power = s.gpus.iter().map(|g| PowerSlot { luid_key: g.adapter.pdh_luid(), dev: None, retry_at: 0.0 }).collect();
            }
            if let Some(smp) = sampler.as_mut() {
                if pids_gen != pids_gen_seen {
                    pids_gen_seen = pids_gen;
                    smp.refresh_processes();
                }
            }
            let frame = sampler.as_mut().map(|s| s.read(&pids)).unwrap_or_default();
            let measured = sampler.is_some();
            let machine = machine_stats(frame.cpu_pct.unwrap_or(0.0), name.clone(), None);
            attribute(&shared, &frame, measured, machine, &mut power, now);
        }
        tick += 1;
        next += period;
        let now_i = Instant::now();
        if next > now_i {
            // Sleep in short slices so Drop does not wait long.
            let mut left = next - now_i;
            while left > Duration::ZERO && !stop.load(Ordering::SeqCst) {
                let d = left.min(Duration::from_millis(100));
                std::thread::sleep(d);
                left = left.saturating_sub(d);
            }
        } else {
            next = now_i;
        }
    }
}

fn attribute(
    shared: &Arc<Mutex<Shared>>,
    frame: &Frame,
    measured: bool,
    machine: klif_common::vm::MachineStats,
    power: &mut [PowerSlot],
    now: f64,
) {
    let mut s = lock(shared);
    s.machine = machine;
    let keys: Vec<String> = s.gpus.iter().map(|g| g.adapter.pdh_luid()).collect();
    let ids: Vec<String> = s.gpus.iter().map(|g| g.id.clone()).collect();
    let n = keys.len();
    // (key, pid) -> (dedicated, shared, committed)
    let mut proc: BTreeMap<(&str, u32), (f64, f64, f64)> = BTreeMap::new();
    for p in &frame.procs {
        if let Some(k) = keys.iter().find(|k| **k == p.key) {
            let e = proc.entry((k.as_str(), p.pid)).or_default();
            e.0 += p.dedicated;
            e.1 += p.shared;
            e.2 += p.committed;
        }
    }

    // Adapter usage per GPU (bytes): a process's resident memory on a GPU is capped at it (PDH can report more
    // for processes that map other processes' surfaces, e.g. the desktop compositor).
    let used_bytes: Vec<f64> = keys.iter().map(|k| frame.adapters.iter().filter(|a| a.key == *k).map(|a| a.dedicated).sum()).collect();
    // Per-session rows.
    let mut rows: Vec<Row> = Vec::new();
    let mut all_pids: Vec<u32> = Vec::new();
    for w in s.sessions.values() {
        let g = lock(w);
        let spec = g.tracker.spec();
        let mut per = vec![(0.0, 0.0, 0.0, false); n];
        for (i, k) in keys.iter().enumerate() {
            for pid in &g.pids {
                if let Some(v) = proc.get(&(k.as_str(), *pid)) {
                    per[i].0 += v.0;
                    per[i].1 += v.1;
                    per[i].2 += v.2;
                    per[i].3 = true;
                }
            }
            if measured {
                per[i].0 = per[i].0.min(used_bytes[i]);
            }
        }
        all_pids.extend(g.pids.iter().copied());
        let decl = declared(spec.gpu.as_deref(), &ids);
        let mine = match &decl {
            Some(v) => v.clone(),
            None => (0..n).collect(),
        };
        rows.push(Row {
            w: w.clone(),
            external: spec.external,
            spill: g.tracker.backend().reports_spill(),
            ready: g.tracker.is_ready(),
            busy_since: g.tracker.busy_since(now),
            log_layers: g.tracker.layers_from_logs(),
            per,
            mine,
            declared: decl,
        });
    }
    all_pids.sort_unstable();
    all_pids.dedup();

    // Does session r run on GPU i (for baseline / dormancy)?
    let on = |r: &Row, i: usize| -> bool {
        if r.external {
            return false;
        }
        match &r.declared {
            Some(d) => d.contains(&i),
            None => (r.per[i].3 && (r.per[i].0 > 0.0 || r.per[i].2 > 0.0)) || n == 1,
        }
    };

    // Session VRAM: sums over the session's GPUs, layers scaled to the claim.
    let mut unmeasured_claim = vec![0.0f64; n];
    let mut session_layers_on: Vec<Vec<(f64, Vec<VramLayer>)>> = vec![Vec::new(); n];
    // Per row: its spill (GiB) and its shared memory (GiB), to split the spill over its GPUs.
    let mut row_spill: Vec<(f64, f64)> = Vec::with_capacity(rows.len());
    for r in &rows {
        let (mut ded, mut sh, mut com, mut found) = (0.0, 0.0, 0.0, false);
        for &i in &r.mine {
            ded += r.per[i].0;
            sh += r.per[i].1;
            com += r.per[i].2;
            found |= r.per[i].3;
        }
        let (ded, sh, com) = (ded / GIB, sh / GIB, com / GIB);
        let claim = if found {
            Some(ded.max(com - sh))
        } else if measured && !r.mine.is_empty() {
            // No per-process numbers yet: the log layers, capped at what the GPU gained since the baseline.
            let i = r.mine[0];
            let room = (s.gpus[i].used_gib - s.gpus[i].baseline_gib).max(0.0);
            let sum: f64 = r.log_layers.as_ref().map(|l| l.iter().map(|x| x.gib).sum()).unwrap_or(0.0);
            Some(sum.min(room))
        } else if measured {
            Some(0.0)
        } else {
            None
        };
        let layers = if r.external { Vec::new() } else { compose_session(r.log_layers.as_deref(), claim, found) };
        if !found && !r.external {
            if let (Some(i), Some(c)) = (r.mine.first(), claim) {
                unmeasured_claim[*i] += c;
            }
        }
        // Split the session's layers over its GPUs by where its allocations are (for the per-GPU history).
        let weights: Vec<(usize, f64)> = r.mine.iter().map(|&i| (i, (r.per[i].0.max(r.per[i].2 - r.per[i].1)).max(0.0))).collect();
        let wsum: f64 = weights.iter().map(|x| x.1).sum();
        for (i, wgt) in &weights {
            let frac = if wsum > 0.0 { wgt / wsum } else if Some(i) == r.mine.first() { 1.0 } else { 0.0 };
            if frac > 0.0 && !layers.is_empty() {
                session_layers_on[*i].push((frac, layers.clone()));
            }
        }
        let per_gpu: Vec<SessionGpu> = (0..n)
            .filter(|&i| r.per[i].3 && (r.per[i].0 > 0.0 || r.per[i].2 > 0.0 || r.per[i].1 > 0.0))
            .map(|i| SessionGpu {
                id: ids[i].clone(),
                resident_gib: round_to(r.per[i].0 / GIB, 3),
                committed_gib: round_to(r.per[i].2 / GIB, 3),
                shared_gib: round_to(r.per[i].1 / GIB, 3),
            })
            .collect();
        let spill = if r.spill {
            let logged = r.log_layers.as_ref().map(|l| l.iter().map(|x| x.gib).sum::<f64>()).filter(|v| *v > 0.0);
            let full = r.mine.iter().any(|&i| {
                let total = s.gpus[i].adapter.dedicated_bytes as f64;
                total > 0.0 && total - used_bytes[i] < full_margin(total)
            });
            spill_gib(sh, (com - sh).max(0.0), logged, r.ready, full)
        } else {
            0.0
        };
        row_spill.push((spill, sh));
        let v = SessionVram {
            resident_gib: ded,
            committed_gib: com,
            spill_mib: spill * GIB / MIB,
            layers,
            per_gpu,
        };
        lock(&r.w).vram = v;
    }

    // Per GPU.
    for i in 0..n {
        let key = keys[i].clone();
        let reading = GpuReading::from_frame(frame, &key, &all_pids);
        let used = reading.used_bytes / GIB;
        let resident_all: f64 = rows.iter().map(|r| r.per[i].0).sum::<f64>() / GIB;
        let committed_all: f64 = rows.iter().map(|r| r.per[i].2).sum::<f64>() / GIB;
        let shared_all: f64 = rows.iter().map(|r| r.per[i].1).sum::<f64>() / GIB;
        let any_found = rows.iter().any(|r| r.per[i].3);
        let spill: f64 = rows
            .iter()
            .zip(&row_spill)
            .filter(|(_, (sp, sh))| *sp > 0.0 && *sh > 0.0)
            .map(|(r, (sp, sh))| sp * (r.per[i].1 / GIB) / sh)
            .sum::<f64>()
            * GIB
            / MIB;
        let running: Vec<&Row> = rows.iter().filter(|r| on(r, i)).collect();
        let any_on = !running.is_empty();
        let ready = running.iter().any(|r| r.ready);
        let busy_since = running.iter().filter_map(|r| r.busy_since).reduce(f64::min);
        let pw = if any_on && s.gpus[i].adapter.is_amd() {
            let (a, nth) = (s.gpus[i].adapter.clone(), s.gpus[i].nth);
            power.iter_mut().find(|p| p.luid_key == key).and_then(|p| p.read(&a, nth, now))
        } else {
            None
        };
        let g = &mut s.gpus[i];
        if !measured {
            continue;
        }
        g.reading = reading;
        g.used_gib = used;
        g.other_gib = (used - resident_all - unmeasured_claim[i]).max(0.0);
        g.spill_mib = spill;
        if !any_on {
            g.baseline_gib = used;
            g.baseline_measured = true;
            g.dormant.reset();
            g.ever_ready = false;
        } else {
            if !g.baseline_measured {
                // Sessions ran since telemetry started (adopted): the best estimate is what they do not hold.
                g.baseline_gib = g.other_gib;
            }
            if g.adapter.is_amd() {
                g.ever_ready |= ready;
                let sample = dormant::DormantSample {
                    at: now,
                    live: g.ever_ready,
                    committed_gib: any_found.then_some(committed_all),
                    resident_gib: any_found.then_some(resident_all),
                    shared_gib: shared_all,
                    power: pw,
                    busy_since,
                };
                g.dormant.sample(&sample);
            }
        }
        // History: used, and per layer id other + every session's layers on this GPU.
        let mut layers: Vec<VramLayer> = vec![VramLayer { id: VramLayerId::Other, label: "other".into(), gib: g.other_gib }];
        for (frac, ls) in &session_layers_on[i] {
            for l in ls {
                layers.push(VramLayer { gib: l.gib * frac, ..l.clone() });
            }
        }
        let mut merged: Vec<VramLayer> = Vec::new();
        for id in LAYER_ORDER {
            let v: f64 = layers.iter().filter(|l| l.id == id).map(|l| l.gib).sum();
            if v > 0.0 {
                merged.push(VramLayer { id, label: String::new(), gib: v });
            }
        }
        g.hist.push(used, &merged);
    }
}
