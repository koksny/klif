//! The view model: every local System (catalog's static part + this engine's live state), ghost sessions,
//! remote nodes' Systems (rewritten into this hub's namespace), the selected System's conveniences, and the
//! per-System console / VRAM views (`Focus`) for `snapshot_focus`. Non-finite floats are clamped before publishing.

use std::collections::{BTreeMap, BTreeSet};

use klif_common::config::{normalize_gpu_id, Config, NodeRight};
use klif_common::vm::{
    Availability, BenchSummary, ConfigInfo, Endpoint, GpuMemory, LastSession, ModelRef, NodeState, Phase, PresetInfo, RecommendationInfo, Right,
    Session, System, SystemId, SystemStatus, ViewModel, VramLayer,
};
use klif_telemetry::{Health, TelemetrySnapshot};

use super::conflicts::{self, external_port_clash, gpu_eq, gpu_list, Candidate, Member};
use super::session::{base_url, cap, display_host, live_parts, model_key, SessionCtx};
use super::{empty_gpu, ghost_label, lock, r1, Focus, GpuFact, Inner, State};
use crate::nodes::RemoteState;

/// Status and reason of a System with a session.
pub(crate) fn session_status(ctx: &SessionCtx) -> (SystemStatus, Option<String>) {
    match ctx.phase {
        Phase::Starting | Phase::Loading => (SystemStatus::Starting, None),
        Phase::Live => (if ctx.busy { SystemStatus::Busy } else { SystemStatus::Online }, None),
        Phase::Stopping => (SystemStatus::Stopping, None),
        Phase::Fault => (SystemStatus::Fault, ctx.fault.as_ref().map(|f| f.title.clone())),
    }
}

/// Whether a session still holds GPU memory: not a faulted one whose processes are all gone (its measured layers
/// are frozen at the fault and must not be stacked on the GPU view).
fn holds_memory(ctx: &SessionCtx) -> bool {
    !(ctx.phase == Phase::Fault && ctx.pids.is_empty())
}

/// Every holder (SPEC 16.5): sessions in any phase (faulted ones while processes are left) and pending launches.
pub(crate) fn holders(st: &State, cfg: &Config) -> Vec<Member> {
    let mut out = Vec::new();
    for (id, ctx) in &st.sessions {
        if ctx.phase == Phase::Fault && ctx.pids.is_empty() {
            continue;
        }
        // The session records the host to CONNECT to (a wildcard bind becomes loopback); for the port rule the
        // BIND host counts, which the current config still knows while the port is unchanged.
        let host = st
            .endpoints
            .get(id)
            .filter(|(_, p)| *p == ctx.p.record.port)
            .map(|(h, _)| h.clone())
            .unwrap_or_else(|| ctx.p.host.clone());
        out.push(Member {
            id: id.clone(),
            label: st.label_of(cfg, id),
            host,
            port: Some(ctx.p.record.port),
            gpus: gpu_list(&ctx.p.all_gpus()),
            exclusive: cfg.system(id.as_str()).map(|s| s.exclusive).unwrap_or(ctx.p.exclusive),
            reservation: ctx.reservation(),
            resident: ctx.resident,
            pending: st.pending.contains_key(id),
            busy: ctx.busy,
            loading: ctx.loading() || st.pending.contains_key(id),
            per_gpu: ctx.per_gpu.clone(),
        });
    }
    for (id, p) in &st.pending {
        if st.sessions.contains_key(id) {
            continue;
        }
        out.push(Member {
            id: id.clone(),
            label: p.label.clone(),
            host: p.host.clone(),
            port: p.port,
            gpus: gpu_list(&p.gpus),
            exclusive: p.exclusive,
            reservation: p.expected_gib.unwrap_or(0.0),
            resident: 0.0,
            pending: true,
            busy: false,
            loading: true,
            per_gpu: Vec::new(),
        });
    }
    out
}

/// The expected VRAM total of a System (GiB), when known.
pub(crate) fn expected_total(s: &System) -> Option<f64> {
    s.expected_vram.as_ref().map(|l| l.iter().map(|x| x.gib).filter(|g| g.is_finite()).sum::<f64>()).filter(|t| *t > 0.0)
}

/// The conflict candidate of a local System.
pub(crate) fn candidate(st: &State, s: &System) -> Candidate {
    let (host, port) = match st.endpoints.get(&s.id) {
        Some((h, p)) => (h.clone(), Some(*p)),
        None => (String::new(), None),
    };
    let gpus = if s.gpus.is_empty() { s.gpu.iter().cloned().collect() } else { s.gpus.clone() };
    Candidate { id: s.id.clone(), host, port, gpus, exclusive: s.exclusive, need: expected_total(s) }
}

/// "Needs System 1 stopped first (port 7030)."
pub(crate) fn conflict_reason(v: &conflicts::Verdict, members: &[Member]) -> String {
    let labels: Vec<String> =
        v.ids.iter().map(|id| members.iter().find(|m| &m.id == id).map(|m| m.label.clone()).unwrap_or_else(|| id.to_string())).collect();
    let mut whys: Vec<String> = Vec::new();
    for id in &v.ids {
        if let Some(w) = v.why.get(id) {
            let t = w.text();
            if !whys.contains(&t) {
                whys.push(t);
            }
        }
    }
    format!("Needs {} stopped first ({}).", conflicts::names(&labels), whys.join(", "))
}

/// The GPU a System's VRAM view shows: its GPU, else `[gpu] inference`, else the first measured one.
fn gpu_view(gpus: &[GpuMemory], gpu: Option<&str>, inference: Option<&str>, warn: f64) -> GpuMemory {
    let pick = |g: &str| gpus.iter().find(|m| gpu_eq(&m.id, g));
    gpu.filter(|g| !g.eq_ignore_ascii_case("cpu"))
        .and_then(pick)
        .or_else(|| inference.and_then(pick))
        .or_else(|| gpus.first())
        .cloned()
        .unwrap_or_else(|| empty_gpu(warn))
}

/// "host" of "host:port" / "[v6]:port".
fn host_of(address: &str) -> String {
    let a = address.trim();
    if let Some(rest) = a.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest).to_string();
    }
    match a.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.contains(':') => h.to_string(),
        _ => a.to_string(),
    }
}

fn loopback_or_wildcard(h: &str) -> bool {
    let h = h.trim().trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    matches!(h.as_str(), "" | "0.0.0.0" | "::" | "*" | "localhost" | "::1") || h.starts_with("127.")
}

/// Replace a loopback / wildcard host in `scheme://host:port/path` by `host`.
fn rewrite_url_host(url: &str, host: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return url.to_string() };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (h, port) = if let Some(r) = authority.strip_prefix('[') {
        match r.split_once(']') {
            Some((h, p)) => (h, p),
            None => (authority, ""),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h, &authority[h.len()..][..p.len() + 1]),
            None => (authority, ""),
        }
    };
    if !loopback_or_wildcard(h) {
        return url.to_string();
    }
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
    format!("{scheme}://{host}{port}{path}")
}

/// What compose takes from the catalog. Built by the tick BEFORE the state lock is taken: the catalog probes files
/// (programs, models, PATH), which can block on a slow or unreachable volume (SPEC 5: no slow IO under `st`).
pub(crate) struct CatalogParts {
    pub(crate) systems: Vec<System>,
    pub(crate) presets: Vec<PresetInfo>,
    pub(crate) recommendations: Vec<RecommendationInfo>,
}

impl Inner {
    /// Compose the view model and every System's Focus.
    pub(super) fn compose(
        &self,
        st: &mut State,
        cfg: &Config,
        parts: CatalogParts,
        snap: Option<&TelemetrySnapshot>,
        remote: &[RemoteState],
        now: f64,
    ) -> (ViewModel, BTreeMap<SystemId, Focus>) {
        let CatalogParts { mut systems, mut presets, recommendations } = parts;
        let cfg_issues = lock(&self.loaded).issues.clone();
        let key_info = lock(&self.key_info).clone();
        let warn = cfg.telemetry.warn_below_gib;
        let inference = cfg.gpu.inference.as_deref().and_then(normalize_gpu_id);

        // ---- GPUs ----------------------------------------------------------------------------------------
        // Telemetry's `layers = [other]` is already per GPU: used minus what the live processes of every watched
        // session hold on that GPU (a multi-GPU session per GPU; a faulted session's freed memory is not charged).
        // Without a snapshot the last composed GPUs (with their layers) are kept.
        let gpus: Vec<GpuMemory> = match snap {
            Some(t) => t.gpus.iter().map(|g| g.memory.clone()).collect(),
            None => st.gpus_mem.clone(),
        };
        let mut machine = snap.map(|t| t.machine.clone()).unwrap_or_else(|| st.machine.clone());
        if let Some(rt) = cfg.telemetry.ram_type.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            machine.ram_type = Some(rt.to_string());
        }
        st.gpus_mem = gpus.clone();
        st.machine = machine.clone();
        st.gpu_facts = gpus.iter().map(|g| GpuFact { id: g.id.clone(), total: g.total_gib, used: g.used_gib }).collect();

        // ---- local Systems ---------------------------------------------------------------------------
        let members = holders(st, cfg);
        // Online externals (rule 16.7: a local System on their port is invalid).
        let online_ext: Vec<(SystemId, String, String, u16)> = st
            .externals
            .iter()
            .filter(|(id, _)| snap.and_then(|t| t.sessions.get(id.as_str())).map(|g| g.health == Health::Ready).unwrap_or(false))
            .map(|(id, w)| (id.clone(), st.label_of(cfg, id), w.host.clone(), w.port))
            .collect();
        let mut bench_wanted: BTreeSet<(String, String)> = BTreeSet::new();
        let mut focus: BTreeMap<SystemId, Focus> = BTreeMap::new();
        let idle_console = |st: &State, id: &SystemId| -> Vec<String> {
            let mut c = st.notes.clone();
            if let Some(t) = st.idle_tails.get(id) {
                c.extend(t.iter().cloned());
            }
            cap(&mut c);
            c
        };
        for s in systems.iter_mut() {
            let id = s.id.clone();
            let sig = snap.and_then(|t| t.sessions.get(id.as_str()));
            s.node = None;
            s.editable = true;
            s.controllable = true;
            if s.model.arch.is_none() {
                if let Some(k) = model_key(cfg, s.preset.as_deref()) {
                    s.model.arch = st.arches.get(&k).cloned();
                }
            }
            s.last_session = st.last.get(&id).map(|l| {
                let mut x: LastSession = l.summary.clone();
                x.ended_ago_s = r1((now - l.ended_at).max(0.0));
                x
            });
            if let (Some(p), Some(c)) = (&s.preset, &s.command) {
                if !c.hash.is_empty() {
                    bench_wanted.insert((p.clone(), c.hash.clone()));
                    s.bench = self.cached_bench(p, &c.hash);
                }
            }
            if let Some(ctx) = st.sessions.get(&id) {
                let (status, reason) = session_status(ctx);
                s.status = status;
                s.reason = reason;
                s.session = Some(self.session_vm(&id, ctx, sig, now));
                s.activity = if ctx.phase == Phase::Live { ctx.activity } else { 0.0 };
                // A port-less server (port 0) has no address clients could use.
                s.endpoint = (ctx.p.record.port != 0).then(|| base_url(ctx.p.kind, &ctx.p.host, ctx.p.record.port));
                // A faulted System is not running and Launch on it checks conflicts (it ends the fault first), so its
                // conflicts are published too (SPEC 3 / 16.8: "Stop X & launch" comes from them). The reason stays
                // the fault title.
                let fault_conflicts =
                    if ctx.phase == Phase::Fault { conflicts::compute(&candidate(st, s), &members, &st.gpu_facts, warn).ids } else { Vec::new() };
                s.conflicts = fault_conflicts;
                let mut vram = gpu_view(&gpus, ctx.p.gpu.as_deref().or(s.gpu.as_deref()), inference.as_deref(), warn);
                if holds_memory(ctx) {
                    vram.layers.extend(ctx.layers.iter().cloned());
                }
                focus.insert(id.clone(), Focus { console: ctx.console_lines(), vram });
                continue;
            }
            let vram = gpu_view(&gpus, s.gpu.as_deref(), inference.as_deref(), warn);
            if let Some(p) = st.pending.get(&id) {
                let left: Vec<String> =
                    p.waiting_for.iter().filter(|w| st.sessions.contains_key(*w)).map(|w| st.label_of(cfg, w)).collect();
                s.status = SystemStatus::Starting;
                s.reason = Some(if left.is_empty() {
                    "Starting.".to_string()
                } else {
                    format!("Waiting for {} to stop.", conflicts::names(&left))
                });
                s.conflicts = Vec::new();
                s.activity = 0.0;
                let mut c = idle_console(st, &id);
                if !left.is_empty() {
                    c.push(format!("[KLIF] waiting for {} to stop", conflicts::names(&left)));
                }
                focus.insert(id.clone(), Focus { console: c, vram });
                continue;
            }
            if s.external {
                s.conflicts = Vec::new();
                // Externals never go invalid for a foreign port holder (SPEC 16.7): only real preset errors count.
                let has_errors =
                    s.status == SystemStatus::NotSet || (s.status == SystemStatus::Invalid && s.availability != Availability::Busy);
                if s.availability == Availability::Busy {
                    s.availability = Availability::Ready;
                    s.reason = None;
                }
                let ready = sig.map(|g| g.health == Health::Ready).unwrap_or(false);
                let ext = st.externals.get_mut(&id);
                if has_errors {
                    // Keep the catalog's not-set / invalid status and reason.
                } else if let (true, Some(w)) = (ready, ext) {
                    let since = *w.online_since.get_or_insert(now);
                    let busy = sig.map(|g| g.busy || super::session::activity_of(g) > 0.0).unwrap_or(false);
                    s.status = if busy { SystemStatus::Busy } else { SystemStatus::Online };
                    s.reason = None;
                    s.activity = sig.map(super::session::activity_of).unwrap_or(0.0);
                    let (llm, image, generic) =
                        live_parts(s.kind, true, sig.and_then(|g| g.llm.clone()), sig.and_then(|g| g.image.clone()), sig.and_then(|g| g.generic.clone()));
                    s.session = Some(Session {
                        system: id.clone(),
                        model: s.model.clone(),
                        phase: Phase::Live,
                        uptime_s: r1((now - since).max(0.0)),
                        endpoint: Endpoint { host: display_host(&w.host).to_string(), port: w.port },
                        api_key_set: false,
                        loading: None,
                        fault: None,
                        llm,
                        image,
                        generic,
                        preset: s.preset.clone(),
                        command: s.command.clone(),
                        gpu: s.gpu.clone(),
                        vram_gib: None,
                    });
                } else {
                    if let Some(w) = st.externals.get_mut(&id) {
                        w.online_since = None;
                    }
                    s.status = SystemStatus::Offline;
                    if s.reason.is_none() {
                        s.reason = Some(match st.externals.get(&id) {
                            Some(w) => format!("Not answering at {}.", base_url(s.kind, &w.host, w.port)),
                            None => "Not answering.".into(),
                        });
                    }
                    s.activity = 0.0;
                }
                let mut c = idle_console(st, &id);
                if let Some(g) = sig {
                    c.extend(g.console.iter().cloned());
                    cap(&mut c);
                }
                focus.insert(id.clone(), Focus { console: c, vram });
                continue;
            }
            // Not running: static status, then ports (external, foreign), then conflicts.
            s.activity = 0.0;
            s.session = None;
            if s.status != SystemStatus::NotSet {
                let ep = st.endpoints.get(&id).cloned();
                let ext = ep.as_ref().and_then(|(h, p)| online_ext.iter().find(|e| e.0 != id && external_port_clash((h, *p), (&e.2, e.3))));
                if let (Some((_, port)), Some(e)) = (&ep, ext) {
                    s.status = SystemStatus::Invalid;
                    s.availability = Availability::Busy;
                    s.reason = Some(format!("Port {port} is used by the external System {}.", e.1));
                } else if let Some((port, (pid, image))) = ep.as_ref().and_then(|(_, p)| st.foreign_ports.get_key_value(p)) {
                    s.status = SystemStatus::Invalid;
                    s.availability = Availability::Busy;
                    if s.reason.is_none() {
                        s.reason = Some(format!("Port {port} is in use by {image} (pid {pid}), which KLIF did not start."));
                    }
                } else if s.availability == Availability::Busy && s.status == SystemStatus::Offline {
                    s.status = SystemStatus::Invalid;
                }
            }
            if s.status == SystemStatus::Offline {
                let v = conflicts::compute(&candidate(st, s), &members, &st.gpu_facts, warn);
                if !v.ids.is_empty() {
                    s.reason = Some(conflict_reason(&v, &members));
                } else if let Some(w) = &v.warn {
                    if s.reason.is_none() {
                        s.reason = Some(w.clone());
                    }
                }
                s.conflicts = v.ids;
            } else {
                s.conflicts = Vec::new();
            }
            focus.insert(id.clone(), Focus { console: idle_console(st, &id), vram });
        }

        // ---- ghost sessions (not in klif.toml any more) ----------------------------------------------
        for (id, ctx) in st.sessions.iter().filter(|(id, _)| cfg.system(id.as_str()).is_none()) {
            let sig = snap.and_then(|t| t.sessions.get(id.as_str()));
            let (status, reason) = session_status(ctx);
            let mut vram = gpu_view(&gpus, ctx.p.gpu.as_deref(), inference.as_deref(), warn);
            if holds_memory(ctx) {
                vram.layers.extend(ctx.layers.iter().cloned());
            }
            focus.insert(id.clone(), Focus { console: ctx.console_lines(), vram });
            systems.push(System {
                id: id.clone(),
                label: ghost_label(id),
                kind: ctx.p.kind,
                class: None,
                node: None,
                status,
                reason: reason.or_else(|| Some("Not in klif.toml any more; it can only be stopped.".into())),
                availability: Availability::Unsupported,
                model: ctx.p.model.clone(),
                preset: ctx.p.preset.clone(),
                params: Vec::new(),
                command: ctx.p.command.clone(),
                bench: None,
                expected_vram: None,
                expected_vram_source: None,
                gpu: ctx.p.gpu.clone(),
                gpus: ctx.p.all_gpus(),
                external: false,
                exclusive: ctx.p.exclusive,
                editable: false,
                controllable: true,
                conflicts: Vec::new(),
                endpoint: (ctx.p.record.port != 0).then(|| base_url(ctx.p.kind, &ctx.p.host, ctx.p.record.port)),
                session: Some(self.session_vm(id, ctx, sig, now)),
                last_session: None,
                activity: if ctx.phase == Phase::Live { ctx.activity } else { 0.0 },
            });
        }

        // ---- remote nodes ------------------------------------------------------------------------------
        // An unreachable node reports no GPUs: its Systems then show this machine's inference GPU (as the UI mock
        // does), never an empty 0 GiB card. An ONLINE node that measures no GPU shows an empty card instead.
        let local_gpu = gpu_view(&gpus, None, inference.as_deref(), warn);
        merge_remote(&mut systems, &mut focus, remote, &local_gpu);

        // ---- selection and conveniences ---------------------------------------------------------------
        let selected =
            st.selected.clone().filter(|id| systems.iter().any(|s| &s.id == id)).or_else(|| systems.first().map(|s| s.id.clone()));
        let sel = selected.as_ref().and_then(|id| systems.iter().find(|s| &s.id == id));
        let (console, vram) = match selected.as_ref().and_then(|id| focus.get(id)) {
            Some(f) => (f.console.clone(), f.vram.clone()),
            None => {
                let mut c = st.notes.clone();
                cap(&mut c);
                (c, gpu_view(&gpus, None, inference.as_deref(), warn))
            }
        };
        for p in presets.iter_mut() {
            if p.bench.is_none() {
                p.bench = self.cached_preset_bench(&p.id);
            }
        }
        let downloads = {
            let d = lock(&self.downloads);
            d.infos.values().map(|(i, _)| i.clone()).collect()
        };
        lock(&self.bench).wanted = bench_wanted;
        let (records, record_events, records_rev) = self.records_view(cfg, remote);
        let mut issues = cfg_issues;
        issues.extend(st.engine_issues.iter().cloned());
        let config = ConfigInfo {
            path: cfg.source.as_ref().map(|p| p.display().to_string()),
            state_dir: self.state_dir.display().to_string(),
            data_dir: self.data_dir.display().to_string(),
            issues,
            api_key: key_info,
            models_dir: cfg.models_dir().map(|p| p.display().to_string()),
            on_conflict: cfg.launch.on_conflict,
            record_moment: cfg.ui.record_moment,
            skin: cfg.ui.skin.clone(),
            webui: self.webui_info(cfg, klif_common::now_s()),
        };
        // The inventory and its suggestions under one guard (a second lock inside the literal would deadlock).
        let (hardware, suggestions) = {
            let h = lock(&self.hardware);
            (h.info.clone(), h.suggestions.clone())
        };
        let mut vm = ViewModel {
            now,
            session: sel.and_then(|s| s.session.clone()),
            last_session: sel.and_then(|s| s.last_session.clone()),
            selected,
            console,
            vram,
            gpus,
            machine,
            host: st.host.clone(),
            presets,
            recommendations,
            hardware,
            suggestions,
            records,
            record_events,
            records_rev,
            downloads,
            config,
            nodes: remote.iter().map(|r| r.view.clone()).collect(),
            systems,
        };
        clamp_vm(&mut vm);
        for f in focus.values_mut() {
            clamp_gpu(&mut f.vram);
        }
        (vm, focus)
    }

    fn cached_bench(&self, preset: &str, hash: &str) -> Option<BenchSummary> {
        lock(&self.bench).by_hash.get(&(preset.to_string(), hash.to_string())).and_then(|(_, b)| b.clone())
    }

    fn cached_preset_bench(&self, preset: &str) -> Option<BenchSummary> {
        lock(&self.bench).by_preset.get(preset).and_then(|(_, b)| b.clone())
    }
}

/// Merge every node's LOCAL Systems as `"<node>/<id>"` (SPEC 5 + 16.14); unreachable nodes show their cached
/// Systems as unreachable.
fn merge_remote(systems: &mut Vec<System>, focus: &mut BTreeMap<SystemId, Focus>, remote: &[RemoteState], local_gpu: &GpuMemory) {
    for r in remote {
        let node = r.view.id.as_str();
        let allow: Vec<NodeRight> = r
            .view
            .allow
            .iter()
            .filter_map(|a| match a.trim().to_ascii_lowercase().as_str() {
                "launch" => Some(NodeRight::Launch),
                "edit" => Some(NodeRight::Edit),
                _ => None,
            })
            .collect();
        let addr_host = host_of(&r.view.address);
        // The node's own GPU list. When it has none, an online node shows an empty card (CPU-only, or measuring no
        // GPU); only an unreachable one falls back to this machine's inference GPU.
        let node_gpu = |gpu: Option<&str>, online: bool| -> GpuMemory {
            gpu.and_then(|g| r.view.gpus.iter().find(|m| gpu_eq(&m.id, g)))
                .or_else(|| r.view.gpus.first())
                .cloned()
                .unwrap_or_else(|| if online { empty_gpu(local_gpu.warn_below_gib) } else { local_gpu.clone() })
        };
        match &r.vm {
            Some(rvm) if r.view.state == NodeState::Online || r.cached.is_empty() => {
                for s in rvm.systems.iter().filter(|s| s.node.is_none() && !s.id.is_remote()) {
                    let local = s.id.clone();
                    let mut s = s.clone();
                    s.id = SystemId::remote(node, local.as_str());
                    s.node = Some(node.to_string());
                    s.conflicts = s.conflicts.iter().map(|c| SystemId::remote(node, c.local())).collect();
                    s.editable = Right::Edit.granted(&allow);
                    s.controllable = Right::Launch.granted(&allow);
                    s.endpoint = s.endpoint.map(|e| rewrite_url_host(&e, &addr_host));
                    if let Some(sess) = s.session.as_mut() {
                        sess.system = s.id.clone();
                        if loopback_or_wildcard(&sess.endpoint.host) {
                            sess.endpoint.host = addr_host.clone();
                        }
                    }
                    if let Some(l) = s.last_session.as_mut() {
                        l.system = s.id.clone();
                    }
                    let focused = rvm.selected.as_ref() == Some(&local);
                    let console = if focused { rvm.console.clone() } else { Vec::new() };
                    let vram = if focused && !rvm.vram.id.is_empty() {
                        rvm.vram.clone()
                    } else {
                        node_gpu(s.gpu.as_deref(), r.view.state == NodeState::Online)
                    };
                    focus.insert(s.id.clone(), Focus { console, vram });
                    systems.push(s);
                }
            }
            _ => {
                let why = r.view.error.clone().unwrap_or_else(|| {
                    format!(
                        "{} is {}.",
                        r.view.name,
                        match r.view.state {
                            NodeState::Connecting => "connecting",
                            NodeState::Online => "online",
                            NodeState::Offline => "offline",
                            NodeState::Unauthorized => "refusing this machine's token",
                            NodeState::Incompatible => "running an incompatible KLIF",
                        }
                    )
                });
                for c in &r.cached {
                    let id = SystemId::remote(node, &c.id);
                    focus.insert(id.clone(), Focus { console: Vec::new(), vram: node_gpu(None, false) });
                    systems.push(System {
                        id,
                        label: if c.label.trim().is_empty() { c.id.clone() } else { c.label.clone() },
                        kind: c.kind,
                        class: c.class,
                        node: Some(node.to_string()),
                        status: SystemStatus::Unreachable,
                        reason: Some(why.clone()),
                        availability: Availability::Unsupported,
                        model: ModelRef { name: c.model_name.clone().unwrap_or_default(), ..ModelRef::default() },
                        preset: c.preset.clone(),
                        params: Vec::new(),
                        command: None,
                        bench: None,
                        expected_vram: None,
                        expected_vram_source: None,
                        gpu: None,
                        gpus: Vec::new(),
                        external: false,
                        exclusive: false,
                        editable: false,
                        controllable: false,
                        conflicts: Vec::new(),
                        endpoint: None,
                        session: None,
                        last_session: None,
                        activity: 0.0,
                    });
                }
            }
        }
    }
}

// ------------------------------------------------------------------------------------------ clamping

fn fin(x: &mut f64) {
    if !x.is_finite() {
        *x = 0.0;
    }
}

fn fin_o(x: &mut Option<f64>) {
    if x.is_some_and(|v| !v.is_finite()) {
        *x = None;
    }
}

fn fin_v(v: &mut [f64]) {
    v.iter_mut().for_each(fin);
}

fn clamp_model(m: &mut ModelRef) {
    fin_o(&mut m.weights_gib);
}

fn clamp_bench(b: &mut Option<BenchSummary>) {
    if let Some(b) = b.as_mut() {
        fin(&mut b.at);
        fin_o(&mut b.load_s);
        fin_o(&mut b.ttft_s);
        fin_o(&mut b.prefill_tps);
        fin_o(&mut b.decode_tps);
        fin_o(&mut b.seconds_per_image);
        fin_o(&mut b.peak_vram_gib);
        fin_o(&mut b.spill_mib);
    }
}

fn clamp_layers(l: &mut [VramLayer]) {
    l.iter_mut().for_each(|x| fin(&mut x.gib));
}

pub(crate) fn clamp_gpu(g: &mut GpuMemory) {
    fin(&mut g.total_gib);
    fin(&mut g.used_gib);
    clamp_layers(&mut g.layers);
    fin(&mut g.spill_mib);
    fin_v(&mut g.history);
    if let Some(h) = g.layer_history.as_mut() {
        h.values_mut().for_each(|v| fin_v(v));
    }
    fin(&mut g.baseline_gib);
    fin(&mut g.warn_below_gib);
    if let Some(d) = g.dormant.as_mut() {
        fin(&mut d.paged_out_gib);
        fin(&mut d.since_s);
    }
}

fn clamp_session(s: &mut Session) {
    clamp_model(&mut s.model);
    fin(&mut s.uptime_s);
    if let Some(l) = s.loading.as_mut() {
        fin(&mut l.fraction);
        fin(&mut l.elapsed_s);
    }
    if let Some(f) = s.fault.as_mut() {
        fin(&mut f.since_s);
    }
    if let Some(l) = s.llm.as_mut() {
        fin(&mut l.decode_tps);
        fin_v(&mut l.decode_history);
        if let Some(p) = l.prefill.as_mut() {
            fin(&mut p.tps);
            fin(&mut p.elapsed_s);
            fin(&mut p.eta_s);
        }
        if let Some(sp) = l.spec.as_mut() {
            fin(&mut sp.acceptance_pct);
        }
        for r in l.requests.iter_mut() {
            fin(&mut r.at);
            fin(&mut r.prefill_s);
            fin(&mut r.decode_s);
        }
    }
    if let Some(i) = s.image.as_mut() {
        fin(&mut i.s_per_it);
        fin(&mut i.elapsed_s);
        for j in i.recent.iter_mut() {
            fin(&mut j.at);
            fin(&mut j.seconds);
        }
    }
    if let Some(g) = s.generic.as_mut() {
        fin_o(&mut g.last_activity_s);
    }
    fin_o(&mut s.vram_gib);
    if let Some(c) = s.command.as_mut() {
        let _ = c;
    }
}

fn clamp_last(l: &mut LastSession) {
    clamp_model(&mut l.model);
    fin(&mut l.uptime_s);
    fin(&mut l.ended_ago_s);
    fin_o(&mut l.decode_tps);
    fin_o(&mut l.seconds_per_image);
}

fn clamp_machine(m: &mut klif_common::vm::MachineStats) {
    fin(&mut m.ram_used_gib);
    fin(&mut m.ram_total_gib);
    fin(&mut m.cpu_pct);
}

/// Replace every non-finite float of the view model (NaN / inf would serialize as null and break readers).
pub(crate) fn clamp_vm(vm: &mut ViewModel) {
    fin(&mut vm.now);
    for s in vm.systems.iter_mut() {
        fin(&mut s.activity);
        s.activity = s.activity.clamp(0.0, 1.0);
        clamp_model(&mut s.model);
        clamp_bench(&mut s.bench);
        if let Some(l) = s.expected_vram.as_mut() {
            clamp_layers(l);
        }
        if let Some(x) = s.session.as_mut() {
            clamp_session(x);
        }
        if let Some(l) = s.last_session.as_mut() {
            clamp_last(l);
        }
    }
    if let Some(x) = vm.session.as_mut() {
        clamp_session(x);
    }
    if let Some(l) = vm.last_session.as_mut() {
        clamp_last(l);
    }
    clamp_gpu(&mut vm.vram);
    vm.gpus.iter_mut().for_each(clamp_gpu);
    clamp_machine(&mut vm.machine);
    for p in vm.presets.iter_mut() {
        clamp_model(&mut p.model);
        clamp_bench(&mut p.bench);
    }
    for r in vm.recommendations.iter_mut() {
        fin_o(&mut r.min_vram_gib);
        if let Some(m) = r.measured.as_mut() {
            fin_o(&mut m.decode_tps);
            fin_o(&mut m.prefill_tps);
            fin_o(&mut m.seconds_per_image);
        }
    }
    for n in vm.nodes.iter_mut() {
        fin_o(&mut n.latency_ms);
        n.gpus.iter_mut().for_each(clamp_gpu);
        if let Some(m) = n.machine.as_mut() {
            clamp_machine(m);
        }
        for p in n.presets.iter_mut() {
            clamp_model(&mut p.model);
            clamp_bench(&mut p.bench);
        }
    }
}
