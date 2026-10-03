//! models list | download | adopt. Downloads run in the engine (`klif_core::download`), only from
//! huggingface.co and only with `--yes`; klif-cli follows the progress until every file is done or failed.

use crate::args::Args;
use crate::cmds::{act, find, sys_value};
use crate::conn;
use crate::out::{gib, note, table, val, CliError, CliResult, Out};
use crate::resolve::resolve;
use klif_core::klif_catalog::Catalog;
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::{Action, DownloadInfo, DownloadState, RecommendationInfo, SystemKind};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

/// Shown with every recommendation list (SPEC 9).
pub const DISCLAIMER: &str = "Recommendations are starting points measured on one machine. Models, drivers and backends change: \
your agent should benchmark and calibrate (klif-cli bench <system>). Each model keeps its own license. KLIF downloads only \
from huggingface.co, and only when you ask.";

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let sub = args.pos("models subcommand (list, download, adopt)")?;
    match sub.as_str() {
        "list" | "ls" => list(args, loaded, out),
        "download" => download(args, loaded, out),
        "adopt" => adopt(args, loaded, out),
        other => Err(CliError::usage(format!("Unknown models subcommand \"{other}\" (list, download, adopt)."))),
    }
}

/// "huggingface.co/org/name" for every repo the recommendation's files come from (a file may name its own repo).
fn sources_of(r: &RecommendationInfo) -> String {
    let mut repos: Vec<String> = vec![r.hf_repo.clone()];
    if let Some(rec) = klif_core::klif_catalog::recommend::embedded().iter().find(|x| x.id == r.id) {
        repos = Vec::new();
        for f in &rec.files {
            let repo = rec.repo_of(f).to_string();
            if !repos.contains(&repo) {
                repos.push(repo);
            }
        }
    }
    repos.iter().map(|r| format!("huggingface.co/{r}")).collect::<Vec<_>>().join(" and ")
}

fn size_of(r: &RecommendationInfo) -> Option<u64> {
    let sizes: Vec<u64> = r.files.iter().filter_map(|f| f.size_bytes).collect();
    (!sizes.is_empty()).then(|| sizes.iter().sum())
}

fn list(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let kind = match args.opt("--kind")? {
        None => None,
        Some(k) => Some(SystemKind::parse(&k).ok_or_else(|| CliError::usage(format!("Unknown kind \"{k}\" (llm, image, tts, stt, video).")))?),
    };
    args.done()?;
    let cfg = &loaded.cfg;
    let recs: Vec<RecommendationInfo> = Catalog::new(cfg).recommendations(cfg).into_iter().filter(|r| kind.is_none_or(|k| r.kind == k)).collect();
    let models_dir = klif_core::klif_catalog::recommend::models_dir(cfg);
    out.doc(
        json!({ "recommendations": recs, "modelsDir": models_dir.as_ref().map(|p| p.display().to_string()), "disclaimer": DISCLAIMER }),
        || {
            let mut s = String::new();
            if recs.is_empty() {
                s.push_str("No recommendations");
                s.push_str(&kind.map(|k| format!(" for {} Systems", k.label())).unwrap_or_default());
                s.push_str(".\n");
            } else {
                let mut rows = vec![["ID", "KIND", "CLASS", "NAME", "QUANT", "SIZE", "LICENSE", "MEASURED", "STATE"].map(String::from).to_vec()];
                for r in &recs {
                    let measured = r
                        .measured
                        .as_ref()
                        .and_then(|m| m.decode_tps.map(|t| format!("{t:.0} tok/s")).or_else(|| m.seconds_per_image.map(|x| format!("{x:.1} s/img"))))
                        .unwrap_or_else(|| "-".into());
                    let state = match (&r.existing, r.installed) {
                        (Some(p), _) => format!("used by preset {p}"),
                        (None, true) => "installed".into(),
                        (None, false) => "-".into(),
                    };
                    rows.push(vec![
                        r.id.clone(),
                        r.kind.as_str().into(),
                        r.class.map(|c| c.as_str().to_string()).unwrap_or_else(|| "-".into()),
                        r.name.clone(),
                        r.quant.clone(),
                        size_of(r).map(gib).unwrap_or_else(|| "-".into()),
                        r.license.clone(),
                        measured,
                        state,
                    ]);
                }
                s.push_str(&table(&rows));
            }
            s.push_str(&format!(
                "\nmodels_dir: {}\n\n{DISCLAIMER}\n",
                models_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "not set (downloads are off until [paths] models_dir is set)".into())
            ));
            s
        },
    );
    Ok(())
}

fn terminal(s: DownloadState) -> bool {
    matches!(s, DownloadState::Done | DownloadState::Failed | DownloadState::Cancelled)
}

fn download(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let yes = args.flag("--yes");
    let id = args.pos("recommendation id")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let rec = vm
        .recommendations
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or_else(|| CliError::new("not_found", format!("There is no recommendation \"{id}\" (klif-cli models list).")))?;
    let total = size_of(&rec);
    if !yes {
        return Err(CliError::needs_yes(format!(
            "models download fetches {} from {} ({}): add --yes.",
            rec.name,
            sources_of(&rec),
            total.map(gib).unwrap_or_else(|| "size unknown".into())
        )));
    }
    if vm.config.models_dir.is_none() {
        return Err(CliError::new("refused", "Downloads are off until [paths] models_dir is set in klif.toml (the folder KLIF downloads models to)."));
    }
    let files: BTreeSet<String> = rec.files.iter().map(|f| f.name.clone()).collect();
    let before: BTreeMap<String, DownloadInfo> = vm.downloads.iter().filter(|d| d.id == id).map(|d| (d.file.clone(), d.clone())).collect();
    note(format!("downloading {} from {} into {}", rec.name, sources_of(&rec), vm.config.models_dir.clone().unwrap_or_default()));
    act(&conn, Action::DownloadRecommendation { id: id.clone(), node: None })?;
    if conn.mode() == "in-process" {
        note("  (klif-cli runs the engine: keep it open until the download ends; an interrupted download resumes next time)");
    }
    let t0 = Instant::now();
    let mut last_note = Instant::now();
    let mut last_change = Instant::now();
    let mut seen: BTreeMap<String, DownloadInfo> = before.clone();
    let mut fresh = false;
    let finals = loop {
        std::thread::sleep(Duration::from_millis(500));
        let vm = conn.snapshot(None)?;
        let now: BTreeMap<String, DownloadInfo> = vm.downloads.iter().filter(|d| d.id == id).map(|d| (d.file.clone(), d.clone())).collect();
        if now != seen {
            last_change = Instant::now();
            seen = now.clone();
        }
        if now.values().any(|d| !terminal(d.state)) || now != before {
            fresh = true;
        }
        let all_terminal = files.iter().all(|f| now.get(f).is_some_and(|d| terminal(d.state)));
        // An unchanged terminal state from an earlier attempt only counts once nothing moved for a while.
        if all_terminal && (fresh || last_change.elapsed() > Duration::from_secs(10)) {
            break now;
        }
        if last_note.elapsed() >= Duration::from_secs(2) {
            last_note = Instant::now();
            let done: u64 = now.values().map(|d| d.done_bytes).sum();
            let tot: u64 = now.values().filter_map(|d| d.total_bytes).sum::<u64>().max(total.unwrap_or(0));
            let speed = done as f64 / t0.elapsed().as_secs_f64().max(0.1) / (1024.0 * 1024.0);
            let current = now.values().find(|d| !terminal(d.state)).map(|d| format!(" {} {}", d.file, val(&d.state).as_str().unwrap_or(""))).unwrap_or_default();
            if tot > 0 {
                note(format!("  {:.1}% {} / {}{current} (~{speed:.1} MiB/s)", done as f64 * 100.0 / tot as f64, gib(done), gib(tot)));
            } else {
                note(format!("  {}{current}", gib(done)));
            }
        }
    };
    let list: Vec<DownloadInfo> = finals.values().cloned().collect();
    let failed: Vec<&DownloadInfo> = list.iter().filter(|d| d.state != DownloadState::Done).collect();
    if let Some(f) = failed.first() {
        let msg = f.error.clone().unwrap_or_else(|| format!("{} was {}", f.file, val(&f.state).as_str().unwrap_or("not finished")));
        return Err(CliError::new(if f.state == DownloadState::Cancelled { "cancelled" } else { "download" }, msg));
    }
    out.doc(json!({ "downloaded": id, "files": list }), || {
        let mut s = format!("Downloaded {} ({} file(s)).", rec.name, list.len());
        s.push_str(&format!("\nNext: klif-cli models adopt {id} --system <system>"));
        s
    });
    Ok(())
}

fn adopt(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let system = args.opt("--system")?;
    let id = args.pos("recommendation id")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let sys = match &system {
        Some(s) => Some(resolve(&vm, s)?),
        None => None,
    };
    let before: BTreeSet<String> = vm.presets.iter().map(|p| p.id.clone()).collect();
    act(&conn, Action::AdoptRecommendation { id: id.clone(), system: sys.clone() })?;
    let vm = conn.snapshot(None)?;
    let new: Vec<String> = vm.presets.iter().map(|p| p.id.clone()).filter(|p| !before.contains(p)).collect();
    let s = sys.as_ref().and_then(|id| find(&vm, id)).cloned();
    out.doc(json!({ "adopted": id, "presets": new, "system": s.as_ref().map(sys_value) }), || {
        let mut text = match new.first() {
            Some(p) => format!("Created preset {p} from {id}."),
            None => format!("Adopted {id}."),
        };
        if let Some(s) = &s {
            text.push_str(&format!(" {} uses preset {}.", s.label, s.preset.as_deref().unwrap_or("-")));
        }
        text
    });
    Ok(())
}
