//! models list | download | adopt. Downloads run in the engine (`klif_core::download`), only from
//! huggingface.co and only with `--yes`; klif-cli follows the progress until every file is done or failed.

use crate::args::Args;
use crate::cmds::{act, find};
use crate::conn;
use crate::out::{gib, json_line, note, table, val, CliError, CliResult, Out};
use crate::outputs::{DownloadEvent, ModelsAdoptDoc, ModelsDownloadDoc, ModelsListDoc, SysBrief};
use crate::resolve::resolve;
use klif_core::klif_catalog::Catalog;
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::now_s;
use klif_core::klif_common::vm::{Action, DownloadInfo, DownloadState, RecommendationInfo, SystemKind};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

/// Shown with every recommendation list (SPEC 9).
pub const DISCLAIMER: &str = "Recommendations are starting points; their memory numbers are estimates. Models, drivers and \
backends change: your agent should benchmark and calibrate (klif-cli bench <system>). Each model keeps its own license. KLIF \
downloads only from huggingface.co, and only when you ask.";

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
        val(&ModelsListDoc {
            recommendations: recs.clone(),
            models_dir: models_dir.as_ref().map(|p| p.display().to_string()),
            disclaimer: DISCLAIMER.to_string(),
        }),
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

/// One JSON line of a download's progress on stderr (`--json`; stdout stays the one final document).
fn progress_line(ev: &DownloadEvent) {
    eprintln!("{}", json_line(val(ev)));
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
    let from = sources_of(&rec);
    let into = vm.config.models_dir.clone().unwrap_or_default();
    if out.json {
        progress_line(&DownloadEvent::Start { at: now_s(), id: id.clone(), name: rec.name.clone(), source: from.clone(), into: into.clone(), total_bytes: total });
    } else {
        note(format!("downloading {} from {from} into {into}", rec.name));
    }
    act(&conn, Action::DownloadRecommendation { id: id.clone(), node: None })?;
    if conn.mode() == "in-process" {
        let msg = "klif-cli runs the engine: keep it open until the download ends; an interrupted download resumes next time";
        if out.json {
            progress_line(&DownloadEvent::Note { at: now_s(), message: msg.into() });
        } else {
            note(format!("  ({msg})"));
        }
    }
    // What the JSON lines told last per file: state, bytes, when.
    let mut told: BTreeMap<String, (DownloadState, u64, Instant)> = BTreeMap::new();
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
        if out.json {
            for d in now.values() {
                let tell = match told.get(&d.file) {
                    None => true,
                    Some((st, done, at)) => *st != d.state || (*done != d.done_bytes && at.elapsed() >= Duration::from_secs(1)),
                };
                if tell {
                    told.insert(d.file.clone(), (d.state, d.done_bytes, Instant::now()));
                    progress_line(&DownloadEvent::Progress {
                        at: now_s(),
                        id: id.clone(),
                        file: d.file.clone(),
                        done_bytes: d.done_bytes,
                        total_bytes: d.total_bytes,
                        state: d.state,
                        error: d.error.clone(),
                    });
                }
            }
        }
        let all_terminal = files.iter().all(|f| now.get(f).is_some_and(|d| terminal(d.state)));
        // An unchanged terminal state from an earlier attempt only counts once nothing moved for a while.
        if all_terminal && (fresh || last_change.elapsed() > Duration::from_secs(10)) {
            break now;
        }
        if !out.json && last_note.elapsed() >= Duration::from_secs(2) {
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
    out.doc(val(&ModelsDownloadDoc { downloaded: id.clone(), files: list.clone() }), || {
        let mut s = format!("Downloaded {} ({} file(s)).", rec.name, list.len());
        s.push_str(&format!("\nNext: klif-cli models adopt {id} --system <system>"));
        s
    });
    Ok(())
}

/// `models adopt <id> [--system S] [--ctx N] [--kv TYPE]`: without --ctx / --kv the engine takes them from this
/// machine's suggestion of that recommendation (when it is one), else the recommendation's defaults.
fn adopt(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let system = args.opt("--system")?;
    let ctx = match args.opt("--ctx")? {
        None => None,
        Some(v) => Some(
            v.trim()
                .parse::<u32>()
                .ok()
                .filter(|c| *c >= 256)
                .ok_or_else(|| CliError::usage(format!("--ctx needs a number of tokens (256 or more), not \"{v}\".")))?,
        ),
    };
    let kv = args.opt("--kv")?.map(|k| k.trim().to_ascii_lowercase());
    if let Some(k) = &kv {
        if !klif_core::klif_catalog::suggest::KV_TYPES.contains(&k.as_str()) {
            return Err(CliError::usage(format!(
                "--kv \"{k}\" is not a KV cache type ({}).",
                klif_core::klif_catalog::suggest::KV_TYPES.join(", ")
            )));
        }
    }
    let id = args.pos("recommendation id")?;
    args.done()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let sys = match &system {
        Some(s) => Some(resolve(&vm, s)?),
        None => None,
    };
    let before: BTreeSet<String> = vm.presets.iter().map(|p| p.id.clone()).collect();
    act(&conn, Action::AdoptRecommendation { id: id.clone(), system: sys.clone(), ctx, kv })?;
    let vm = conn.snapshot(None)?;
    let new: Vec<String> = vm.presets.iter().map(|p| p.id.clone()).filter(|p| !before.contains(p)).collect();
    let s = sys.as_ref().and_then(|id| find(&vm, id)).cloned();
    out.doc(val(&ModelsAdoptDoc { adopted: id.clone(), presets: new.clone(), system: s.as_ref().map(SysBrief::of) }), || {
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
