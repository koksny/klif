//! klif-catalog: turns klif.toml Systems and presets into what KLIF shows and runs.
//!
//! - `Catalog::systems`: the static part of every local System (label, kind, preset, params, command, model,
//!   availability, expected VRAM, endpoint); the engine fills status / session / conflicts / activity.
//! - `Catalog::presets` / `preset_info` / `preset_detail`: the presets (masked).
//! - `Catalog::command_view`: the exact `CommandView` of a (possibly unsaved) preset spec for a System.
//! - `Catalog::plan`: the `LaunchPlan` (a resolved `LaunchSpec` for the supervisor plus telemetry facts).
//! - `store`: comment-preserving klif.toml writes (toml_edit). `facts`: per-adapter flag tables.
//!   `recommend`: the embedded model pool `data/recommendations.toml` and its recommendations. `suggest`: the
//!   suggested model per slot for a machine (estimates). `probe`: cached file checks.
//!
//! Everything a preset runs comes from one resolver (`resolve.rs`): params → placeholders → program / cwd / env →
//! port / host → facts → issues. `CommandView.display` is `klif_common::cmdline::render` of the same program and
//! args the supervisor gets (secrets masked, `{env:X}` shown as `%X%`).
//!
//! Frozen API: SPEC section 4 (klif-catalog). Owner: package B.

pub mod facts;
pub mod probe;
pub mod recommend;
mod resolve;
pub mod store;
pub mod suggest;

use anyhow::{anyhow, bail, Result};
use klif_common::cmdline;
use klif_common::config::{Config, PresetCfg, SystemCfg};
use klif_common::launch::LaunchSpec;
use klif_common::vm::{
    AdapterId, Availability, CommandView, HealthCheck, Issue, IssueLevel, ModelRef, ParamOption, ParamView, PresetDetail, PresetInfo,
    RecFile, RecommendationInfo, System, SystemId, SystemKind, SystemStatus, VramLayer, VramLayerId, VramSource,
};
use klif_common::Secret;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub use recommend::Recommendation;
pub use resolve::{external_port_clash, ports_overlap};
use resolve::{Cat, ForSystem, KeyInfo, Req, Resolved};

/// Facts the catalog needs from the live system.
#[derive(Debug, Clone, Default)]
pub struct LiveFacts {
    /// Ports held by a process KLIF does not own: port -> "image.exe (pid N)" (for the Busy / invalid reason).
    pub foreign_ports: BTreeMap<u16, String>,
    /// An API key is configured (the managed key env row shows in CommandView).
    pub api_key_set: bool,
    /// Measured VRAM composition of the last live session per preset hash (expected VRAM "measured").
    pub layers: BTreeMap<String, Vec<VramLayer>>,
}

/// Everything needed to start one session of a System.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    /// What the supervisor starts (API key as `EnvVal::Secret`).
    pub spec: LaunchSpec,
    pub system: SystemId,
    pub kind: SystemKind,
    pub preset_id: String,
    pub adapter: AdapterId,
    pub model: ModelRef,
    /// Host to probe and to show in the endpoint (a wildcard bind host becomes loopback).
    pub host: String,
    pub port: u16,
    pub health: HealthCheck,
    /// The server exposes Prometheus /metrics (llama.cpp `--metrics` / LLAMA_ARG_ENDPOINT_METRICS, vllm always).
    pub metrics: bool,
    /// What runs, masked (persisted as Session.command).
    pub command: CommandView,
    /// "VEN:DEV" or "cpu" (preset `gpu`, else `[gpu] inference`): the first listed GPU.
    pub gpu: Option<String>,
    /// Every GPU the preset lists (SPEC 16.22; `gpu` is the first). Added by B.
    pub gpus: Vec<String>,
    pub spec_mode: Option<String>,
    pub ctx_tokens: Option<u32>,
}

pub struct Catalog {
    pub(crate) probe: probe::FileProbe,
    pub(crate) recs: Vec<Recommendation>,
}

// The engine shares one catalog between its tick thread and the host's command threads.
const _: fn() = || {
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<Catalog>();
};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// The local Systems' labels, configs and active presets, in file order.
struct SysRow<'a> {
    id: SystemId,
    label: String,
    sys: &'a SystemCfg,
}

/// One System's resolved active preset (or why there is none).
enum Active<'a> {
    NotSet,
    Missing(String),
    Bad(String, String),
    Ok(&'a str, &'a PresetCfg, Box<Resolved>),
}

/// Where a System's active preset listens, for the cross-System port warning.
struct PortRow {
    host: String,
    port: u16,
    external: bool,
    gpus: Vec<String>,
}

impl PortRow {
    fn of(cfg: &Config, spec: &PresetCfg, r: &Resolved) -> Option<PortRow> {
        Some(PortRow { host: r.host.clone(), port: r.port?, external: spec.is_external(), gpus: gpus_of(cfg, spec) })
    }
}

impl Catalog {
    /// Build the catalog for a config (embedded recommendations, empty file cache). Cheap; never fails.
    pub fn new(cfg: &Config) -> Catalog {
        let _ = cfg;
        Catalog { probe: probe::FileProbe::default(), recs: recommend::embedded().to_vec() }
    }

    /// A catalog with these recommendations instead of the embedded ones (checking a candidate data file before it
    /// is embedded; entries are not re-validated: use `recommend::parse`). Added by B.
    pub fn with_recommendations(cfg: &Config, recs: Vec<Recommendation>) -> Catalog {
        let _ = cfg;
        Catalog { probe: probe::FileProbe::default(), recs }
    }

    /// Forget cached file checks (after a download finished or the user installed a model). Added by B.
    pub fn refresh_files(&self) {
        self.probe.clear();
    }

    fn rows<'a>(cfg: &'a Config) -> Vec<SysRow<'a>> {
        cfg.system_labels().into_iter().zip(cfg.systems.iter()).map(|((id, label), (_, sys))| SysRow { id, label, sys }).collect()
    }

    fn resolve_for<'a>(&self, cfg: &'a Config, row: &SysRow<'a>, key: KeyInfo, checks: bool, stamp: Option<&str>) -> Active<'a> {
        self.resolve_with(&self.probe, cfg, row, key, checks, stamp)
    }

    /// `resolve_for` with an explicit file probe (a launch asks the disk itself: `FileProbe::fresh`).
    fn resolve_with<'a>(
        &self,
        probe: &probe::FileProbe,
        cfg: &'a Config,
        row: &SysRow<'a>,
        key: KeyInfo,
        checks: bool,
        stamp: Option<&str>,
    ) -> Active<'a> {
        let Some(pid) = row.sys.preset_id() else { return Active::NotSet };
        if let Some(e) = cfg.bad_presets.get(pid) {
            return Active::Bad(pid.to_string(), e.clone());
        }
        let Some((pid, spec)) = cfg.presets.get_key_value(pid) else { return Active::Missing(pid.to_string()) };
        let r = resolve::resolve(&Req {
            cfg,
            spec,
            params: &row.sys.params,
            system: Some(ForSystem { label: &row.label, sys: row.sys }),
            key,
            probe,
            checks,
            stamp,
        });
        Active::Ok(pid.as_str(), spec, Box::new(r))
    }

    /// Where every local System's active preset listens (no file checks), in row order.
    fn port_rows(&self, cfg: &Config, rows: &[SysRow]) -> Vec<Option<PortRow>> {
        rows.iter()
            .map(|row| match self.resolve_for(cfg, row, KeyInfo::Unknown, false, None) {
                Active::Ok(_, spec, r) => PortRow::of(cfg, spec, &r),
                _ => None,
            })
            .collect()
    }

    /// The cross-System port warning of a local System's active preset ("Shares port N with System X; they cannot
    /// run at the same time."), as `systems()` and `command_view()` add it to the System's command. `plan()`
    /// leaves it out (its command is persisted with the session and this warning depends on other Systems): a
    /// caller that SHOWS a plan adds it. None when there is no clash. Added by the fix round (SPEC 4).
    pub fn cross_port_issue_for(&self, cfg: &Config, system: &SystemId) -> Option<Issue> {
        let rows = Self::rows(cfg);
        let i = rows.iter().position(|r| r.id == *system)?;
        cross_port_issue(i, &self.port_rows(cfg, &rows), &rows)
    }

    /// The static part of every local System in file order. Status is not-set (no preset), invalid (errors,
    /// missing files, port held by a foreign process) or offline here; the engine maps the live state onto it.
    /// `editable` and `controllable` = true (local).
    pub fn systems(&self, cfg: &Config, live: &LiveFacts) -> Vec<System> {
        let rows = Self::rows(cfg);
        let actives: Vec<Active> =
            rows.iter().map(|row| self.resolve_for(cfg, row, KeyInfo::Set(live.api_key_set), true, None)).collect();
        let ports: Vec<Option<PortRow>> = actives
            .iter()
            .map(|a| match a {
                Active::Ok(_, spec, r) => PortRow::of(cfg, spec, r),
                _ => None,
            })
            .collect();
        let mut out = Vec::with_capacity(rows.len());
        for (i, (row, active)) in rows.iter().zip(actives).enumerate() {
            let mut sys = System {
                id: row.id.clone(),
                label: row.label.clone(),
                kind: row.sys.kind,
                class: row.sys.class,
                node: None,
                status: SystemStatus::NotSet,
                reason: None,
                availability: Availability::Unsupported,
                model: ModelRef::default(),
                preset: row.sys.preset_id().map(str::to_string),
                params: Vec::new(),
                command: None,
                bench: None,
                expected_vram: None,
                expected_vram_source: None,
                gpu: None,
                gpus: Vec::new(),
                external: false,
                exclusive: row.sys.exclusive,
                editable: true,
                controllable: true,
                conflicts: Vec::new(),
                endpoint: None,
                session: None,
                last_session: None,
                activity: 0.0,
            };
            match active {
                Active::NotSet => sys.reason = Some("No preset is selected for this System.".into()),
                Active::Missing(pid) => {
                    sys.status = SystemStatus::Invalid;
                    sys.reason = Some(format!("Preset \"{pid}\" does not exist in klif.toml."));
                }
                Active::Bad(_pid, e) => {
                    // The sentence already names the preset ("Preset \"x\" could not be read: ...").
                    sys.status = SystemStatus::Invalid;
                    sys.availability = Availability::Invalid;
                    sys.reason = Some(e);
                }
                Active::Ok(_pid, spec, r) => {
                    let mut view = self.view_of(cfg, spec, &r);
                    if let Some(found) = cross_port_issue(i, &ports, &rows) {
                        view.issues.push(found);
                    }
                    let (availability, reason) = self.availability(spec, &r, &live.foreign_ports);
                    sys.availability = availability;
                    sys.status = if availability == Availability::Ready { SystemStatus::Offline } else { SystemStatus::Invalid };
                    sys.reason = reason;
                    sys.model = self.model_ref_of(cfg, spec, &r);
                    sys.params = param_views(spec, &row.sys.params);
                    let (layers, source) = self.expected_vram(cfg, spec, &r, &view.hash, live);
                    sys.expected_vram = layers;
                    sys.expected_vram_source = source;
                    sys.gpus = gpus_of(cfg, spec);
                    sys.gpu = sys.gpus.first().cloned();
                    sys.external = spec.is_external();
                    sys.endpoint = endpoint_of(&r);
                    sys.command = Some(view);
                }
            }
            out.push(sys);
        }
        out
    }

    /// Every local preset as listed, by id (including ids in `cfg.bad_presets`, as Invalid).
    pub fn presets(&self, cfg: &Config, live: &LiveFacts) -> Vec<PresetInfo> {
        let mut ids: Vec<&String> = cfg.presets.keys().chain(cfg.bad_presets.keys()).collect();
        ids.sort();
        ids.dedup();
        ids.into_iter().filter_map(|id| self.preset_info(cfg, id, live)).collect()
    }

    /// One preset as listed (also for ids in `cfg.bad_presets`, as Invalid).
    pub fn preset_info(&self, cfg: &Config, id: &str, live: &LiveFacts) -> Option<PresetInfo> {
        if let Some(e) = cfg.bad_presets.get(id) {
            return Some(PresetInfo {
                id: id.to_string(),
                name: id.to_string(),
                adapter: AdapterId::default(),
                kind: SystemKind::Llm,
                model: ModelRef::default(),
                availability: Availability::Invalid,
                reason: Some(e.clone()),
                recommended: None,
                bench: None,
                gpu: None,
                external: false,
                node: None,
                spec_hash: String::new(),
            });
        }
        let spec = cfg.presets.get(id)?;
        let r = self.resolve_plain(cfg, spec, &BTreeMap::new(), KeyInfo::Set(live.api_key_set), true);
        Some(self.info_of(cfg, id, spec, &r, live))
    }

    fn info_of(&self, cfg: &Config, id: &str, spec: &PresetCfg, r: &Resolved, live: &LiveFacts) -> PresetInfo {
        let (availability, reason) = self.availability(spec, r, &live.foreign_ports);
        PresetInfo {
            id: id.to_string(),
            name: spec.display_name(id).to_string(),
            adapter: spec.adapter,
            kind: r.kind.unwrap_or(SystemKind::Llm),
            model: self.model_ref_of(cfg, spec, r),
            availability,
            reason,
            recommended: spec.recommended.clone(),
            bench: None,
            gpu: gpus_of(cfg, spec).into_iter().next(),
            external: spec.is_external(),
            node: None,
            spec_hash: Self::spec_hash(spec),
        }
    }

    /// One preset in full for the editor; secret env / arg values are MASK; `command` with the params' defaults
    /// (`command_view` gives a System's view). None for unknown ids and for entries that do not parse
    /// (`cfg.bad_presets`; fix those in klif.toml).
    pub fn preset_detail(&self, cfg: &Config, id: &str, live: &LiveFacts) -> Option<PresetDetail> {
        let spec = cfg.presets.get(id)?;
        let r = self.resolve_plain(cfg, spec, &BTreeMap::new(), KeyInfo::Set(live.api_key_set), true);
        let command = self.view_of(cfg, spec, &r);
        let info = self.info_of(cfg, id, spec, &r, live);
        Some(PresetDetail { id: id.to_string(), spec: spec.masked(), command, info, spec_hash: Self::spec_hash(spec) })
    }

    /// Hash of a stored preset spec, every field (not only what runs: also name, notes, health, gpu, api_key,
    /// params...), taken over the MASKED spec the editor is served (`PresetDetail.spec_hash`), so no secret can
    /// be guessed from it; a MASK sent back is resolved against the file at write time anyway. sha256 of the
    /// spec's JSON (struct field order, sorted env, params in file order), first 16 hex digits. Comment or
    /// whitespace edits do not change it. `store::upsert_preset` compares `base_hash` with it.
    pub fn spec_hash(spec: &PresetCfg) -> String {
        let json = serde_json::to_string(&spec.masked()).unwrap_or_default();
        hex16(&Sha256::digest(json.as_bytes()))
    }

    fn resolve_plain(&self, cfg: &Config, spec: &PresetCfg, params: &BTreeMap<String, String>, key: KeyInfo, checks: bool) -> Resolved {
        resolve::resolve(&Req { cfg, spec, params, system: None, key, probe: &self.probe, checks, stamp: None })
    }

    /// The command a (possibly unsaved) preset spec would run for `system` (its params selection, its kind),
    /// with issues (including "shares port N with System X").
    pub fn command_view(&self, cfg: &Config, spec: &PresetCfg, system: Option<&SystemId>, live: &LiveFacts) -> CommandView {
        let rows = Self::rows(cfg);
        let me = system.and_then(|s| rows.iter().position(|r| r.id == *s));
        let r = match me {
            Some(i) => resolve::resolve(&Req {
                cfg,
                spec,
                params: &rows[i].sys.params,
                system: Some(ForSystem { label: &rows[i].label, sys: rows[i].sys }),
                key: KeyInfo::Set(live.api_key_set),
                probe: &self.probe,
                checks: true,
                stamp: None,
            }),
            None => self.resolve_plain(cfg, spec, &BTreeMap::new(), KeyInfo::Set(live.api_key_set), true),
        };
        let mut view = self.view_of(cfg, spec, &r);
        if let Some(port) = r.port.filter(|_| r.external.is_none()) {
            if let Some(holder) = live.foreign_ports.get(&port) {
                view.issues.push(Issue::warn(Some("port"), format!("Port {port} is held by {holder}, which KLIF does not own.")));
            }
        }
        if let Some(i) = me {
            let mut ports = self.port_rows(cfg, &rows);
            ports[i] = PortRow::of(cfg, spec, &r);
            if let Some(found) = cross_port_issue(i, &ports, &rows) {
                view.issues.push(found);
            }
        }
        view
    }

    /// The launch plan of a System's active preset. `stamp` (yyyyMMdd-HHmmss-fff) names the session and is what
    /// `{stamp}` expands to in the command it runs (the plan's CommandView and hash keep `{stamp}` as written).
    /// Errors are the REASON as one sentence, without the System's name (callers say "<label> cannot start:
    /// <reason>"): no preset, error issues (the first, plus how many more), missing exe/model, external preset. A
    /// busy port is the engine's check. The command carries no cross-System issues (see `cross_port_issue_for`).
    pub fn plan(&self, cfg: &Config, system: &SystemId, api_key: Option<&Secret>, stamp: &str) -> Result<LaunchPlan> {
        let rows = Self::rows(cfg);
        let row = rows.iter().find(|r| r.id == *system).ok_or_else(|| anyhow!("There is no System \"{system}\" in klif.toml."))?;
        // What is launched is checked against the disk now, not against the view's cached file answers.
        let fresh = probe::FileProbe::fresh();
        let (pid, spec, r) = match self.resolve_with(&fresh, cfg, row, KeyInfo::Value(api_key), true, Some(stamp)) {
            Active::NotSet => bail!("No preset is selected; choose one in Tune or with `klif-cli presets use {system} <preset>`."),
            Active::Missing(pid) => bail!("Preset \"{pid}\" does not exist in klif.toml."),
            Active::Bad(_pid, e) => bail!("{}", e.trim_end_matches('.')),
            Active::Ok(pid, spec, r) => (pid, spec, r),
        };
        if let Some(ep) = &r.external {
            bail!("It is an external server ({}); KLIF does not start or stop it. Start it where it runs.", ep.url);
        }
        let errors: Vec<&str> = r.errors().map(|f| f.issue.text.as_str()).collect();
        if let Some(first) = errors.first() {
            let first = first.trim_end_matches('.');
            match errors.len() {
                1 => bail!("{first}."),
                n => bail!("{first} (and {} more: `klif-cli presets show {pid}` lists them).", n - 1),
            }
        }
        let exe = r.program.clone().ok_or_else(|| anyhow!("Its program could not be resolved."))?;
        let cwd = r.cwd.clone().ok_or_else(|| anyhow!("Its working folder is unknown."))?;
        let port = r.port.unwrap_or(0);
        let session_name = session_name(stamp, system.as_str(), pid, port);
        let logs = cfg.logs_dir();
        let command = self.view_of(cfg, spec, &r);
        let model = self.model_ref_of(cfg, spec, &r);
        let gpus = gpus_of(cfg, spec);
        let metrics = match spec.adapter {
            AdapterId::Vllm => true,
            AdapterId::LlamaCpp => r.facts.metrics,
            _ => false,
        };
        Ok(LaunchPlan {
            spec: LaunchSpec {
                exe,
                args: r.args.clone(),
                cwd,
                env_remove: r.env_remove.clone(),
                env_set: r.env_set.clone(),
                out_log: logs.join(format!("{session_name}.out.log")),
                err_log: logs.join(format!("{session_name}.err.log")),
                session_name,
                port,
            },
            system: system.clone(),
            kind: r.kind.unwrap_or(row.sys.kind),
            preset_id: pid.to_string(),
            adapter: spec.adapter,
            host: resolve::connect_host(&r.host),
            port,
            health: r.health.clone(),
            metrics,
            spec_mode: model.spec_mode.clone(),
            ctx_tokens: model.ctx_tokens,
            model,
            command,
            gpu: gpus.first().cloned(),
            gpus,
        })
    }

    /// The (host, port) a System's active preset listens on (external: from `endpoint`); None when unknown or
    /// the System has no usable preset. The host is the BIND host (a wildcard stays "0.0.0.0" / "::", which
    /// overlaps every host: see [`ports_overlap`]).
    pub fn effective_port(&self, cfg: &Config, system: &SystemId) -> Option<(String, u16)> {
        let rows = Self::rows(cfg);
        let row = rows.iter().find(|r| r.id == *system)?;
        match self.resolve_for(cfg, row, KeyInfo::Unknown, false, None) {
            Active::Ok(_, _, r) => r.port.map(|p| (r.host.clone(), p)),
            _ => None,
        }
    }

    /// Display facts of a preset for a System (name, quant, adapter label, backend, device, ctx, spec mode...).
    pub fn model_ref(&self, cfg: &Config, spec: &PresetCfg, system: Option<&SystemId>) -> ModelRef {
        let empty = BTreeMap::new();
        let params = system.and_then(|s| cfg.system(s.as_str())).map(|s| &s.params).unwrap_or(&empty);
        let r = self.resolve_plain(cfg, spec, params, KeyInfo::Unknown, false);
        self.model_ref_of(cfg, spec, &r)
    }

    /// Hash of what a preset runs with these params: resolved program, args, env and cwd (not display metadata).
    /// Bench records and measured VRAM layers are keyed by it; `CommandView.hash` carries it. Stable across
    /// processes: `{env:X}` counts as written (not its value, also in `command`), `{stamp}` as written, secret
    /// values and the API key itself (and whether one is set) do not count; the preset's `api_key = false`
    /// opt-out does (it changes the child's environment). For "changed on disk" use [`Catalog::spec_hash`].
    pub fn preset_hash(cfg: &Config, spec: &PresetCfg, params: &BTreeMap<String, String>) -> String {
        let probe = probe::FileProbe::fresh();
        let r =
            resolve::resolve(&Req { cfg, spec, params, system: None, key: KeyInfo::Unknown, probe: &probe, checks: false, stamp: None });
        hash_of(&r)
    }

    /// Every recommendation with its install state for this config. The pool has a hundred rungs, so the presets'
    /// model files are resolved once per call, and a rung's files are probed only when its repo folder exists.
    pub fn recommendations(&self, cfg: &Config) -> Vec<RecommendationInfo> {
        let loaded = self.preset_models(cfg);
        let dir_ok = recommend::models_dir(cfg).is_some_and(|d| self.probe.is_dir(&d));
        self.recs.iter().map(|rec| self.rec_info(cfg, rec, dir_ok, &loaded)).collect()
    }

    fn rec_info(&self, cfg: &Config, rec: &Recommendation, dir_ok: bool, loaded: &[(String, PathBuf)]) -> RecommendationInfo {
        let present = dir_ok
            && rec.files.iter().all(|f| {
                recommend::install_path(cfg, rec, &f.name)
                    .filter(|p| p.ancestors().nth(f.install_name().split('/').count()).is_none_or(|repo| self.probe.is_dir(repo)))
                    .and_then(|p| self.probe.file_size(&p))
                    .is_some_and(|size| f.size.is_none_or(|want| want == size))
            });
        let existing = self.existing_in(rec, loaded);
        RecommendationInfo {
            id: rec.id.clone(),
            kind: rec.kind,
            class: rec.class,
            name: rec.name.clone(),
            adapter: rec.adapter,
            hf_repo: rec.hf_repo.clone(),
            revision: rec.revision.clone(),
            files: rec.files.iter().map(|f| RecFile { name: f.name.clone(), role: f.role, size_bytes: f.size }).collect(),
            quant: rec.quant.clone(),
            license: rec.license.clone(),
            hardware_class: rec.hardware_class.clone(),
            min_vram_gib: rec.min_vram_gib,
            measured: rec.measured.as_ref().map(Into::into),
            notes: rec.notes.clone(),
            installed: present || existing.is_some(),
            existing,
        }
    }

    /// Every preset's model file: `model` first (relative = under the server's working folder), else the model the
    /// args load.
    fn preset_models(&self, cfg: &Config) -> Vec<(String, PathBuf)> {
        cfg.presets
            .iter()
            .filter_map(|(id, spec)| {
                let r = self.resolve_plain(cfg, spec, &BTreeMap::new(), KeyInfo::Unknown, false);
                let path = r.model_path.as_deref().map(|p| r.abs(&p.to_string_lossy())).or_else(|| r.loaded_model())?;
                Some((id.clone(), path))
            })
            .collect()
    }

    /// A preset that already references the recommendation's model file (same file name, and the same size
    /// when the recommendation knows it).
    fn existing_preset(&self, cfg: &Config, rec: &Recommendation) -> Option<String> {
        self.existing_in(rec, &self.preset_models(cfg))
    }

    fn existing_in(&self, rec: &Recommendation, loaded: &[(String, PathBuf)]) -> Option<String> {
        let file = rec.model_file()?;
        let want_name = file.install_name().rsplit('/').next().unwrap_or(file.install_name());
        loaded.iter().find_map(|(id, path)| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let same_name = if cfg!(windows) { name.eq_ignore_ascii_case(want_name) } else { name == want_name };
            let same_size = same_name
                && match file.size {
                    Some(want) => self.probe.file_size(path) == Some(want),
                    None => true,
                };
            same_size.then(|| id.clone())
        })
    }

    pub fn recommendation(&self, id: &str) -> Option<&Recommendation> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// A new preset (id, spec) from a recommendation, with the command/cwd/env/env_remove (and the gpu / backend
    /// display that belong to that program) copied from a preset of the same adapter (the target System's current
    /// one first, else the first by id that has a command). Without one the command stays empty (Invalid: Tune
    /// focuses Program). The target System's current preset of the same adapter also gives its `port`. The id is the
    /// recommendation's with '.' as '-' (plus "-2", "-3"... when taken). Context, KV type and fit margin are the
    /// caller's (`suggest::apply_fit`). The caller stores it.
    pub fn preset_from_recommendation(&self, cfg: &Config, id: &str, system: Option<&SystemId>) -> Result<(String, PresetCfg)> {
        let rec = self.recommendation(id).ok_or_else(|| anyhow!("There is no recommendation \"{id}\"."))?;
        let sys = match system {
            Some(s) => {
                let sys = cfg.system(s.as_str()).ok_or_else(|| anyhow!("There is no System \"{s}\" in klif.toml."))?;
                if sys.kind != rec.kind {
                    let label = cfg.system_label(s.as_str()).unwrap_or_else(|| s.to_string());
                    bail!(
                        "{} is {} model, but {label} is {} System.",
                        rec.name,
                        resolve::with_article(rec.kind.label()),
                        resolve::with_article(sys.kind.label())
                    );
                }
                Some(sys)
            }
            None => None,
        };
        // Systems run side by side: a System keeps the port of its own preset (same adapter); the pool's port is
        // only the default.
        let own_port = sys
            .and_then(|s| s.preset_id())
            .and_then(|pid| cfg.presets.get(pid))
            .filter(|p| p.adapter == rec.adapter && !p.is_external())
            .and_then(|p| p.port);
        // Candidates: the System's own preset, the presets active on the Systems (file order), then every preset by
        // id; the first of the same adapter whose program resolves wins.
        let mut candidates: Vec<&str> = Vec::new();
        candidates.extend(sys.and_then(|s| s.preset_id()));
        candidates.extend(cfg.systems.iter().filter_map(|(_, s)| s.preset_id()));
        candidates.extend(cfg.presets.keys().map(String::as_str));
        let source = candidates.into_iter().filter_map(|pid| cfg.presets.get(pid)).find(|p| {
            p.adapter == rec.adapter && !p.is_external() && !p.command.trim().is_empty() && {
                let r = self.resolve_plain(cfg, p, &BTreeMap::new(), KeyInfo::Unknown, true);
                r.program.is_some() && !r.errors().any(|f| f.issue.field.as_deref() == Some("command"))
            }
        });

        let path_of = |f: Option<&recommend::RecFileSpec>| -> Option<String> {
            let f = f?;
            recommend::install_path(cfg, rec, &f.name).map(|p| p.to_string_lossy().into_owned())
        };
        let mut model = path_of(rec.model_file());
        if let Some(existing) = self.existing_preset(cfg, rec) {
            // The file is already referenced by a preset: use that exact path.
            if let Some(p) = cfg.presets.get(&existing).and_then(|p| p.model.clone()) {
                model = Some(p);
            }
        }
        // `{file:<name>}` in the recommendation's args: that file's install path (with models_dir unset,
        // `{models_dir}\...`, which the preset then reports as "models_dir is not set").
        let file_path = |name: &str| -> String {
            match recommend::install_path(cfg, rec, name) {
                Some(p) => p.to_string_lossy().into_owned(),
                None => match recommend::install_rel(rec, name) {
                    Some(rel) => format!("{{models_dir}}{}{}", std::path::MAIN_SEPARATOR, rel.to_string_lossy()),
                    None => name.to_string(),
                },
            }
        };
        let args: Vec<String> = rec.args.iter().map(|a| recommend::expand_file_refs(a, file_path)).collect();
        let mut spec = PresetCfg {
            name: Some(rec.name.clone()),
            adapter: rec.adapter,
            kind: (rec.adapter.default_kind() != Some(rec.kind)).then_some(rec.kind),
            args,
            port: own_port.or(rec.port),
            model,
            mmproj: path_of(rec.mmproj_file()),
            ctx: rec.ctx,
            health: rec.health.clone(),
            quant: Some(rec.quant.clone()).filter(|q| !q.trim().is_empty()),
            recommended: Some(rec.id.clone()),
            notes: rec.notes.clone(),
            ..PresetCfg::default()
        };
        if let Some(src) = source {
            spec.command = src.command.clone();
            spec.cwd = src.cwd.clone();
            spec.env = src.env.clone();
            spec.env_remove = src.env_remove.clone();
            spec.gpu = src.gpu.clone();
            spec.backend = src.backend.clone();
            spec.device = src.device.clone();
        }
        // A recommendation id is `<model>.<rung>`; a preset id has no dots ("qwen3.8-27b.ud-q4-k-xl" ->
        // "qwen3-8-27b-ud-q4-k-xl"), leaving room for a "-N" suffix.
        let base: String = rec.id.chars().map(|c| if c == '.' { '-' } else { c }).take(60).collect();
        let base = base.trim_end_matches('-').to_string();
        let mut new_id = base.clone();
        let mut n = 2;
        while cfg.presets.contains_key(&new_id) || cfg.bad_presets.contains_key(&new_id) {
            new_id = format!("{base}-{n}");
            n += 1;
        }
        klif_common::config::validate_preset_id(&new_id).map_err(|e| anyhow!(e))?;
        Ok((new_id, spec))
    }

    // ---------------------------------------------------------------------------------------- helpers

    /// The CommandView of a resolved preset (no cross-System issues).
    fn view_of(&self, _cfg: &Config, spec: &PresetCfg, r: &Resolved) -> CommandView {
        let hash = hash_of(r);
        if let Some(ep) = &r.external {
            return CommandView {
                program: String::new(),
                args: Vec::new(),
                cwd: String::new(),
                env: Vec::new(),
                port: ep.port,
                host: ep.host.clone(),
                health: r.health.clone(),
                adapter: spec.adapter,
                display: ep.url.clone(),
                issues: r.issue_list(),
                hash,
                external: Some(ep.url.clone()),
            };
        }
        CommandView {
            program: r.program_display.clone(),
            args: r.args_display.clone(),
            cwd: r.cwd_display.clone(),
            env: r.env_view.clone(),
            port: r.port.unwrap_or(0),
            host: r.host.clone(),
            health: r.health.clone(),
            adapter: spec.adapter,
            display: cmdline::render(&r.program_display, &r.args_display),
            issues: r.issue_list(),
            hash,
            external: None,
        }
    }

    /// Availability and the reason sentence (precedence: errors > program missing > model missing > foreign port).
    fn availability(&self, spec: &PresetCfg, r: &Resolved, foreign: &BTreeMap<u16, String>) -> (Availability, Option<String>) {
        let first = |cat: Cat| r.errors().find(|f| f.cat == cat).map(|f| f.issue.text.clone());
        if let Some(t) = first(Cat::Other) {
            return (Availability::Invalid, Some(t));
        }
        if let Some(t) = first(Cat::Exe) {
            return (Availability::ExeMissing, Some(t));
        }
        if let Some(t) = first(Cat::Model) {
            return (Availability::ModelMissing, Some(t));
        }
        if !spec.is_external() {
            if let Some((port, holder)) = r.port.and_then(|p| foreign.get(&p).map(|h| (p, h))) {
                return (Availability::Busy, Some(format!("Port {port} is held by {holder}, which KLIF does not own.")));
            }
        }
        (Availability::Ready, None)
    }

    fn model_ref_of(&self, cfg: &Config, spec: &PresetCfg, r: &Resolved) -> ModelRef {
        let loaded = r.loaded_model();
        let file_name = loaded
            .as_deref()
            .or(r.model_path.as_deref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .or_else(|| r.facts.model.clone());
        let stem = file_name.as_deref().map(|n| n.trim_end_matches(".gguf").trim_end_matches(".safetensors").to_string());
        let name = nonempty(&spec.model_name).or_else(|| nonempty(&spec.name)).or(stem).unwrap_or_default();
        let quant = nonempty(&spec.quant)
            .or_else(|| r.facts.quant.as_deref().map(str::to_ascii_uppercase))
            .or_else(|| file_name.as_deref().and_then(quant_from_name))
            .unwrap_or_default();
        let gpus = gpus_of(cfg, spec);
        let device = nonempty(&spec.device).unwrap_or_else(|| match gpus.first().map(String::as_str) {
            Some("cpu") => "CPU".into(),
            _ if r.external.is_some() => String::new(),
            Some(g) if !same_gpu(cfg.gpu.inference.as_deref(), g) => String::new(),
            _ => cfg.gpu.inference_name.clone().unwrap_or_default(),
        });
        let weights = if r.external.is_some() { None } else { loaded.as_deref().and_then(|p| self.probe.model_bytes(p)) };
        ModelRef {
            name,
            quant,
            engine: spec.adapter.as_str().to_string(),
            backend: nonempty(&spec.backend).or_else(|| r.facts.backend.clone()).unwrap_or_default(),
            device,
            ctx_tokens: r.facts.ctx.or(spec.ctx),
            kv_type: r.facts.kv_type.clone(),
            spec_mode: r.facts.spec_mode.clone(),
            vision: r.facts.vision,
            mode: r.facts.mode.clone(),
            image_size: r.facts.image_size.clone(),
            weights_gib: weights.map(|b| b as f64 / GIB),
            arch: None,
        }
    }

    /// Measured layers of this exact command (by hash), else weights-only from the files the args load when no
    /// CPU-offload flag is present and the preset is not on the CPU.
    fn expected_vram(
        &self,
        cfg: &Config,
        spec: &PresetCfg,
        r: &Resolved,
        hash: &str,
        live: &LiveFacts,
    ) -> (Option<Vec<VramLayer>>, Option<VramSource>) {
        if let Some(l) = live.layers.get(hash).filter(|l| !l.is_empty()) {
            return (Some(l.clone()), Some(VramSource::Measured));
        }
        if r.external.is_some() || r.facts.cpu_offload || gpus_of(cfg, spec).first().is_some_and(|g| g == "cpu") {
            return (None, None);
        }
        // Relative paths are relative to the server's working folder (as the existence checks read them).
        let size = |p: &str| self.probe.model_bytes(&r.abs(p));
        let Some(model) = r.loaded_model().and_then(|p| self.probe.model_bytes(&p)) else { return (None, None) };
        let extra: u64 = r.facts.extra_weights.iter().filter_map(|p| size(p)).sum();
        let mut layers = vec![VramLayer { id: VramLayerId::Weights, label: "Weights".into(), gib: (model + extra) as f64 / GIB }];
        if let Some(b) = r.facts.mmproj.as_deref().and_then(size) {
            layers.push(VramLayer { id: VramLayerId::Projector, label: "Projector".into(), gib: b as f64 / GIB });
        }
        if let Some(b) = r.facts.draft_model.as_deref().and_then(size) {
            layers.push(VramLayer { id: VramLayerId::Draft, label: "Draft".into(), gib: b as f64 / GIB });
        }
        (Some(layers), Some(VramSource::FileSize))
    }
}

fn nonempty(v: &Option<String>) -> Option<String> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn same_gpu(a: Option<&str>, b: &str) -> bool {
    match a.and_then(klif_common::config::normalize_gpu_id) {
        Some(a) => a == b || a.split('#').next() == b.split('#').next() && !b.contains('#'),
        None => true,
    }
}

/// The GPUs a preset runs on (external: only what it states itself; it may run on another machine).
fn gpus_of(cfg: &Config, spec: &PresetCfg) -> Vec<String> {
    if spec.is_external() {
        return spec.gpu.as_deref().map(|l| l.split(',').filter_map(klif_common::config::normalize_gpu_id).collect()).unwrap_or_default();
    }
    cfg.preset_gpus(spec)
}

/// The base URL clients use (LLM: `.../v1`).
fn endpoint_of(r: &Resolved) -> Option<String> {
    let llm = r.kind == Some(SystemKind::Llm);
    let base = match &r.external {
        Some(ep) => ep.url.clone(),
        None => resolve::base_url(&r.host, r.port?),
    };
    Some(if llm && !base.ends_with("/v1") { format!("{base}/v1") } else { base })
}

/// "shares port N with X" for System `i` (warn on the System itself; externals are never warned about here,
/// their clash shows on the local System that uses the same address). Two local Systems on one GPU where either is
/// `exclusive` never run together anyway, so their shared port is not worth a warning (the engine's conflict rules
/// are unchanged).
fn cross_port_issue(i: usize, ports: &[Option<PortRow>], rows: &[SysRow]) -> Option<Issue> {
    let me = ports.get(i)?.as_ref()?;
    if me.external {
        return None;
    }
    let mine = (me.host.as_str(), me.port);
    let mut local: Vec<&str> = Vec::new();
    let mut ext: Vec<&str> = Vec::new();
    for (j, other) in ports.iter().enumerate() {
        let Some(o) = other else { continue };
        if j == i {
            continue;
        }
        if o.external {
            // An external endpoint: only the same address (or a loopback one when this binds a wildcard).
            if external_port_clash(mine, (&o.host, o.port)) {
                ext.push(&rows[j].label);
            }
            continue;
        }
        if !ports_overlap(mine, (&o.host, o.port)) {
            continue;
        }
        let exclusive = rows[i].sys.exclusive || rows[j].sys.exclusive;
        if exclusive && me.gpus.iter().any(|g| o.gpus.contains(g)) {
            continue;
        }
        local.push(&rows[j].label);
    }
    let port = me.port;
    let mut parts = Vec::new();
    if !local.is_empty() {
        parts.push(format!("Shares port {port} with {}; they cannot run at the same time.", local.join(", ")));
    }
    if !ext.is_empty() {
        parts.push(format!("Port {port} is used by the external System {}.", ext.join(", ")));
    }
    (!parts.is_empty()).then(|| Issue { level: IssueLevel::Warn, field: Some("port".into()), text: parts.join(" ") })
}

/// The params of a preset with a System's selection.
fn param_views(spec: &PresetCfg, selection: &BTreeMap<String, String>) -> Vec<ParamView> {
    spec.params
        .iter()
        .map(|(name, p)| {
            let value = selection
                .get(name)
                .filter(|v| p.choices.contains_key(*v))
                .cloned()
                .or_else(|| p.default_choice().map(str::to_string))
                .unwrap_or_default();
            ParamView {
                name: name.clone(),
                label: p.label.clone().filter(|l| !l.trim().is_empty()).unwrap_or_else(|| name.clone()),
                value,
                choices: p
                    .choices
                    .iter()
                    .map(|(v, c)| ParamOption {
                        value: v.clone(),
                        label: c.label.clone().filter(|l| !l.trim().is_empty()).unwrap_or_else(|| v.clone()),
                    })
                    .collect(),
            }
        })
        .collect()
}

/// `klif-<stamp>-<system>-<preset>-p<port>`, every character outside `[A-Za-z0-9_.-]` replaced by `-`.
fn session_name(stamp: &str, system: &str, preset: &str, port: u16) -> String {
    format!("klif-{stamp}-{system}-{preset}-p{port}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') { c } else { '-' })
        .collect()
}

/// The quant in a model file name: IQ4_XS, Q4_K_M, Q8_0, UD-Q4_K_XL → Q4_K_XL, BF16, F16, MXFP4...
pub fn quant_from_name(name: &str) -> Option<String> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?i)(IQ\d_(?:XXS|XS|NL|S|M|L)|TQ\d_\d|Q\d_K(?:_(?:XXL|XL|S|M|L))?|Q\d_\d|MXFP4|BF16|F16|F32)")
            .expect("quant regex")
    });
    re.find(name).map(|m| m.as_str().to_ascii_uppercase())
}

/// The preset hash: sha256 over the canonical form of what runs (first 16 hex digits).
fn hash_of(r: &Resolved) -> String {
    let doc = serde_json::json!({
        "adapter": r.adapter.as_str(),
        "external": r.external.as_ref().map(|e| e.url.clone()),
        "program": r.program_display,
        "args": r.args_display,
        "cwd": r.cwd_display,
        "env": r.hash_env,
        "remove": r.hash_remove,
        "port": r.port,
        "host": r.host,
    });
    hex16(&Sha256::digest(doc.to_string().as_bytes()))
}

/// The first 8 bytes of a digest as 16 lower-case hex digits.
fn hex16(digest: &[u8]) -> String {
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}
