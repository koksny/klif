//! `records [--kind K] [--metric M] [--backend B] [--node N]`: the best values per model file and backend, this
//! machine and every online node (`ViewModel.records`); `records forget <key> --yes` removes a junk entry. The store
//! and the counting rules are `klif_core::records`.

use crate::args::Args;
use crate::cmds::act;
use crate::conn;
use crate::out::{date, table, CliError, CliResult, Out};
use crate::outputs::{RecordsDoc, RecordsForgetDoc, RecordsHistoryDoc};
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::{Action, RecordEntry, RecordMetric, RecordValue, SystemKind, ViewModel};

/// Every metric, in display order.
const METRICS: [RecordMetric; 8] = [
    RecordMetric::DecodeTps,
    RecordMetric::PrefillTps,
    RecordMetric::TtftS,
    RecordMetric::ImageS,
    RecordMetric::TtsRtf,
    RecordMetric::SttRtf,
    RecordMetric::VideoS,
    RecordMetric::MusicRtf,
];

pub(crate) fn metric_id(m: RecordMetric) -> &'static str {
    match m {
        RecordMetric::DecodeTps => "decodeTps",
        RecordMetric::PrefillTps => "prefillTps",
        RecordMetric::TtftS => "ttftS",
        RecordMetric::ImageS => "imageS",
        RecordMetric::TtsRtf => "ttsRtf",
        RecordMetric::SttRtf => "sttRtf",
        RecordMetric::VideoS => "videoS",
        RecordMetric::MusicRtf => "musicRtf",
    }
}

pub(crate) fn metric_title(m: RecordMetric) -> &'static str {
    match m {
        RecordMetric::DecodeTps => "Decode (tok/s, higher is better)",
        RecordMetric::PrefillTps => "Prefill (tok/s without cached tokens, higher is better)",
        RecordMetric::TtftS => "Time to first token (s, lower is better)",
        RecordMetric::ImageS => "Seconds per image (lower is better)",
        RecordMetric::TtsRtf => "Speech, audio s per wall s (higher is better)",
        RecordMetric::SttRtf => "Transcription, audio s per wall s (higher is better)",
        RecordMetric::VideoS => "Seconds per video (lower is better)",
        RecordMetric::MusicRtf => "Music, audio s per wall s (higher is better)",
    }
}

/// `decodeTps`, also `decode`, `prefill`, `ttft`, `image`, `tts`, `stt`, `video`, `music` (any case).
pub(crate) fn parse_metric(s: &str) -> CliResult<RecordMetric> {
    let t = s.trim().to_ascii_lowercase();
    METRICS
        .iter()
        .copied()
        .find(|m| {
            let id = metric_id(*m).to_ascii_lowercase();
            t == id || id.strip_suffix("tps").or_else(|| id.strip_suffix("rtf")).or_else(|| id.strip_suffix('s')) == Some(t.as_str())
        })
        .ok_or_else(|| {
            CliError::usage(format!(
                "Unknown metric \"{s}\" ({}).",
                METRICS.iter().map(|m| metric_id(*m)).collect::<Vec<_>>().join(", ")
            ))
        })
}

pub(crate) fn value_text(m: RecordMetric, v: f64) -> String {
    match m {
        RecordMetric::DecodeTps | RecordMetric::PrefillTps => format!("{v:.1} tok/s"),
        RecordMetric::TtftS => format!("{v:.3} s"),
        RecordMetric::ImageS | RecordMetric::VideoS => format!("{v:.2} s"),
        RecordMetric::TtsRtf | RecordMetric::SttRtf | RecordMetric::MusicRtf => format!("{v:.2}x"),
    }
}

/// "ctx 32768 · 2048 prompt (512 cached) · 256 gen · KV q8_0" / "1024x1024 · 8 steps".
fn conditions(v: &RecordValue) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let (Some(w), Some(h)) = (v.width, v.height) {
        parts.push(format!("{w}x{h}"));
    }
    if let Some(f) = v.frames {
        parts.push(format!("{f} frames"));
    }
    if let Some(s) = v.steps {
        parts.push(format!("{s} step{}", if s == 1 { "" } else { "s" }));
    }
    if let Some(c) = v.ctx {
        parts.push(format!("ctx {c}"));
    }
    if let Some(p) = v.prompt_tokens {
        match v.cached_tokens.filter(|c| *c > 0) {
            Some(c) => parts.push(format!("{p} prompt ({c} cached)")),
            None => parts.push(format!("{p} prompt")),
        }
    }
    if let Some(g) = v.gen_tokens {
        parts.push(format!("{g} gen"));
    }
    if let Some(k) = &v.kv {
        parts.push(format!("KV {k}"));
    }
    if parts.is_empty() {
        "-".into()
    } else {
        parts.join(" · ")
    }
}

/// A short id for the tables: the first 12 hex of the file's SHA-256, else its name (enough for `records forget`).
fn short_id(e: &RecordEntry) -> String {
    match &e.model.sha256 {
        Some(s) => s.chars().take(12).collect(),
        None => e.model.file.clone(),
    }
}

/// Entries of `metric`, best first.
fn ranked<'a>(entries: &[&'a RecordEntry], metric: RecordMetric) -> Vec<(&'a RecordEntry, &'a RecordValue)> {
    let mut rows: Vec<(&RecordEntry, &RecordValue)> = entries.iter().filter_map(|e| e.best.get(&metric).map(|v| (*e, v))).collect();
    rows.sort_by(|a, b| {
        let o = a.1.value.partial_cmp(&b.1.value).unwrap_or(std::cmp::Ordering::Equal);
        if metric.higher_is_better() {
            o.reverse()
        } else {
            o
        }
    });
    rows
}

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let kind = args.opt("--kind")?;
    let metric = args.opt("--metric")?;
    let backend = args.opt("--backend")?;
    let node = args.opt("--node")?;
    let yes = args.flag("--yes");
    match args.next_pos().as_deref() {
        None | Some("list") | Some("ls") => {
            args.done()?;
            if yes {
                return Err(CliError::usage("--yes only applies to records forget."));
            }
            list(loaded, out, kind.as_deref(), metric.as_deref(), backend.as_deref(), node.as_deref())
        }
        Some("forget") => {
            let key = args.pos("record key (klif-cli records lists them)")?;
            args.done()?;
            if kind.is_some() || metric.is_some() || backend.is_some() || node.is_some() {
                return Err(CliError::usage("records forget takes only <key> and --yes."));
            }
            forget(loaded, out, &key, yes)
        }
        Some("history") => {
            let key = args.pos("record key (klif-cli records lists them)")?;
            args.done()?;
            if kind.is_some() || backend.is_some() || node.is_some() || yes {
                return Err(CliError::usage("records history takes only <key> and --metric."));
            }
            history(loaded, out, &key, metric.as_deref())
        }
        Some(other) => Err(CliError::usage(format!("Unknown records subcommand \"{other}\" (list, forget, history)."))),
    }
}

/// The entries the filters keep (`--node local` = this machine, unless a node is called "local").
fn filtered<'a>(vm: &'a ViewModel, kind: Option<SystemKind>, metric: Option<RecordMetric>, backend: Option<&str>, node: Option<&str>) -> Vec<&'a RecordEntry> {
    let this = node.is_some_and(|n| matches!(n, "local" | "this") && !vm.nodes.iter().any(|x| x.id == n));
    vm.records
        .iter()
        .filter(|e| kind.is_none_or(|k| e.kind == k))
        .filter(|e| metric.is_none_or(|m| e.best.contains_key(&m)))
        .filter(|e| {
            backend.is_none_or(|b| {
                let b = b.trim();
                e.backend.eq_ignore_ascii_case(b) || (e.backend.is_empty() && matches!(b.to_ascii_lowercase().as_str(), "" | "-" | "?" | "unknown"))
            })
        })
        .filter(|e| match node {
            None => true,
            Some(_) if this => e.node.is_none(),
            Some(n) => e.node.as_deref() == Some(n),
        })
        .collect()
}

fn list(loaded: &LoadedConfig, out: Out, kind: Option<&str>, metric: Option<&str>, backend: Option<&str>, node: Option<&str>) -> CliResult {
    let kind = match kind {
        Some(k) => Some(SystemKind::parse(k).ok_or_else(|| CliError::usage(format!("Unknown kind \"{k}\" (llm, image, tts, stt, video, music).")))?),
        None => None,
    };
    let metric = metric.map(parse_metric).transpose()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    if let Some(n) = node.filter(|n| !matches!(*n, "local" | "this") && !vm.nodes.iter().any(|x| x.id == *n)) {
        let ids: Vec<&str> = vm.nodes.iter().map(|x| x.id.as_str()).collect();
        return Err(CliError::new(
            "not_found",
            if ids.is_empty() {
                format!("There is no node \"{n}\" ([nodes.<id>] in klif.toml); --node local is this machine.")
            } else {
                format!("There is no node \"{n}\". Nodes: {}; --node local is this machine.", ids.join(", "))
            },
        ));
    }
    let mut entries = filtered(&vm, kind, metric, backend, node);
    if let Some(m) = metric {
        entries = ranked(&entries, m).into_iter().map(|(e, _)| e).collect();
    }
    out.doc(crate::out::val(&RecordsDoc { records: entries.iter().map(|e| (*e).clone()).collect() }), || human(&entries, metric));
    Ok(())
}

fn human(entries: &[&RecordEntry], metric: Option<RecordMetric>) -> String {
    if entries.is_empty() {
        return "No records yet. They come from finished requests (decode with 128+ generated tokens, prefill with 1024+ \
                uncached prompt tokens, time to first token from any request), finished images and klif-cli bench."
            .into();
    }
    let metrics: Vec<RecordMetric> = match metric {
        Some(m) => vec![m],
        None => METRICS.iter().copied().filter(|m| entries.iter().any(|e| e.best.contains_key(m))).collect(),
    };
    let mut s = String::new();
    for m in metrics {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(metric_title(m));
        s.push('\n');
        let mut rows = vec![["#", "VALUE", "MODEL", "QUANT", "BACKEND", "MACHINE", "CONDITIONS", "WHEN", "SOURCE", "ID"].map(String::from).to_vec()];
        for (i, (e, v)) in ranked(entries, m).into_iter().enumerate() {
            rows.push(vec![
                (i + 1).to_string(),
                value_text(m, v.value),
                e.model.name.clone(),
                e.model.quant.clone().unwrap_or_else(|| "-".into()),
                if e.backend.is_empty() { "?".into() } else { e.backend.clone() },
                e.machine.clone(),
                conditions(v),
                date(v.at),
                match v.source {
                    klif_core::klif_common::vm::RecordSource::Live => "live".into(),
                    klif_core::klif_common::vm::RecordSource::Bench => "bench".into(),
                },
                short_id(e),
            ]);
        }
        s.push_str(&table(&rows));
    }
    s.push_str("\nFull keys: klif-cli --json records. Remove a junk entry: klif-cli records forget <key or ID> --yes\n");
    s
}

/// The entry a key argument names: the whole key, else a part of it that is unique (case ignored).
fn find_entry<'a>(vm: &'a ViewModel, want: &str) -> CliResult<&'a RecordEntry> {
    let want = want.trim();
    let exact: Vec<&RecordEntry> = vm.records.iter().filter(|e| e.key == want).collect();
    let found: Vec<&RecordEntry> = if exact.is_empty() {
        let low = want.to_ascii_lowercase();
        vm.records.iter().filter(|e| !low.is_empty() && e.key.to_ascii_lowercase().contains(&low)).collect()
    } else {
        exact
    };
    match found.as_slice() {
        [] => Err(CliError::new("not_found", format!("There is no record \"{want}\" (klif-cli --json records lists the keys)."))),
        [one] => Ok(*one),
        many => {
            let keys: Vec<&str> = many.iter().map(|e| e.key.as_str()).collect();
            Err(CliError::new("ambiguous", format!("\"{want}\" matches {} records: {}. Pass the whole key.", many.len(), keys.join(", "))))
        }
    }
}

/// `records history <key> [--metric M]`: every broken record of one entry, oldest first (the engine reads
/// `records-history.jsonl`, or asks the node that keeps the entry).
fn history(loaded: &LoadedConfig, out: Out, arg: &str, metric: Option<&str>) -> CliResult {
    let metric = metric.map(parse_metric).transpose()?;
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let e = find_entry(&vm, arg)?;
    let points = conn.link().records_history(&e.key, metric).map_err(|err| CliError::new("refused", format!("{err:#}")))?;
    let doc = RecordsHistoryDoc {
        key: e.key.clone(),
        model: e.model.name.clone(),
        quant: e.model.quant.clone(),
        backend: e.backend.clone(),
        machine: e.machine.clone(),
        node: e.node.clone(),
        metric,
        points: points.clone(),
    };
    out.doc(crate::out::val(&doc), || {
        let mut s = format!(
            "{}{} on {} ({})\n",
            e.model.name,
            e.model.quant.as_deref().map(|q| format!(" {q}")).unwrap_or_default(),
            if e.backend.is_empty() { "an unknown backend" } else { e.backend.as_str() },
            e.machine
        );
        if points.is_empty() {
            s.push_str("\nNo history yet: every broken record is written to records-history.jsonl from now on.\n");
            return s;
        }
        for m in METRICS.iter().copied().filter(|m| points.iter().any(|p| p.metric == Some(*m))) {
            s.push('\n');
            s.push_str(metric_title(m));
            s.push('\n');
            let mut rows = vec![["WHEN", "VALUE", "WAS"].map(String::from).to_vec()];
            for p in points.iter().filter(|p| p.metric == Some(m)) {
                rows.push(vec![date(p.at), value_text(m, p.new), p.old.map(|o| value_text(m, o)).unwrap_or_else(|| "-".into())]);
            }
            s.push_str(&table(&rows));
        }
        s
    });
    Ok(())
}

fn forget(loaded: &LoadedConfig, out: Out, arg: &str, yes: bool) -> CliResult {
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let want = arg.trim();
    let e = find_entry(&vm, want)?;
    let what = format!(
        "{}{} on {} ({})",
        e.model.name,
        e.model.quant.as_deref().map(|q| format!(" {q}")).unwrap_or_default(),
        if e.backend.is_empty() { "an unknown backend" } else { e.backend.as_str() },
        e.machine
    );
    if !yes {
        return Err(CliError::needs_yes(format!("records forget removes every record of {what}: add --yes.")));
    }
    // A node's entry is forgotten by that node, under the key it knows (without "<node>/").
    let (key, node) = match &e.node {
        Some(n) => (e.key.strip_prefix(&format!("{n}/")).unwrap_or(&e.key).to_string(), Some(n.clone())),
        None => (e.key.clone(), None),
    };
    act(&conn, Action::ForgetRecord { key, node })?;
    out.doc(crate::out::val(&RecordsForgetDoc { forgotten: e.key.clone() }), || format!("Forgot the records of {what}."));
    Ok(())
}
