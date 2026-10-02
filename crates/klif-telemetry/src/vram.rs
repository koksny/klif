//! Composition of the inference GPU's memory (the "cliff") from the adapter counter, the session's
//! per-process counters and the composition parsed from the server log, plus the 1 Hz histories.

use crate::text::round_to;
use klif_common::vm::{VramLayer, VramLayerId};
use std::collections::{BTreeMap, VecDeque};

pub const HISTORY_LEN: usize = 300;

/// Bottom layer first.
pub const LAYER_ORDER: [VramLayerId; 6] =
    [VramLayerId::Other, VramLayerId::Weights, VramLayerId::Kv, VramLayerId::Buffers, VramLayerId::Draft, VramLayerId::Projector];

fn order_of(id: VramLayerId) -> usize {
    LAYER_ORDER.iter().position(|x| *x == id).unwrap_or(LAYER_ORDER.len())
}

/// Result of one composition.
#[derive(Debug, Clone, PartialEq)]
pub struct Composition {
    /// Sums to `used_gib` (within rounding); 'other' first.
    pub layers: Vec<VramLayer>,
    /// VRAM held by things other than the session (driver, other processes).
    pub baseline_gib: f64,
    /// Session VRAM that the log composition does not explain (counted in 'other').
    pub unexplained_gib: f64,
    /// Factor applied to the log layers when they exceed what the session holds (spill / loading).
    pub log_scale: Option<f64>,
}

/// Compose `used_gib` into layers.
///
/// - No session: one 'other' layer = used.
/// - Session with log layers: the layers as logged; if they exceed the session's measured dedicated
///   usage they are scaled down to it (the rest is not resident: spill or not yet allocated); if they
///   fall short, the residual stays in 'other' (reported, never smeared over the named layers).
/// - Session without log layers: one 'weights' layer labelled "model" = the session's measured usage.
/// - Session with log layers but no per-process measurement (PIDs not known yet): the layers are
///   capped at `used - idle_baseline` (the usage measured before the session started).
/// - `other` = used - session layers. `baseline` = used - measured session usage.
pub fn compose(
    used_gib: f64,
    in_session: bool,
    session_gib: Option<f64>,
    log_layers: Option<&[VramLayer]>,
    idle_baseline_gib: Option<f64>,
) -> Composition {
    let used = used_gib.max(0.0);
    if !in_session {
        return Composition {
            layers: vec![other(used)],
            baseline_gib: round_to(used, 3),
            unexplained_gib: 0.0,
            log_scale: None,
        };
    }
    let meas = session_gib.filter(|s| *s > 0.0).map(|s| s.min(used));
    let logs = log_layers.filter(|l| !l.is_empty());
    let mut session: Vec<VramLayer> = Vec::new();
    let mut scale = None;
    let (baseline, unexplained);
    match (logs, meas) {
        (Some(l), Some(s)) => {
            let sum: f64 = l.iter().map(|x| x.gib).sum();
            if sum > s && sum > 0.0 {
                let k = s / sum;
                scale = Some(round_to(k, 4));
                session = l.iter().map(|x| VramLayer { gib: x.gib * k, ..x.clone() }).collect();
                unexplained = 0.0;
            } else {
                session = l.to_vec();
                unexplained = s - sum;
            }
            baseline = used - s;
        }
        (Some(l), None) => {
            let sum: f64 = l.iter().map(|x| x.gib).sum();
            let room = (used - idle_baseline_gib.unwrap_or(0.0)).max(0.0);
            let s = sum.min(room);
            let k = if sum > 0.0 { s / sum } else { 1.0 };
            if k < 0.9999 {
                scale = Some(round_to(k, 4));
            }
            session = l.iter().map(|x| VramLayer { gib: x.gib * k, ..x.clone() }).collect();
            baseline = used - s;
            unexplained = 0.0;
        }
        (None, Some(s)) => {
            session.push(VramLayer { id: VramLayerId::Weights, label: "model".into(), gib: s });
            baseline = used - s;
            unexplained = 0.0;
        }
        (None, None) => {
            baseline = used;
            unexplained = 0.0;
        }
    }
    session.sort_by_key(|l| order_of(l.id));
    let mut layers = Vec::with_capacity(session.len() + 1);
    for l in session.iter_mut() {
        l.gib = round_to(l.gib, 3);
    }
    let named: f64 = session.iter().map(|l| l.gib).sum();
    layers.push(other((used - named).max(0.0)));
    layers.extend(session.into_iter().filter(|l| l.gib > 0.0005));
    Composition {
        layers,
        baseline_gib: round_to(baseline.max(0.0), 3),
        unexplained_gib: round_to(unexplained.max(0.0), 3),
        log_scale: scale,
    }
}

/// Composition while the GPU is dormant (see `dormant`): the session's ALLOCATIONS, which are not
/// resident right now, on top of an 'other' layer = what is resident besides the session. The layers
/// therefore sum to more than `used_gib` (which stays the resident amount).
/// - Log layers when known (scaled down to `vram_gib` if they exceed it).
/// - Else one 'weights' layer labelled "model" = `vram_gib` (the session's committed allocations
///   minus what lives in shared system memory).
pub fn compose_dormant(used_gib: f64, resident_gib: f64, vram_gib: f64, log_layers: Option<&[VramLayer]>) -> Composition {
    let used = used_gib.max(0.0);
    let baseline = (used - resident_gib.max(0.0)).max(0.0);
    let vram = vram_gib.max(0.0);
    let mut scale = None;
    let mut session: Vec<VramLayer> = match log_layers.filter(|l| !l.is_empty()) {
        Some(l) => {
            let sum: f64 = l.iter().map(|x| x.gib).sum();
            let k = if sum > vram && sum > 0.0 { vram / sum } else { 1.0 };
            if k < 0.9999 {
                scale = Some(round_to(k, 4));
            }
            l.iter().map(|x| VramLayer { gib: x.gib * k, ..x.clone() }).collect()
        }
        None => vec![VramLayer { id: VramLayerId::Weights, label: "model".into(), gib: vram }],
    };
    session.sort_by_key(|l| order_of(l.id));
    let mut layers = Vec::with_capacity(session.len() + 1);
    layers.push(other(baseline));
    layers.extend(session.into_iter().map(|l| VramLayer { gib: round_to(l.gib, 3), ..l }).filter(|l| l.gib > 0.0005));
    Composition { layers, baseline_gib: round_to(baseline, 3), unexplained_gib: 0.0, log_scale: scale }
}

fn other(gib: f64) -> VramLayer {
    VramLayer { id: VramLayerId::Other, label: "other".into(), gib: round_to(gib, 3) }
}

/// 1 Hz history of used GiB and of each layer id (same cadence and length).
#[derive(Debug, Default, Clone)]
pub struct VramHistory {
    used: VecDeque<f64>,
    per: BTreeMap<VramLayerId, VecDeque<f64>>,
}

impl VramHistory {
    pub fn push(&mut self, used_gib: f64, layers: &[VramLayer]) {
        push_cap(&mut self.used, round_to(used_gib, 3));
        for id in LAYER_ORDER {
            let v: f64 = layers.iter().filter(|l| l.id == id).map(|l| l.gib).sum();
            let n = self.used.len();
            let q = self.per.entry(id).or_insert_with(|| VecDeque::from(vec![0.0; n.saturating_sub(1)]));
            push_cap(q, round_to(v, 3));
        }
    }

    pub fn used(&self) -> Vec<f64> {
        self.used.iter().copied().collect()
    }

    /// Only ids that were ever non-zero (like the mock).
    pub fn per_layer(&self) -> BTreeMap<VramLayerId, Vec<f64>> {
        self.per
            .iter()
            .filter(|(_, v)| v.iter().any(|x| *x > 0.0))
            .map(|(k, v)| (*k, v.iter().copied().collect()))
            .collect()
    }
}

pub fn push_cap(q: &mut VecDeque<f64>, v: f64) {
    q.push_back(v);
    while q.len() > HISTORY_LEN {
        q.pop_front();
    }
}
