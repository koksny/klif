//! What the engine keeps in sync with the world each tick: klif.toml (hot reload, own writes ignored by mtime +
//! size, a file that does not parse keeps the last good config + issues), the API key file, the measured GPUs, the
//! external Systems' permanent watches, foreign port owners, the bench cache, the network listener and its auth.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use klif_catalog::{Catalog, LiveFacts};
use klif_common::config::{normalize_gpu_id, Config, LoadedConfig, FILE_FIELD};
use klif_common::vm::{BenchSummary, HealthCheck, Issue, SystemId, SystemKind};
use klif_supervisor::{PortOwner, ProcessHost};
use klif_telemetry::{Adapter, WatchSpec};

use super::{lock, ExternalWatch, HardwareState, Inner, State, Stat};
use crate::control::{serve_network, NodeAuth};
use crate::keys;

/// The API key info is recomputed at least this often (env sources cannot be stat'ed).
const KEY_INFO_S: f64 = 5.0;
/// Bench summaries are re-read at most this often.
const BENCH_TTL_S: f64 = 5.0;
/// `NodeAuth::refresh` cadence.
const AUTH_REFRESH_S: f64 = 1.0;
/// A wanted listener that did not start is retried this often.
const LISTEN_RETRY_S: f64 = 5.0;

pub(crate) fn stat(path: &Path) -> Option<Stat> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// `[nodes.*]` as compared between reloads.
pub(crate) fn nodes_signature(cfg: &Config) -> String {
    format!("{:?}", cfg.nodes)
}

/// The GPU ids a config uses: `[gpu] inference` first, then every System's active preset GPUs, then `extra`
/// (running sessions). Normalized, deduplicated, without "cpu".
pub(crate) fn wanted_gpus(cfg: &Config, extra: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |g: &str| {
        if let Some(n) = normalize_gpu_id(g).filter(|n| n != "cpu") {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    };
    if let Some(i) = cfg.gpu.inference.as_deref() {
        push(i);
    }
    for (id, _) in &cfg.systems {
        if let Some((_, spec)) = cfg.system_preset(id.as_str()) {
            for g in cfg.preset_gpus(spec) {
                push(&g);
            }
        }
    }
    for g in extra {
        push(g);
    }
    out
}

/// Resolve GPU ids ("VEN:DEV" or "VEN:DEV#n", n = 0-based among identical adapters in DXGI order) to adapters.
/// Returns the adapters, the ids measured and console notes for ids that were not found.
pub(crate) fn resolve_gpus(cfg: &Config, extra: &[String]) -> (Vec<Adapter>, Vec<String>, Vec<String>) {
    let wanted = wanted_gpus(cfg, extra);
    if wanted.is_empty() {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let all = klif_telemetry::adapters();
    let inference = cfg.gpu.inference.as_deref().and_then(normalize_gpu_id);
    let mut adapters = Vec::new();
    let mut ids = Vec::new();
    let mut notes = Vec::new();
    for id in wanted {
        let (pci, n) = match id.split_once('#') {
            Some((p, n)) => (p.to_string(), n.parse::<usize>().unwrap_or(0)),
            None => (id.clone(), 0),
        };
        let Some((ven, dev)) = klif_telemetry::parse_pci(&pci) else { continue };
        match all.iter().filter(|a| a.vendor_id == ven && a.device_id == dev).nth(n) {
            Some(a) => {
                let mut a = a.clone();
                if inference.as_deref() == Some(id.as_str()) {
                    if let Some(name) = cfg.gpu.inference_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                        a.name = name.to_string();
                    }
                }
                adapters.push(a);
                ids.push(id);
            }
            None => {
                log::warn!("GPU {id} not found among the DXGI adapters");
                notes.push(format!("[KLIF] GPU {id} was not found; its VRAM is not measured"));
            }
        }
    }
    (adapters, ids, notes)
}

impl Inner {
    // ---- config ----------------------------------------------------------------------------

    /// Reload klif.toml when its mtime / size changed (own writes reload at once and are then unchanged).
    pub(super) fn reload_if_changed(&self, _now: f64) {
        self.reload_config(false);
    }

    /// Re-read klif.toml (`force`: even when unchanged). Returns true when a new config was applied.
    pub(crate) fn reload_config(&self, force: bool) -> bool {
        // One reload at a time (tick, own writes, ensure_config): a reloader that waited re-stats and re-reads, so
        // an older file is never applied after a newer one's stat was recorded.
        let _reload = lock(&self.reload_lock);
        let path = self.cfg().file_path();
        let now_stat = stat(&path);
        {
            let mut f = lock(&self.files);
            let same_path = f.config_path.as_deref() == Some(path.as_path());
            if !force && same_path && f.config == now_stat {
                return false;
            }
            let had_file = f.config.is_some();
            f.config = now_stat;
            f.config_path = Some(path.clone());
            if now_stat.is_none() && had_file {
                // Deleted (or mid-replace by an editor): keep the last good config, say so.
                drop(f);
                let mut l = lock(&self.loaded);
                l.issues.retain(|i| i.field.as_deref() != Some(FILE_FIELD));
                l.issues.push(Issue::warn(
                    Some(FILE_FIELD),
                    format!("{} is missing; KLIF keeps the configuration it last read.", path.display()),
                ));
                return false;
            }
            if now_stat.is_none() {
                return false;
            }
        }
        let new = klif_common::config::load_from(&path);
        self.apply_config(new)
    }

    /// Use a freshly loaded config (a file that does not parse keeps the last good config, with its issues).
    pub(crate) fn apply_config(&self, new: LoadedConfig) -> bool {
        if new.unreadable() {
            log::warn!("klif.toml does not parse; keeping the last good configuration");
            lock(&self.loaded).issues = new.issues;
            return false;
        }
        for i in new.issues.iter().filter(|i| i.is_error()) {
            log::warn!("config: {}", i.text);
        }
        let cfg = new.cfg.clone();
        *lock(&self.catalog) = Arc::new(Catalog::new(&cfg));
        *lock(&self.loaded) = new;
        lock(&self.files).key = None;
        log::info!("klif.toml reloaded ({} Systems, {} presets)", cfg.systems.len(), cfg.presets.len());
        // Remote nodes.
        let sig = nodes_signature(&cfg);
        let changed = {
            let mut s = lock(&self.nodes_sig);
            let changed = *s != sig;
            *s = sig;
            changed
        };
        if changed {
            self.nodes.reconfigure(&cfg);
        }
        self.sync_gpus(&cfg);
        self.sync_hardware(&cfg);
        self.sync_listener(&cfg);
        self.sync_webui(&cfg);
        true
    }

    /// Point telemetry at the GPUs the config (and the running sessions) use.
    pub(super) fn sync_gpus(&self, cfg: &Config) {
        let extra: Vec<String> = lock(&self.st).sessions.values().flat_map(|s| s.p.all_gpus()).collect();
        let wanted = wanted_gpus(cfg, &extra);
        if *lock(&self.gpu_ids) == wanted {
            return;
        }
        let (adapters, ids, notes) = resolve_gpus(cfg, &extra);
        *lock(&self.gpu_ids) = wanted;
        log::info!("measuring GPUs: {}", if ids.is_empty() { "none".to_string() } else { ids.join(", ") });
        self.tel(|t| t.set_gpus(adapters));
        if !notes.is_empty() {
            lock(&self.st).notes.extend(notes);
        }
    }

    /// Recompute the machine inventory when `[hardware]` changed (the adapters are read again then, not per tick).
    pub(super) fn sync_hardware(&self, cfg: &Config) {
        if lock(&self.hardware).cfg == cfg.hardware {
            return;
        }
        let state = HardwareState::read(&cfg.hardware);
        *lock(&self.hardware) = state;
    }

    /// Start, stop or move the network listener to match `[node] listen`; refresh its auth every second.
    pub(super) fn sync_listener(&self, cfg: &Config) {
        self.sync_listener_with(cfg, false)
    }

    /// `retry`: try again to start a listener that is wanted but not running (e.g. the token file appeared).
    fn sync_listener_with(&self, cfg: &Config, retry: bool) {
        let want = cfg.node.as_ref().and_then(|n| n.listen_addr());
        let mut s = lock(&self.servers);
        if s.listen == want && !(retry && want.is_some() && s.network.is_none()) {
            return;
        }
        if let Some(n) = s.network.take() {
            log::info!("network listener on {} stopped", n.addr());
            n.shutdown();
        }
        s.auth = None;
        s.listen = want.clone();
        let mut issue: Option<Issue> = None;
        if let Some(addr) = want {
            match NodeAuth::load(cfg) {
                Ok(auth) => {
                    let auth = Arc::new(auth);
                    match self.handle().map(|h| serve_network(h, auth.clone())) {
                        Some(Ok(n)) => {
                            log::info!("network listener on {}", n.addr());
                            s.network = Some(n);
                            s.auth = Some(auth);
                        }
                        Some(Err(e)) => {
                            issue = Some(Issue::error(Some("node.listen"), format!("This machine does not listen on {addr}: {e:#}")));
                            s.auth = Some(auth);
                        }
                        None => {}
                    }
                }
                Err(e) => {
                    // NodeAuth's sentence says why and what to run (`klif-cli node token --create`).
                    issue = Some(Issue::error(Some("node.listen"), format!("{e:#}")));
                }
            }
        }
        drop(s);
        let mut st = lock(&self.st);
        st.engine_issues.retain(|i| i.field.as_deref() != Some("node.listen"));
        st.engine_issues.extend(issue);
    }

    /// Start, stop or move klif-webui to match `[webui]`. It serves only when the host gave the engine the page's
    /// files (the window app does; klif-cli does not).
    pub(super) fn sync_webui(&self, cfg: &Config) {
        self.sync_webui_with(cfg, false)
    }

    /// `retry`: try again to start a page that is wanted but not serving (port was busy, the files arrived).
    pub(crate) fn sync_webui_with(&self, cfg: &Config, retry: bool) {
        let want = cfg.webui.listen_addr();
        let assets = lock(&self.web_assets).clone();
        let mut s = lock(&self.servers);
        if s.webui_addr == want && !(retry && want.is_some() && s.webui.is_none()) {
            return;
        }
        if let Some(w) = s.webui.take() {
            w.shutdown();
        }
        // A code shown for the old page must not pair with the new one.
        self.web_auth.pair_cancel();
        s.webui_addr = want.clone();
        let last_error = s.webui_error.take();
        s.webui_urls = (0.0, Vec::new());
        let Some(addr) = want else { return };
        s.last_webui_try = klif_common::now_s();
        let Some(assets) = assets else {
            s.webui_error = Some("This KLIF engine has no page to serve: klif-webui runs in the KLIF window app, not in klif-cli.".into());
            return;
        };
        match self.handle().map(|h| crate::webui::serve(h, self.web_auth.clone(), assets, &addr)) {
            Some(Ok(w)) => {
                log::info!("klif-webui on {}", w.addr());
                s.webui = Some(w);
            }
            Some(Err(e)) => {
                let kind = e.root_cause().downcast_ref::<std::io::Error>().map(std::io::Error::kind);
                s.webui_error = Some(match kind {
                    Some(std::io::ErrorKind::AddrInUse) => {
                        format!("Port {} is taken by another program. Choose another port.", cfg.webui.port)
                    }
                    Some(std::io::ErrorKind::AddrNotAvailable) => {
                        format!("This machine has no address {}. Choose 0.0.0.0 (every network) or one of its addresses.", cfg.webui.host)
                    }
                    _ => format!("klif-webui could not listen on {addr}: {e:#}"),
                });
                if s.webui_error != last_error {
                    log::warn!("klif-webui: {}", s.webui_error.as_deref().unwrap_or_default());
                }
            }
            None => {}
        }
    }

    /// Whether klif-webui serves now.
    pub(crate) fn webui_serving(&self) -> bool {
        lock(&self.servers).webui.is_some()
    }

    /// klif-webui as Tune shows it: settings, whether it serves and where, devices, the open pairing.
    pub(crate) fn webui_info(&self, cfg: &Config, now: f64) -> klif_common::vm::WebUiInfo {
        let (listening, error, urls) = {
            let mut s = lock(&self.servers);
            let listening = s.webui.is_some();
            let age = now - s.webui_urls.0;
            if listening && (age >= 30.0 || (s.webui_urls.1.is_empty() && age >= 5.0)) {
                s.webui_urls = (now, crate::webui::page_urls(&cfg.webui.host, cfg.webui.port));
            }
            (listening, if cfg.webui.enabled { s.webui_error.clone() } else { None }, if listening { s.webui_urls.1.clone() } else { Vec::new() })
        };
        let pairing = self.web_auth.pairing(now).filter(|_| listening).map(|(secret, code, expires_at)| klif_common::vm::WebUiPairing {
            url: urls.first().map(|u| format!("{u}#pair={secret}")).unwrap_or_default(),
            code,
            expires_at,
        });
        klif_common::vm::WebUiInfo {
            enabled: cfg.webui.enabled,
            host: cfg.webui.host.clone(),
            port: cfg.webui.port,
            listening,
            urls,
            error,
            devices: self.web_auth.devices(),
            pairing,
        }
    }

    /// The API key info (re-read when api-key.txt changes), the listener's auth, the bench cache and downloads.
    pub(super) fn refresh_side_facts(&self, now: f64) {
        let cfg = self.cfg();
        let source = cfg.security.api_key.clone();
        let key_path = cfg.state_path(keys::KEY_FILE);
        let kstat = stat(&key_path);
        let recompute = {
            let f = lock(&self.files);
            match &f.key {
                Some((src, st, at)) => *src != source || *st != kstat || now - at >= KEY_INFO_S,
                None => true,
            }
        };
        if recompute {
            let info = keys::info(&cfg);
            *lock(&self.key_info) = info;
            lock(&self.files).key = Some((source, kstat, now));
        }
        // NodeAuth re-reads [node] allow and node-token.txt (mtime) every second; a wanted listener that could not
        // start (no token yet) is retried every few seconds.
        let (auth, retry) = {
            let mut s = lock(&self.servers);
            let retry = s.listen.is_some() && s.network.is_none() && now - s.last_listen_try >= LISTEN_RETRY_S;
            if retry {
                s.last_listen_try = now;
            }
            if now - s.last_auth_refresh >= AUTH_REFRESH_S {
                s.last_auth_refresh = now;
                (s.auth.clone(), retry)
            } else {
                (None, retry)
            }
        };
        if let Some(a) = auth {
            a.refresh(&cfg);
        }
        if retry {
            self.sync_listener_with(&cfg, true);
        }
        let webui_retry = {
            let mut s = lock(&self.servers);
            let due = s.webui_addr.is_some() && s.webui.is_none() && now - s.last_webui_try >= LISTEN_RETRY_S;
            if due {
                s.last_webui_try = now;
            }
            due
        };
        if webui_retry {
            self.sync_webui_with(&cfg, true);
        }
        // Downloads: a finished file changes the installed flags.
        let rebuild = {
            let mut d = lock(&self.downloads);
            d.infos.retain(|_, (i, at)| {
                !(matches!(i.state, klif_common::vm::DownloadState::Done | klif_common::vm::DownloadState::Cancelled) && now - *at > 30.0)
            });
            std::mem::take(&mut d.rebuild)
        };
        if rebuild {
            // A downloaded file: forget the cached file checks so `installed` / availability see it.
            self.catalog().refresh_files();
        }
    }

    /// LiveFacts for catalog calls made outside the tick.
    pub(crate) fn live_facts(&self, _cfg: &Config) -> LiveFacts {
        let api_key_set = lock(&self.key_info).set;
        let st = lock(&self.st);
        live_facts_of(&st, api_key_set)
    }

    // ---- externals ---------------------------------------------------------------------------

    /// Watch every configured external System permanently (re-watched when its endpoint changes).
    pub(super) fn sync_externals(&self, st: &mut State, cfg: &Config, catalog: &Catalog, now: f64) {
        let mut desired: BTreeMap<SystemId, (String, WatchSpec)> = BTreeMap::new();
        for (id, sys) in &cfg.systems {
            if st.sessions.contains_key(id) {
                continue;
            }
            let Some((pid, spec)) = cfg.system_preset(id.as_str()) else { continue };
            if !spec.is_external() {
                continue;
            }
            let Some((host, port)) = catalog.effective_port(cfg, id) else { continue };
            let kind = spec.effective_kind().unwrap_or(sys.kind);
            let health = HealthCheck::from_preset(spec.health.as_deref()).unwrap_or_default();
            let gpu = spec
                .gpu
                .as_deref()
                .map(|g| g.split(',').filter_map(normalize_gpu_id).collect::<Vec<_>>().join(","))
                .filter(|g| !g.is_empty());
            let signature = format!("{pid}|{}|{host}|{port}|{kind}|{:?}|{gpu:?}", spec.adapter, health);
            let w = WatchSpec {
                kind,
                adapter: spec.adapter,
                external: true,
                out_log: None,
                err_log: None,
                host: host.clone(),
                port,
                // Never KLIF's key to a server KLIF does not own (SPEC 16.12).
                api_key: None,
                started_at: now,
                ctx_tokens: spec.ctx,
                spec_mode: None,
                health,
                metrics: spec.adapter == klif_common::vm::AdapterId::Vllm,
                expect_device: None,
                gpu,
            };
            desired.insert(id.clone(), (signature, w));
        }
        let gone: Vec<SystemId> =
            st.externals.iter().filter(|(id, w)| desired.get(*id).map(|d| d.0 != w.signature).unwrap_or(true)).map(|(id, _)| id.clone()).collect();
        for id in gone {
            st.externals.remove(&id);
            if !st.sessions.contains_key(&id) {
                self.tel(|t| t.unwatch(id.as_str()));
            }
        }
        for (id, (signature, spec)) in desired {
            if st.externals.contains_key(&id) {
                continue;
            }
            let (host, port) = (spec.host.clone(), spec.port);
            log::info!("watching external System {id} at {host}:{port}");
            self.tel(|t| t.watch(id.as_str(), spec, false));
            st.externals.insert(id, ExternalWatch { signature, host, port, online_since: None });
        }
    }

    // ---- ports -------------------------------------------------------------------------------

    /// Who listens on every local System's port (queried outside the state lock). A port held by a PID of one
    /// of our sessions is ours; anything else is foreign (never touched).
    pub(super) fn refresh_ports(&self, cfg: &Config, catalog: &Catalog) {
        let mut endpoints: BTreeMap<SystemId, (String, u16)> = BTreeMap::new();
        for (id, _) in &cfg.systems {
            if let Some(hp) = catalog.effective_port(cfg, id) {
                endpoints.insert(id.clone(), hp);
            }
        }
        let (ports, known): (BTreeSet<u16>, BTreeSet<u32>) = {
            let st = lock(&self.st);
            let external: BTreeSet<&SystemId> = st.externals.keys().collect();
            let mut ports: BTreeSet<u16> =
                endpoints.iter().filter(|(id, _)| !external.contains(id)).map(|(_, (_, p))| *p).collect();
            ports.extend(st.sessions.values().map(|s| s.p.record.port));
            let known = st.sessions.values().flat_map(|s| s.known_pids.iter().copied()).collect();
            (ports, known)
        };
        let mut foreign: BTreeMap<u16, (u32, String)> = BTreeMap::new();
        for port in ports {
            match self.sup.port_owner(port, None) {
                PortOwner::Free | PortOwner::Ours { .. } => {}
                PortOwner::Foreign { pid, .. } if known.contains(&pid) => {}
                PortOwner::Foreign { pid, image } => {
                    foreign.insert(port, (pid, image));
                }
            }
        }
        let mut st = lock(&self.st);
        st.foreign_ports = foreign;
        st.endpoints = endpoints;
    }

    // ---- bench -------------------------------------------------------------------------------

    /// Refresh the cached bench summaries the view model shows: per System (preset id, its command hash, as the
    /// last compose asked for) and per preset (hash with default params). Reads happen here, outside `st`.
    pub(super) fn refresh_bench(&self, cfg: &Config, now: f64) {
        let (stale_hash, stale_presets) = {
            let b = lock(&self.bench);
            let hashes: Vec<(String, String)> = b
                .wanted
                .iter()
                .filter(|k| b.by_hash.get(*k).map(|(at, _)| now - at >= BENCH_TTL_S).unwrap_or(true))
                .cloned()
                .collect();
            let presets: Vec<String> = cfg
                .presets
                .keys()
                .filter(|p| b.by_preset.get(*p).map(|(at, _)| now - at >= BENCH_TTL_S).unwrap_or(true))
                .cloned()
                .collect();
            (hashes, presets)
        };
        if stale_hash.is_empty() && stale_presets.is_empty() {
            return;
        }
        let fresh_hash: Vec<((String, String), Option<BenchSummary>)> =
            stale_hash.into_iter().map(|(p, h)| { let b = crate::bench::latest(&self.data_dir, &p, &h); ((p, h), b) }).collect();
        let fresh_presets: Vec<(String, Option<BenchSummary>)> = stale_presets
            .into_iter()
            .filter_map(|p| {
                let spec = cfg.presets.get(&p)?;
                // Current = recorded for the default params or for the params any System runs this preset with.
                let mut hashes = vec![Catalog::preset_hash(cfg, spec, &BTreeMap::new())];
                for (_, sys) in cfg.systems.iter().filter(|(_, s)| s.preset.as_deref() == Some(p.as_str())) {
                    hashes.push(Catalog::preset_hash(cfg, spec, &sys.params));
                }
                let b = crate::bench::latest_any(&self.data_dir, &p, &hashes);
                Some((p, b))
            })
            .collect();
        let mut b = lock(&self.bench);
        let wanted = b.wanted.clone();
        b.by_hash.retain(|k, _| wanted.contains(k));
        b.by_preset.retain(|k, _| cfg.presets.contains_key(k));
        for (k, v) in fresh_hash {
            b.by_hash.insert(k, (now, v));
        }
        for (k, v) in fresh_presets {
            b.by_preset.insert(k, (now, v));
        }
    }

    // ---- diag ----------------------------------------------------------------------------------

    pub(super) fn diag_json(&self) -> serde_json::Value {
        let (cfg, issues) = {
            let l = lock(&self.loaded);
            (l.cfg.clone(), l.issues.clone())
        };
        let loc = klif_common::config::locate();
        let gpus: Vec<serde_json::Value> = klif_telemetry::adapters()
            .iter()
            .map(|a| {
                let pci = a.pci_id();
                serde_json::json!({
                    "id": pci,
                    "name": a.display_name(),
                    "dedicatedGiB": (a.dedicated_bytes as f64 / (1u64 << 30) as f64 * 100.0).round() / 100.0,
                    "powerState": klif_telemetry::power_state(&pci).map(|p| p.as_str().to_string()),
                    "enableUlps": if a.vendor_id == 0x1002 { self.ulps_for(&pci).and_then(|u| u.enable_ulps) } else { None },
                })
            })
            .collect();
        let measured = lock(&self.gpu_ids).clone();
        let (sessions, ports, notes, engine_issues) = {
            let st = lock(&self.st);
            let sessions: Vec<serde_json::Value> = st
                .sessions
                .iter()
                .map(|(id, s)| {
                    serde_json::json!({
                        "system": id,
                        "session": s.p.record.session_name,
                        "phase": s.phase,
                        "rootPid": s.p.record.root_pid,
                        "job": s.owned.has_job(),
                        "port": s.p.record.port,
                        "preset": s.p.preset,
                        "adapter": s.p.adapter(),
                        "inConfig": cfg.system(id.as_str()).is_some(),
                    })
                })
                .collect();
            let ports: Vec<serde_json::Value> = st
                .endpoints
                .iter()
                .map(|(id, (host, port))| {
                    let owner = st.foreign_ports.get(port).map(|(pid, image)| format!("{image} (pid {pid})"));
                    serde_json::json!({ "system": id, "host": host, "port": port, "foreignOwner": owner })
                })
                .collect();
            (sessions, ports, st.notes.clone(), st.engine_issues.clone())
        };
        let servers = {
            let s = lock(&self.servers);
            serde_json::json!({
                "localPort": s.local.as_ref().map(|l| l.port()),
                "networkListen": s.network.as_ref().map(|n| n.addr().to_string()),
            })
        };
        serde_json::json!({
            "klifVersion": klif_common::KLIF_VERSION,
            "config": cfg.source.as_ref().map(|p| p.display().to_string()),
            "configLocation": { "path": loc.path.display().to_string(), "origin": loc.origin, "exists": loc.exists },
            "stateDir": self.state_dir.display().to_string(),
            "dataDir": self.data_dir.display().to_string(),
            "logsDir": cfg.logs_dir().display().to_string(),
            "issues": issues,
            "engineIssues": engine_issues,
            "systems": cfg.systems.iter().map(|(id, s)| serde_json::json!({ "id": id, "kind": s.kind, "preset": s.preset, "exclusive": s.exclusive })).collect::<Vec<_>>(),
            "sessions": sessions,
            "ports": ports,
            "gpus": gpus,
            "measuredGpus": measured,
            "apiKey": lock(&self.key_info).clone(),
            "parentJobWarning": klif_supervisor::parent_job_warning(),
            "onConflict": cfg.launch.on_conflict,
            "servers": servers,
            "nodes": self.nodes.remote().iter().map(|r| serde_json::json!({ "id": r.view.id, "state": r.view.state, "address": r.view.address, "error": r.view.error })).collect::<Vec<_>>(),
            "notes": notes,
        })
    }
}

/// LiveFacts from the engine state.
pub(super) fn live_facts_of(st: &State, api_key_set: bool) -> LiveFacts {
    LiveFacts {
        foreign_ports: st.foreign_ports.iter().map(|(p, (pid, image))| (*p, format!("{image} (pid {pid})"))).collect(),
        api_key_set,
        layers: st.layers.clone(),
    }
}

/// Kind of a System as the config says (ghosts: None).
#[allow(dead_code)]
pub(super) fn config_kind(cfg: &Config, id: &SystemId) -> Option<SystemKind> {
    cfg.system(id.as_str()).map(|s| s.kind)
}
