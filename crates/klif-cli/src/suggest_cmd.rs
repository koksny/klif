//! suggest: the model the embedded pool suggests per slot for this machine (System 1 / 2 / 3, image, ...), with the
//! estimated memory against each tier's budget. Reads this machine directly (no engine, no GPU touched).

use crate::args::Args;
use crate::out::{table, val, CliError, CliResult, Out};
use crate::outputs::{SuggestDoc, SuggestGpu, SuggestHardware};
use klif_core::klif_catalog::recommend::{embedded_pool, slot_name};
use klif_core::klif_catalog::suggest::{ctx_label, suggest};
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::{HardwareInfo, LlmClass, Suggestion, SystemKind};

/// Printed with every suggestion list.
pub const DISCLAIMER: &str = "Every number here is an estimate from the embedded model pool (weights + KV cache + \
overhead against each tier's budget). Benchmark and calibrate on the machine (klif-cli bench <system>). Each model keeps \
its own license. KLIF downloads only from huggingface.co, and only when you ask (klif-cli models download <rec> --yes).";

/// `suggest [--kind K] [--class C]`.
pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let kind = match args.opt("--kind")? {
        None => None,
        Some(k) => Some(SystemKind::parse(&k).ok_or_else(|| CliError::usage(format!("Unknown kind \"{k}\" (llm, image, tts, stt, video, music).")))?),
    };
    let class = match args.opt("--class")? {
        None => None,
        Some(c) => Some(LlmClass::parse(&c).ok_or_else(|| CliError::usage(format!("Unknown class \"{c}\" (fast, deep, max).")))?),
    };
    args.done()?;
    let hw = klif_core::klif_telemetry::hardware::detect(&loaded.cfg.hardware);
    let list: Vec<Suggestion> = suggest(&hw, embedded_pool())
        .into_iter()
        .filter(|s| kind.is_none_or(|k| s.kind == k) && class.is_none_or(|c| s.class == Some(c)))
        .collect();
    out.doc(val(&SuggestDoc { hardware: summary(&hw), suggestions: list.clone(), disclaimer: DISCLAIMER.to_string() }), || human(&hw, &list));
    Ok(())
}

/// The machine facts the suggestions were made for.
fn summary(hw: &HardwareInfo) -> SuggestHardware {
    let gpus: Vec<SuggestGpu> = hw
        .gpus
        .iter()
        .filter(|g| g.counted)
        .map(|g| SuggestGpu { id: g.id.clone(), name: g.name.clone(), vram_gib: g.vram_gib, integrated: g.integrated })
        .collect();
    SuggestHardware {
        gpus,
        cpu: hw.cpu.as_ref().map(|c| c.name.clone()),
        vram_pool_gib: hw.vram_pool_gib,
        largest_gpu_gib: hw.largest_gpu_gib,
        ram_total_gib: hw.ram_total_gib,
        unified: hw.unified,
    }
}

fn gib(x: f64) -> String {
    format!("{x:.1}")
}

fn human(hw: &HardwareInfo, list: &[Suggestion]) -> String {
    let mut s = String::new();
    let gpus: Vec<String> = hw
        .gpus
        .iter()
        .filter(|g| g.counted)
        .map(|g| format!("{} {} GiB", g.name, g.vram_gib.map(gib).unwrap_or_else(|| "?".into())))
        .collect();
    let pool = if gpus.is_empty() { "no counted GPU".to_string() } else { gpus.join(" + ") };
    let unified = if hw.unified { ", unified" } else { "" };
    s.push_str(&format!(
        "This machine: {pool} (pool {} GiB{unified}, largest {} GiB), {} GiB RAM\n\n",
        gib(hw.vram_pool_gib),
        gib(hw.largest_gpu_gib),
        gib(hw.ram_total_gib)
    ));
    if list.is_empty() {
        s.push_str("No suggestions for that kind or class.\n");
    } else {
        let mut rows = vec![["SLOT", "MODEL", "QUANT", "CTX", "KV", "VRAM EST/BUDGET", "RAM EST/BUDGET", "PLACEMENT", "REC"].map(String::from).to_vec()];
        for x in list {
            let slot = match x.class {
                Some(c) => format!("{} ({})", slot_name(x.slot), c.default_label()),
                None => slot_name(x.slot).to_string(),
            };
            let dash = || "-".to_string();
            match &x.rec {
                None => rows.push(vec![
                    slot,
                    "(nothing fits)".into(),
                    dash(),
                    dash(),
                    dash(),
                    format!("- / {}", gib(x.budget_vram_gib)),
                    format!("- / {}", gib(x.budget_ram_gib)),
                    dash(),
                    dash(),
                ]),
                Some(rec) => rows.push(vec![
                    slot,
                    x.model.clone(),
                    x.quant.clone(),
                    x.ctx.map(ctx_label).unwrap_or_else(dash),
                    x.kv.clone().unwrap_or_else(dash),
                    format!("{} / {}", gib(x.est_vram_gib), gib(x.budget_vram_gib)),
                    format!("{} / {}", gib(x.est_ram_gib), gib(x.budget_ram_gib)),
                    x.placement.clone().unwrap_or_else(dash),
                    rec.clone(),
                ]),
            }
        }
        s.push_str(&table(&rows));
        let notes: Vec<String> = list.iter().filter_map(|x| x.note.as_ref().map(|n| format!("  {}: {n}", slot_name(x.slot)))).collect();
        if !notes.is_empty() {
            s.push('\n');
            s.push_str(&notes.join("\n"));
            s.push('\n');
        }
        s.push_str("\nGiB; VRAM and RAM are estimates against the tier's budget. Next: klif-cli models download <rec> --yes, then\nklif-cli models adopt <rec> --system <system> (context and KV type come from this suggestion).\n");
    }
    s.push_str(&format!("\n{DISCLAIMER}\n"));
    s
}
