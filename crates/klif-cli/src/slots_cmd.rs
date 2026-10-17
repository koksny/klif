//! `slots <system>`, `slots save|load <system> [1-3]`, `slots use <system> <1-3>`, `slots rename <system> <1-3> <name>`,
//! `slots clear <system> <1-3> --yes`: a System's conversation
//! slots on disk (`[systems.<id>] slots`, `systems slots <system> off|manual|auto`). Save and load run in the engine;
//! the command waits until they are done and prints how they ended.

use std::time::{Duration, Instant};

use crate::args::Args;
use crate::cmds::act;
use crate::conn;
use crate::out::{date, gib, table, val, CliError, CliResult, Out};
use crate::outputs::{SlotsDoc, SysBrief};
use crate::resolve::resolve_system;
use klif_core::klif_common::config::{LoadedConfig, SLOT_COUNT};
use klif_core::klif_common::now_s;
use klif_core::klif_common::vm::{Action, SlotsView, System};

/// How long `slots save|load` waits for the engine (a large conversation takes a while to write).
const WAIT: Duration = Duration::from_secs(30 * 60);

fn slot_num(s: &str) -> CliResult<u8> {
    match s.trim().parse::<u8>() {
        Ok(n) if (1..=SLOT_COUNT).contains(&n) => Ok(n),
        _ => Err(CliError::usage(format!("\"{s}\" is not a slot; a System has slots 1 to {SLOT_COUNT}."))),
    }
}

fn need_slots(s: &System) -> CliResult<&SlotsView> {
    s.slots
        .as_ref()
        .ok_or_else(|| CliError::new("refused", format!("{} has slots off; turn them on with `klif-cli systems slots {} on`.", s.label, s.id)))
}

fn doc(s: &System, result: Option<String>) -> SlotsDoc {
    SlotsDoc { system: SysBrief::of(s), slots: s.slots.clone(), result }
}

fn human(s: &System, result: Option<&str>) -> String {
    let Some(v) = &s.slots else { return format!("{} ({}): slots off", s.label, s.id) };
    let mut rows = vec![vec!["slot".into(), "name".into(), "saved".into(), "tokens".into(), "size".into(), "model".into(), "".into()]];
    for e in &v.entries {
        let mark = if e.n == v.active { "*" } else { "" };
        let name = e.name.clone().unwrap_or_default();
        match e.saved_at {
            Some(t) => rows.push(vec![
                format!("{}{mark}", e.n),
                name,
                date(t),
                e.tokens.map(|x| x.to_string()).unwrap_or_default(),
                e.bytes.map(gib).unwrap_or_default(),
                e.model.clone().unwrap_or_default(),
                if e.compatible { "".into() } else { "another command".into() },
            ]),
            None => rows.push(vec![format!("{}{mark}", e.n), name, "empty".into(), "".into(), "".into(), "".into(), "".into()]),
        }
    }
    let mut text = format!(
        "{} ({}): slots {}{}\n{}",
        s.label,
        s.id,
        crate::cmds::slots_word(Some(v)),
        if v.supported { "" } else { " (its server cannot save slots)" },
        table(&rows)
    );
    if let Some(r) = result {
        text.push_str(&format!("\n{r}"));
    }
    text
}

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let yes = args.flag("--yes");
    let first = args.pos("System, or save | load | clear")?;
    let (verb, target, n) = match first.as_str() {
        "rename" => {
            let target = args.pos("System")?;
            let n = slot_num(&args.pos("slot (1-3)")?)?;
            let name = args.rest().join(" ");
            args.done()?;
            let conn = conn::connect(loaded)?;
            let vm = conn.snapshot(None)?;
            let sys = resolve_system(&vm, &target)?.clone();
            need_slots(&sys)?;
            act(&conn, Action::RenameSlot { system: sys.id.clone(), slot: n, name })?;
            std::thread::sleep(Duration::from_millis(700));
            let vm = conn.snapshot(None)?;
            let s = resolve_system(&vm, sys.id.as_str())?.clone();
            let result = s.slots.as_ref().and_then(|v| v.notice.as_ref()).map(|x| x.text.clone());
            out.doc(val(&doc(&s, result.clone())), || human(&s, result.as_deref()));
            return Ok(());
        }
        "save" | "load" | "clear" | "use" => {
            let target = args.pos("System")?;
            let n = match args.next_pos() {
                Some(n) => Some(slot_num(&n)?),
                None if first == "clear" => return Err(CliError::usage("slots clear needs the slot: slots clear <system> <1-3> --yes.")),
                None if first == "use" => return Err(CliError::usage("slots use needs the slot: slots use <system> <1-3>.")),
                None => None,
            };
            (first, target, n)
        }
        _ => ("show".to_string(), first, None),
    };
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let sys = resolve_system(&vm, &target)?.clone();
    if verb == "show" {
        out.doc(val(&doc(&sys, None)), || human(&sys, None));
        return Ok(());
    }
    let v = need_slots(&sys)?;
    let n = n.unwrap_or(v.active);
    let id = sys.id.clone();
    if verb == "clear" || verb == "use" {
        if verb == "clear" && !yes {
            return Err(CliError::needs_yes(format!("slots clear deletes slot {n} of {}: add --yes.", sys.label)));
        }
        let action = if verb == "use" { Action::UseSlot { system: id.clone(), slot: n } } else { Action::ClearSlot { system: id.clone(), slot: n } };
        act(&conn, action)?;
        std::thread::sleep(Duration::from_millis(700));
        let vm = conn.snapshot(None)?;
        let s = resolve_system(&vm, id.as_str())?.clone();
        let result = s.slots.as_ref().and_then(|v| v.notice.as_ref()).map(|x| x.text.clone());
        out.doc(val(&doc(&s, result.clone())), || human(&s, result.as_deref()));
        return Ok(());
    }
    let started = now_s() - 1.0;
    let action = if verb == "save" { Action::SaveSlot { system: id.clone(), slot: n } } else { Action::LoadSlot { system: id.clone(), slot: n } };
    act(&conn, action)?;
    let deadline = Instant::now() + WAIT;
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let vm = conn.snapshot(None)?;
        let s = resolve_system(&vm, id.as_str())?.clone();
        let Some(v) = &s.slots else { return Err(CliError::new("refused", format!("{} has slots off now.", s.label))) };
        if v.job.is_none() {
            if let Some(notice) = v.notice.as_ref().filter(|x| x.at >= started) {
                if !notice.ok {
                    return Err(CliError::new("refused", notice.text.clone()));
                }
                let result = Some(notice.text.clone());
                out.doc(val(&doc(&s, result.clone())), || human(&s, result.as_deref()));
                return Ok(());
            }
        }
        if Instant::now() > deadline {
            return Err(CliError::new("timeout", format!("The {verb} of slot {n} of {} did not end in {} minutes.", s.label, WAIT.as_secs() / 60)));
        }
    }
}
