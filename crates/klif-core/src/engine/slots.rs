//! Conversation slots on disk (`[systems.<id>] slots`). A System with slots on runs its server with
//! `--slot-save-path <data dir>/slots/<id>` (llama.cpp and Strata take the same flag and the same
//! `POST /slots/{id}?action=save|restore {"filename"}`). KLIF keeps three slots per System in that folder:
//! `slot<N>-<server slot>.bin` files plus `slots.json`, which records what each slot holds and the command hash it
//! was saved with. A slot loads only into the same command (model, context, KV type).
//!
//! Saving and loading are HTTP calls that can take a while (gigabytes of KV), so they run on their own threads; the
//! registry here tells the view model what runs and how the last one ended. Auto mode saves on the stop thread
//! before the process is stopped, and loads once a session KLIF launched is live.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use klif_common::config::SLOT_COUNT;
use klif_common::now_s;
use klif_common::vm::{CommandView, SlotEntryView, SlotJobView, SlotNotice, SlotOp, SlotsView, SystemId};
use klif_common::Secret;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::session::display_host;
use crate::state::PersistedSession;

const INDEX: &str = "slots.json";
/// How long the outcome of a save or load stays in the view model.
const NOTICE_S: f64 = 30.0;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Index {
    /// The slot Auto uses: the one saved or loaded last.
    active: u8,
    /// "1".."3".
    entries: BTreeMap<String, Entry>,
    /// The names the user gave the slots ("1".."3"); kept when a slot is cleared.
    names: BTreeMap<String, String>,
}

/// The longest slot name.
const NAME_MAX: usize = 40;

/// How a slot is named in a sentence: `slot 2 ("OMP")`, or `slot 2`.
fn slot_words(idx: &Index, n: u8) -> String {
    match idx.names.get(&n.to_string()) {
        Some(name) => format!("slot {n} (\"{name}\")"),
        None => format!("slot {n}"),
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Entry {
    saved_at: f64,
    /// The command hash of the session it was saved from.
    hash: String,
    tokens: u64,
    bytes: u64,
    model: Option<String>,
    preset: Option<String>,
    files: Vec<FileRec>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct FileRec {
    /// The server's slot id.
    id: u64,
    file: String,
    tokens: u64,
}

fn read_index(dir: &Path) -> Index {
    let mut idx: Index = std::fs::read(dir.join(INDEX)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if !(1..=SLOT_COUNT).contains(&idx.active) {
        idx.active = 1;
    }
    idx
}

fn write_index(dir: &Path, idx: &Index) -> Result<()> {
    let tmp = dir.join(format!("{INDEX}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(idx)?).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, dir.join(INDEX)).with_context(|| format!("writing {}", dir.join(INDEX).display()))
}

fn check_slot(n: u8) -> Result<()> {
    if !(1..=SLOT_COUNT).contains(&n) {
        bail!("There is no slot {n}; a System has slots 1 to {SLOT_COUNT}.");
    }
    Ok(())
}

/// The folder a session's server saves slots in: its `--slot-save-path` (None: it was started without one).
pub(crate) fn session_dir(command: Option<&CommandView>) -> Option<PathBuf> {
    let args = &command?.args;
    args.iter().enumerate().find_map(|(i, a)| {
        if a == "--slot-save-path" {
            args.get(i + 1).map(PathBuf::from)
        } else {
            a.strip_prefix("--slot-save-path=").map(PathBuf::from)
        }
    })
}

/// Slot `n` in `dir` was saved with `hash`.
pub(crate) fn loadable(dir: &Path, n: u8, hash: &str) -> bool {
    read_index(dir).entries.get(&n.to_string()).is_some_and(|e| !e.hash.is_empty() && e.hash == hash)
}

/// A running session's server, for a save or load.
pub(crate) struct Target {
    pub(crate) label: String,
    base: String,
    key: Option<Secret>,
    pub(crate) dir: PathBuf,
    hash: String,
    model: Option<String>,
    preset: Option<String>,
}

impl Target {
    /// None when the session was not started with `--slot-save-path`.
    pub(crate) fn of(label: &str, p: &PersistedSession, key: Option<Secret>) -> Option<Target> {
        let dir = session_dir(p.command.as_ref())?;
        let host = match display_host(&p.host) {
            h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
            h => h.to_string(),
        };
        Some(Target {
            label: label.to_string(),
            base: format!("http://{host}:{}", p.record.port),
            key,
            dir,
            hash: p.command.as_ref().map(|c| c.hash.clone()).unwrap_or_default(),
            model: Some(p.model.name.clone()).filter(|m| !m.trim().is_empty()),
            preset: p.preset.clone(),
        })
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .proxy(None)
        .timeout_connect(Some(Duration::from_secs(5)))
        // A server answers a save only when its slot is free (llama.cpp defers it, Strata queues it), and a large
        // conversation takes a while to write.
        .timeout_recv_response(Some(Duration::from_secs(30 * 60)))
        .timeout_recv_body(Some(Duration::from_secs(60)))
        .user_agent(format!("KLIF/{} slots", klif_common::KLIF_VERSION))
        .build()
        .into()
}

fn call(t: &Target, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let agent = agent();
    let url = format!("{}{path}", t.base);
    let resp = match body {
        Some(b) => {
            let mut req = agent.post(&url);
            if let Some(k) = &t.key {
                req = req.header("Authorization", format!("Bearer {}", k.expose()));
            }
            req.send_json(b)
        }
        None => {
            let mut req = agent.get(&url);
            if let Some(k) = &t.key {
                req = req.header("Authorization", format!("Bearer {}", k.expose()));
            }
            req.call()
        }
    }
    .map_err(|e| anyhow!("{}'s server did not answer {method} {path}: {e}", t.label))?;
    let status = resp.status().as_u16();
    let v: Value = resp.into_body().with_config().limit(4 * 1024 * 1024).read_json().unwrap_or(Value::Null);
    if status != 200 {
        let msg = v
            .pointer("/error/message")
            .or_else(|| v.get("error"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("HTTP {status}"));
        bail!("{}'s server refused: {}", t.label, msg.trim_end_matches('.'));
    }
    Ok(v)
}

fn u(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
}

/// Save what the server holds to slot `n`. Returns the sentence for the user.
pub(crate) fn save(t: &Target, n: u8) -> Result<String> {
    check_slot(n)?;
    std::fs::create_dir_all(&t.dir).with_context(|| format!("creating {}", t.dir.display()))?;
    let slots = call(t, "GET", "/slots", None)?;
    let list: Vec<(u64, u64, bool)> = slots
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let id = s.get("id").and_then(Value::as_u64)?;
                    Some((id, u(s, "n_prompt_tokens"), s.get("is_processing").and_then(Value::as_bool).unwrap_or(false)))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut held: Vec<u64> = list.iter().filter(|(_, tokens, _)| *tokens > 0).map(|(id, ..)| *id).collect();
    if held.is_empty() {
        // The size is not always reported: Strata reads 0 right after a save or restore (it clears its last prompt
        // size) while it still holds the conversation. Ask every idle slot; the server's n_saved says what it held.
        held = list.iter().filter(|(_, _, busy)| !busy).map(|(id, ..)| *id).collect();
    }
    let mut files = Vec::new();
    for id in held {
        // Written under a temporary name first: a slot that turns out empty must not replace the last good save.
        let file = format!("slot{n}-{id}.bin");
        let tmp = format!("slot{n}-{id}.new.bin");
        let r = call(t, "POST", &format!("/slots/{id}?action=save"), Some(&json!({ "filename": tmp })))?;
        let tokens = u(&r, "n_saved");
        if tokens == 0 {
            let _ = std::fs::remove_file(t.dir.join(&tmp));
            continue;
        }
        std::fs::rename(t.dir.join(&tmp), t.dir.join(&file)).with_context(|| format!("replacing {}", t.dir.join(&file).display()))?;
        files.push((FileRec { id, file, tokens }, u(&r, "n_written")));
    }
    if files.is_empty() {
        bail!("{} holds no conversation to save.", t.label);
    }
    let mut idx = read_index(&t.dir);
    // Files of the slot's previous save that this one did not overwrite (another server slot id).
    if let Some(old) = idx.entries.get(&n.to_string()) {
        for f in &old.files {
            if !files.iter().any(|(nf, _)| nf.file == f.file) {
                let _ = std::fs::remove_file(t.dir.join(&f.file));
            }
        }
    }
    let tokens = files.iter().map(|(f, _)| f.tokens).sum();
    let bytes = files
        .iter()
        .map(|(f, w)| if *w > 0 { *w } else { std::fs::metadata(t.dir.join(&f.file)).map(|m| m.len()).unwrap_or(0) })
        .sum();
    idx.entries.insert(
        n.to_string(),
        Entry {
            saved_at: now_s(),
            hash: t.hash.clone(),
            tokens,
            bytes,
            model: t.model.clone(),
            preset: t.preset.clone(),
            files: files.into_iter().map(|(f, _)| f).collect(),
        },
    );
    idx.active = n;
    write_index(&t.dir, &idx)?;
    Ok(format!("Saved {} to {}: {tokens} tokens, {}.", t.label, slot_words(&idx, n), gib(bytes)))
}

/// Load slot `n` into the server. Returns the sentence for the user.
pub(crate) fn load(t: &Target, n: u8) -> Result<String> {
    check_slot(n)?;
    let mut idx = read_index(&t.dir);
    let e = idx.entries.get(&n.to_string()).cloned().ok_or_else(|| anyhow!("Slot {n} of {} is empty.", t.label))?;
    if e.hash != t.hash {
        bail!(
            "Slot {n} was saved with another command of {} (model, context or KV type); it loads only into that command.",
            t.label
        );
    }
    let mut tokens = 0;
    for f in &e.files {
        if !t.dir.join(&f.file).is_file() {
            bail!("Slot {n}'s file {} is missing from {}.", f.file, t.dir.display());
        }
        let r = call(t, "POST", &format!("/slots/{}?action=restore", f.id), Some(&json!({ "filename": f.file })))?;
        tokens += u(&r, "n_restored");
    }
    idx.active = n;
    write_index(&t.dir, &idx)?;
    Ok(format!("Loaded {} into {}: {tokens} tokens.", slot_words(&idx, n), t.label))
}

/// Delete slot `n`'s files.
pub(crate) fn clear(dir: &Path, label: &str, n: u8) -> Result<String> {
    check_slot(n)?;
    let mut idx = read_index(dir);
    let Some(e) = idx.entries.remove(&n.to_string()) else { return Ok(format!("Slot {n} of {label} was already empty.")) };
    for f in &e.files {
        let p = dir.join(&f.file);
        if p.exists() {
            std::fs::remove_file(&p).with_context(|| format!("deleting {}", p.display()))?;
        }
    }
    write_index(dir, &idx)?;
    Ok(format!("Cleared slot {n} of {label}."))
}

/// Make slot `n` the active one (auto-save and auto-load use it).
pub(crate) fn set_active(dir: &Path, label: &str, n: u8) -> Result<String> {
    check_slot(n)?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut idx = read_index(dir);
    idx.active = n;
    write_index(dir, &idx)?;
    Ok(format!("{label}'s active slot is now {}: auto-save writes to it and auto-load reads it.", slot_words(&idx, n)))
}

/// Name slot `n` ("" = back to "Slot N").
pub(crate) fn rename(dir: &Path, label: &str, n: u8, name: &str) -> Result<String> {
    check_slot(n)?;
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().count() > NAME_MAX {
        bail!("A slot name has at most {NAME_MAX} characters.");
    }
    if name.chars().any(char::is_control) {
        bail!("A slot name cannot hold control characters.");
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut idx = read_index(dir);
    if name.is_empty() {
        idx.names.remove(&n.to_string());
    } else {
        idx.names.insert(n.to_string(), name.clone());
    }
    write_index(dir, &idx)?;
    Ok(if name.is_empty() { format!("Slot {n} of {label} has no name now.") } else { format!("Slot {n} of {label} is \"{name}\" now.") })
}

/// What runs now per System, and how the last save or load ended.
#[derive(Default)]
pub(crate) struct SlotJobs {
    running: BTreeMap<SystemId, SlotJobView>,
    notices: BTreeMap<SystemId, SlotNotice>,
    /// `slots.json` of every slot folder in use, read by the tick outside `st` (compose reads no files).
    indexes: BTreeMap<PathBuf, Index>,
    /// A slot to load once the System's next launch is up (`Launch { load_slot }`), whatever auto-load says.
    armed: BTreeMap<SystemId, u8>,
}

pub(crate) type Jobs = Arc<Mutex<SlotJobs>>;

impl SlotJobs {
    pub(crate) fn busy(&self, id: &SystemId) -> bool {
        self.running.contains_key(id)
    }

    pub(crate) fn note(&mut self, id: &SystemId, ok: bool, text: String) {
        self.notices.insert(id.clone(), SlotNotice { ok, text, at: now_s() });
    }
}

/// Mark System `id`'s slot job as running (refused while another one runs).
fn begin(jobs: &Jobs, id: &SystemId, op: SlotOp, n: u8) -> Result<()> {
    let mut j = jobs.lock().unwrap_or_else(|e| e.into_inner());
    if j.busy(id) {
        bail!("A slot of this System is being saved or loaded already; wait until it is done.");
    }
    j.running.insert(id.clone(), SlotJobView { op, slot: n, since: now_s() });
    Ok(())
}

/// The job ended: its outcome becomes the notice.
fn finish(jobs: &Jobs, id: &SystemId, out: &Result<String>) {
    let mut j = jobs.lock().unwrap_or_else(|e| e.into_inner());
    j.running.remove(id);
    match out {
        Ok(text) => j.note(id, true, text.clone()),
        Err(e) => j.note(id, false, format!("{e:#}")),
    }
}

/// Run `f` as System `id`'s slot job on this thread: the view model shows it while it runs and its outcome after.
pub(crate) fn run_job(jobs: &Jobs, id: &SystemId, op: SlotOp, n: u8, f: impl FnOnce() -> Result<String>) -> Result<String> {
    begin(jobs, id, op, n)?;
    let out = f();
    finish(jobs, id, &out);
    out
}

/// `run_job` on a thread of its own. The job is registered before this returns, so the next view model shows it.
pub(crate) fn spawn_job(jobs: Jobs, id: SystemId, op: SlotOp, n: u8, f: impl FnOnce() -> Result<String> + Send + 'static) -> Result<()> {
    begin(&jobs, &id, op, n)?;
    let what = if op == SlotOp::Save { "save" } else { "load" };
    let (j2, id2) = (jobs.clone(), id.clone());
    std::thread::Builder::new()
        .name(format!("klif-slot-{what}"))
        .spawn(move || {
            let out = f();
            if let Err(e) = &out {
                log::warn!("slot {what} of {id2} failed: {e:#}");
            }
            finish(&j2, &id2, &out);
        })
        .map(|_| ())
        .map_err(|e| {
            finish(&jobs, &id, &Err(anyhow!("could not start the slot thread: {e}")));
            anyhow!("could not start the slot thread: {e}")
        })
}

/// The view of a System's slots. `dir`: its slot folder; `hash`: the command it runs or would run now (None: no
/// command, nothing is loadable).
pub(crate) fn view(auto_save: bool, auto_load: bool, supported: bool, dir: &Path, hash: Option<&str>, jobs: &Jobs, id: &SystemId) -> SlotsView {
    let mut j = jobs.lock().unwrap_or_else(|e| e.into_inner());
    let mut idx = j.indexes.get(dir).cloned().unwrap_or_default();
    if !(1..=SLOT_COUNT).contains(&idx.active) {
        idx.active = 1;
    }
    let entries = (1..=SLOT_COUNT)
        .map(|n| match idx.entries.get(&n.to_string()) {
            Some(e) => SlotEntryView {
                n,
                name: idx.names.get(&n.to_string()).cloned(),
                saved_at: Some(e.saved_at),
                tokens: Some(e.tokens),
                bytes: Some(e.bytes),
                model: e.model.clone(),
                compatible: hash.is_some_and(|h| !e.hash.is_empty() && h == e.hash),
            },
            None => SlotEntryView { n, name: idx.names.get(&n.to_string()).cloned(), ..Default::default() },
        })
        .collect();
    let now = now_s();
    j.notices.retain(|_, n| now - n.at < NOTICE_S);
    SlotsView { auto_save, auto_load, active: idx.active, supported, entries, job: j.running.get(id).cloned(), notice: j.notices.get(id).cloned() }
}

// ------------------------------------------------------------------------------------------ engine

use klif_common::config::Config;

use klif_common::vm::Phase;

use super::session::SessionCtx;
use super::Inner;
use crate::keys;

impl Inner {
    /// A running session's slot target (None: its adapter cannot save slots, or it runs without `--slot-save-path`).
    fn slot_target(&self, cfg: &Config, id: &SystemId, p: &PersistedSession) -> Option<Target> {
        let adapter = p.adapter();
        if !adapter.can_save_slots() {
            return None;
        }
        let key = if p.api_key_set && adapter.api_key_env().is_some() { keys::load(cfg) } else { None };
        let label = cfg.system_label(id.as_str()).unwrap_or_else(|| id.to_string());
        Target::of(&label, p, key)
    }

    /// Auto-save: the save the stop thread runs before it stops a live session (None: nothing to save).
    pub(super) fn auto_slot_save(&self, id: &SystemId, ctx: &SessionCtx) -> Option<Box<dyn FnOnce() + Send>> {
        let cfg = self.cfg();
        if !cfg.system(id.as_str()).is_some_and(|s| s.slots && s.slots_auto_save) {
            return None;
        }
        let t = self.slot_target(&cfg, id, &ctx.p)?;
        // Under `st`: the folder's record as the tick last read it, no file IO here.
        let n = self.cached_index(&t.dir).active;
        let jobs = self.slot_jobs.clone();
        let id = id.clone();
        Some(Box::new(move || match run_job(&jobs, &id, SlotOp::Save, n, || save(&t, n)) {
            Ok(text) => log::info!("{text}"),
            Err(e) => log::warn!("auto save of {id}'s slot {n} failed: {e:#}"),
        }))
    }

    /// Auto-load: a session KLIF launched has come up; load its active slot when it was saved with this command. A
    /// launch that asked for a slot (`Launch { load_slot }`) loads that one, also with auto-load off.
    pub(super) fn auto_slot_load(&self, cfg: &Config, id: &SystemId, ctx: &mut SessionCtx) {
        let armed = self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).armed.remove(id);
        if !cfg.system(id.as_str()).is_some_and(|s| s.slots && (s.slots_auto_load || armed.is_some())) {
            return;
        }
        let Some(t) = self.slot_target(cfg, id, &ctx.p) else { return };
        // Under `st`: the folder's record as the tick last read it, no file IO here.
        let idx = self.cached_index(&t.dir);
        let n = armed.unwrap_or(idx.active);
        if !idx.entries.get(&n.to_string()).is_some_and(|e| !e.hash.is_empty() && e.hash == t.hash) {
            return;
        }
        ctx.tail.push(format!("[KLIF] loading conversation slot {n}"));
        if let Err(e) = spawn_job(self.slot_jobs.clone(), id.clone(), SlotOp::Load, n, move || load(&t, n)) {
            log::warn!("auto load of {id}'s slot {n}: {e:#}");
        }
    }

    /// Read `slots.json` of every System with slots on (on the tick, outside `st`), for `slots_view`.
    pub(super) fn refresh_slot_indexes(&self, cfg: &Config) {
        let ids: Vec<&SystemId> = cfg.systems.iter().filter(|(_, s)| s.slots).map(|(id, _)| id).collect();
        if ids.is_empty() {
            self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).indexes.clear();
            return;
        }
        let dirs: Vec<PathBuf> = {
            let st = self.st.lock().unwrap_or_else(|e| e.into_inner());
            ids.iter()
                .map(|id| {
                    st.sessions
                        .get(*id)
                        .and_then(|c| session_dir(c.p.command.as_ref()))
                        .unwrap_or_else(|| klif_catalog::slots_dir(cfg, id.as_str()))
                })
                .collect()
        };
        let indexes: BTreeMap<PathBuf, Index> = dirs.into_iter().map(|d| (d.clone(), read_index(&d))).collect();
        self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).indexes = indexes;
    }

    /// A slot folder's record as the tick last read it (empty when it was not read yet).
    fn cached_index(&self, dir: &Path) -> Index {
        let mut idx = self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).indexes.get(dir).cloned().unwrap_or_default();
        if !(1..=SLOT_COUNT).contains(&idx.active) {
            idx.active = 1;
        }
        idx
    }

    /// A save or load of System `id`'s slot runs now.
    pub(super) fn slot_saving(&self, id: &SystemId) -> bool {
        self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).busy(id)
    }

    /// The SaveSlot / LoadSlot actions: the System must be running with slots.
    pub(super) fn slot_action(&self, id: &SystemId, op: SlotOp, n: u8) -> Result<()> {
        check_slot(n)?;
        let cfg = self.cfg();
        let label = cfg.system_label(id.as_str()).unwrap_or_else(|| id.to_string());
        let t = {
            let st = self.st.lock().unwrap_or_else(|e| e.into_inner());
            let ctx = st.sessions.get(id).filter(|c| c.phase == Phase::Live).ok_or_else(|| {
                anyhow!("{label} is not running; start it to {} a slot.", if op == SlotOp::Save { "save" } else { "load" })
            })?;
            if !ctx.p.adapter().can_save_slots() {
                bail!("{label}'s server ({}) cannot save conversation slots; llama.cpp and Strata can.", ctx.p.adapter().as_str());
            }
            self.slot_target(&cfg, id, &ctx.p)
                .ok_or_else(|| anyhow!("{label} was started without slots: turn slots on in Options (or `klif-cli systems slots <system> on`) and restart it."))?
        };
        match op {
            SlotOp::Save => spawn_job(self.slot_jobs.clone(), id.clone(), op, n, move || save(&t, n)),
            SlotOp::Load => {
                if !loadable(&t.dir, n, &t.hash) {
                    // The same sentences the load itself would give, before a thread is started.
                    load(&t, n)?;
                }
                spawn_job(self.slot_jobs.clone(), id.clone(), op, n, move || load(&t, n))
            }
        }
    }

    /// A System's slot folder now: its running server's `--slot-save-path`, else KLIF's folder for it.
    fn slot_dir_of(&self, cfg: &Config, id: &SystemId) -> PathBuf {
        {
            let st = self.st.lock().unwrap_or_else(|e| e.into_inner());
            st.sessions.get(id).and_then(|c| session_dir(c.p.command.as_ref()))
        }
        .unwrap_or_else(|| klif_catalog::slots_dir(cfg, id.as_str()))
    }

    /// The ClearSlot action.
    pub(super) fn clear_slot(&self, id: &SystemId, n: u8) -> Result<()> {
        let cfg = self.cfg();
        let label = cfg.system_label(id.as_str()).unwrap_or_else(|| id.to_string());
        if self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).busy(id) {
            bail!("A slot of {label} is being saved or loaded; wait until it is done.");
        }
        let text = clear(&self.slot_dir_of(&cfg, id), &label, n)?;
        self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).note(id, true, text);
        Ok(())
    }

    /// The RenameSlot action.
    pub(super) fn rename_slot(&self, id: &SystemId, n: u8, name: &str) -> Result<()> {
        let cfg = self.cfg();
        let label = cfg.system_label(id.as_str()).unwrap_or_else(|| id.to_string());
        if !cfg.system(id.as_str()).is_some_and(|s| s.slots) {
            bail!("{label} has slots off; turn them on first.");
        }
        let dir = self.slot_dir_of(&cfg, id);
        let text = rename(&dir, &label, n, name)?;
        let idx = read_index(&dir);
        let mut j = self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner());
        j.indexes.insert(dir, idx);
        j.note(id, true, text);
        Ok(())
    }

    /// `Launch { load_slot }`: slot `n` becomes the active one and is loaded once the launch is up. Refused unless
    /// it is saved with the command the System launches now.
    pub(super) fn arm_slot_load(&self, id: &SystemId, n: u8) -> Result<()> {
        check_slot(n)?;
        let cfg = self.cfg();
        let label = cfg.system_label(id.as_str()).unwrap_or_else(|| id.to_string());
        let view = self.published_system(id).and_then(|s| s.slots).ok_or_else(|| anyhow!("{label} has slots off; turn them on first."))?;
        if !view.supported {
            bail!("{label}'s server cannot save conversation slots; llama.cpp and Strata can.");
        }
        match view.entries.iter().find(|e| e.n == n) {
            Some(e) if e.saved_at.is_none() => bail!("Slot {n} of {label} is empty."),
            Some(e) if !e.compatible => {
                bail!("Slot {n} was saved with another command of {label} (model, context or KV type); it loads only into that command.")
            }
            Some(_) => {}
            None => bail!("{label} has no slot {n}."),
        }
        self.use_slot(id, n)?;
        self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).armed.insert(id.clone(), n);
        Ok(())
    }

    /// A launch without a slot: forget one an earlier launch asked for.
    pub(super) fn disarm_slot_load(&self, id: &SystemId) {
        self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).armed.remove(id);
    }

    /// The UseSlot action: the slot auto-save and auto-load use.
    pub(super) fn use_slot(&self, id: &SystemId, n: u8) -> Result<()> {
        let cfg = self.cfg();
        let label = cfg.system_label(id.as_str()).unwrap_or_else(|| id.to_string());
        if !cfg.system(id.as_str()).is_some_and(|s| s.slots) {
            bail!("{label} has slots off; turn them on first.");
        }
        if self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner()).busy(id) {
            bail!("A slot of {label} is being saved or loaded; wait until it is done.");
        }
        let dir = self.slot_dir_of(&cfg, id);
        let text = set_active(&dir, &label, n)?;
        let mut j = self.slot_jobs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(idx) = j.indexes.get_mut(&dir) {
            idx.active = n;
        }
        j.note(id, true, text);
        Ok(())
    }

    /// Every local System's slots for the view model (None while they are off).
    pub(super) fn slots_view(&self, cfg: &Config, id: &SystemId, command: Option<&CommandView>, running: Option<&PersistedSession>) -> Option<SlotsView> {
        let sys = cfg.system(id.as_str()).filter(|s| s.slots)?;
        let run_cmd = running.and_then(|p| p.command.as_ref());
        let dir = session_dir(run_cmd).unwrap_or_else(|| klif_catalog::slots_dir(cfg, id.as_str()));
        let cmd = run_cmd.or(command);
        let supported = cmd.map(|c| c.adapter.can_save_slots()).unwrap_or(false);
        let hash = cmd.map(|c| c.hash.as_str()).filter(|h| !h.is_empty());
        Some(view(sys.slots_auto_save, sys.slots_auto_load, supported, &dir, hash, &self.slot_jobs, id))
    }
}
