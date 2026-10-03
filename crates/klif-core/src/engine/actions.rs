//! Every `Action`: routing to remote nodes (SPEC 16.1), launches with conflicts and pending launches (16.5 / 16.8),
//! stop / restart / dismiss / stop all, store writes through `klif_catalog::store` validated BEFORE any write
//! (16.11), downloads. Actions are serialized (`act_serial`); slow work (spawning, file writes, network) never runs
//! under the state lock. After a local action the engine ticks at once, so `snapshot()` shows the result.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use klif_catalog::store;
use klif_common::config::{default_system_label, validate_preset_id, validate_system_id, OnConflict, PresetCfg, SystemCfg};
use klif_common::now_s;
use klif_common::secret::{is_secret_env, is_secret_flag, MASK};
use klif_common::vm::{
    Action, AdapterId, DownloadInfo, DownloadState, LlmClass, ModelRef, Phase, System, SystemId, SystemKind, SystemStatus,
};
use klif_supervisor::{PortOwner, ProcessHost};

use super::compose::{candidate, expected_total, holders};
use super::conflicts::{self, names, Candidate, Member, Verdict};
use super::session::{model_key, SessionCtx};
use super::{lock, Inner, PendingLaunch, State};
use crate::state::PersistedSession;
use crate::{keys, timefmt};

/// A pending launch whose Systems did not stop within this time is given up.
const PENDING_TIMEOUT_S: f64 = 90.0;

/// `"Error text."` -> `"Error text"` (to embed it in a sentence).
fn trim_dot(e: impl std::fmt::Display) -> String {
    e.to_string().trim().trim_end_matches('.').to_string()
}

fn is_are(n: usize) -> &'static str {
    if n == 1 {
        "is"
    } else {
        "are"
    }
}

/// Actions whose System defaults to the selected one.
fn fill_selected(a: Action, sel: Option<&SystemId>) -> Action {
    match a {
        Action::Launch { system: None, stop_others } => Action::Launch { system: sel.cloned(), stop_others },
        Action::Stop { system: None } => Action::Stop { system: sel.cloned() },
        Action::Restart { system: None } => Action::Restart { system: sel.cloned() },
        Action::Dismiss { system: None } => Action::Dismiss { system: sel.cloned() },
        other => other,
    }
}

fn target(system: Option<SystemId>) -> Result<SystemId> {
    system.ok_or_else(|| anyhow!("Select a System first."))
}

/// The sentence for a clear secret on its way to a remote node (the same as Tune's `REMOTE_SECRET_REFUSAL`).
pub(crate) const REMOTE_SECRET_REFUSAL: &str = "Secrets cannot cross plain TCP; set them in that node's klif.toml";

/// A value that only names a variable of the node's own environment (`{env:NAME}`) carries no secret.
fn env_reference(v: &str) -> bool {
    let v = v.trim();
    v.strip_prefix("{env:").and_then(|r| r.strip_suffix('}')).is_some_and(|n| !n.is_empty() && !n.contains(['{', '}']))
}

fn clear_value(v: &str) -> bool {
    !v.is_empty() && v != MASK && !env_reference(v)
}

fn clear_env(env: &std::collections::BTreeMap<String, String>, at: &str, out: &mut Vec<String>) {
    for (k, v) in env {
        if is_secret_env(k) && clear_value(v) {
            out.push(format!("{at}{k}"));
        }
    }
}

fn clear_args(args: &[String], at: &str, out: &mut Vec<String>) {
    for (i, a) in args.iter().enumerate() {
        if is_secret_flag(a) {
            if args.get(i + 1).is_some_and(|v| clear_value(v)) {
                out.push(format!("{at}{a}"));
            }
        } else if let Some((flag, v)) = a.split_once('=').filter(|(f, _)| is_secret_flag(f)) {
            if clear_value(v) {
                out.push(format!("{at}{flag}"));
            }
        }
    }
}

/// The secret values a preset would send in clear (anything but MASK, empty or a `{env:NAME}` reference), named
/// for the message: secret env values (`is_secret_env`) and secret-flag args (`SECRET_ARG_FLAGS`), also inside
/// params' choices ("NAME.choice: HF_TOKEN"). Empty = safe to send to a remote node. Tune's `clearSecrets`
/// (tune/secret.ts) applies the same rule before the engine sees the spec.
pub(crate) fn clear_secrets(spec: &PresetCfg) -> Vec<String> {
    let mut out = Vec::new();
    clear_env(&spec.env, "", &mut out);
    clear_args(&spec.args, "", &mut out);
    for (name, p) in &spec.params {
        for (value, c) in &p.choices {
            let at = format!("{name}.{value}: ");
            clear_env(&c.env, &at, &mut out);
            clear_args(&c.args, &at, &mut out);
        }
    }
    out
}

/// "an LLM / an Image", "a Speech (TTS) / a Transcription (STT) / a Video".
fn with_article(kind: SystemKind) -> String {
    let article = match kind {
        SystemKind::Llm | SystemKind::Image => "an",
        SystemKind::Tts | SystemKind::Stt | SystemKind::Video => "a",
    };
    format!("{article} {}", kind.label())
}

/// The distinct reasons of a verdict for `ids` ("port 7030", "exclusive GPU"...), in order.
fn why_texts(v: &Verdict, ids: &[SystemId]) -> Vec<String> {
    let mut w: Vec<String> = Vec::new();
    for c in ids {
        if let Some(t) = v.why.get(c).map(|y| y.text()) {
            if !w.contains(&t) {
                w.push(t);
            }
        }
    }
    w
}

fn member_label(members: &[Member], id: &SystemId) -> String {
    members.iter().find(|m| &m.id == id).map(|m| m.label.clone()).unwrap_or_else(|| id.to_string())
}

impl Inner {
    pub(crate) fn act(&self, action: Action) -> Result<()> {
        let selected = lock(&self.published).vm.selected.clone();
        let action = fill_selected(action, selected.as_ref());
        if let Action::Select { system } = action {
            return self.select(system);
        }
        if let Some(node) = action.route().map_err(|e| anyhow!(e))?.map(str::to_string) {
            if self.cfg().remote_node(&node).is_none() {
                bail!("There is no node \"{node}\" in klif.toml.");
            }
            // A clear secret never crosses plain TCP (SPEC 7 / 16.10), whichever frontend sends it (Tune, klif-cli
            // `presets set/save --node`, any control client). MASK passes: the node keeps its stored value.
            if let Action::SavePreset { preset, .. } = &action {
                let clear = clear_secrets(preset);
                if !clear.is_empty() {
                    bail!("{REMOTE_SECRET_REFUSAL} ({}).", clear.join(", "));
                }
            }
            let target = match &action {
                Action::Launch { system: Some(t), .. } | Action::Restart { system: Some(t) } => Some(t.clone()),
                _ => None,
            };
            let r = self.nodes.act(&node, action.for_forwarding());
            if let (Ok(()), Some(t)) = (&r, target) {
                self.follow_remote(t);
            }
            self.poke();
            return r;
        }
        let r = {
            let _serial = lock(&self.act_serial);
            self.act_local(action)
        };
        self.flush();
        self.tick();
        r
    }

    fn act_local(&self, action: Action) -> Result<()> {
        match action {
            Action::Select { system } => self.select(system),
            Action::Launch { system, stop_others } => self.launch(target(system)?, stop_others),
            Action::Stop { system } => self.stop(target(system)?),
            Action::StopAll => self.stop_all(),
            Action::Restart { system } => self.restart(target(system)?),
            Action::Dismiss { system } => self.dismiss_action(target(system)?),
            Action::UsePreset { system, preset } => self.use_preset(&system, &preset),
            Action::SetParam { system, name, value } => self.set_param(&system, &name, &value),
            Action::SavePreset { id, preset, select_for, secrets_from, base_hash, node: _ } => {
                self.save_preset(&id, &preset, select_for.as_ref(), secrets_from.as_deref(), base_hash.as_deref())
            }
            Action::DeletePreset { id, node: _ } => self.delete_preset(&id),
            Action::AddSystem { id, label, kind, class, preset, node: _ } => self.add_system(id.as_deref(), label.as_deref(), kind, class, preset.as_deref()),
            Action::RemoveSystem { system } => self.remove_system(&system),
            Action::UpdateSystem { system, label, move_to, exclusive } => self.update_system(&system, label.as_deref(), move_to, exclusive),
            Action::DownloadRecommendation { id, node: _ } => self.download(&id),
            Action::CancelDownload { id, node: _ } => self.cancel_download(&id),
            Action::AdoptRecommendation { id, system } => self.adopt_recommendation(&id, system.as_ref()),
        }
    }

    // ---- selection -------------------------------------------------------------------------

    fn select(&self, system: SystemId) -> Result<()> {
        let exists = lock(&self.published).vm.systems.iter().any(|s| s.id == system) || self.cfg().system(system.as_str()).is_some();
        if !exists {
            bail!("There is no System \"{system}\".");
        }
        let prev = {
            let mut st = lock(&self.st);
            let prev = st.selected.replace(system.clone());
            st.dirty = true;
            prev
        };
        if let Some(node) = prev.as_ref().and_then(|p| p.node()) {
            if Some(node) != system.node() {
                self.nodes.set_focus(node, None);
            }
        }
        if let Some(node) = system.node() {
            self.nodes.set_focus(node, Some(system.local()));
        }
        self.flush();
        self.tick();
        Ok(())
    }

    /// Launch/Restart: the selection follows the target when the selected System holds nothing (SPEC 16.20), or
    /// when it is one of the Systems stopped for this launch (`stopping`). `live` = the Systems published with a
    /// session or starting / stopping: externals and remote Systems never are in `st.sessions`, yet an online
    /// external or a running remote System holds a session. Returns the previous selection when it moved.
    fn follow_selection(st: &mut State, id: &SystemId, stopping: &[SystemId], live: &BTreeSet<SystemId>) -> Option<Option<SystemId>> {
        let busy = st.selected.as_ref().is_some_and(|s| {
            (st.sessions.contains_key(s) || st.pending.contains_key(s) || live.contains(s)) && !stopping.contains(s)
        });
        if !busy && st.selected.as_ref() != Some(id) {
            st.dirty = true;
            return Some(st.selected.replace(id.clone()));
        }
        None
    }

    /// External and remote Systems the last view model shows with a session, or starting / stopping (taken before
    /// `st`; local Systems are judged from `st` itself).
    fn published_live(&self) -> BTreeSet<SystemId> {
        let p = lock(&self.published);
        p.vm.systems
            .iter()
            .filter(|s| s.external || s.node.is_some() || s.id.is_remote())
            .filter(|s| s.session.is_some() || matches!(s.status, SystemStatus::Starting | SystemStatus::Stopping))
            .map(|s| s.id.clone())
            .collect()
    }

    /// The selection moved away from a remote System: that node no longer needs to serve its console (no stale
    /// 2 Hz focus). Called after `st` is released.
    fn unfocus_moved(&self, prev: Option<Option<SystemId>>) {
        if let Some(node) = prev.flatten().as_ref().and_then(|p| p.node()) {
            self.nodes.set_focus(node, None);
        }
    }

    /// A forwarded Launch/Restart: select the remote target when the selected System holds nothing.
    fn follow_remote(&self, target: SystemId) {
        let selected_busy = {
            let p = lock(&self.published);
            p.vm.selected.as_ref().and_then(|s| p.vm.systems.iter().find(|x| &x.id == s)).is_some_and(|s| {
                s.session.is_some() || matches!(s.status, SystemStatus::Starting | SystemStatus::Stopping) || *s.id.as_str() == *target.as_str()
            })
        };
        if !selected_busy {
            if let Err(e) = self.select(target) {
                log::debug!("selection does not follow the remote launch: {e:#}");
            }
        }
    }

    /// The System as last published (static part + status).
    fn published_system(&self, id: &SystemId) -> Option<System> {
        lock(&self.published).vm.systems.iter().find(|s| &s.id == id).cloned()
    }

    // ---- launch ------------------------------------------------------------------------------

    fn launch(&self, id: SystemId, stop_others: bool) -> Result<()> {
        let cfg = self.cfg();
        let view = self.published_system(&id);
        let live = self.published_live();
        let (fire_now, moved) = {
            let mut st = lock(&self.st);
            let label = st.label_of(&cfg, &id);
            if cfg.system(id.as_str()).is_none() {
                if st.sessions.contains_key(&id) {
                    bail!("{label} is not in klif.toml any more; it can only be stopped.");
                }
                bail!("There is no System \"{id}\".");
            }
            if cfg.system_preset(id.as_str()).is_some_and(|(_, p)| p.is_external()) {
                bail!("{label} is an external server; start it where it runs.");
            }
            if st.pending.contains_key(&id) {
                bail!("{label} is about to launch.");
            }
            let mut wait_self = false;
            if let Some(ctx) = st.sessions.get(&id) {
                match ctx.phase {
                    // Launching over a faulted session ends that one first.
                    Phase::Fault => wait_self = true,
                    Phase::Stopping => bail!("{label} is stopping; launch it again when it has stopped."),
                    _ => bail!("{label} is already running."),
                }
            }
            let sys = view.ok_or_else(|| anyhow!("{label} is not ready yet; try again in a moment."))?;
            if !wait_self {
                match sys.status {
                    SystemStatus::NotSet => bail!("{label} has no preset; pick one in Tune or with `klif-cli presets use`."),
                    SystemStatus::Invalid => {
                        bail!("{label} cannot start: {}.", trim_dot(sys.reason.as_deref().unwrap_or("its preset has errors")))
                    }
                    _ => {}
                }
            }
            let members = holders(&st, &cfg);
            let cand = candidate(&st, &sys);
            let v = conflicts::compute(&cand, &members, &st.gpu_facts, cfg.telemetry.warn_below_gib);
            let label_of = |id: &SystemId| member_label(&members, id);
            let others: Vec<SystemId> = v.ids.iter().filter(|c| **c != id).cloned().collect();
            let pending: Vec<String> =
                others.iter().filter(|c| members.iter().any(|m| &m.id == *c && m.pending)).map(&label_of).collect();
            if !pending.is_empty() {
                bail!("{} {} about to launch.", names(&pending), is_are(pending.len()));
            }
            if !others.is_empty() {
                let labels: Vec<String> = others.iter().map(&label_of).collect();
                let busy: Vec<String> = others.iter().filter(|c| members.iter().any(|m| &m.id == *c && m.busy)).map(&label_of).collect();
                let auto = cfg.launch.on_conflict == OnConflict::Stop && busy.is_empty();
                if !stop_others && !auto {
                    let whys = why_texts(&v, &others);
                    let n = names(&labels);
                    if cfg.launch.on_conflict == OnConflict::Stop {
                        bail!(
                            "{label} needs {n} stopped first, and {} {} busy. Use \"Stop {n} & launch\" to stop {} anyway.",
                            names(&busy),
                            is_are(busy.len()),
                            if labels.len() == 1 { "it" } else { "them" }
                        );
                    }
                    bail!("{label} cannot launch while {n} {} running ({}). Use \"Stop {n} & launch\".", is_are(labels.len()), whys.join(", "));
                }
                log::info!("launching {id}: stopping {} first", names(&labels));
            }
            let moved = Self::follow_selection(&mut st, &id, &others, &live);
            // Stopped by `[launch] on_conflict = "stop"` (not an explicit stopOthers): re-checked for busy before
            // every stop (SPEC 16.8).
            let auto = !stop_others && cfg.launch.on_conflict == OnConflict::Stop && !others.is_empty();
            let mut waiting = others.clone();
            if wait_self {
                waiting.insert(0, id.clone());
                self.dismiss(&mut st, &id);
            }
            for w in waiting.iter().filter(|w| **w != id) {
                if let Some(c) = st.sessions.get_mut(w) {
                    c.tail.push(format!("[KLIF] stopping so that {label} can launch"));
                }
            }
            let Candidate { host, port, gpus, exclusive, need, .. } = cand;
            st.pending.insert(
                id.clone(),
                PendingLaunch {
                    waiting_for: waiting.clone(),
                    at: now_s(),
                    spawning: false,
                    label,
                    host,
                    port,
                    gpus,
                    exclusive,
                    expected_gib: need,
                    auto,
                },
            );
            st.dirty = true;
            (waiting.iter().all(|w| !st.sessions.contains_key(w)), moved)
        };
        self.unfocus_moved(moved);
        if fire_now {
            self.fire(&id)
        } else {
            Ok(())
        }
    }

    /// Advance pending launches: stop the Systems they wait for one after the other; return those ready to spawn.
    pub(super) fn drive_pending(&self, st: &mut State, cfg: &klif_common::config::Config, now: f64) -> Vec<SystemId> {
        let ids: Vec<SystemId> = st.pending.keys().cloned().collect();
        let mut ready = Vec::new();
        let mut failed: Vec<(SystemId, String)> = Vec::new();
        for id in ids {
            let Some(p) = st.pending.get(&id) else { continue };
            if p.spawning {
                continue;
            }
            let auto = p.auto;
            let remaining: Vec<SystemId> = p.waiting_for.iter().filter(|w| st.sessions.contains_key(*w)).cloned().collect();
            if remaining.is_empty() {
                ready.push(id);
                continue;
            }
            if now - p.at > PENDING_TIMEOUT_S {
                let labels: Vec<String> = remaining.iter().map(|w| st.label_of(cfg, w)).collect();
                failed.push((id, format!("{} did not stop in time", names(&labels))));
                continue;
            }
            // One at a time: wait while one of them is stopping or being cleaned up.
            if remaining.iter().any(|w| st.sessions.get(w).is_some_and(|c| c.phase == Phase::Stopping || c.cleanup.is_some())) {
                continue;
            }
            let first = remaining[0].clone();
            // on_conflict = "stop" never stops a holder that became busy after the launch was asked (SPEC 16.8):
            // the launch is cancelled instead; an explicit "Stop X & launch" (stopOthers) still stops it.
            if auto && st.sessions.get(&first).is_some_and(|c| c.busy && c.phase == Phase::Live) {
                let l = st.label_of(cfg, &first);
                failed.push((id, format!("{l} became busy; use \"Stop {l} & launch\" to stop it anyway")));
                continue;
            }
            match st.sessions.get(&first).map(|c| c.phase) {
                Some(Phase::Fault) => self.dismiss(st, &first),
                Some(_) => self.begin_stop(st, &first),
                None => {}
            }
        }
        for (id, why) in failed {
            let label = st.label_of(cfg, &id);
            log::warn!("launch of {id} given up: {why}");
            st.pending.remove(&id);
            st.idle_tails.entry(id).or_default().push(format!("[KLIF] launch of {label} cancelled: {why}"));
            st.dirty = true;
        }
        ready
    }

    /// Spawn a pending launch now (outside the state lock).
    pub(super) fn fire(&self, id: &SystemId) -> Result<()> {
        let cfg = self.cfg();
        let expected = {
            let mut st = lock(&self.st);
            let Some(p) = st.pending.get_mut(id) else { return Ok(()) };
            if p.spawning {
                return Ok(());
            }
            p.spawning = true;
            let p = st.pending.get(id).map(|p| (p.host.clone(), p.port, p.gpus.clone(), p.exclusive, p.expected_gib));
            // Re-check what cannot have changed for the better while it waited: ports and exclusive GPUs.
            let (host, port, gpus, exclusive, expected) = p.unwrap_or_default();
            let members: Vec<_> = holders(&st, &cfg).into_iter().filter(|m| &m.id != id).collect();
            let v = conflicts::compute(&Candidate { id: id.clone(), host, port, gpus, exclusive, need: None }, &members, &st.gpu_facts, 0.0);
            if !v.ids.is_empty() {
                let labels: Vec<String> = v.ids.iter().map(|c| st.label_of(&cfg, c)).collect();
                let label = st.label_of(&cfg, id);
                st.pending.remove(id);
                let msg = format!("{label} cannot launch: {} {} running meanwhile.", names(&labels), is_are(labels.len()));
                st.idle_tails.entry(id.clone()).or_default().push(format!("[KLIF] {msg}"));
                bail!(msg);
            }
            expected
        };
        let r = self.spawn(id, expected);
        let out = {
            let mut st = lock(&self.st);
            st.pending.remove(id);
            st.dirty = true;
            match r {
                Ok(mut ctx) => {
                    if ctx.p.model.arch.is_none() {
                        if let Some(k) = model_key(&cfg, ctx.p.preset.as_deref()) {
                            ctx.p.model.arch = st.arches.get(&k).cloned();
                        }
                    }
                    self.begin_session(&mut st, &cfg, id.clone(), ctx, false);
                    Ok(())
                }
                Err(e) => {
                    st.idle_tails.entry(id.clone()).or_default().push(format!("[KLIF] launch failed: {e:#}"));
                    Err(e)
                }
            }
        };
        self.flush();
        self.poke();
        out
    }

    /// Plan, check the port and start the process of a System's active preset.
    fn spawn(&self, id: &SystemId, expected: Option<f64>) -> Result<SessionCtx> {
        let cfg = self.cfg();
        let catalog = self.catalog();
        let label = cfg.system_label(id.as_str()).unwrap_or_else(|| id.to_string());
        let sys = cfg.system(id.as_str()).ok_or_else(|| anyhow!("There is no System \"{id}\"."))?;
        let (_, spec) = cfg.system_preset(id.as_str()).ok_or_else(|| anyhow!("{label} has no preset."))?;
        // The key only for llama.cpp / vllm presets that want it (SPEC 16.12).
        let key = if spec.api_key && !spec.is_external() && matches!(spec.adapter, AdapterId::LlamaCpp | AdapterId::Vllm) {
            keys::load(&cfg)
        } else {
            None
        };
        let plan = catalog.plan(&cfg, id, key.as_ref(), &timefmt::local_stamp()).map_err(|e| anyhow!("{label} cannot start: {}.", trim_dot(format!("{e:#}"))))?;
        match self.sup.port_owner(plan.port, None) {
            PortOwner::Free => {}
            PortOwner::Ours { pid } | PortOwner::Foreign { pid, .. } => {
                let ours = {
                    let st = lock(&self.st);
                    st.sessions.iter().find(|(_, s)| s.known_pids.contains(&pid)).map(|(k, _)| st.label_of(&cfg, k))
                };
                if let Some(o) = ours {
                    bail!("Port {} is in use by {o}.", plan.port);
                }
                let image = klif_supervisor::image_name(pid).unwrap_or_else(|| "another program".into());
                bail!("Port {} is in use by {image} (pid {pid}). KLIF will not stop a process it did not start.", plan.port);
            }
        }
        let owned = self.sup.launch(&plan.spec).map_err(|e| anyhow!("{label} could not be started: {}.", trim_dot(format!("{e:#}"))))?;
        let record = owned.record.clone();
        log::info!("launched {} as {id} (pid {}, port {}, preset {})", record.session_name, record.root_pid, plan.port, plan.preset_id);
        // Only a key KLIF really injected: when the preset's own env sets the key variable, the user's value wins
        // (shown "overridden") and probes must not send KLIF's key as the Bearer (SPEC 6 / 16.12).
        let key_env = plan.adapter.api_key_env();
        let overridden = plan.command.env.iter().any(|e| e.managed && e.overridden && Some(e.name.as_str()) == key_env);
        let api_key_set = key.is_some() && !overridden;
        drop(key);
        let header = vec![
            format!("[KLIF] started {} (pid {})", record.session_name, record.root_pid),
            format!("[KLIF] command: {}", plan.command.display),
            format!("[KLIF] stdout: {}", record.out_log.display()),
            format!("[KLIF] stderr: {}", record.err_log.display()),
        ];
        let gpus = {
            let g = cfg.preset_gpus(spec);
            if g.is_empty() {
                plan.gpu.iter().cloned().collect()
            } else {
                g
            }
        };
        let p = PersistedSession {
            record,
            kind: plan.kind,
            system: id.clone(),
            preset: Some(plan.preset_id.clone()),
            adapter: Some(plan.adapter),
            health: Some(plan.health.clone()),
            metrics: plan.metrics,
            command: Some(plan.command.clone()),
            model: ModelRef { ctx_tokens: plan.model.ctx_tokens.or(plan.ctx_tokens), ..plan.model.clone() },
            host: plan.host.clone(),
            spec_mode: plan.spec_mode.clone(),
            ctx_tokens: plan.ctx_tokens.or(plan.model.ctx_tokens),
            api_key_set,
            gpu: plan.gpu.clone().or_else(|| gpus.first().cloned()),
            gpus,
            exclusive: sys.exclusive,
            expected_gib: expected,
            origin: "klif".into(),
            card_id: plan.preset_id.clone(),
        };
        Ok(SessionCtx::new(p, owned, Phase::Starting, header))
    }

    // ---- stop / restart / dismiss ------------------------------------------------------------

    fn stop(&self, id: SystemId) -> Result<()> {
        let cfg = self.cfg();
        let mut st = lock(&self.st);
        let label = st.label_of(&cfg, &id);
        if !st.sessions.contains_key(&id) && cfg.system_preset(id.as_str()).is_some_and(|(_, p)| p.is_external()) {
            bail!("{label} is an external server; stop it where it runs.");
        }
        let cancelled = match st.pending.get(&id) {
            Some(p) if p.spawning => bail!("{label} is being started; stop it again in a moment."),
            Some(_) => {
                st.pending.remove(&id);
                st.idle_tails.entry(id.clone()).or_default().push(format!("[KLIF] launch of {label} cancelled"));
                st.dirty = true;
                true
            }
            None => false,
        };
        match st.sessions.get(&id).map(|c| c.phase) {
            None if cancelled => Ok(()),
            None => bail!("{label} is not running."),
            Some(Phase::Fault) => {
                self.dismiss(&mut st, &id);
                Ok(())
            }
            Some(Phase::Stopping) => Ok(()),
            Some(_) => {
                self.begin_stop(&mut st, &id);
                Ok(())
            }
        }
    }

    fn stop_all(&self) -> Result<()> {
        let mut st = lock(&self.st);
        let cancelled: Vec<SystemId> = st.pending.iter().filter(|(_, p)| !p.spawning).map(|(k, _)| k.clone()).collect();
        for id in cancelled {
            st.pending.remove(&id);
        }
        let ids: Vec<(SystemId, Phase)> = st.sessions.iter().map(|(k, s)| (k.clone(), s.phase)).collect();
        for (id, phase) in ids {
            match phase {
                Phase::Fault => self.dismiss(&mut st, &id),
                Phase::Stopping => {}
                _ => self.begin_stop(&mut st, &id),
            }
        }
        st.dirty = true;
        Ok(())
    }

    fn restart(&self, id: SystemId) -> Result<()> {
        let cfg = self.cfg();
        let view = self.published_system(&id);
        let live = self.published_live();
        let moved = self.restart_locked(&cfg, id, view, &live)?;
        self.unfocus_moved(moved);
        Ok(())
    }

    fn restart_locked(&self, cfg: &klif_common::config::Config, id: SystemId, view: Option<System>, live: &BTreeSet<SystemId>) -> Result<Option<Option<SystemId>>> {
        let mut st = lock(&self.st);
        let label = st.label_of(cfg, &id);
        if cfg.system(id.as_str()).is_none() {
            if st.sessions.contains_key(&id) {
                bail!("{label} is not in klif.toml any more; it can only be stopped.");
            }
            bail!("There is no System \"{id}\".");
        }
        if cfg.system_preset(id.as_str()).is_some_and(|(_, p)| p.is_external()) {
            bail!("{label} is an external server; restart it where it runs.");
        }
        if st.pending.contains_key(&id) {
            bail!("{label} is about to launch.");
        }
        let Some(phase) = st.sessions.get(&id).map(|c| c.phase) else { bail!("{label} is not running.") };
        if cfg.system_preset(id.as_str()).is_none() {
            bail!("{label} has no preset to restart with.");
        }
        let cand = match &view {
            Some(s) => candidate(&st, s),
            None => Candidate { id: id.clone(), host: String::new(), port: None, gpus: Vec::new(), exclusive: false, need: None },
        };
        // The relaunch would be refused for these (the same port / exclusive check `fire` makes; VRAM is left to
        // Launch, as the System's own memory would count as foreign here): refuse BEFORE stopping anything, so the
        // running session survives and the caller gets the sentence.
        {
            let members = holders(&st, cfg);
            let v = conflicts::compute(&Candidate { need: None, ..cand.clone() }, &members, &st.gpu_facts, 0.0);
            if !v.ids.is_empty() {
                let labels: Vec<String> = v.ids.iter().map(|c| member_label(&members, c)).collect();
                let pending: Vec<String> =
                    v.ids.iter().filter(|c| members.iter().any(|m| &m.id == *c && m.pending)).map(|c| member_label(&members, c)).collect();
                if !pending.is_empty() {
                    bail!("{label} cannot restart: {} {} about to launch.", names(&pending), is_are(pending.len()));
                }
                let n = names(&labels);
                bail!(
                    "{label} cannot restart while {n} {} running ({}). Stop {n} first.",
                    is_are(labels.len()),
                    why_texts(&v, &v.ids).join(", ")
                );
            }
        }
        match phase {
            Phase::Fault => self.dismiss(&mut st, &id),
            Phase::Stopping => {}
            _ => self.begin_stop(&mut st, &id),
        }
        let moved = Self::follow_selection(&mut st, &id, &[], live);
        st.pending.insert(
            id.clone(),
            PendingLaunch {
                waiting_for: vec![id.clone()],
                at: now_s(),
                spawning: false,
                label,
                host: cand.host,
                port: cand.port,
                gpus: cand.gpus,
                exclusive: cand.exclusive,
                expected_gib: view.as_ref().and_then(expected_total),
                // A restart only waits for itself.
                auto: false,
            },
        );
        st.dirty = true;
        Ok(moved)
    }

    fn dismiss_action(&self, id: SystemId) -> Result<()> {
        let cfg = self.cfg();
        let mut st = lock(&self.st);
        let label = st.label_of(&cfg, &id);
        match st.sessions.get(&id).map(|c| c.phase) {
            None => Ok(()),
            Some(Phase::Fault) => {
                self.dismiss(&mut st, &id);
                Ok(())
            }
            Some(_) => bail!("{label} is running; stop it first."),
        }
    }

    // ---- klif.toml writes ----------------------------------------------------------------------

    /// Write klif.toml through the store (creating the file first when needed), then reload at once (the reload
    /// records the new mtime + size, so the tick does not reload again).
    fn write(&self, f: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
        let cfg = self.cfg();
        let path = cfg.file_path();
        if !path.is_file() {
            store::ensure_file(&cfg)?;
        }
        let r = f(&path);
        self.reload_config(true);
        r
    }

    fn local_system(&self, cfg: &klif_common::config::Config, id: &SystemId) -> Result<SystemCfg> {
        if let Some(s) = cfg.system(id.as_str()) {
            return Ok(s.clone());
        }
        if let Some(e) = cfg.bad_systems.get(id.as_str()) {
            bail!("System \"{id}\" cannot be read from klif.toml: {}.", trim_dot(e));
        }
        if lock(&self.st).sessions.contains_key(id) {
            bail!("{id} is not in klif.toml any more; it can only be stopped.");
        }
        bail!("There is no System \"{id}\".")
    }

    /// A preset that exists and fits a System kind (checked before any write).
    fn check_preset_for(&self, cfg: &klif_common::config::Config, preset: &str, kind: SystemKind, label: &str) -> Result<()> {
        if let Some(e) = cfg.bad_presets.get(preset) {
            bail!("Preset \"{preset}\" cannot be read: {}.", trim_dot(e));
        }
        let spec = cfg.presets.get(preset).ok_or_else(|| anyhow!("There is no preset \"{preset}\"."))?;
        check_kind(preset, spec, kind, label)
    }

    fn use_preset(&self, system: &SystemId, preset: &str) -> Result<()> {
        let cfg = self.cfg();
        let sys = self.local_system(&cfg, system)?;
        let label = cfg.system_label(system.as_str()).unwrap_or_else(|| system.to_string());
        self.check_preset_for(&cfg, preset, sys.kind, &label)?;
        self.write(|path| store::set_system_preset(path, system, Some(preset)))
    }

    fn set_param(&self, system: &SystemId, name: &str, value: &str) -> Result<()> {
        let cfg = self.cfg();
        self.local_system(&cfg, system)?;
        let label = cfg.system_label(system.as_str()).unwrap_or_else(|| system.to_string());
        let (pid, spec) = cfg.system_preset(system.as_str()).ok_or_else(|| anyhow!("{label} has no preset."))?;
        let param = spec.params.get(name).ok_or_else(|| anyhow!("Preset \"{pid}\" has no param \"{name}\"."))?;
        if !param.choices.contains_key(value) {
            let choices: Vec<&str> = param.choices.keys().map(String::as_str).collect();
            bail!("\"{value}\" is not a choice of {name} (choices: {}).", choices.join(", "));
        }
        self.write(|path| store::set_system_param(path, system, name, value))
    }

    fn save_preset(&self, id: &str, preset: &PresetCfg, select_for: Option<&SystemId>, secrets_from: Option<&str>, base_hash: Option<&str>) -> Result<()> {
        validate_preset_id(id).map_err(|e| anyhow!(e))?;
        let cfg = self.cfg();
        if let Some(s) = select_for {
            let sys = self.local_system(&cfg, s)?;
            let label = cfg.system_label(s.as_str()).unwrap_or_else(|| s.to_string());
            check_kind(id, preset, sys.kind, &label)?;
        }
        // Changing the kind of a preset other Systems use would make them invalid (SPEC 5: kind mismatches are
        // refused, like DeletePreset of a preset in use). Only a change that breaks a System that matched before:
        // a preset already mismatched, unreadable or without a kind stays editable.
        let new_kind = preset.effective_kind();
        if let Some(old) = cfg.presets.get(id).and_then(PresetCfg::effective_kind).filter(|old| new_kind != Some(*old)) {
            let broken: Vec<String> = cfg
                .systems_using(id)
                .iter()
                .filter(|s| Some(*s) != select_for)
                .filter(|s| cfg.system(s.as_str()).is_some_and(|sys| sys.kind == old))
                .map(|s| cfg.system_label(s.as_str()).unwrap_or_else(|| s.to_string()))
                .collect();
            if !broken.is_empty() {
                let (n, verb) = (names(&broken), is_are(broken.len()));
                match new_kind {
                    Some(k) => bail!(
                        "Preset \"{id}\" would become {} preset, but {n} {verb} using it as {} System; pick another preset there first.",
                        with_article(k),
                        with_article(old)
                    ),
                    None => bail!(
                        "Preset \"{id}\" would no longer say its kind (a generic preset needs kind = \"{}\"), but {n} {verb} using it.",
                        old.as_str()
                    ),
                }
            }
        }
        if let Some(src) = secrets_from {
            validate_preset_id(src).map_err(|e| anyhow!(e))?;
        }
        self.write(|path| {
            store::upsert_preset(path, id, preset, secrets_from, base_hash)?;
            if let Some(s) = select_for {
                store::set_system_preset(path, s, Some(id))?;
            }
            Ok(())
        })
    }

    fn delete_preset(&self, id: &str) -> Result<()> {
        let cfg = self.cfg();
        if !cfg.presets.contains_key(id) && !cfg.bad_presets.contains_key(id) {
            bail!("There is no preset \"{id}\".");
        }
        let users: Vec<String> = cfg.systems_using(id).iter().map(|s| cfg.system_label(s.as_str()).unwrap_or_else(|| s.to_string())).collect();
        if !users.is_empty() {
            bail!("Preset \"{id}\" is used by {}; pick another preset there first.", names(&users));
        }
        {
            let st = lock(&self.st);
            if let Some((sid, _)) = st.sessions.iter().find(|(_, s)| s.p.preset.as_deref() == Some(id)) {
                bail!("Preset \"{id}\" is running on {}; stop it first.", st.label_of(&cfg, sid));
            }
        }
        self.write(|path| store::delete_preset(path, id))
    }

    fn add_system(&self, id: Option<&str>, label: Option<&str>, kind: SystemKind, class: Option<LlmClass>, preset: Option<&str>) -> Result<()> {
        let cfg = self.cfg();
        let id = id.map(str::trim).filter(|s| !s.is_empty());
        if let Some(i) = id {
            validate_system_id(i).map_err(|e| anyhow!(e))?;
            if cfg.system(i).is_some() || cfg.bad_systems.contains_key(i) {
                bail!("A System with the id \"{i}\" exists already.");
            }
        }
        let class = if kind == SystemKind::Llm { class } else { None };
        let preset = preset.map(str::trim).filter(|s| !s.is_empty());
        let taken: Vec<String> = cfg.system_labels().into_iter().map(|(_, l)| l).collect();
        let label = label.map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).unwrap_or_else(|| default_system_label(kind, class, &taken));
        if let Some(p) = preset {
            self.check_preset_for(&cfg, p, kind, &label)?;
        }
        let sys = SystemCfg { label: Some(label), kind, class, preset: preset.map(str::to_string), params: Default::default(), exclusive: false };
        let mut added: Option<SystemId> = None;
        self.write(|path| {
            added = Some(store::add_system(path, id, &sys)?);
            Ok(())
        })?;
        if let Some(a) = added {
            log::info!("added System {a}");
        }
        Ok(())
    }

    fn remove_system(&self, system: &SystemId) -> Result<()> {
        let cfg = self.cfg();
        if cfg.system(system.as_str()).is_none() && !cfg.bad_systems.contains_key(system.as_str()) {
            bail!("There is no System \"{system}\" in klif.toml.");
        }
        {
            let st = lock(&self.st);
            if st.sessions.contains_key(system) || st.pending.contains_key(system) {
                bail!("{} is running; stop it before removing it.", st.label_of(&cfg, system));
            }
        }
        self.write(|path| store::remove_system(path, system))?;
        let mut st = lock(&self.st);
        st.last.remove(system);
        st.idle_tails.remove(system);
        st.dirty = true;
        Ok(())
    }

    fn update_system(&self, system: &SystemId, label: Option<&str>, move_to: Option<u32>, exclusive: Option<bool>) -> Result<()> {
        let cfg = self.cfg();
        self.local_system(&cfg, system)?;
        let label = match label {
            Some(l) if l.trim().is_empty() => bail!("A System needs a label."),
            Some(l) => Some(l.trim()),
            None => None,
        };
        self.write(|path| store::update_system(path, system, label, move_to, exclusive))
    }

    // ---- recommendations / downloads -----------------------------------------------------------

    fn download(&self, id: &str) -> Result<()> {
        let cfg = self.cfg();
        let catalog = self.catalog();
        let rec = catalog.recommendation(id).cloned().ok_or_else(|| anyhow!("There is no recommendation \"{id}\"."))?;
        if cfg.models_dir().is_none() {
            bail!("Set [paths] models_dir in klif.toml first; KLIF downloads models only there.");
        }
        {
            let d = lock(&self.downloads);
            let running = d.infos.values().any(|(i, _)| i.id == id && matches!(i.state, DownloadState::Running | DownloadState::Verifying));
            if running {
                bail!("\"{id}\" is downloading already.");
            }
        }
        let dl = self.downloads.clone();
        let progress = Box::new(move |info: DownloadInfo| {
            let mut d = lock(&dl);
            if info.state == DownloadState::Done {
                d.rebuild = true;
            }
            d.infos.insert((info.id.clone(), info.file.clone()), (info, now_s()));
        });
        let h = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::download::start(&cfg, &rec, progress)))
            .map_err(|_| anyhow!("Downloads are not available in this build."))?;
        log::info!("download of {id} started");
        lock(&self.downloads).handles.insert(id.to_string(), h);
        Ok(())
    }

    fn cancel_download(&self, id: &str) -> Result<()> {
        let d = lock(&self.downloads);
        match d.handles.get(id) {
            Some(h) => {
                h.cancel();
                Ok(())
            }
            None => bail!("\"{id}\" is not downloading."),
        }
    }

    fn adopt_recommendation(&self, id: &str, system: Option<&SystemId>) -> Result<()> {
        let cfg = self.cfg();
        let catalog = self.catalog();
        let (pid, spec) = catalog.preset_from_recommendation(&cfg, id, system)?;
        validate_preset_id(&pid).map_err(|e| anyhow!(e))?;
        if let Some(s) = system {
            let sys = self.local_system(&cfg, s)?;
            let label = cfg.system_label(s.as_str()).unwrap_or_else(|| s.to_string());
            check_kind(&pid, &spec, sys.kind, &label)?;
        }
        self.write(|path| {
            store::upsert_preset(path, &pid, &spec, None, None)?;
            if let Some(s) = system {
                store::set_system_preset(path, s, Some(&pid))?;
            }
            Ok(())
        })
    }
}

/// A preset's kind must be the System's (generic presets must say their kind).
fn check_kind(preset: &str, spec: &PresetCfg, kind: SystemKind, label: &str) -> Result<()> {
    match spec.effective_kind() {
        Some(k) if k == kind => Ok(()),
        // "an LLM / an Image System", "a Speech (TTS) / a Transcription (STT) / a Video System".
        Some(k) => bail!("Preset \"{preset}\" is for {} Systems; {label} is {} System.", k.label(), with_article(kind)),
        None => bail!("Preset \"{preset}\" does not say its kind (a generic preset needs kind = \"{}\").", kind.as_str()),
    }
}
