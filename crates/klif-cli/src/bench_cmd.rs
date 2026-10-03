//! `bench <system> [--runs N] [--prompt N] [--gen N] [--keep-running] [--allow-shared] [--yes]`, `bench list
//! [--preset ID]`. The measuring is `klif_core::bench`; results go to `<data_dir>\bench\<preset-id>.json`.

use crate::args::Args;
use crate::conn;
use crate::out::{date, num, table, CliError, CliResult, Out};
use crate::resolve::resolve;
use klif_core::bench::{self, BenchOpts, BenchRecord};
use klif_core::klif_common::config::{validate_preset_id, LoadedConfig};
use klif_core::klif_common::vm::SystemKind;
use serde_json::json;

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    // `bench list` vs `bench <system>`: options first, then the positional.
    let runs = args.opt_num::<u32>("--runs")?;
    let prompt = args.opt_num::<u32>("--prompt")?;
    let gen = args.opt_num::<u32>("--gen")?;
    let keep_running = args.flag("--keep-running");
    let allow_shared = args.flag("--allow-shared");
    let yes = args.flag("--yes");
    let preset = args.opt("--preset")?;
    let first = args.pos("System (or list)")?;
    if first == "list" || first == "ls" {
        args.done()?;
        return list(loaded, out, preset.as_deref());
    }
    args.done()?;
    if preset.is_some() {
        return Err(CliError::usage("--preset only applies to bench list."));
    }
    let d = BenchOpts::default();
    let opts = BenchOpts {
        runs: runs.unwrap_or(d.runs),
        prompt_tokens: prompt.unwrap_or(d.prompt_tokens),
        gen_tokens: gen.unwrap_or(d.gen_tokens),
        keep_running,
        yes,
        allow_shared,
    };
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let id = resolve(&vm, &first)?;
    if id.is_remote() {
        return Err(CliError::new("unsupported", "Bench is not supported for remote Systems; run klif-cli on that machine."));
    }
    let needs_launch = vm.systems.iter().find(|s| s.id == id).is_some_and(|s| !s.external && !crate::cmds::holds(s));
    if needs_launch && !opts.yes {
        return Err(CliError::needs_yes(format!(
            "bench launches {id} for the runs and stops it afterwards (unless --keep-running): add --yes."
        )));
    }
    let llm = vm.systems.iter().find(|s| s.id == id).is_some_and(|s| s.kind == SystemKind::Llm);
    crate::out::note(format!(
        "bench {id}: {} run(s){} (engine: {})",
        opts.runs,
        if llm { format!(", {} prompt / {} generated tokens", opts.prompt_tokens, opts.gen_tokens) } else { String::new() },
        conn.describe()
    ));
    let rec = bench::run(conn.link(), &id, &opts).map_err(|e| CliError::new("bench", format!("{e:#}")))?;
    let data_dir = conn.link().data_dir();
    bench::store(&data_dir, &rec.preset_id, &rec).map_err(|e| CliError::new("io", format!("The result could not be saved: {e:#}")))?;
    let file = bench::bench_file(&data_dir, &rec.preset_id);
    let summary = bench::summarize(&rec, false);
    out.doc(json!({ "file": file.display().to_string(), "summary": summary, "record": rec }), || {
        let mut s = human_record(&rec);
        s.push_str(&format!("\nSaved to {}\n", file.display()));
        s
    });
    Ok(())
}

fn human_record(rec: &BenchRecord) -> String {
    let mut s = format!(
        "{} · preset {} · {} · {}\n{} · {}{}\n",
        rec.system,
        rec.preset_id,
        rec.adapter.as_str(),
        if rec.model.name.is_empty() { "-" } else { rec.model.name.as_str() },
        rec.hardware.gpu,
        date(rec.at),
        rec.backend_build.as_deref().map(|b| format!(" · {b}")).unwrap_or_default()
    );
    let image = rec.runs.iter().any(|r| r.seconds_per_image.is_some());
    let mut rows = if image {
        vec![vec!["RUN".to_string(), "S/IMAGE".into()]]
    } else {
        vec![["RUN", "TTFT S", "PREFILL TOK/S", "DECODE TOK/S", "PROMPT", "GEN"].map(String::from).to_vec()]
    };
    for (i, r) in rec.runs.iter().enumerate() {
        if image {
            rows.push(vec![(i + 1).to_string(), num(r.seconds_per_image, 2)]);
        } else {
            rows.push(vec![
                (i + 1).to_string(),
                num(r.ttft_s, 3),
                num(r.prefill_tps, 1),
                num(r.decode_tps, 1),
                r.prompt_tokens.map(|x| x.to_string()).unwrap_or_else(|| "-".into()),
                r.gen_tokens.map(|x| x.to_string()).unwrap_or_else(|| "-".into()),
            ]);
        }
    }
    s.push('\n');
    s.push_str(&table(&rows));
    let sum = bench::summarize(rec, false);
    s.push_str(&format!(
        "\nmedian: {}load {} s · peak VRAM {} GiB · spill {} MiB\n",
        if image {
            format!("{} s/image · ", num(sum.seconds_per_image, 2))
        } else {
            format!("ttft {} s · prefill {} tok/s · decode {} tok/s · ", num(sum.ttft_s, 3), num(sum.prefill_tps, 1), num(sum.decode_tps, 1))
        },
        num(rec.load_s, 1),
        num(rec.peak_vram_gib, 2),
        num(rec.spill_mib, 0)
    ));
    s
}

fn list(loaded: &LoadedConfig, out: Out, preset: Option<&str>) -> CliResult {
    let data_dir = &loaded.cfg.data_dir;
    let files: Vec<(String, Vec<BenchRecord>)> = match preset {
        Some(p) => {
            validate_preset_id(p).map_err(CliError::usage)?;
            vec![(p.to_string(), bench::records(data_dir, p).map_err(|e| CliError::new("io", format!("{e:#}")))?)]
        }
        None => bench::all_records(data_dir),
    };
    let docs: Vec<serde_json::Value> = files
        .iter()
        .map(|(id, recs)| json!({ "presetId": id, "file": bench::bench_file(data_dir, id).display().to_string(), "records": recs }))
        .collect();
    out.doc(json!({ "dataDir": data_dir.display().to_string(), "presets": docs }), || {
        let mut rows = vec![["PRESET", "DATE", "SYSTEM", "RUNS", "TTFT S", "PREFILL", "DECODE", "S/IMG", "PEAK GIB", "GPU", "HASH"].map(String::from).to_vec()];
        for (id, recs) in &files {
            for r in recs {
                let s = bench::summarize(r, false);
                rows.push(vec![
                    id.clone(),
                    date(r.at),
                    r.system.to_string(),
                    s.runs.to_string(),
                    num(s.ttft_s, 3),
                    num(s.prefill_tps, 1),
                    num(s.decode_tps, 1),
                    num(s.seconds_per_image, 2),
                    num(r.peak_vram_gib, 2),
                    r.hardware.gpu.clone(),
                    r.preset_hash.chars().take(10).collect(),
                ]);
            }
        }
        if rows.len() == 1 {
            return format!("No bench results in {} yet (klif-cli bench <system> --yes).", data_dir.join("bench").display());
        }
        table(&rows)
    });
    Ok(())
}
