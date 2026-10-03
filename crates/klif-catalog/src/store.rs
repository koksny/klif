//! Writes to klif.toml (toml_edit, comment preserving). Every write re-reads the file first, refuses when the
//! on-disk file does not parse (never regenerates it), omits default-valued fields, and replaces the file
//! atomically (temp file + rename, retried briefly when another program holds it). Writing `[systems]` into a
//! file that only had the 0.2 `[tiers]` first materialises all fallback Systems (`Config::parse` of the file
//! gives them; `[tiers]` itself stays for 0.2). Errors are sentences. The engine records the file's mtime + size
//! after its own write so its hot reload ignores it. Owner: package B.
//!
//! How the text is kept: values that do not change keep their exact text (and comments); a changed value keeps
//! the comments around it; a table that moves (System order) takes the comment lines above its header with it.
//! A missing file is created (from the starter text of [`ensure_file`]). CRLF line endings and a UTF-8 BOM are
//! kept. Before replacing the file, the result is parsed again and checked (a write that would not read back as
//! intended is refused), and the file is re-checked for changes made meanwhile (then the edit is redone).

use anyhow::{anyhow, bail, Context, Result};
use klif_common::config::{
    default_system_id, line_col, scrub, validate_preset_id, validate_system_id, Config, PresetCfg, SystemCfg, FILE_FIELD,
};
use klif_common::secret::{is_secret_flag, MASK};
use klif_common::vm::{SystemId, SystemKind};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, TableLike, Value};

// ------------------------------------------------------------------------------------------ public API

/// `[systems.<system>] preset = id` (None removes the key). The preset must exist and serve the System's kind.
/// In the same write, `[systems.<system>] params` entries the new preset does not declare (or whose value is not
/// one of its choices) are dropped, so a stale selection never carries over to another preset.
pub fn set_system_preset(path: &Path, system: &SystemId, preset: Option<&str>) -> Result<()> {
    let id = local_id(system)?;
    edit(
        path,
        |doc, cfg| {
            let sys = system_cfg(cfg, id)?;
            if let Some(p) = preset {
                check_preset_for(cfg, p, id, sys.kind)?;
            }
            ensure_systems(doc, cfg)?;
            let entry = system_entry(doc, id)?;
            match preset {
                Some(p) => {
                    set_value(entry, "preset", Value::from(p));
                    prune_params(entry, cfg.presets.get(p));
                }
                None => {
                    entry.remove("preset");
                }
            }
            Ok(())
        },
        |cfg, _| match cfg.system(id) {
            Some(s) if s.preset_id() == preset => Ok(()),
            _ => bail!("The new preset of System \"{id}\" does not read back from klif.toml; nothing was written."),
        },
    )
}

/// `[systems.<system>] params.<name> = value`. The System's active preset must declare the param and the choice.
pub fn set_system_param(path: &Path, system: &SystemId, name: &str, value: &str) -> Result<()> {
    let id = local_id(system)?;
    edit(
        path,
        |doc, cfg| {
            let sys = system_cfg(cfg, id)?;
            let label = cfg.system_label(id).unwrap_or_else(|| id.to_string());
            let pid = sys.preset_id().ok_or_else(|| anyhow!("{label} has no preset, so it has no params."))?;
            let spec = cfg.presets.get(pid).ok_or_else(|| anyhow!("{label}'s preset \"{pid}\" is missing or unreadable."))?;
            let param = spec.params.get(name).ok_or_else(|| anyhow!("Preset \"{pid}\" has no param \"{name}\"."))?;
            if !param.choices.contains_key(value) {
                let choices: Vec<&str> = param.choices.keys().map(String::as_str).collect();
                bail!("\"{value}\" is not a choice of param \"{name}\" (choices: {}).", choices.join(", "));
            }
            ensure_systems(doc, cfg)?;
            let entry = system_entry(doc, id)?;
            match entry.get_mut("params").and_then(Item::as_table_like_mut) {
                Some(params) => set_value(params, name, Value::from(value)),
                None => {
                    let mut t = InlineTable::new();
                    t.insert(name, Value::from(value));
                    entry.insert("params", Item::Value(Value::InlineTable(t)));
                }
            }
            Ok(())
        },
        |cfg, _| match cfg.system(id) {
            Some(s) if s.params.get(name).map(String::as_str) == Some(value) => Ok(()),
            _ => bail!("The param of System \"{id}\" does not read back from klif.toml; nothing was written."),
        },
    )
}

/// Append `[systems.<id>]`; `id` None = the next free default id (`klif_common::config::default_system_id`).
/// Returns the id written. `label` None = the derived default label (not written).
pub fn add_system(path: &Path, id: Option<&str>, system: &SystemCfg) -> Result<SystemId> {
    if system.class.is_some() && system.kind != SystemKind::Llm {
        bail!("class only applies to llm Systems.");
    }
    let new_id = edit(
        path,
        |doc, cfg| {
            ensure_systems(doc, cfg)?;
            let raw: Vec<String> = doc
                .get("systems")
                .and_then(Item::as_table_like)
                .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
                .unwrap_or_default();
            let mut taken: Vec<&str> = cfg.systems.iter().map(|(k, _)| k.as_str()).collect();
            taken.extend(cfg.bad_systems.keys().map(String::as_str));
            taken.extend(raw.iter().map(String::as_str));
            let new_id = match id.map(str::trim) {
                Some(i) => {
                    validate_system_id(i).map_err(|e| anyhow!("System {e}"))?;
                    if taken.contains(&i) {
                        bail!("A System with id \"{i}\" already exists.");
                    }
                    SystemId::new(i)
                }
                None => default_system_id(system.kind, system.class, &taken),
            };
            if let Some(p) = system.preset_id() {
                check_preset_for(cfg, p, new_id.as_str(), system.kind)?;
            }
            let systems = doc["systems"].as_table_mut().ok_or_else(|| anyhow!("[systems] in klif.toml is not a table."))?;
            systems.insert(new_id.as_str(), Item::Table(system_table(system)));
            let prefix = vec![seg("systems"), seg(new_id.as_str())];
            relayout(doc, |order| {
                let group = take_group(order, &prefix);
                let at = order.iter().rposition(|p| starts_with(p, &[seg("systems")])).map(|i| i + 1).unwrap_or(order.len());
                splice(order, at, group);
            });
            Ok(new_id)
        },
        |cfg, new_id| match cfg.system(new_id.as_str()) {
            Some(s) if s.kind == system.kind && s.preset == system.preset && s.label == system.label => Ok(()),
            _ => bail!("The new System \"{new_id}\" does not read back from klif.toml; nothing was written."),
        },
    )?;
    Ok(new_id)
}

/// Remove `[systems.<system>]` (the engine refuses while it runs). Removing the last System keeps an empty
/// `[systems]` so the 0.2 `[tiers]` fallback does not come back.
pub fn remove_system(path: &Path, system: &SystemId) -> Result<()> {
    let id = local_id(system)?;
    edit(
        path,
        |doc, cfg| {
            if cfg.system(id).is_none() && !cfg.bad_systems.contains_key(id) {
                bail!("There is no System \"{id}\" in klif.toml.");
            }
            ensure_systems(doc, cfg)?;
            if !remove_keeping_comments(doc, "systems", id, true) {
                bail!("There is no System \"{id}\" in klif.toml.");
            }
            let systems = doc["systems"].as_table_mut().ok_or_else(|| anyhow!("[systems] in klif.toml is not a table."))?;
            if systems.is_empty() {
                systems.set_implicit(false);
            }
            Ok(())
        },
        |cfg, _| {
            if cfg.system(id).is_some() || cfg.bad_systems.contains_key(id) {
                bail!("System \"{id}\" is still in klif.toml after removing it; nothing was written.");
            }
            Ok(())
        },
    )
}

/// Rename (`label`; empty = back to the default label), move to tab index `move_to` (0-based among the local
/// Systems; reorders the `[systems.*]` tables, comments move with them), set `exclusive`. `None` leaves a field
/// unchanged.
pub fn update_system(path: &Path, system: &SystemId, label: Option<&str>, move_to: Option<u32>, exclusive: Option<bool>) -> Result<()> {
    let id = local_id(system)?;
    let wanted_order = std::cell::RefCell::new(None::<Vec<String>>);
    edit(
        path,
        |doc, cfg| {
            system_cfg(cfg, id)?;
            ensure_systems(doc, cfg)?;
            {
                let entry = system_entry(doc, id)?;
                if let Some(l) = label {
                    match l.trim() {
                        "" => {
                            entry.remove("label");
                        }
                        l => set_value(entry, "label", Value::from(l)),
                    }
                }
                match exclusive {
                    Some(true) => set_value(entry, "exclusive", Value::from(true)),
                    Some(false) => {
                        entry.remove("exclusive");
                    }
                    None => {}
                }
            }
            if let Some(to) = move_to {
                let good: Vec<&str> = cfg.systems.iter().map(|(k, _)| k.as_str()).collect();
                let order = move_system(doc, id, to as usize, &good)?;
                *wanted_order.borrow_mut() = Some(order.into_iter().filter(|k| good.contains(&k.as_str())).collect());
            }
            Ok(())
        },
        |cfg, _| {
            let s = cfg.system(id).ok_or_else(|| anyhow!("System \"{id}\" does not read back from klif.toml; nothing was written."))?;
            if let Some(l) = label {
                let want = Some(l.trim()).filter(|l| !l.is_empty());
                if s.label.as_deref() != want {
                    bail!("The new label of System \"{id}\" does not read back from klif.toml; nothing was written.");
                }
            }
            if exclusive.is_some_and(|x| x != s.exclusive) {
                bail!("exclusive of System \"{id}\" does not read back from klif.toml; nothing was written.");
            }
            if let Some(want) = wanted_order.borrow().as_ref() {
                let got: Vec<&str> = cfg.systems.iter().map(|(k, _)| k.as_str()).collect();
                if got != want.iter().map(String::as_str).collect::<Vec<_>>() {
                    bail!(
                        "System \"{id}\" could not be moved: it is written inline in [systems] (or mixed with tables). Reorder it in klif.toml by hand."
                    );
                }
            }
            Ok(())
        },
    )
}

/// Create or replace `[presets.<id>]`. Values equal to `klif_common::secret::MASK` are resolved against the
/// on-disk preset `secrets_from` (default `id`); an unresolved mask is an error. With `base_hash` (the
/// `PresetDetail.spec_hash` the editor loaded), the write is refused when the stored preset's
/// [`crate::Catalog::spec_hash`] differs ("changed on disk": any field, not only what runs). An existing preset is
/// updated in place (unchanged values keep their text and comments); a new one is appended after the last preset.
pub fn upsert_preset(path: &Path, id: &str, spec: &PresetCfg, secrets_from: Option<&str>, base_hash: Option<&str>) -> Result<()> {
    validate_preset_id(id).map_err(|e| anyhow!(e))?;
    let written = std::cell::RefCell::new(None::<PresetCfg>);
    edit(
        path,
        |doc, cfg| {
            if let Some(h) = base_hash.map(str::trim).filter(|h| !h.is_empty()) {
                let stored = cfg
                    .presets
                    .get(id)
                    .ok_or_else(|| anyhow!("Preset \"{id}\" changed on disk (it is gone or unreadable now); reload it before saving."))?;
                if crate::Catalog::spec_hash(stored) != h {
                    bail!("Preset \"{id}\" changed on disk since it was opened; reload it before saving.");
                }
            }
            let mut spec = spec.clone();
            unmask(&mut spec, cfg, secrets_from.unwrap_or(id))?;
            let desired = preset_toml(&spec)?;
            let presets = match doc.get_mut("presets") {
                Some(Item::Table(t)) => t,
                Some(_) => bail!("\"presets\" in klif.toml is not a table of presets; fix it by hand first."),
                None => {
                    let mut t = Table::new();
                    t.set_implicit(true);
                    doc.insert("presets", Item::Table(t));
                    doc["presets"].as_table_mut().expect("just inserted")
                }
            };
            let fresh = match presets.get_mut(id) {
                Some(Item::Table(t)) => {
                    merge_table(t, &desired, &[]);
                    false
                }
                Some(item) => {
                    *item = Item::Table(build_preset_table(&desired));
                    true
                }
                None => {
                    presets.insert(id, Item::Table(build_preset_table(&desired)));
                    true
                }
            };
            if fresh {
                let prefix = vec![seg("presets"), seg(id)];
                relayout(doc, |order| {
                    let group = take_group(order, &prefix);
                    let at = order.iter().rposition(|p| starts_with(p, &[seg("presets")])).map(|i| i + 1).unwrap_or(order.len());
                    splice(order, at, group);
                });
            }
            *written.borrow_mut() = Some(spec);
            Ok(())
        },
        |cfg, _| {
            let want = written.borrow();
            match (cfg.presets.get(id), want.as_ref()) {
                (Some(got), Some(want)) if got == want => Ok(()),
                _ => bail!("Preset \"{id}\" does not read back from klif.toml as saved; nothing was written."),
            }
        },
    )
}

/// Remove `[presets.<id>]` (the engine refuses while a System uses it or it runs; the store refuses while a
/// System of the file selects it).
pub fn delete_preset(path: &Path, id: &str) -> Result<()> {
    edit(
        path,
        |doc, cfg| {
            let users = cfg.systems_using(id);
            if !users.is_empty() {
                let labels: Vec<String> = users.iter().map(|s| cfg.system_label(s.as_str()).unwrap_or_else(|| s.to_string())).collect();
                bail!("Preset \"{id}\" is the active preset of {}; choose another preset there first.", labels.join(", "));
            }
            if !remove_keeping_comments(doc, "presets", id, false) {
                bail!("There is no preset \"{id}\" in klif.toml.");
            }
            Ok(())
        },
        |cfg, _| {
            if cfg.presets.contains_key(id) || cfg.bad_presets.contains_key(id) {
                bail!("Preset \"{id}\" is still in klif.toml after deleting it; nothing was written.");
            }
            Ok(())
        },
    )
}

/// Make sure the config file exists (a commented starter file at `cfg.file_path()`); returns its path.
pub fn ensure_file(cfg: &Config) -> Result<PathBuf> {
    let path = cfg.file_path();
    if path.is_file() {
        return Ok(path);
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("The folder {} could not be created", dir.display()))?;
    }
    let tmp = temp_name(&path);
    let text = if cfg!(windows) { STARTER.replace('\n', "\r\n") } else { STARTER.to_string() };
    std::fs::write(&tmp, text).with_context(|| format!("{} could not be written", tmp.display()))?;
    if path.exists() {
        // Someone created it meanwhile: keep theirs.
        let _ = std::fs::remove_file(&tmp);
        return Ok(path);
    }
    replace(&tmp, &path)?;
    Ok(path)
}

/// The text of a new klif.toml (no Systems yet: KLIF shows its onboarding).
pub const STARTER: &str = r#"# KLIF configuration (0.3). KLIF edits this file too (Tune, klif-cli) and keeps your comments.
# Each [systems.<id>] is a tab; each [presets.<id>] is one exact server command. A documented example is
# config/klif.example.toml in the KLIF repository.

# [paths]
# models_dir = 'D:\models'                  # recommended models are downloaded here (refused until set)

# [systems.s1]
# label = "System 1"
# kind = "llm"                              # llm | image | tts | stt | video
# class = "fast"                            # llm only: fast | deep | max
# preset = "my-llm"

# [presets.my-llm]
# name = "My LLM"
# adapter = "llama.cpp"                     # llama.cpp | sd.cpp | vllm | openai | generic
# command = 'D:\llama.cpp\llama-server.exe'
# args = ["-m", "{model}", "-c", "{ctx}", "--host", "{host}", "--port", "{port}"]
# model = 'D:\models\model.gguf'
# ctx = 16384
"#;

// ------------------------------------------------------------------------------------------ file IO

struct Loaded {
    /// The text without a BOM, as read.
    text: String,
    /// The exact file content (for "nothing changed").
    raw: Option<String>,
    crlf: bool,
    bom: bool,
    stamp: Option<(SystemTime, u64)>,
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

fn read_file(path: &Path) -> Result<Loaded> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let raw = String::from_utf8(bytes).map_err(|_| anyhow!("{} is not UTF-8 text, so KLIF does not write it.", path.display()))?;
            let bom = raw.starts_with('\u{feff}');
            let text = raw.trim_start_matches('\u{feff}').to_string();
            Ok(Loaded { crlf: text.contains("\r\n"), bom, text, stamp: stamp(path), raw: Some(raw) })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let text = STARTER.to_string();
            Ok(Loaded { crlf: cfg!(windows), bom: false, text, stamp: None, raw: None })
        }
        Err(e) => bail!("{} could not be read: {e}.", path.display()),
    }
}

impl Loaded {
    fn restore(&self, text: String) -> String {
        let mut t = text.replace("\r\n", "\n");
        if self.crlf {
            t = t.replace('\n', "\r\n");
        }
        if self.bom {
            t.insert(0, '\u{feff}');
        }
        t
    }
}

fn temp_name(path: &Path) -> PathBuf {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "klif.toml".into());
    path.with_file_name(format!(".{name}.{}-{nanos}.tmp", std::process::id()))
}

/// Rename `tmp` over `path`, retrying for about 2 s while another program holds the file.
fn replace(tmp: &Path, path: &Path) -> Result<()> {
    let mut last = None;
    for _ in 0..25 {
        match std::fs::rename(tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(80));
            }
        }
    }
    let _ = std::fs::remove_file(tmp);
    bail!(
        "{} could not be replaced (another program may hold it open): {}. Nothing was changed.",
        path.display(),
        last.map(|e| e.to_string()).unwrap_or_default()
    )
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("The folder {} could not be created", dir.display()))?;
    }
    let tmp = temp_name(path);
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).with_context(|| format!("{} could not be written", tmp.display()))?;
        f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).with_context(|| format!("{} could not be written", tmp.display()))?;
    }
    replace(&tmp, path)
}

fn one_line(e: impl std::fmt::Display) -> String {
    e.to_string().trim().replace(['\r', '\n'], " ")
}

/// Read, edit, check and write klif.toml. `op` edits the document (with the config parsed from the same text);
/// `check` validates the config parsed from the result. Redone (up to 3 times) when the file changes meanwhile.
fn edit<T>(
    path: &Path,
    mut op: impl FnMut(&mut DocumentMut, &Config) -> Result<T>,
    check: impl Fn(&Config, &T) -> Result<()>,
) -> Result<T> {
    for _ in 0..3 {
        let loaded = read_file(path)?;
        // Line and column only: the error's Display quotes the source line, which may hold a secret.
        let mut doc: DocumentMut = loaded.text.parse().map_err(|e: toml_edit::TomlError| {
            let at = e
                .span()
                .map(|s| {
                    let (line, col) = line_col(&loaded.text, s.start);
                    format!(" (line {line}, column {col})")
                })
                .unwrap_or_default();
            anyhow!(
                "{} does not parse{at}, so KLIF does not write it (fix it by hand first): {}.",
                path.display(),
                scrub(e.message()).trim_end_matches('.')
            )
        })?;
        let parsed = Config::parse(&loaded.text, path);
        if parsed.unreadable() {
            bail!("{} could not be read, so KLIF does not write it (fix it by hand first).", path.display());
        }
        comments_first(&mut doc);
        let out = op(&mut doc, &parsed.cfg)?;
        let text = loaded.restore(doc.to_string());
        let after = Config::parse(text.trim_start_matches('\u{feff}'), path);
        if let Some(i) = after.issues.iter().find(|i| i.is_error() && i.field.as_deref() == Some(FILE_FIELD)) {
            bail!("The edited klif.toml would not read back ({}); nothing was written.", i.text);
        }
        check(&after.cfg, &out)?;
        if loaded.raw.as_deref() == Some(text.as_str()) {
            return Ok(out);
        }
        if stamp(path) != loaded.stamp {
            continue;
        }
        write_atomic(path, &text)?;
        return Ok(out);
    }
    bail!("{} kept changing while KLIF was writing it; try again.", path.display())
}

/// A file of comments only: keep them at the top (toml_edit would print new tables before them).
fn comments_first(doc: &mut DocumentMut) {
    if !doc.as_table().is_empty() {
        return;
    }
    let trailing = doc.trailing().as_str().unwrap_or("").to_string();
    if trailing.trim().is_empty() {
        return;
    }
    doc.set_trailing("");
    let mut prefix = trailing.trim_end().to_string();
    // A blank line after the block: it stays detached from the first table (see split_prefix).
    prefix.push_str("\n\n");
    doc.as_table_mut().decor_mut().set_prefix(prefix);
}

/// A table header's leading text split at its last blank line: (detached, attached). The detached part (section
/// banners, file headers, the blank line itself) belongs to the place in the file; the attached part (comment
/// lines right above the header) belongs to the table and moves or goes with it.
fn split_prefix(p: &str) -> (String, String) {
    let mut end = 0;
    let mut offset = 0;
    for line in p.split_inclusive('\n') {
        offset += line.len();
        if line.trim().is_empty() && line.ends_with('\n') {
            end = offset;
        }
    }
    (p[..end].to_string(), p[end..].to_string())
}

fn prefix_at(doc: &mut DocumentMut, path: &[Seg]) -> Option<String> {
    let t = table_at(doc.as_table_mut(), path)?;
    Some(t.decor().prefix().and_then(|r| r.as_str()).unwrap_or("\n").to_string())
}

fn set_prefix_at(doc: &mut DocumentMut, path: &[Seg], prefix: String) {
    if let Some(t) = table_at(doc.as_table_mut(), path) {
        t.decor_mut().set_prefix(prefix);
    }
}

/// `detached` (comments ending in a blank line) + `next` without doubling the blank line between them.
fn join_prefix(detached: &str, next: &str) -> String {
    format!("{detached}{}", next.trim_start_matches(['\r', '\n']))
}

/// Remove `[<container>.<key>]` (table or inline entry). The detached comments above its header stay in the file
/// (moved to the table that follows; with `keep_header` an emptied container is written as an explicit `[container]`
/// and takes them; else they go to the end). False when there is no such entry.
fn remove_keeping_comments(doc: &mut DocumentMut, container: &str, key: &str, keep_header: bool) -> bool {
    let order = table_order(doc);
    let prefix = [seg(container), seg(key)];
    let head = order.iter().position(|p| starts_with(p, &prefix));
    let detached = head.and_then(|h| prefix_at(doc, &order[h])).map(|p| split_prefix(&p).0).unwrap_or_default();
    let next = head.and_then(|h| order[h..].iter().find(|p| !starts_with(p, &prefix)).cloned());
    let removed = match doc.get_mut(container).and_then(Item::as_table_like_mut) {
        Some(t) => t.remove(key).is_some(),
        None => false,
    };
    let emptied = doc.get(container).and_then(Item::as_table_like).is_some_and(|t| t.is_empty());
    if removed && keep_header && emptied {
        if let Some(t) = doc.get_mut(container).and_then(Item::as_table_mut) {
            t.set_implicit(false);
            if !detached.trim().is_empty() {
                let cur = t.decor().prefix().and_then(|r| r.as_str()).unwrap_or("").to_string();
                t.decor_mut().set_prefix(join_prefix(&detached, &cur));
            }
            return true;
        }
    }
    if removed && !detached.trim().is_empty() {
        match next {
            Some(n) => {
                let cur = prefix_at(doc, &n).unwrap_or_default();
                set_prefix_at(doc, &n, join_prefix(&detached, &cur));
            }
            None => {
                let trailing = doc.trailing().as_str().unwrap_or("").to_string();
                doc.set_trailing(join_prefix(&detached, &trailing));
            }
        }
    }
    removed
}

// ----------------------------------------------------------------------------------------- systems

fn local_id(system: &SystemId) -> Result<&str> {
    if system.is_remote() {
        bail!("System \"{system}\" lives on another node; change it there.");
    }
    Ok(system.as_str())
}

fn system_cfg<'a>(cfg: &'a Config, id: &str) -> Result<&'a SystemCfg> {
    match cfg.system(id) {
        Some(s) => Ok(s),
        None => match cfg.bad_systems.get(id) {
            // The sentence already names the System.
            Some(e) => bail!("{}. Fix it in klif.toml first.", e.trim_end_matches('.')),
            None => bail!("There is no System \"{id}\" in klif.toml."),
        },
    }
}

/// The preset exists (parsed or not) and serves `kind`.
fn check_preset_for(cfg: &Config, preset: &str, system: &str, kind: SystemKind) -> Result<()> {
    if let Some(spec) = cfg.presets.get(preset) {
        if let Some(k) = spec.effective_kind().filter(|k| *k != kind) {
            let label = cfg.system_label(system).unwrap_or_else(|| system.to_string());
            bail!(
                "Preset \"{preset}\" serves {}, but {label} is {} System.",
                crate::resolve::with_article(k.label()),
                crate::resolve::with_article(kind.label())
            );
        }
        return Ok(());
    }
    if cfg.bad_presets.contains_key(preset) {
        return Ok(());
    }
    bail!("There is no preset \"{preset}\" in klif.toml.")
}

/// A file without `[systems]`: write every System the config has (the `[tiers]` fallback) first.
fn ensure_systems(doc: &mut DocumentMut, cfg: &Config) -> Result<()> {
    match doc.get("systems") {
        Some(Item::Table(_)) => return Ok(()),
        Some(_) => bail!("\"systems\" in klif.toml is not a table of Systems ([systems.<id>]); fix it by hand first."),
        None => {}
    }
    let mut st = Table::new();
    st.set_implicit(true);
    for (id, sys) in &cfg.systems {
        st.insert(id.as_str(), Item::Table(system_table(sys)));
    }
    doc.insert("systems", Item::Table(st));
    relayout(doc, |order| {
        let group = take_group(order, &[seg("systems")]);
        let at = order.iter().rposition(|p| starts_with(p, &[seg("tiers")])).map(|i| i + 1).unwrap_or(order.len());
        splice(order, at, group);
    });
    Ok(())
}

fn system_entry<'a>(doc: &'a mut DocumentMut, id: &str) -> Result<&'a mut dyn TableLike> {
    doc.get_mut("systems")
        .and_then(Item::as_table_like_mut)
        .and_then(|s| s.get_mut(id))
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| anyhow!("There is no System \"{id}\" in klif.toml."))
}

/// Drop the `params` entries of a System entry that `spec` does not declare, or whose value is not one of that
/// param's choices; the `params` key goes when nothing is left. A preset that does not parse (None) cannot be
/// checked, so its selection is kept.
fn prune_params(entry: &mut dyn TableLike, spec: Option<&PresetCfg>) {
    let Some(spec) = spec else {
        return;
    };
    let Some(params) = entry.get_mut("params").and_then(Item::as_table_like_mut) else {
        return;
    };
    let stale: Vec<String> = params
        .iter()
        .filter(|(k, v)| {
            let keep = spec.params.get(*k).zip(v.as_str()).is_some_and(|(p, val)| p.choices.contains_key(val));
            !keep
        })
        .map(|(k, _)| k.to_string())
        .collect();
    for k in &stale {
        params.remove(k);
    }
    if params.is_empty() {
        entry.remove("params");
    }
}

/// `[systems.<id>]` for a SystemCfg: label, kind, class, preset, params (inline), exclusive (only when true).
fn system_table(sys: &SystemCfg) -> Table {
    let mut t = Table::new();
    if let Some(l) = sys.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        t.insert("label", toml_edit::value(l));
    }
    t.insert("kind", toml_edit::value(sys.kind.as_str()));
    if let Some(c) = sys.class {
        t.insert("class", toml_edit::value(c.as_str()));
    }
    if let Some(p) = sys.preset_id() {
        t.insert("preset", toml_edit::value(p));
    }
    if !sys.params.is_empty() {
        let mut it = InlineTable::new();
        for (k, v) in &sys.params {
            it.insert(k, Value::from(v.as_str()));
        }
        t.insert("params", Item::Value(Value::InlineTable(it)));
    }
    if sys.exclusive {
        t.insert("exclusive", toml_edit::value(true));
    }
    t
}

/// Move System `id` to index `to` among the `good` (parsed) Systems. Returns the new key order of [systems].
fn move_system(doc: &mut DocumentMut, id: &str, to: usize, good: &[&str]) -> Result<Vec<String>> {
    let order = table_order(doc);
    let systems_rank = order.iter().position(|p| p.len() == 1 && p[0] == seg("systems"));
    let st = doc.get("systems").and_then(Item::as_table).ok_or_else(|| anyhow!("[systems] in klif.toml is not a table."))?;
    // Text order of the entries: inline ones sit in the [systems] body, tables at their header.
    let mut keys: Vec<(usize, usize, String, bool)> = st
        .iter()
        .enumerate()
        .map(|(i, (k, item))| {
            let is_table = item.is_table();
            let rank = if is_table {
                order.iter().position(|p| p.len() >= 2 && p[0] == seg("systems") && p[1] == seg(k)).unwrap_or(usize::MAX)
            } else {
                systems_rank.unwrap_or(0)
            };
            (rank, i, k.to_string(), is_table)
        })
        .collect();
    keys.sort();
    let mut names: Vec<String> = keys.iter().map(|(_, _, k, _)| k.clone()).collect();
    let from = names.iter().position(|k| k == id).ok_or_else(|| anyhow!("There is no System \"{id}\" in klif.toml."))?;
    let me = names.remove(from);
    let good_rest: Vec<usize> = names.iter().enumerate().filter(|(_, k)| good.contains(&k.as_str())).map(|(i, _)| i).collect();
    let at = match good_rest.get(to) {
        Some(&i) => i,
        None => good_rest.last().map(|&i| i + 1).unwrap_or(names.len()),
    };
    names.insert(at, me);

    let rank: HashMap<&str, usize> = names.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect();
    let st = doc["systems"].as_table_mut().expect("checked above");
    st.sort_values_by(|a, _, b, _| rank.get(a.get()).cmp(&rank.get(b.get())));
    let tables_old: Vec<String> = keys.iter().filter(|(_, _, _, t)| *t).map(|(_, _, k, _)| k.clone()).collect();
    let tables_new: Vec<String> = names.iter().filter(|k| tables_old.contains(k)).cloned().collect();
    // Each System's tables (header + sub-tables) form a group. The groups' head slots keep their places in the
    // text (counted among the other tables); the groups take them in the new order.
    let is_sys_table = |p: &TPath| p.len() >= 2 && p[0] == seg("systems") && matches!(&p[1], Seg::Key(k) if tables_old.contains(k));
    let slots: Vec<usize> = tables_old
        .iter()
        .map(|k| {
            let head = order.iter().position(|p| p.len() >= 2 && p[0] == seg("systems") && p[1] == seg(k)).unwrap_or(order.len());
            order[..head].iter().filter(|p| !is_sys_table(p)).count()
        })
        .collect();
    // Comments: each slot keeps the detached part of its old head's prefix; tables carry their attached part.
    let head_of = |k: &str| order.iter().find(|p| starts_with(p, &[seg("systems"), seg(k)])).cloned();
    let mut detached: Vec<String> = Vec::new();
    let mut attached: HashMap<String, String> = HashMap::new();
    for k in &tables_old {
        let (d, a) = head_of(k).and_then(|h| prefix_at(doc, &h)).map(|p| split_prefix(&p)).unwrap_or_default();
        detached.push(d);
        attached.insert(k.clone(), a);
    }
    for (n, k) in tables_new.iter().enumerate() {
        if let Some(h) = head_of(k) {
            let a = attached.get(k).cloned().unwrap_or_default();
            set_prefix_at(doc, &h, format!("{}{a}", detached[n]));
        }
    }
    relayout(doc, |order| {
        let groups: Vec<(String, Vec<TPath>)> =
            tables_old.iter().map(|k| (k.clone(), take_group(order, &[seg("systems"), seg(k)]))).collect();
        let mut placed: Vec<(usize, usize, Vec<TPath>)> = tables_new
            .iter()
            .enumerate()
            .map(|(n, k)| (slots[n], n, groups.iter().find(|(g, _)| g == k).map(|(_, g)| g.clone()).unwrap_or_default()))
            .collect();
        // Insert from the last slot backwards; groups sharing a slot end up in the new order.
        placed.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        for (slot, _, g) in placed {
            splice(order, slot, g);
        }
    });
    Ok(names)
}

// -------------------------------------------------------------------------------------------- layout

/// One step of a table's path in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Key(String),
    Idx(usize),
}

type TPath = Vec<Seg>;

fn seg(k: &str) -> Seg {
    Seg::Key(k.to_string())
}

fn starts_with(p: &TPath, prefix: &[Seg]) -> bool {
    p.len() >= prefix.len() && p[..prefix.len()] == *prefix
}

/// Every (non-dotted, non-root) table in the order toml_edit prints them: by position, a table without one
/// right after the table visited before it.
fn table_order(doc: &DocumentMut) -> Vec<TPath> {
    fn visit(t: &Table, path: &mut TPath, last: &mut isize, acc: &mut Vec<(isize, TPath)>) {
        if !t.is_dotted() {
            if let Some(p) = t.position() {
                *last = p;
            }
            if !path.is_empty() {
                acc.push((*last, path.clone()));
            }
        }
        for (k, item) in t.iter() {
            match item {
                Item::Table(sub) => {
                    path.push(seg(k));
                    visit(sub, path, last, acc);
                    path.pop();
                }
                Item::ArrayOfTables(a) => {
                    for (i, sub) in a.iter().enumerate() {
                        path.push(seg(k));
                        path.push(Seg::Idx(i));
                        visit(sub, path, last, acc);
                        path.pop();
                        path.pop();
                    }
                }
                _ => {}
            }
        }
    }
    let mut acc = Vec::new();
    let mut last = 0isize;
    visit(doc.as_table(), &mut Vec::new(), &mut last, &mut acc);
    acc.sort_by_key(|(p, _)| *p);
    acc.into_iter().map(|(_, p)| p).collect()
}

fn table_at<'a>(t: &'a mut Table, path: &[Seg]) -> Option<&'a mut Table> {
    match path {
        [] => Some(t),
        [Seg::Key(k), Seg::Idx(i), rest @ ..] if matches!(t.get(k), Some(Item::ArrayOfTables(_))) => {
            table_at(t.get_mut(k)?.as_array_of_tables_mut()?.get_mut(*i)?, rest)
        }
        [Seg::Key(k), rest @ ..] => table_at(t.get_mut(k)?.as_table_mut()?, rest),
        _ => None,
    }
}

/// Let `f` reorder the printed table list, then pin every table's position to it.
fn relayout(doc: &mut DocumentMut, f: impl FnOnce(&mut Vec<TPath>)) {
    let mut order = table_order(doc);
    f(&mut order);
    for (i, p) in order.iter().enumerate() {
        if let Some(t) = table_at(doc.as_table_mut(), p) {
            t.set_position(Some(i as isize + 1));
        }
    }
}

fn take_group(order: &mut Vec<TPath>, prefix: &[Seg]) -> Vec<TPath> {
    let (group, rest): (Vec<TPath>, Vec<TPath>) = std::mem::take(order).into_iter().partition(|p| starts_with(p, prefix));
    *order = rest;
    group
}

fn splice(order: &mut Vec<TPath>, at: usize, group: Vec<TPath>) {
    let at = at.min(order.len());
    order.splice(at..at, group);
}

// ------------------------------------------------------------------------------------------- values

/// Set `key` to `v`, keeping the key's comments and the value's surrounding text; no-op when equal.
fn set_value(t: &mut dyn TableLike, key: &str, mut v: Value) {
    match t.get_mut(key) {
        Some(item) => {
            if let Some(old) = item.as_value() {
                if value_to_toml(old) == value_to_toml(&v) {
                    return;
                }
                *v.decor_mut() = old.decor().clone();
            }
            *item = Item::Value(v);
        }
        None => {
            t.insert(key, Item::Value(v));
        }
    }
}

fn value_to_toml(v: &Value) -> toml::Value {
    match v {
        Value::String(s) => toml::Value::String(s.value().clone()),
        Value::Integer(i) => toml::Value::Integer(*i.value()),
        Value::Float(f) => toml::Value::Float(*f.value()),
        Value::Boolean(b) => toml::Value::Boolean(*b.value()),
        Value::Datetime(d) => toml::Value::String(d.value().to_string()),
        Value::Array(a) => toml::Value::Array(a.iter().map(value_to_toml).collect()),
        Value::InlineTable(t) => toml::Value::Table(t.iter().map(|(k, v)| (k.to_string(), value_to_toml(v))).collect()),
    }
}

fn item_to_toml(i: &Item) -> Option<toml::Value> {
    match i {
        Item::None => None,
        Item::Value(v) => Some(value_to_toml(v)),
        Item::Table(t) => Some(toml::Value::Table(t.iter().filter_map(|(k, i)| item_to_toml(i).map(|v| (k.to_string(), v))).collect())),
        Item::ArrayOfTables(a) => Some(toml::Value::Array(
            a.iter()
                .map(|t| toml::Value::Table(t.iter().filter_map(|(k, i)| item_to_toml(i).map(|v| (k.to_string(), v))).collect()))
                .collect(),
        )),
    }
}

/// A toml value as a toml_edit value (strings pick their own quoting: paths with `\` become literal strings).
fn to_edit_value(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::from(s.as_str()),
        toml::Value::Integer(i) => Value::from(*i),
        toml::Value::Float(f) => Value::from(*f),
        toml::Value::Boolean(b) => Value::from(*b),
        toml::Value::Datetime(d) => Value::from(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.iter().map(to_edit_value).collect()),
        toml::Value::Table(t) => {
            let mut it = InlineTable::new();
            for (k, v) in t {
                it.insert(k, to_edit_value(v));
            }
            Value::InlineTable(it)
        }
    }
}

/// An args-like array: one line when short, else one `flag value` pair per line.
fn token_array(tokens: &[toml::Value]) -> Value {
    let strs: Vec<String> = tokens.iter().map(|t| t.as_str().map(str::to_string).unwrap_or_else(|| t.to_string())).collect();
    let one_line: usize = strs.iter().map(|s| s.len() + 4).sum();
    let mut arr = Array::new();
    for s in &strs {
        arr.push(s.as_str());
    }
    if one_line <= 110 {
        return Value::Array(arr);
    }
    // Pair a flag with the value that follows it (a token matching ^-?\d is a value, never a flag).
    let is_flag = |s: &str| s.starts_with('-') && !s[1..].starts_with(|c: char| c.is_ascii_digit()) && s.len() > 1;
    let mut i = 0;
    let mut line_start = vec![false; strs.len()];
    while i < strs.len() {
        line_start[i] = true;
        if is_flag(&strs[i]) && i + 1 < strs.len() && !is_flag(&strs[i + 1]) {
            i += 2;
        } else {
            i += 1;
        }
    }
    for (i, v) in arr.iter_mut().enumerate() {
        let prefix = if line_start[i] { "\n  " } else { " " };
        v.decor_mut().set_prefix(prefix);
        v.decor_mut().set_suffix("");
    }
    arr.set_trailing(",\n");
    arr.set_trailing_comma(false);
    Value::Array(arr)
}

// ------------------------------------------------------------------------------------------- presets

/// The preset as a toml table with default-valued fields left out (`adapter` is always written).
fn preset_toml(spec: &PresetCfg) -> Result<toml::Table> {
    let full = toml::Table::try_from(spec).map_err(|e| anyhow!("The preset could not be written: {}", one_line(e)))?;
    let defaults = toml::Table::try_from(PresetCfg::default()).map_err(|e| anyhow!("The preset could not be written: {}", one_line(e)))?;
    Ok(full.into_iter().filter(|(k, v)| k == "adapter" || defaults.get(k) != Some(v)).collect())
}

/// How a new value at `path` (keys below the preset table) is written.
fn build_item(path: &[&str], v: &toml::Value) -> Item {
    match (path, v) {
        (["args"] | ["env_remove"], toml::Value::Array(a)) => Item::Value(token_array(a)),
        (["env"], toml::Value::Table(t)) => {
            let inline = to_edit_value(v);
            if t.len() <= 4 && inline.to_string().len() <= 100 {
                Item::Value(inline)
            } else {
                let mut tbl = Table::new();
                for (k, v) in t {
                    tbl.insert(k, Item::Value(to_edit_value(v)));
                }
                Item::Table(tbl)
            }
        }
        (["params"], toml::Value::Table(t)) => {
            let mut tbl = Table::new();
            tbl.set_implicit(true);
            for (k, v) in t {
                tbl.insert(k, build_item(&["params", k], v));
            }
            Item::Table(tbl)
        }
        (["params", _], toml::Value::Table(t)) => {
            let mut tbl = Table::new();
            for (k, v) in t {
                tbl.insert(k, build_item(&["params", "_", k], v));
            }
            Item::Table(tbl)
        }
        (["params", _, "choices"], toml::Value::Table(t)) => {
            let mut tbl = Table::new();
            tbl.set_dotted(true);
            for (k, v) in t {
                tbl.insert(k, Item::Value(to_edit_value(v)));
            }
            Item::Table(tbl)
        }
        _ => Item::Value(to_edit_value(v)),
    }
}

fn build_preset_table(desired: &toml::Table) -> Table {
    let mut t = Table::new();
    for (k, v) in desired {
        t.insert(k, build_item(&[k.as_str()], v));
    }
    t
}

/// Update `t` in place to hold `desired`: unchanged entries keep their text, changed values keep their
/// comments, sub-tables are merged recursively, missing keys are removed.
fn merge_table(t: &mut Table, desired: &toml::Table, path: &[&str]) {
    let stale: Vec<String> = t.iter().map(|(k, _)| k.to_string()).filter(|k| !desired.contains_key(k)).collect();
    for k in stale {
        t.remove(&k);
    }
    for (k, v) in desired {
        let mut sub: Vec<&str> = path.to_vec();
        sub.push(k.as_str());
        let style_path: Vec<&str> = match sub.as_slice() {
            ["params", _, rest @ ..] => [&["params", "_"][..], rest].concat(),
            other => other.to_vec(),
        };
        match t.get_mut(k) {
            None => {
                t.insert(k, build_item(&style_path, v));
            }
            Some(item) => {
                if item_to_toml(item).as_ref() == Some(v) {
                    continue;
                }
                match (item, v) {
                    (Item::Table(inner), toml::Value::Table(d)) => merge_table(inner, d, &sub),
                    (item @ Item::Value(_), _) => {
                        let decor = item.as_value().map(|o| o.decor().clone());
                        let mut new = match build_item(&style_path, v) {
                            Item::Value(nv) => nv,
                            // Keep an inline value inline (e.g. an env written as { ... }).
                            _ => to_edit_value(v),
                        };
                        if let Some(d) = decor {
                            *new.decor_mut() = d;
                        }
                        *item = Item::Value(new);
                    }
                    (item, _) => *item = build_item(&style_path, v),
                }
            }
        }
    }
}

/// Resolve MASK values (secret env values, secret arg values, param vars that feed a secret, the leading arg of a
/// choice whose `{p.NAME}` follows a secret flag: see `PresetCfg::masked`) against the stored preset `from`.
fn unmask(spec: &mut PresetCfg, cfg: &Config, from: &str) -> Result<()> {
    let src = cfg.presets.get(from);
    let gone = |what: &str| {
        anyhow!("The value of {what} is hidden (••••) and preset \"{from}\" has no stored value to keep; type the value again.")
    };
    fix_env(&mut spec.env, src.map(|s| &s.env), "env", &gone)?;
    fix_args(&mut spec.args, src.map(|s| s.args.as_slice()), "args", &gone)?;
    for (pname, p) in spec.params.iter_mut() {
        for (cname, c) in p.choices.iter_mut() {
            let sc = src.and_then(|s| s.params.get(pname)).and_then(|sp| sp.choices.get(cname));
            let what = format!("params.{pname}.choices.{cname}");
            fix_env(&mut c.env, sc.map(|c| &c.env), &format!("{what}.env"), &gone)?;
            if c.args.first().is_some_and(|a| a == MASK) {
                // The value of a secret flag in the preset args (`"--api-key", "{p.NAME}"`).
                match sc.and_then(|s| s.args.first()).filter(|s| s.as_str() != MASK) {
                    Some(s) => c.args[0] = s.clone(),
                    None => return Err(gone(&format!("{what}.args"))),
                }
            }
            fix_args(&mut c.args, sc.map(|c| c.args.as_slice()), &format!("{what}.args"), &gone)?;
            fix_env(&mut c.vars, sc.map(|c| &c.vars), &format!("{what}.vars"), &gone)?;
        }
    }
    Ok(())
}

fn fix_env(
    env: &mut BTreeMap<String, String>,
    src: Option<&BTreeMap<String, String>>,
    what: &str,
    gone: &dyn Fn(&str) -> anyhow::Error,
) -> Result<()> {
    for (k, v) in env.iter_mut() {
        if v != MASK {
            continue;
        }
        let stored = src.and_then(|s| s.get(k).or_else(|| s.iter().find(|(sk, _)| sk.eq_ignore_ascii_case(k)).map(|(_, v)| v)));
        match stored.filter(|s| s.as_str() != MASK) {
            Some(s) => *v = s.clone(),
            None => return Err(gone(&format!("{what}.{k}"))),
        }
    }
    Ok(())
}

fn fix_args(args: &mut [String], src: Option<&[String]>, what: &str, gone: &dyn Fn(&str) -> anyhow::Error) -> Result<()> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for i in 0..args.len() {
        if args[i] == MASK && i > 0 && is_secret_flag(&args[i - 1]) {
            let flag = args[i - 1].clone();
            let n = seen.entry(flag.clone()).or_default();
            let stored = src.and_then(|s| s.windows(2).filter(|w| w[0] == flag).nth(*n).map(|w| w[1].clone()));
            *n += 1;
            match stored.filter(|s| s != MASK) {
                Some(s) => args[i] = s,
                None => return Err(gone(&format!("{what} ({flag})"))),
            }
        } else if let Some(flag) = args[i].strip_suffix(&format!("={MASK}")).filter(|f| is_secret_flag(f)).map(str::to_string) {
            let n = seen.entry(format!("{flag}=")).or_default();
            let lead = format!("{flag}=");
            let stored = src.and_then(|s| s.iter().filter(|a| a.starts_with(&lead)).nth(*n).cloned());
            *n += 1;
            match stored.filter(|s| !s.ends_with(MASK)) {
                Some(s) => args[i] = s,
                None => return Err(gone(&format!("{what} ({flag})"))),
            }
        } else if args[i].contains(MASK) {
            return Err(gone(what));
        }
    }
    Ok(())
}
