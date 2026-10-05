//! Which running Systems must stop before a System can launch (SPEC 5 + 16.5 / 16.6 / 16.7 / 16.22 / 16.23).
//!
//! Holders: every System with a session in any phase (a faulted one only while processes of it are left) or a
//! pending launch; external Systems never are. reservation(H) = max(committed, expected total) while H is
//! starting / loading / pending, committed otherwise.
//! (a) port: a holder on the same port whose host overlaps (0.0.0.0 / :: overlap any host);
//! (b) exclusive: a holder sharing a GPU, when either side is `exclusive`;
//! (c) VRAM, per GPU of the subject: foreign = max(0, used - sum of resident of KLIF sessions on that GPU),
//!     free_eff = total - foreign - sum of reservations; when need > free_eff - warn, holders on that GPU are added,
//!     largest reservation first, until it fits. When need > free_eff + sum of reservations it does not fit even
//!     with everything stopped: a warning, and no VRAM conflicts.
//! Need counts on every listed GPU of a multi-GPU preset. Resident and committed memory are taken per GPU from
//! telemetry (`SessionSignals.per_gpu`), so a session on GPUs A and B is charged on each with what it holds there;
//! while a holder is starting / loading / pending its reservation (the expected total) counts on every listed GPU
//! (conservative). Without per-GPU numbers (before the first sample) the totals fall back to the first GPU
//! (resident) and every listed GPU (reservation).

use std::collections::BTreeMap;

use klif_common::config::normalize_gpu_id;
use klif_common::vm::SystemId;

use super::GpuFact;

/// A System that holds (or is about to hold) a port, a GPU and VRAM.
#[derive(Debug, Clone)]
pub(crate) struct Member {
    pub(crate) id: SystemId,
    pub(crate) label: String,
    pub(crate) host: String,
    pub(crate) port: Option<u16>,
    /// Normalized GPU ids; `[""]` when unknown.
    pub(crate) gpus: Vec<String>,
    pub(crate) exclusive: bool,
    pub(crate) reservation: f64,
    pub(crate) resident: f64,
    pub(crate) pending: bool,
    pub(crate) busy: bool,
    /// Starting / loading / pending: `reservation` (which covers the expected total) counts on every listed GPU.
    pub(crate) loading: bool,
    /// Measured per GPU: (GPU id, resident GiB, committed GiB). Empty when telemetry has no per-process numbers.
    pub(crate) per_gpu: Vec<(String, f64, f64)>,
}

impl Member {
    /// What this holder has resident on GPU `g` (per-GPU numbers; else its total, on its first GPU only).
    fn resident_on(&self, g: &str) -> f64 {
        if self.per_gpu.is_empty() {
            return if self.gpus.first().is_some_and(|x| gpu_eq(x, g)) { self.resident.max(0.0) } else { 0.0 };
        }
        self.per_gpu.iter().filter(|e| gpu_eq(&e.0, g)).map(|e| e.1.max(0.0)).sum()
    }

    /// What this holder reserves on GPU `g`: its committed memory there once settled; while it starts, loads or
    /// is pending (or without per-GPU numbers) its whole reservation, on every listed GPU.
    fn reservation_on(&self, g: &str) -> f64 {
        if self.loading || self.per_gpu.is_empty() {
            return self.reservation.max(0.0);
        }
        self.per_gpu.iter().filter(|e| gpu_eq(&e.0, g)).map(|e| e.2.max(0.0)).sum()
    }
}

/// A System that wants to launch.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub(crate) id: SystemId,
    pub(crate) host: String,
    pub(crate) port: Option<u16>,
    pub(crate) gpus: Vec<String>,
    pub(crate) exclusive: bool,
    /// Expected VRAM total (GiB), when known.
    pub(crate) need: Option<f64>,
    /// Keeps weights in system RAM and takes the VRAM it finds: never "does not fit"; the Systems on its GPU are
    /// still stopped first when the need does not fit beside them (it runs faster with the whole card).
    pub(crate) adapts: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    Port(u16),
    Exclusive,
    Vram,
}

impl Why {
    pub(crate) fn text(self) -> String {
        match self {
            Why::Port(p) => format!("port {p}"),
            Why::Exclusive => "exclusive GPU".into(),
            Why::Vram => "not enough VRAM".into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Verdict {
    pub(crate) ids: Vec<SystemId>,
    pub(crate) why: BTreeMap<SystemId, Why>,
    /// "Does not fit on <GPU> even with everything stopped."
    pub(crate) warn: Option<String>,
}

impl Verdict {
    fn add(&mut self, id: &SystemId, why: Why) {
        if !self.why.contains_key(id) {
            self.ids.push(id.clone());
            self.why.insert(id.clone(), why);
        }
    }
}

/// GPU ids as lists to compare: normalized, `[""]` when none is known.
pub(crate) fn gpu_list(ids: &[String]) -> Vec<String> {
    let v: Vec<String> = ids.iter().filter_map(|g| normalize_gpu_id(g)).collect();
    if v.is_empty() {
        vec![String::new()]
    } else {
        v
    }
}

/// Same GPU: equal ids, or the same PCI id where a missing `#n` means `#0`. "" (unknown) only matches "".
pub(crate) fn gpu_eq(a: &str, b: &str) -> bool {
    let na = normalize_gpu_id(a).unwrap_or_default();
    let nb = normalize_gpu_id(b).unwrap_or_default();
    if na == nb {
        return true;
    }
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    let split = |s: &str| -> (String, u32) {
        match s.split_once('#') {
            Some((p, n)) => (p.to_string(), n.parse().unwrap_or(0)),
            None => (s.to_string(), 0),
        }
    };
    split(&na) == split(&nb)
}

fn shares_gpu(a: &[String], b: &[String]) -> bool {
    a.iter().any(|x| b.iter().any(|y| gpu_eq(x, y)))
}

/// Two listeners on the same port collide when the hosts are equal, either is a wildcard (0.0.0.0 / :: / empty),
/// or both are loopback names. The catalog's rule (`klif_catalog::ports_overlap`), so the "shares port N"
/// warning and the port conflict always agree.
pub(crate) fn hosts_overlap(a: &str, b: &str) -> bool {
    klif_catalog::ports_overlap((a, 1), (b, 1))
}

/// A local System's port collides with an ONLINE EXTERNAL server (SPEC 16.7) only when the endpoint names this
/// machine: the same port and an endpoint host that is loopback or equal to the local bind host. A wildcard bind
/// does not collide with a server on another machine (an external on this machine reached by a LAN address is
/// caught as a foreign port holder instead). The catalog's "shares port N with the external System" warning uses
/// the same rule (`klif_catalog` `cross_port_issue`), so the warning and the invalid status agree: this is that
/// very function.
pub(crate) fn external_port_clash(local: (&str, u16), ext: (&str, u16)) -> bool {
    klif_catalog::external_port_clash(local, ext)
}

/// The conflicts of `c` among `holders` on the measured `gpus` (`warn` = `[telemetry] warn_below_gib`).
pub(crate) fn compute(c: &Candidate, holders: &[Member], gpus: &[GpuFact], warn: f64) -> Verdict {
    let mut v = Verdict::default();
    let others: Vec<&Member> = holders.iter().filter(|h| h.id != c.id).collect();
    let cg = gpu_list(&c.gpus);
    // (a) port
    if let Some(port) = c.port {
        for h in &others {
            if h.port == Some(port) && hosts_overlap(&c.host, &h.host) {
                v.add(&h.id, Why::Port(port));
            }
        }
    }
    // (b) exclusive GPU
    for h in &others {
        if (c.exclusive || h.exclusive) && shares_gpu(&cg, &h.gpus) {
            v.add(&h.id, Why::Exclusive);
        }
    }
    // (c) VRAM
    let Some(need) = c.need.filter(|n| n.is_finite() && *n > 0.0) else { return v };
    for g in cg.iter().filter(|g| !g.is_empty() && g.as_str() != "cpu") {
        let Some(f) = gpus.iter().find(|f| gpu_eq(&f.id, g)) else { continue };
        // Not measured (0 or NaN): no VRAM rule on this GPU.
        if f.total.is_nan() || f.total <= 0.0 {
            continue;
        }
        let mut on_g: Vec<&Member> = others.iter().copied().filter(|h| h.gpus.iter().any(|x| gpu_eq(x, g))).collect();
        // Everything KLIF's sessions hold on this GPU is not foreign (also memory a session holds on a GPU it does
        // not list).
        let resident: f64 = others.iter().map(|h| h.resident_on(g)).sum();
        let reserved: f64 = on_g.iter().map(|h| h.reservation_on(g)).sum();
        let foreign = (f.used - resident).max(0.0);
        let free_eff = f.total - foreign - reserved;
        if need <= free_eff - warn {
            continue;
        }
        if need > free_eff + reserved && !c.adapts {
            v.warn = Some(format!(
                "Does not fit even with everything stopped: needs {need:.1} GiB, at most {:.1} GiB can be free on this GPU.",
                (free_eff + reserved).max(0.0)
            ));
            continue;
        }
        on_g.sort_by(|a, b| b.reservation_on(g).partial_cmp(&a.reservation_on(g)).unwrap_or(std::cmp::Ordering::Equal));
        let mut freed = 0.0;
        for h in on_g {
            if need <= free_eff + freed - warn {
                break;
            }
            v.add(&h.id, Why::Vram);
            freed += h.reservation_on(g);
        }
    }
    v
}

/// "System 1 and System 2" / "System 1, System 2 and System 3".
pub(crate) fn names(labels: &[String]) -> String {
    match labels.len() {
        0 => String::new(),
        1 => labels[0].clone(),
        n => format!("{} and {}", labels[..n - 1].join(", "), labels[n - 1]),
    }
}
