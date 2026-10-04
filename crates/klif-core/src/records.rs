//! Records (the "high scores", SPEC section 3): per exact model file and per backend, the best values ever reached
//! on this machine (fastest decode and prefill, lowest TTFT, fastest seconds per image and per video job, highest
//! TTS / STT real-time factor) with the conditions they were reached under. Not a certified benchmark: values come
//! from everyday use (the requests, image and video jobs the adapters parse from the server logs) and from
//! `klif-cli bench` (the only source of TTS / STT values: those servers log no per-request timing).
//!
//! Counted: decode with >= 128 generated tokens, prefill with >= 1024 prompt tokens not served from the cache, TTFT
//! for any request (its prompt and cached tokens are kept), an image or a video when its generation finished (a
//! video keeps its size, frames and steps), a TTS / STT bench run. Nothing else is filtered: a record is whatever the
//! machine really did.
//!
//! Key = `"<machine>|<sha256 or file:size>|<backend>"`, machine = `[node] name`, else "This machine" (never the host
//! name). Entries are kept by `"<identity>|<backend>"` and published with the current machine name, so renaming the
//! machine re-keys them (and the history file) instead of starting over. The identity is the SHA-256 of the model
//! file (the first part of a split GGUF; sd.cpp: the diffusion model), hashed once on a background thread at low IO
//! priority and cached in `<data_dir>\hashes.json` by path + size + mtime. Samples that arrive before the hash is
//! known wait in their session's pending list and are applied when it is.
//!
//! Files (data dir): `records.json` (the best values, written atomically), `records-history.jsonl` (one line per
//! broken record: key, metric, old, new, at), `hashes.json`. The last [`MAX_EVENTS`] broken records stay in memory
//! (newest last) for the "new record" moment. `observe` only touches memory; files are written by `flush` (the tick,
//! the hash thread) and only when a record broke or was forgotten.
//!
//! Locks: `store` and `io` are leaves for the engine (`io` -> `store` inside `flush` only); the engine may call
//! `observe` / `register` / `source_for` while it holds its state lock.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{bail, Result};
use klif_common::vm::{ImageJob, RecordEntry, RecordEvent, RecordMetric, RecordModel, RecordSource, RecordValue, RequestRecord, SystemKind};
use klif_common::{now_s, KLIF_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::wire::write_atomic;

pub const RECORDS_FILE: &str = "records.json";
pub const HISTORY_FILE: &str = "records-history.jsonl";
pub const HASHES_FILE: &str = "hashes.json";
/// The machine name when `[node] name` is not set.
pub const THIS_MACHINE: &str = "This machine";
/// Broken records kept in memory for the "new record" moment.
pub const MAX_EVENTS: usize = 8;
/// The most points [`Records::history`] returns for one key.
pub const MAX_HISTORY: usize = 5000;
/// A decode value counts from this many generated tokens.
pub const MIN_DECODE_TOKENS: u64 = 128;
/// A prefill value counts from this many prompt tokens that were not served from the cache.
pub const MIN_PREFILL_TOKENS: u64 = 1024;
/// Remembered file hashes (oldest dropped first).
const MAX_HASHES: usize = 512;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The machine name records are kept under: `[node] name`, else [`THIS_MACHINE`].
pub fn machine_name(cfg: &klif_common::config::Config) -> String {
    cfg.node
        .as_ref()
        .and_then(|n| n.name.as_deref())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| THIS_MACHINE.to_string())
}

// ------------------------------------------------------------------------------------------ samples

/// What a session's samples were measured under (the part of `RecordValue` that is not per sample).
#[derive(Debug, Clone, Default)]
pub struct Conditions {
    pub kind: SystemKind,
    /// Model display name (preset model name, else the file stem).
    pub name: String,
    pub quant: Option<String>,
    /// "HIP", "Vulkan", "CUDA", "CPU", "Metal", "" (unknown).
    pub backend: String,
    /// The context the server was launched with.
    pub ctx: Option<u32>,
    pub kv: Option<String>,
    pub gpus: Vec<String>,
    pub backend_build: Option<String>,
    pub preset: Option<String>,
    /// The machine's FP32 TFLOPS total.
    pub tflops_fp32: Option<f64>,
}

/// One measured value of one metric.
#[derive(Debug, Clone)]
pub struct Sample {
    pub metric: RecordMetric,
    pub value: f64,
    /// Epoch seconds.
    pub at: f64,
    pub source: RecordSource,
    pub prompt_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub gen_tokens: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub steps: Option<u32>,
    /// Video frames (videoS).
    pub frames: Option<u32>,
}

impl Sample {
    pub(crate) fn new(metric: RecordMetric, value: f64, at: f64, source: RecordSource) -> Sample {
        Sample {
            metric,
            value,
            at,
            source,
            prompt_tokens: None,
            cached_tokens: None,
            gen_tokens: None,
            width: None,
            height: None,
            steps: None,
            frames: None,
        }
    }
}

/// The record samples of one finished LLM request (llama.cpp log timings): decode (>= 128 generated tokens),
/// prefill (>= 1024 uncached prompt tokens) and TTFT (= the prompt eval time, any request with prompt tokens).
pub fn llm_samples(r: &RequestRecord, source: RecordSource) -> Vec<Sample> {
    let mut out = Vec::new();
    let at = if r.at > 0.0 { r.at } else { now_s() };
    let with_tokens = |mut s: Sample| {
        s.prompt_tokens = Some(r.prompt_tokens);
        s.cached_tokens = Some(r.cached_tokens);
        s.gen_tokens = Some(r.generated_tokens);
        s
    };
    if r.generated_tokens >= MIN_DECODE_TOKENS && r.decode_s > 0.0 {
        out.push(with_tokens(Sample::new(RecordMetric::DecodeTps, r.generated_tokens as f64 / r.decode_s, at, source)));
    }
    let fresh = r.prompt_tokens.saturating_sub(r.cached_tokens);
    if fresh >= MIN_PREFILL_TOKENS && r.prefill_s > 0.0 {
        out.push(with_tokens(Sample::new(RecordMetric::PrefillTps, fresh as f64 / r.prefill_s, at, source)));
    }
    if r.prompt_tokens > 0 && r.prefill_s > 0.0 {
        out.push(with_tokens(Sample::new(RecordMetric::TtftS, r.prefill_s, at, source)));
    }
    out
}

/// The record sample of one finished image job (seconds per image, with its size and steps).
pub fn image_sample(j: &ImageJob, source: RecordSource) -> Option<Sample> {
    if !(j.seconds.is_finite() && j.seconds > 0.0) {
        return None;
    }
    let mut s = Sample::new(RecordMetric::ImageS, j.seconds, if j.at > 0.0 { j.at } else { now_s() }, source);
    s.width = (j.width > 0).then_some(j.width);
    s.height = (j.height > 0).then_some(j.height);
    s.steps = j.steps.filter(|n| *n > 0);
    Some(s)
}

/// The record sample of one finished video job (seconds per job, with its size, frames and steps).
pub fn video_sample(j: &ImageJob, source: RecordSource) -> Option<Sample> {
    let mut s = image_sample(j, source)?;
    s.metric = RecordMetric::VideoS;
    s.frames = j.frames.filter(|n| *n > 0);
    Some(s)
}

/// Display precision of a metric's values (comparisons use the rounded value).
fn rounded(metric: RecordMetric, v: f64) -> f64 {
    let p = if metric.higher_is_better() { 100.0 } else { 1000.0 };
    (v * p).round() / p
}

// ------------------------------------------------------------------------------------------ store

/// A model file of a session, to identify its records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    /// The file as the server opens it (absolute when known).
    pub path: Option<PathBuf>,
    /// File name ("Qwen3.8-27B-UD-Q4_K_XL.gguf").
    pub file: String,
}

/// A file's identity once known: its SHA-256 (None: not hashable) and size.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Ident {
    sha: Option<String>,
    size: Option<u64>,
    /// "repo@revision" of a matching recommendation file.
    source: Option<String>,
}

impl Ident {
    /// `<sha256>`, else `<file>:<size>`.
    fn key(&self, file: &str) -> String {
        match &self.sha {
            Some(s) => s.clone(),
            None => format!("{file}:{}", self.size.unwrap_or(0)),
        }
    }
}

struct Sess {
    model: ModelFile,
    /// None until the hash thread answered.
    ident: Option<Ident>,
    pending: Vec<(Conditions, Sample)>,
    ended: bool,
}

/// One broken record (local key: the published key without the machine).
#[derive(Debug, Clone)]
struct Broken {
    local: String,
    metric: RecordMetric,
    old: Option<f64>,
    new: f64,
    at: f64,
}

#[derive(Default)]
struct Store {
    machine: String,
    /// "<identity>|<backend>" -> entry (`key` / `machine` / `node` are set when published).
    entries: BTreeMap<String, RecordEntry>,
    events: VecDeque<Broken>,
    sessions: BTreeMap<String, Sess>,
    /// System id -> until (epoch s): samples captured meanwhile are bench samples.
    bench: BTreeMap<String, f64>,
    /// History lines not written yet.
    history: Vec<Broken>,
    /// The machine name the history file's lines carry (they are rewritten when it differs).
    history_machine: String,
    /// Local keys whose history lines are dropped at the next write (forgotten entries).
    purge: BTreeSet<String>,
    /// Local key -> when it was forgotten: older samples of it (a running session's log, replayed after a KLIF
    /// restart) do not bring it back.
    forgotten: BTreeMap<String, f64>,
    /// records.json must be written.
    dirty: bool,
    /// The published view (rebuilt when `rev` moved).
    view: Option<(u64, Vec<RecordEntry>, Vec<RecordEvent>)>,
}

impl Store {
    fn published_key(&self, local: &str) -> String {
        format!("{}|{local}", self.machine)
    }

    /// Apply one sample; true when it broke (or set) the record.
    fn apply(&mut self, local: &str, model: &RecordModel, cond: &Conditions, s: &Sample) -> bool {
        if !(s.value.is_finite() && s.value > 0.0) {
            return false;
        }
        if self.forgotten.get(local).is_some_and(|at| s.at <= *at) {
            return false;
        }
        let value = rounded(s.metric, s.value);
        let entry = self.entries.entry(local.to_string()).or_insert_with(|| RecordEntry {
            key: String::new(),
            node: None,
            machine: String::new(),
            kind: cond.kind,
            model: model.clone(),
            backend: cond.backend.clone(),
            best: BTreeMap::new(),
        });
        let old = entry.best.get(&s.metric).map(|v| v.value);
        let better = match old {
            None => true,
            Some(o) if s.metric.higher_is_better() => value > o,
            Some(o) => value < o,
        };
        if !better {
            return false;
        }
        // The latest facts of the file (a preset rename, a sha that matched a recommendation since).
        entry.model = model.clone();
        entry.kind = cond.kind;
        entry.best.insert(
            s.metric,
            RecordValue {
                value,
                at: s.at,
                source: s.source,
                ctx: cond.ctx,
                prompt_tokens: s.prompt_tokens,
                cached_tokens: s.cached_tokens,
                gen_tokens: s.gen_tokens,
                kv: cond.kv.clone(),
                width: s.width,
                height: s.height,
                steps: s.steps,
                frames: s.frames,
                gpus: cond.gpus.clone(),
                backend_build: cond.backend_build.clone(),
                preset: cond.preset.clone(),
                klif_version: KLIF_VERSION.to_string(),
                tflops_fp32: cond.tflops_fp32.filter(|t| t.is_finite() && *t > 0.0),
            },
        );
        let b = Broken { local: local.to_string(), metric: s.metric, old, new: value, at: s.at };
        match old {
            Some(o) => log::info!("new record {} {:?}: {value} (was {o})", self.published_key(local), s.metric),
            None => log::info!("first record {} {:?}: {value}", self.published_key(local), s.metric),
        }
        self.events.push_back(b.clone());
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
        self.history.push(b);
        self.dirty = true;
        true
    }

    /// Apply a session's sample now that its file is known.
    fn apply_for(&mut self, model: &ModelFile, ident: &Ident, cond: &Conditions, s: &Sample) -> bool {
        let local = format!("{}|{}", ident.key(&model.file), cond.backend);
        let stem = Path::new(&model.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| model.file.clone());
        let rm = RecordModel {
            sha256: ident.sha.clone(),
            file: model.file.clone(),
            name: Some(cond.name.trim()).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or(stem),
            quant: cond.quant.clone().filter(|q| !q.trim().is_empty()),
            source: ident.source.clone(),
            size_bytes: ident.size,
        };
        self.apply(&local, &rm, cond, s)
    }

    /// The local key of a published key (or of a local key).
    fn local_of(&self, key: &str) -> Option<String> {
        let k = key.trim();
        if let Some(l) = k.strip_prefix(&format!("{}|", self.machine)).filter(|l| self.entries.contains_key(*l)) {
            return Some(l.to_string());
        }
        self.entries.contains_key(k).then(|| k.to_string())
    }
}

/// `records.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RecordsFile {
    version: u32,
    machine: String,
    entries: Vec<Value>,
    /// Local key -> when it was forgotten.
    forgotten: BTreeMap<String, f64>,
}

// ------------------------------------------------------------------------------------------ records

struct Shared {
    data_dir: PathBuf,
    store: Mutex<Store>,
    /// Serializes file writes (taken before `store`).
    io: Mutex<()>,
    /// Bumped on every change of entries / events. Starts at the engine start time in ms, so a revision is never
    /// reused by a restarted engine (a hub's cached copy is then refreshed).
    rev: AtomicU64,
    /// A cheap "anything to write?" for the tick.
    dirty: AtomicBool,
    cancel: AtomicBool,
    hasher: Mutex<Option<Sender<PathBuf>>>,
    /// When this engine opened the records (epoch s).
    opened_at: f64,
    /// The revision of the merged view (this machine + nodes): what it was made of, and its number.
    combo: Mutex<(Vec<(String, u64)>, u64)>,
}

/// The records of this machine (cheap to clone; the hash thread holds one).
#[derive(Clone)]
pub struct Records {
    sh: Arc<Shared>,
}

impl Records {
    /// Read `records.json` (leniently: an unreadable entry is skipped, an unreadable file is kept aside).
    pub fn open(data_dir: &Path, machine: &str) -> Records {
        let mut store = Store { machine: machine.to_string(), history_machine: machine.to_string(), ..Store::default() };
        let path = data_dir.join(RECORDS_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<RecordsFile>(&bytes) {
                Ok(f) => {
                    let file_machine = if f.machine.trim().is_empty() { machine.to_string() } else { f.machine.clone() };
                    for v in f.entries {
                        let Ok(mut e) = serde_json::from_value::<RecordEntry>(v) else { continue };
                        let prefix = format!("{}|", if e.machine.is_empty() { &file_machine } else { &e.machine });
                        let local = e.key.strip_prefix(&prefix).unwrap_or(&e.key).to_string();
                        if local.is_empty() {
                            continue;
                        }
                        e.node = None;
                        store.entries.insert(local, e);
                    }
                    store.forgotten = f.forgotten;
                    store.history_machine = file_machine.clone();
                    if file_machine != machine {
                        store.dirty = true;
                    }
                }
                Err(e) => {
                    let aside = data_dir.join(format!("{RECORDS_FILE}.bad-{}", now_s() as u64));
                    log::warn!("{} is not readable ({e}); kept as {}", path.display(), aside.display());
                    let _ = std::fs::rename(&path, &aside);
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("{} could not be read: {e}", path.display()),
        }
        let dirty = store.dirty;
        let rev = (now_s() * 1000.0) as u64;
        Records {
            sh: Arc::new(Shared {
                data_dir: data_dir.to_path_buf(),
                store: Mutex::new(store),
                io: Mutex::new(()),
                rev: AtomicU64::new(rev.max(1)),
                dirty: AtomicBool::new(dirty),
                cancel: AtomicBool::new(false),
                hasher: Mutex::new(None),
                opened_at: now_s(),
                combo: Mutex::new((Vec::new(), 0)),
            }),
        }
    }

    /// When this engine opened the records (epoch s): older events of a node are not "new" here.
    pub fn opened_at(&self) -> f64 {
        self.sh.opened_at
    }

    /// One revision for a merged view made of `parts` (this machine's revision, each node's): it changes whenever a
    /// part does, and never repeats a number published before.
    pub fn combined_rev(&self, parts: Vec<(String, u64)>) -> u64 {
        let mut c = lock(&self.sh.combo);
        if c.0 != parts || c.1 == 0 {
            let floor = parts.first().map(|p| p.1).unwrap_or(0);
            c.0 = parts;
            c.1 = c.1.max(floor) + 1;
        }
        c.1
    }

    fn bump(&self) {
        self.sh.rev.fetch_add(1, Ordering::SeqCst);
    }

    /// The revision of the local entries / events (changes with every change).
    pub fn rev(&self) -> u64 {
        self.sh.rev.load(Ordering::SeqCst)
    }

    /// Follow a changed `[node] name` (entries and history are re-keyed at the next write).
    pub fn set_machine(&self, machine: &str) {
        let mut st = lock(&self.sh.store);
        if st.machine == machine {
            return;
        }
        log::info!("records: machine name \"{}\" -> \"{machine}\"", st.machine);
        st.machine = machine.to_string();
        st.dirty = true;
        drop(st);
        self.sh.dirty.store(true, Ordering::SeqCst);
        self.bump();
    }

    /// A live session's model file: its hash is looked up (or computed) on the hash thread. Idempotent.
    pub fn register(&self, session: &str, model: ModelFile) {
        let path = {
            let mut st = lock(&self.sh.store);
            if st.sessions.contains_key(session) {
                return;
            }
            let ident = match &model.path {
                Some(_) => None,
                // Nothing to hash (no file KLIF knows): the file name stands for it.
                None => Some(Ident::default()),
            };
            let path = model.path.clone();
            st.sessions.insert(session.to_string(), Sess { model, ident, pending: Vec::new(), ended: false });
            path
        };
        if let Some(p) = path {
            self.hash_later(p);
        }
    }

    /// The session ended: it is forgotten once its pending samples are applied.
    pub fn end_session(&self, session: &str) {
        let mut st = lock(&self.sh.store);
        let done = match st.sessions.get_mut(session) {
            Some(s) => {
                s.ended = true;
                s.ident.is_some() || s.pending.is_empty()
            }
            None => false,
        };
        if done {
            st.sessions.remove(session);
        }
    }

    /// New samples of a registered session (memory only: applied now when its file is known, else when the hash
    /// thread answers). Samples of an unknown session are dropped.
    pub fn observe(&self, session: &str, cond: Conditions, samples: Vec<Sample>) {
        if samples.is_empty() {
            return;
        }
        let mut changed = false;
        {
            let mut guard = lock(&self.sh.store);
            let st = &mut *guard;
            let Some(sess) = st.sessions.get_mut(session) else { return };
            match sess.ident.clone() {
                None => {
                    for s in samples {
                        sess.pending.push((cond.clone(), s));
                    }
                }
                Some(ident) => {
                    let model = sess.model.clone();
                    for s in &samples {
                        changed |= st.apply_for(&model, &ident, &cond, s);
                    }
                }
            }
        }
        if changed {
            self.sh.dirty.store(true, Ordering::SeqCst);
            self.bump();
        }
    }

    /// Mark a System as being benched until `until` (epoch s): its samples are bench samples meanwhile.
    pub fn bench_until(&self, system: &str, until: f64) {
        lock(&self.sh.store).bench.insert(system.to_string(), until);
    }

    /// Live, or bench while the System is being benched.
    pub fn source_for(&self, system: &str, now: f64) -> RecordSource {
        let mut st = lock(&self.sh.store);
        st.bench.retain(|_, until| *until > now);
        if st.bench.contains_key(system) {
            RecordSource::Bench
        } else {
            RecordSource::Live
        }
    }

    /// Remove an entry (published key, or the key without the machine). Err: no such entry.
    pub fn forget(&self, key: &str) -> Result<()> {
        {
            let mut st = lock(&self.sh.store);
            let Some(local) = st.local_of(key) else { bail!("There is no record \"{key}\" on this machine.") };
            st.entries.remove(&local);
            st.events.retain(|e| e.local != local);
            st.history.retain(|e| e.local != local);
            log::info!("record {} forgotten", st.published_key(&local));
            st.forgotten.insert(local.clone(), now_s());
            st.purge.insert(local);
            st.dirty = true;
        }
        self.sh.dirty.store(true, Ordering::SeqCst);
        self.bump();
        self.flush();
        Ok(())
    }

    /// The climb of one entry: every broken record of `key` (published key, or the key without the machine) from
    /// `records-history.jsonl`, oldest first, optionally of one metric. At most the last [`MAX_HISTORY`] points.
    /// Err: no such entry on this machine (a forgotten one has no history either).
    pub fn history(&self, key: &str, metric: Option<RecordMetric>) -> Result<Vec<RecordEvent>> {
        // Lines not written yet (and a pending machine rename) are settled first.
        self.flush();
        let (local, machines) = {
            let st = lock(&self.sh.store);
            let Some(local) = st.local_of(key) else { bail!("There is no record \"{key}\" on this machine.") };
            (local, [st.machine.clone(), st.history_machine.clone()])
        };
        let _io = lock(&self.sh.io);
        let text = match std::fs::read_to_string(self.sh.data_dir.join(HISTORY_FILE)) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => bail!("{HISTORY_FILE} could not be read: {e}."),
        };
        let published = format!("{}|{local}", machines[0]);
        let mut points: Vec<RecordEvent> = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(mut ev) = serde_json::from_str::<RecordEvent>(line) else { continue };
            if !machines.iter().any(|m| ev.key == format!("{m}|{local}")) || ev.metric.is_none() || metric.is_some_and(|m| ev.metric != Some(m)) {
                continue;
            }
            ev.key = published.clone();
            points.push(ev);
        }
        points.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        if points.len() > MAX_HISTORY {
            points.drain(..points.len() - MAX_HISTORY);
        }
        Ok(points)
    }

    /// The published entries and events (keys with the machine name) and their revision.
    pub fn view(&self) -> (u64, Vec<RecordEntry>, Vec<RecordEvent>) {
        let rev = self.rev();
        let mut st = lock(&self.sh.store);
        if let Some((r, e, v)) = &st.view {
            if *r == rev {
                return (rev, e.clone(), v.clone());
            }
        }
        let machine = st.machine.clone();
        let entries: Vec<RecordEntry> = st
            .entries
            .iter()
            .map(|(local, e)| {
                let mut e = e.clone();
                e.key = format!("{machine}|{local}");
                e.machine = machine.clone();
                e.node = None;
                e
            })
            .collect();
        let events: Vec<RecordEvent> = st
            .events
            .iter()
            .map(|b| RecordEvent { key: format!("{machine}|{}", b.local), metric: Some(b.metric), old: b.old, new: b.new, at: b.at })
            .collect();
        st.view = Some((rev, entries.clone(), events.clone()));
        (rev, entries, events)
    }

    /// Write records.json (and append / rewrite the history) when something changed. Cheap when nothing did.
    pub fn flush(&self) {
        if !self.sh.dirty.load(Ordering::SeqCst) {
            return;
        }
        let _io = lock(&self.sh.io);
        let (doc, lines, rewrite, purge, machine) = {
            let mut st = lock(&self.sh.store);
            self.sh.dirty.store(false, Ordering::SeqCst);
            if !st.dirty && st.history.is_empty() {
                return;
            }
            st.dirty = false;
            let machine = st.machine.clone();
            let entries: Vec<Value> = st
                .entries
                .iter()
                .map(|(local, e)| {
                    let mut e = e.clone();
                    e.key = format!("{machine}|{local}");
                    e.machine = machine.clone();
                    e.node = None;
                    serde_json::to_value(e).unwrap_or(Value::Null)
                })
                .collect();
            let doc = json!({ "version": 1, "machine": machine, "entries": entries, "forgotten": st.forgotten });
            let rewrite = (st.history_machine != machine || !st.purge.is_empty()).then(|| st.history_machine.clone());
            let purge = std::mem::take(&mut st.purge);
            (doc, std::mem::take(&mut st.history), rewrite, purge, machine)
        };
        let path = self.sh.data_dir.join(RECORDS_FILE);
        let bytes = serde_json::to_vec_pretty(&doc).unwrap_or_default();
        if let Err(e) = write_atomic(&path, &bytes) {
            log::warn!("could not write {}: {e}", path.display());
            let mut st = lock(&self.sh.store);
            st.dirty = true;
            let mut lines = lines;
            lines.append(&mut st.history);
            st.history = lines;
            st.purge.extend(purge);
            self.sh.dirty.store(true, Ordering::SeqCst);
            return;
        }
        let hist = self.sh.data_dir.join(HISTORY_FILE);
        if let Some(old) = rewrite {
            match rewrite_history(&hist, &old, &machine, &purge) {
                Ok(()) => lock(&self.sh.store).history_machine = machine.clone(),
                Err(e) => log::warn!("could not rewrite {}: {e}", hist.display()),
            }
        }
        if !lines.is_empty() {
            let mut text = String::new();
            for b in &lines {
                let line = json!({ "key": format!("{machine}|{}", b.local), "metric": b.metric, "old": b.old, "new": b.new, "at": b.at });
                text.push_str(&line.to_string());
                text.push('\n');
            }
            let r = std::fs::OpenOptions::new().create(true).append(true).open(&hist).and_then(|mut f| f.write_all(text.as_bytes()));
            if let Err(e) = r {
                log::warn!("could not append to {}: {e}", hist.display());
            }
        }
    }

    /// Stop the hash thread (a hash in progress is abandoned) and write what is pending.
    pub fn shutdown(&self) {
        self.sh.cancel.store(true, Ordering::SeqCst);
        *lock(&self.sh.hasher) = None;
        self.flush();
    }

    // ---- hashing -----------------------------------------------------------------------------

    fn hash_later(&self, path: PathBuf) {
        if self.sh.cancel.load(Ordering::SeqCst) {
            return;
        }
        let mut h = lock(&self.sh.hasher);
        if h.is_none() {
            let (tx, rx) = channel::<PathBuf>();
            let me = self.clone();
            match std::thread::Builder::new().name("klif-records-hash".into()).spawn(move || me.hash_loop(rx)) {
                Ok(_) => *h = Some(tx),
                Err(e) => {
                    log::warn!("could not start the records hash thread: {e}");
                    drop(h);
                    self.resolved(&path, Ident::default());
                    return;
                }
            }
        }
        if let Some(tx) = h.as_ref() {
            let _ = tx.send(path);
        }
    }

    fn hash_loop(self, rx: Receiver<PathBuf>) {
        background_io();
        let mut cache = HashCache::load(&self.sh.data_dir);
        while let Ok(path) = rx.recv() {
            if self.sh.cancel.load(Ordering::SeqCst) {
                break;
            }
            let ident = match std::fs::metadata(&path) {
                Ok(m) if m.is_file() => {
                    let (size, mtime) = (m.len(), mtime_ms(&m));
                    let sha = match cache.get(&path, size, mtime) {
                        Some(s) => Some(s),
                        None => {
                            let t0 = std::time::Instant::now();
                            match hash_file(&path, &self.sh.cancel) {
                                Ok(s) => {
                                    log::info!("hashed {} in {:.1} s", path.display(), t0.elapsed().as_secs_f64());
                                    cache.put(&path, size, mtime, &s);
                                    cache.save(&self.sh.data_dir);
                                    Some(s)
                                }
                                Err(e) => {
                                    if self.sh.cancel.load(Ordering::SeqCst) {
                                        break;
                                    }
                                    log::warn!("{} could not be hashed ({e}); its records are kept by name and size", path.display());
                                    None
                                }
                            }
                        }
                    };
                    let source = sha.as_deref().and_then(recommendation_source);
                    Ident { sha, size: Some(size), source }
                }
                _ => {
                    log::info!("{} is not a file; its records are kept by name", path.display());
                    Ident::default()
                }
            };
            self.resolved(&path, ident);
            self.flush();
        }
    }

    /// A file's identity arrived: apply the pending samples of every session that uses it.
    fn resolved(&self, path: &Path, ident: Ident) {
        let mut changed = false;
        {
            let mut guard = lock(&self.sh.store);
            let st = &mut *guard;
            let ids: Vec<String> =
                st.sessions.iter().filter(|(_, s)| s.ident.is_none() && s.model.path.as_deref() == Some(path)).map(|(k, _)| k.clone()).collect();
            for id in ids {
                let Some(sess) = st.sessions.get_mut(&id) else { continue };
                sess.ident = Some(ident.clone());
                let pending = std::mem::take(&mut sess.pending);
                let (model, ended) = (sess.model.clone(), sess.ended);
                for (cond, s) in &pending {
                    changed |= st.apply_for(&model, &ident, cond, s);
                }
                if ended {
                    st.sessions.remove(&id);
                }
            }
        }
        if changed {
            self.sh.dirty.store(true, Ordering::SeqCst);
            self.bump();
        }
    }
}

/// "repo@revision" of the embedded recommendation file with this SHA-256.
fn recommendation_source(sha: &str) -> Option<String> {
    klif_catalog::recommend::embedded().iter().find_map(|rec| {
        rec.files
            .iter()
            .find(|f| f.sha256.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(sha)))
            .map(|f| format!("{}@{}", rec.repo_of(f), rec.revision_of(f)))
    })
}

/// Rewrite the history lines: keys of `old_machine` get `machine`, lines of `purge` (local keys) are dropped.
fn rewrite_history(path: &Path, old_machine: &str, machine: &str, purge: &BTreeSet<String>) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let prefix = format!("{old_machine}|");
    let mut out = String::with_capacity(text.len());
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(mut v) = serde_json::from_str::<Value>(line) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let key = v.get("key").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(local) = key.strip_prefix(&prefix) {
            if purge.contains(local) {
                continue;
            }
            v["key"] = Value::String(format!("{machine}|{local}"));
        }
        out.push_str(&v.to_string());
        out.push('\n');
    }
    write_atomic(path, out.as_bytes())
}

// ------------------------------------------------------------------------------------------ hashes

fn mtime_ms(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// SHA-256 of a file as lower-case hex (1 MiB reads; `cancel` stops it).
fn hash_file(path: &Path, cancel: &AtomicBool) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("cancelled"));
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(crate::wire::hex(&hasher.finalize()))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct HashEntry {
    path: String,
    size: u64,
    mtime_ms: u64,
    sha256: String,
}

/// `<data_dir>\hashes.json`: SHA-256 per path + size + mtime (owned by the hash thread).
#[derive(Default)]
struct HashCache {
    files: Vec<HashEntry>,
}

impl HashCache {
    fn load(data_dir: &Path) -> HashCache {
        let files = std::fs::read(data_dir.join(HASHES_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("files").and_then(Value::as_array).cloned())
            .map(|a| a.into_iter().filter_map(|x| serde_json::from_value::<HashEntry>(x).ok()).collect())
            .unwrap_or_default();
        HashCache { files }
    }

    fn same_path(a: &str, b: &Path) -> bool {
        let b = b.to_string_lossy();
        if cfg!(windows) {
            a.eq_ignore_ascii_case(&b)
        } else {
            a == b
        }
    }

    fn get(&self, path: &Path, size: u64, mtime: u64) -> Option<String> {
        self.files.iter().find(|e| Self::same_path(&e.path, path) && e.size == size && e.mtime_ms == mtime).map(|e| e.sha256.clone())
    }

    fn put(&mut self, path: &Path, size: u64, mtime: u64, sha: &str) {
        self.files.retain(|e| !Self::same_path(&e.path, path));
        self.files.push(HashEntry { path: path.to_string_lossy().into_owned(), size, mtime_ms: mtime, sha256: sha.to_string() });
        if self.files.len() > MAX_HASHES {
            let drop = self.files.len() - MAX_HASHES;
            self.files.drain(..drop);
        }
    }

    fn save(&self, data_dir: &Path) {
        let path = data_dir.join(HASHES_FILE);
        let bytes = serde_json::to_vec_pretty(&json!({ "version": 1, "files": self.files })).unwrap_or_default();
        if let Err(e) = write_atomic(&path, &bytes) {
            log::warn!("could not write {}: {e}", path.display());
        }
    }
}

/// Background mode for the calling thread: low CPU and very low IO priority (a model file is tens of GB).
#[cfg(windows)]
fn background_io() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThread() -> isize;
        fn SetThreadPriority(thread: isize, priority: i32) -> i32;
    }
    const THREAD_MODE_BACKGROUND_BEGIN: i32 = 0x0001_0000;
    // SAFETY: the pseudo handle of the current thread is always valid; the call only changes this thread's priority.
    let ok = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
    if ok == 0 {
        log::debug!("records: the hash thread could not enter background mode");
    }
}

#[cfg(not(windows))]
fn background_io() {}
