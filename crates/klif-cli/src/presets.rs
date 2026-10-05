//! presets list | show | use | param | save | set | delete.
//! `set` edits the stored preset as the engine shows it (secrets masked; masked values are kept as stored by
//! `klif_catalog::store::upsert_preset`), so a secret never passes through klif-cli unless the caller sets it.

use crate::args::Args;
use crate::cmds::{act, find, human_command};
use crate::conn;
use crate::out::{refused, table, val, CliError, CliResult, Out};
use crate::outputs::*;
use crate::resolve::resolve;
use klif_core::klif_common::config::{validate_preset_id, LoadedConfig, PresetCfg, PRESET_KEYS};
use klif_core::klif_common::vm::{Action, AdapterId, PresetInfo, SystemKind, ViewModel};
use std::collections::BTreeMap;

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let sub = args.pos("presets subcommand (list, show, use, param, save, set, delete)")?;
    match sub.as_str() {
        "list" | "ls" => list(args, loaded, out),
        "show" => show(args, loaded, out),
        "use" => use_preset(args, loaded, out),
        "param" => param(args, loaded, out),
        "save" => save(args, loaded, out),
        "set" => set(args, loaded, out),
        "delete" | "rm" => delete(args, loaded, out),
        other => Err(CliError::usage(format!("Unknown presets subcommand \"{other}\" (list, show, use, param, save, set, delete)."))),
    }
}

fn presets_of<'a>(vm: &'a ViewModel, node: Option<&str>) -> CliResult<&'a [PresetInfo]> {
    match node {
        None => Ok(&vm.presets),
        Some(n) => vm
            .nodes
            .iter()
            .find(|x| x.id == n)
            .map(|x| x.presets.as_slice())
            .ok_or_else(|| CliError::new("not_found", format!("There is no node \"{n}\" ([nodes.{n}] in klif.toml)."))),
    }
}

fn list(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let node = args.opt("--node")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let presets = presets_of(&vm, node.as_deref())?;
    let used_by = |id: &str| -> Vec<String> {
        vm.systems
            .iter()
            .filter(|s| s.node.as_deref() == node.as_deref() && s.preset.as_deref() == Some(id))
            .map(|s| s.id.to_string())
            .collect()
    };
    let doc = val(&PresetsListDoc { node: node.clone(), presets: presets.to_vec() });
    out.doc(doc, || {
        if presets.is_empty() {
            return "No presets yet ([presets.<id>] in klif.toml, or klif-cli presets save / set).".into();
        }
        let mut rows = vec![["ID", "NAME", "ADAPTER", "KIND", "AVAILABILITY", "USED BY", "GPU", "BENCH"].map(String::from).to_vec()];
        for p in presets {
            let bench = p
                .bench
                .as_ref()
                .map(|b| {
                    let main = b.decode_tps.map(|t| format!("{t:.1} tok/s")).or_else(|| b.seconds_per_image.map(|s| format!("{s:.1} s/img"))).unwrap_or_else(|| "ran".into());
                    if b.stale { format!("{main} (stale)") } else { main }
                })
                .unwrap_or_else(|| "-".into());
            let mut avail = val(&p.availability).as_str().unwrap_or("").to_string();
            if p.external {
                avail.push_str(" (external)");
            }
            rows.push(vec![
                p.id.clone(),
                p.name.clone(),
                p.adapter.as_str().into(),
                p.kind.as_str().into(),
                avail,
                used_by(&p.id).join(",").chars().take(40).collect(),
                p.gpu.clone().unwrap_or_else(|| "-".into()),
                bench,
            ]);
        }
        let mut s = table(&rows);
        let reasons: Vec<String> = presets.iter().filter_map(|p| p.reason.as_ref().map(|r| format!("  {}: {r}", p.id))).collect();
        if !reasons.is_empty() {
            s.push('\n');
            s.push_str(&reasons.join("\n"));
            s.push('\n');
        }
        s
    });
    Ok(())
}

/// A preset as a `[presets.<id>]` TOML block.
fn preset_toml(id: &str, spec: &PresetCfg) -> String {
    let mut inner = BTreeMap::new();
    inner.insert(id.to_string(), spec.clone());
    let mut outer = BTreeMap::new();
    outer.insert("presets".to_string(), inner);
    toml::to_string(&outer).unwrap_or_else(|e| format!("# could not render: {e}\n"))
}

fn show(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let mut node = args.opt("--node")?;
    let mut id = args.pos("preset id")?;
    args.done()?;
    // "<node>/<id>", as a remote System's id is written, means --node <node>.
    if node.is_none() {
        if let Some((n, p)) = id.split_once('/').map(|(n, p)| (n.to_string(), p.to_string())) {
            node = Some(n);
            id = p;
        }
    }
    let conn = conn::connect(loaded)?;
    let detail = conn.link().preset(&id, node.as_deref()).map_err(refused)?;
    let Some(detail) = detail else {
        let mut msg = format!("There is no preset \"{id}\"{}.", node.as_deref().map(|n| format!(" on node {n}")).unwrap_or_else(|| " on this machine".into()));
        // A preset another machine has: say how to ask for it (a remote System's preset lives on its node).
        if node.is_none() {
            let vm = conn.snapshot(None)?;
            let on: Vec<&str> = vm.nodes.iter().filter(|n| n.presets.iter().any(|p| p.id == id)).map(|n| n.id.as_str()).collect();
            if let Some(n) = on.first() {
                msg.push_str(&format!(" Node {} has it: klif-cli presets show {id} --node {n}", on.join(", ")));
            }
        }
        return Err(CliError::new("not_found", msg));
    };
    out.doc(val(&PresetsShowDoc { node: node.clone(), preset: detail.clone() }), || {
        let i = &detail.info;
        let mut s = format!(
            "{} ({}) · {} · {} · {}{}\n",
            i.name,
            detail.id,
            i.adapter.as_str(),
            i.kind.as_str(),
            val(&i.availability).as_str().unwrap_or(""),
            i.reason.as_deref().map(|r| format!(": {r}")).unwrap_or_default()
        );
        if let Some(b) = &i.bench {
            s.push_str(&format!(
                "bench: {} runs, decode {} tok/s, prefill {} tok/s, ttft {} s, {} s/img{}\n",
                b.runs,
                crate::out::num(b.decode_tps, 1),
                crate::out::num(b.prefill_tps, 1),
                crate::out::num(b.ttft_s, 2),
                crate::out::num(b.seconds_per_image, 2),
                if b.stale { " (stale: the command changed since)" } else { "" }
            ));
        }
        s.push('\n');
        s.push_str(&preset_toml(&detail.id, &detail.spec));
        s.push('\n');
        s.push_str(&human_command(&detail.command));
        s
    });
    Ok(())
}

fn use_preset(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let sys = args.pos("System")?;
    let preset = args.pos("preset id")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let id = resolve(&vm, &sys)?;
    act(&conn, Action::UsePreset { system: id.clone(), preset: preset.clone() })?;
    let vm = conn.snapshot(None)?;
    let s = find(&vm, &id).cloned();
    out.doc(val(&PresetsUseDoc { system: s.as_ref().map(SysBrief::of) }), || format!("{id} now uses preset {preset} (applies on the next launch)."));
    Ok(())
}

fn param(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let sys = args.pos("System")?;
    let name = args.pos("param name")?;
    let value = args.pos("param value")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let id = resolve(&vm, &sys)?;
    let previous = find(&vm, &id).and_then(|s| s.params.iter().find(|p| p.name == name).map(|p| p.value.clone()));
    act(&conn, Action::SetParam { system: id.clone(), name: name.clone(), value: value.clone() })?;
    let vm = conn.snapshot(None)?;
    let sys = find(&vm, &id);
    let params = sys.map(|s| s.params.clone()).unwrap_or_default();
    let running = sys.is_some_and(|s| s.session.is_some());
    let applies = if running { "restart" } else { "next launch" };
    let doc = PresetsParamDoc { system: id.clone(), name: name.clone(), previous: previous.clone(), value: value.clone(), applies: applies.into(), params };
    out.doc(val(&doc), || {
        let was = previous.as_deref().map(|p| format!("{p} -> ")).unwrap_or_default();
        let when = if running { "when it restarts" } else { "on the next launch" };
        format!("{id}: {name} {was}{value} (applies {when}).")
    });
    Ok(())
}

/// A preset from a TOML file: either a `[presets.<id>]` table (the id must match, or be the only one) or the
/// preset's keys at the top level.
fn parse_preset_file(text: &str, id: &str) -> CliResult<(PresetCfg, Vec<String>)> {
    let text = text.trim_start_matches('\u{feff}');
    // Never echo the source line or a quoted value: a secret typed into the wrong field would land in the output.
    let table: toml::Table = toml::from_str(text).map_err(|e| {
        let at = e
            .span()
            .map(|s| {
                let (line, col) = klif_core::klif_common::config::line_col(text, s.start);
                format!(" (line {line}, column {col})")
            })
            .unwrap_or_default();
        CliError::new("invalid", format!("The file does not parse as TOML{at}: {}", klif_core::klif_common::config::scrub(e.message()).trim()))
    })?;
    let body = match table.get("presets") {
        Some(toml::Value::Table(presets)) => match presets.get(id) {
            Some(toml::Value::Table(t)) => t.clone(),
            Some(_) => return Err(CliError::new("invalid", format!("[presets.{id}] must be a table."))),
            None if presets.len() == 1 => match presets.values().next() {
                Some(toml::Value::Table(t)) => t.clone(),
                _ => return Err(CliError::new("invalid", "The [presets.*] entry must be a table.")),
            },
            None => return Err(CliError::new("invalid", format!("The file has several [presets.*] tables but none named \"{id}\"."))),
        },
        _ => table,
    };
    let unknown: Vec<String> = body.keys().filter(|k| !PRESET_KEYS.contains(&k.as_str())).map(|k| format!("unknown key \"{k}\" is ignored")).collect();
    let spec = toml::Value::Table(body)
        .try_into::<PresetCfg>()
        .map_err(|e| CliError::new("invalid", format!("The preset could not be read: {}", klif_core::klif_common::config::scrub(e).trim())))?;
    Ok((spec, unknown))
}

fn save(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let file = args.opt("--file")?.ok_or_else(|| CliError::usage("presets save needs --file <preset.toml>."))?;
    let use_for = args.opt("--use")?;
    let node = args.opt("--node")?;
    let id = args.pos("preset id")?;
    args.done()?;
    validate_preset_id(&id).map_err(CliError::usage)?;
    let text = std::fs::read_to_string(&file).map_err(|e| CliError::new("io", format!("{file} could not be read: {e}")))?;
    let (spec, warnings) = parse_preset_file(&text, &id)?;
    for w in &warnings {
        crate::out::note(format!("warning: {w}"));
    }
    let conn = conn::connect(loaded)?;
    let select_for = match &use_for {
        None => None,
        Some(s) => Some(resolve(&conn.snapshot(None)?, s)?),
    };
    act(&conn, Action::SavePreset { id: id.clone(), preset: spec, select_for: select_for.clone(), secrets_from: None, base_hash: None, node: node.clone() })?;
    finish_saved(&conn, out, &id, node.as_deref(), warnings, select_for.map(|s| s.to_string()))
}

fn finish_saved(conn: &conn::Conn, out: Out, id: &str, node: Option<&str>, warnings: Vec<String>, used_for: Option<String>) -> CliResult {
    // The write succeeded; a failed read-back only loses the summary below.
    let detail = match conn.link().preset(id, node) {
        Ok(d) => d,
        Err(e) => {
            crate::out::note(format!("warning: the saved preset could not be read back: {e:#}"));
            None
        }
    };
    let issues = detail.as_ref().map(|d| d.command.issues.clone()).unwrap_or_default();
    let doc = PresetsSaveDoc { saved: id.to_string(), node: node.map(str::to_string), used_for: used_for.clone(), warnings, preset: detail };
    out.doc(val(&doc), || {
        let mut s = format!("Saved preset {id}{}.", used_for.as_deref().map(|u| format!(" and selected it for {u}")).unwrap_or_default());
        for i in &issues {
            s.push_str(&format!("\n{}: {}", if i.is_error() { "error" } else { "warn" }, i.text));
        }
        s
    });
    Ok(())
}

fn parse_bool(key: &str, v: &str) -> CliResult<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        _ => Err(CliError::usage(format!("{key} must be true or false, not \"{v}\"."))),
    }
}

fn opt_str(v: &str) -> Option<String> {
    let t = v.trim();
    (!t.is_empty()).then(|| v.to_string())
}

fn json_list(key: &str, v: &str) -> CliResult<Vec<String>> {
    serde_json::from_str::<Vec<String>>(v.trim()).map_err(|_| {
        CliError::usage(format!("{key}= takes a JSON array of strings, e.g. {key}='[\"-m\",\"{{model}}\"]'; use {key}+=TOKEN to append one token."))
    })
}

/// Apply one `key=value` / `key+=value` / `key-=value` / `env.NAME-` to a preset.
fn apply(spec: &mut PresetCfg, kv: &str) -> CliResult {
    if !kv.contains('=') {
        if let Some(name) = kv.strip_prefix("env.").and_then(|r| r.strip_suffix('-')) {
            if spec.env.remove(name).is_none() {
                return Err(CliError::usage(format!("env.{name} is not set in this preset.")));
            }
            return Ok(());
        }
        return Err(CliError::usage(format!("Expected key=value, not \"{kv}\".")));
    }
    let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
    let (key, op) = if let Some(k) = k.strip_suffix('+') {
        (k.trim(), '+')
    } else if let Some(k) = k.strip_suffix('-') {
        (k.trim(), '-')
    } else {
        (k.trim(), '=')
    };
    let list_op = |list: &mut Vec<String>, name: &str| -> CliResult {
        match op {
            '+' => list.push(v.to_string()),
            '-' => {
                let before = list.len();
                list.retain(|x| x != v);
                if list.len() == before {
                    return Err(CliError::usage(format!("\"{v}\" is not in {name}.")));
                }
            }
            _ => *list = json_list(name, v)?,
        }
        Ok(())
    };
    if op != '=' && !matches!(key, "args" | "env_remove") {
        return Err(CliError::usage(format!("{key} does not take += / -= (only args and env_remove do).")));
    }
    if let Some(name) = key.strip_prefix("env.") {
        if name.is_empty() {
            return Err(CliError::usage("env.NAME needs a variable name."));
        }
        spec.env.insert(name.to_string(), v.to_string());
        return Ok(());
    }
    match key {
        "args" => list_op(&mut spec.args, "args")?,
        "env_remove" => list_op(&mut spec.env_remove, "env_remove")?,
        "command" => spec.command = v.trim().to_string(),
        "cwd" => spec.cwd = opt_str(v),
        "host" => spec.host = opt_str(v),
        "endpoint" => spec.endpoint = opt_str(v),
        "health" => spec.health = opt_str(v),
        "model" => spec.model = opt_str(v),
        "mmproj" => spec.mmproj = opt_str(v),
        "gpu" => spec.gpu = opt_str(v),
        "name" => spec.name = opt_str(v),
        "model_name" => spec.model_name = opt_str(v),
        "quant" => spec.quant = opt_str(v),
        "backend" => spec.backend = opt_str(v),
        "device" => spec.device = opt_str(v),
        "notes" => spec.notes = opt_str(v),
        "port" => {
            spec.port = match v.trim() {
                "" => None,
                p => Some(p.parse::<u16>().map_err(|_| CliError::usage(format!("port must be 1-65535, not \"{p}\".")))?),
            }
        }
        "ctx" => {
            spec.ctx = match v.trim() {
                "" => None,
                c => Some(c.parse::<u32>().map_err(|_| CliError::usage(format!("ctx must be a number of tokens, not \"{c}\".")))?),
            }
        }
        "kind" => {
            spec.kind = match v.trim() {
                "" => None,
                k => Some(SystemKind::parse(k).ok_or_else(|| CliError::usage(format!("kind must be llm, image, tts, stt, video or music, not \"{k}\".")))?),
            }
        }
        "adapter" => {
            spec.adapter = AdapterId::parse(v)
                .ok_or_else(|| CliError::usage(format!("adapter must be llama.cpp, sd.cpp, vllm, openai, audiocpp or generic, not \"{v}\".")))?
        }
        "managed" => spec.managed = parse_bool(key, v)?,
        "api_key" => spec.api_key = parse_bool(key, v)?,
        "params" | "recommended" => {
            return Err(CliError::usage(format!("{key} cannot be set here; edit klif.toml or use presets save --file.")));
        }
        other => return Err(CliError::usage(format!("Unknown preset key \"{other}\" (klif-cli --help lists them)."))),
    }
    Ok(())
}

fn set(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let node = args.opt("--node")?;
    let id = args.pos("preset id")?;
    let kvs = args.rest();
    args.done()?;
    if kvs.is_empty() {
        return Err(CliError::usage("presets set needs at least one key=value."));
    }
    validate_preset_id(&id).map_err(CliError::usage)?;
    if node.is_none() {
        if let Some(err) = loaded.cfg.bad_presets.get(&id) {
            return Err(CliError::new(
                "invalid",
                format!("[presets.{id}] does not parse ({err}); fix it in klif.toml or replace it with presets save {id} --file."),
            ));
        }
    }
    let conn = conn::connect(loaded)?;
    let existing = conn.link().preset(&id, node.as_deref()).map_err(refused)?;
    let created = existing.is_none();
    // Refuse the write if the preset changes on disk meanwhile (Tune or another agent saving at the same time).
    let base_hash = existing.as_ref().map(|d| d.spec_hash.clone()).filter(|h| !h.is_empty());
    let mut spec = existing.map(|d| d.spec).unwrap_or_default();
    for kv in &kvs {
        apply(&mut spec, kv)?;
    }
    act(&conn, Action::SavePreset { id: id.clone(), preset: spec, select_for: None, secrets_from: None, base_hash, node: node.clone() })?;
    let warnings = if created { vec![format!("preset {id} did not exist; it was created")] } else { Vec::new() };
    finish_saved(&conn, out, &id, node.as_deref(), warnings, None)
}

fn delete(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let yes = args.flag("--yes");
    let node = args.opt("--node")?;
    let id = args.pos("preset id")?;
    args.done()?;
    if !yes {
        return Err(CliError::needs_yes(format!("presets delete removes [presets.{id}] from klif.toml: add --yes.")));
    }
    let conn = conn::connect(loaded)?;
    act(&conn, Action::DeletePreset { id: id.clone(), node: node.clone() })?;
    out.doc(val(&PresetsDeleteDoc { deleted: id.clone(), node: node.clone() }), || format!("Deleted preset {id}."));
    Ok(())
}

