//! The suggested model per slot (System 1 / 2 / 3, image, tts, stt, video, music) for a machine: a pure function of its
//! hardware inventory and the embedded pool (`recommend::Pool`). Every number is an estimate. Owner: package B.
//!
//! Fit of an LLM rung at a context and KV type: weights (the rung's file sizes) + KV (`KvCost::mib`) + the model's
//! overhead, against the tier's budget:
//! - fast: a model that fits entirely in VRAM wins; only when none of the tier does, a MoE may keep experts in RAM
//!   (its dense share, KV and overhead stay on the GPUs). Dense models always stay in VRAM.
//!   VRAM <= pool x (1 - vram_free_frac) - margins, RAM <= RAM x (1 - ram_free_frac).
//! - deep: everything in VRAM <= pool - margins.
//! - max: VRAM + RAM minus the OS reserve; anything may spill.
//! - image / tts / stt / video: the rung's own estimate against the largest counted GPU, else with
//!   --offload-to-cpu plus RAM.
//!
//! A machine without a counted GPU runs everything on the CPU: every LLM tier then budgets RAM (fast keeps a third
//! of it free, deep and max keep the OS reserve).
//!
//! Choice per model: the highest quality up to `[choice] quality_first` that fits at the floor context with q8_0 KV
//! (the smaller file wins a tie), then the longest context up to ctx_max, then the highest quality again at that
//! context, then f16 KV when it fits too. Per tier the first model with an option wins, preferring one that a lower
//! tier did not take.

use crate::recommend::{rec_id, slot_class, slot_kind, Pool, PoolModel, PoolQuant, SLOTS};
use klif_common::config::{normalize_gpu_id, PresetCfg};
use klif_common::vm::{AdapterId, HardwareInfo, LlmClass, SuggestSlot, Suggestion};

/// q8_0 KV relative to f16 (1.0625 of 2 bytes per element).
pub const KV_Q8_FACTOR: f64 = 0.53;

/// Context steps tried between a tier's floor and the model's ctx_max (tokens).
const CTX_STEPS: [u32; 11] = [32768, 49152, 65536, 98304, 131072, 196608, 262144, 393216, 524288, 786432, 1_048_576];

/// `Suggestion.placement` values.
pub const PLACE_EXPERTS: &str = "experts in RAM";
pub const PLACE_LAYERS: &str = "layers in RAM";
pub const PLACE_OFFLOAD: &str = "weights in RAM (--offload-to-cpu)";
pub const PLACE_CPU: &str = "CPU only (RAM)";

/// KV cache types llama.cpp accepts for -ctk / -ctv.
pub const KV_TYPES: [&str; 9] = ["f16", "bf16", "f32", "q8_0", "q5_1", "q5_0", "q4_1", "q4_0", "iq4_nl"];

/// The machine as the suggester sees it (GiB).
struct Machine {
    gpus: usize,
    pool: f64,
    largest: f64,
    ram: f64,
    /// No counted GPU: everything runs from RAM.
    cpu_only: bool,
}

impl Machine {
    fn of(hw: &HardwareInfo) -> Machine {
        let num = |x: f64| if x.is_finite() && x > 0.0 { x } else { 0.0 };
        let gpus = hw.gpus.iter().filter(|g| g.counted).count();
        let pool = num(hw.vram_pool_gib);
        // A counted integrated GPU's pool includes system memory: that part is not RAM to spend a second time.
        let shared: f64 = hw.gpus.iter().filter(|g| g.counted).filter_map(|g| g.shared_gib).map(num).sum();
        let ram = (num(hw.ram_total_gib) - shared).max(0.0);
        Machine { gpus, pool, largest: num(hw.largest_gpu_gib).min(pool.max(0.0)), ram, cpu_only: gpus == 0 || pool <= 0.0 }
    }
}

/// What a slot may use (GiB).
#[derive(Debug, Clone, Copy)]
struct Budget {
    vram: f64,
    ram: f64,
}

fn budget(pool: &Pool, m: &Machine, slot: SuggestSlot) -> Budget {
    let b = &pool.budget;
    let reserve = b.max.ram_reserve_gib.max(b.max.ram_reserve_frac * m.ram);
    let ram_left = (m.ram - reserve).max(0.0);
    let clamp = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
    if m.cpu_only {
        let ram = match slot {
            SuggestSlot::Fast => (m.ram * (1.0 - b.fast.vram_free_frac)).min(m.ram * (1.0 - b.fast.ram_free_frac)).min(ram_left),
            _ => ram_left,
        };
        return Budget { vram: 0.0, ram: clamp(ram) };
    }
    let margin = m.gpus as f64 * b.gpu_margin_gib;
    match slot {
        SuggestSlot::Fast => Budget {
            vram: clamp(m.pool * (1.0 - b.fast.vram_free_frac) - margin),
            ram: clamp(m.ram * (1.0 - b.fast.ram_free_frac)),
        },
        SuggestSlot::Deep => Budget { vram: clamp(m.pool - margin), ram: 0.0 },
        SuggestSlot::Max => Budget { vram: clamp(m.pool - margin), ram: ram_left },
        _ => Budget { vram: clamp(m.largest - b.gpu_margin_gib), ram: ram_left },
    }
}

/// Where a rung would sit (GiB).
#[derive(Debug, Clone, Copy)]
struct Fit {
    vram: f64,
    ram: f64,
    placement: Option<&'static str>,
}

/// One option: a rung at a context and KV type.
#[derive(Debug, Clone, Copy)]
struct Pick<'a> {
    model: &'a PoolModel,
    quant: &'a PoolQuant,
    ctx: Option<u32>,
    kv: Option<&'static str>,
    fit: Fit,
}

/// Total GiB of an LLM rung at `ctx` with KV factor `factor`, and the part that must stay on the GPU when a MoE's
/// experts go to RAM.
fn llm_need(model: &PoolModel, quant: &PoolQuant, ctx: u32, factor: f64) -> (f64, f64) {
    let weights = quant.gib();
    let cache = model.kv.as_ref().map(|k| k.mib(ctx, factor)).unwrap_or(0.0) / 1024.0;
    let over = model.overhead_mib / 1024.0;
    (weights + cache + over, weights * model.dense_share() + cache + over)
}

fn fit_llm(m: &Machine, slot: SuggestSlot, b: Budget, model: &PoolModel, quant: &PoolQuant, ctx: u32, factor: f64) -> Option<Fit> {
    let (total, gpu_part) = llm_need(model, quant, ctx, factor);
    if m.cpu_only {
        return (total <= b.ram).then_some(Fit { vram: 0.0, ram: total, placement: Some(PLACE_CPU) });
    }
    if total <= b.vram {
        return Some(Fit { vram: total, ram: 0.0, placement: None });
    }
    let spill = total - b.vram;
    match slot {
        SuggestSlot::Fast if model.moe => {
            (gpu_part <= b.vram && spill <= b.ram).then_some(Fit { vram: b.vram, ram: spill, placement: Some(PLACE_EXPERTS) })
        }
        SuggestSlot::Max if spill <= b.ram => {
            let placement = if model.moe && gpu_part <= b.vram { PLACE_EXPERTS } else { PLACE_LAYERS };
            Some(Fit { vram: b.vram, ram: spill, placement: Some(placement) })
        }
        _ => None,
    }
}

/// The contexts tried for a model in a tier: the floor (capped by ctx_max), the steps above it, ctx_max.
fn ctx_ladder(floor: u32, ctx_max: u32) -> Vec<u32> {
    let mut out = vec![floor];
    out.extend(CTX_STEPS.iter().copied().filter(|&c| c > floor && c < ctx_max));
    if ctx_max > floor {
        out.push(ctx_max);
    }
    out
}

/// Higher quality first; on a tie the smaller rung.
fn better(a: &PoolQuant, qa: f64, b: &PoolQuant, qb: f64) -> bool {
    qa > qb || (qa == qb && a.bytes() < b.bytes())
}

fn best<'a>(rungs: impl Iterator<Item = &'a PoolQuant>, quality: impl Fn(&PoolQuant) -> f64) -> Option<&'a PoolQuant> {
    let mut out: Option<&PoolQuant> = None;
    for q in rungs {
        if out.is_none_or(|o| better(q, quality(q), o, quality(o))) {
            out = Some(q);
        }
    }
    out
}

/// The rungs of a model above its quality floor.
fn rungs<'a>(pool: &Pool, model: &'a PoolModel) -> impl Iterator<Item = &'a PoolQuant> {
    let big_moe = model.moe && model.params_total_b.unwrap_or(0.0) >= pool.floors.moe_floor_from_b;
    let floor = if big_moe { pool.floors.moe_min_quality } else { pool.floors.dense_min_quality };
    model.quants.iter().filter(move |q| q.quality >= floor)
}

/// The KV factor of the lowest KV type offered (q8_0 unless the floor is f16).
fn kv_low(pool: &Pool) -> (f64, &'static str) {
    if pool.floors.kv_min.trim().eq_ignore_ascii_case("f16") {
        (1.0, "f16")
    } else {
        (KV_Q8_FACTOR, "q8_0")
    }
}

/// A model's best option in an LLM tier, if any fits.
fn choose_llm<'a>(pool: &Pool, m: &Machine, slot: SuggestSlot, b: Budget, model: &'a PoolModel) -> Option<Pick<'a>> {
    let ctx_max = model.ctx_max?;
    let floor = pool.floors.ctx.of(slot_class(slot)).min(ctx_max);
    let (low, low_name) = kv_low(pool);
    let cap = pool.choice.quality_first;
    let fits = |q: &PoolQuant, ctx: u32, f: f64| fit_llm(m, slot, b, model, q, ctx, f);
    // 1. Quality up to the cap at the floor context.
    let first = best(rungs(pool, model).filter(|q| fits(q, floor, low).is_some()), |q| q.quality.min(cap))?;
    // 2. The longest context with that rung.
    let ctx = ctx_ladder(floor, ctx_max).into_iter().rev().find(|&c| fits(first, c, low).is_some()).unwrap_or(floor);
    // 3. Quality again at that context (the first rung still fits there).
    let quant = best(rungs(pool, model).filter(|q| fits(q, ctx, low).is_some()), |q| q.quality).unwrap_or(first);
    // 4. f16 KV when it fits too.
    let (kv, fit) = match fits(quant, ctx, 1.0) {
        Some(f) => ("f16", f),
        None => (low_name, fits(quant, ctx, low)?),
    };
    Some(Pick { model, quant, ctx: Some(ctx), kv: Some(kv), fit })
}

/// A model's best rung for an image (or other single-GPU) slot: the highest quality resident, else with
/// --offload-to-cpu.
fn choose_single<'a>(pool: &Pool, m: &Machine, b: Budget, model: &'a PoolModel) -> Option<Pick<'a>> {
    let pick = |quant: &'a PoolQuant, fit: Fit| Pick { model, quant, ctx: None, kv: None, fit };
    if m.cpu_only {
        let ram = |q: &PoolQuant| q.offload_ram_gib.unwrap_or_else(|| q.gib() + 1.0);
        let q = best(rungs(pool, model).filter(|q| ram(q) <= b.ram), |q| q.quality)?;
        return Some(pick(q, Fit { vram: 0.0, ram: ram(q), placement: Some(PLACE_CPU) }));
    }
    let resident = best(rungs(pool, model).filter(|q| q.min_vram_gib.is_some_and(|v| v <= b.vram)), |q| q.quality);
    if let Some(q) = resident {
        return Some(pick(q, Fit { vram: q.min_vram_gib.unwrap_or(0.0), ram: 0.0, placement: None }));
    }
    let offload = |q: &PoolQuant| match (q.offload_vram_gib, q.offload_ram_gib) {
        (Some(v), Some(r)) => (v <= b.vram && r <= b.ram).then_some((v, r)),
        _ => None,
    };
    let q = best(rungs(pool, model).filter(|q| offload(q).is_some()), |q| q.quality)?;
    let (v, r) = offload(q)?;
    Some(pick(q, Fit { vram: v, ram: r, placement: Some(PLACE_OFFLOAD) }))
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// "32k", "1M".
pub fn ctx_label(ctx: u32) -> String {
    if ctx >= 1 << 20 && ctx.is_multiple_of(1 << 20) {
        format!("{}M", ctx >> 20)
    } else if ctx.is_multiple_of(1024) {
        format!("{}k", ctx / 1024)
    } else {
        ctx.to_string()
    }
}

/// "System 1", "an image System".
fn slot_label(slot: SuggestSlot) -> String {
    match slot_class(slot) {
        Some(c) => c.default_label().to_string(),
        None => format!("a {} System", slot_kind(slot).label()),
    }
}

/// Why nothing fits a slot: the smallest option and what the slot may use, as one sentence.
fn nothing_fits(pool: &Pool, m: &Machine, slot: SuggestSlot, b: Budget) -> String {
    let (low, _) = kv_low(pool);
    let mut smallest: Option<(f64, String)> = None;
    for model in pool.tiers.of(slot).iter().filter_map(|id| pool.model(id)) {
        for q in rungs(pool, model) {
            let (need, what) = match slot_class(slot) {
                Some(c) => {
                    let ctx = pool.floors.ctx.of(Some(c)).min(model.ctx_max.unwrap_or(u32::MAX));
                    let (total, gpu) = llm_need(model, q, ctx, low);
                    let need = if c == LlmClass::Fast && model.moe && !m.cpu_only { gpu } else { total };
                    (need, format!("{} {} at {} context", model.name, q.label, ctx_label(ctx)))
                }
                None => (q.offload_vram_gib.or(q.min_vram_gib).unwrap_or(f64::MAX), format!("{} {}", model.name, q.label)),
            };
            if smallest.as_ref().is_none_or(|(n, _)| need < *n) {
                smallest = Some((need, what));
            }
        }
    }
    let room = if m.cpu_only {
        format!("{:.1} GiB of RAM (no GPU)", b.ram)
    } else {
        format!("{:.1} GiB of VRAM", b.vram)
    };
    match smallest {
        Some((need, what)) => format!(
            "Nothing in the pool fits: the smallest option, {what}, needs about {need:.1} GiB and {} may use {room} here.",
            slot_label(slot)
        ),
        None => "The pool has no model for this slot.".into(),
    }
}

/// The suggested model for every slot whose tier lists models (estimates; `rec` None with a `note` when nothing fits).
pub fn suggest(hw: &HardwareInfo, pool: &Pool) -> Vec<Suggestion> {
    let m = Machine::of(hw);
    let mut out = Vec::new();
    let mut taken: Vec<&str> = Vec::new();
    for slot in SLOTS {
        let list = pool.tiers.of(slot);
        if list.is_empty() {
            continue;
        }
        let full = budget(pool, &m, slot);
        let llm = slot_class(slot).is_some();
        let choose = |b: Budget| -> Vec<Pick> {
            list.iter()
                .filter_map(|id| pool.model(id))
                .filter_map(|model| if llm { choose_llm(pool, &m, slot, b, model) } else { choose_single(pool, &m, b, model) })
                .collect()
        };
        // System 1 is the fast one: a model that sits entirely on the GPUs wins over one that needs RAM; RAM is
        // used only when nothing in the tier fits the VRAM budget alone.
        let vram_only = Budget { ram: 0.0, ..full };
        let (b, picks) = match slot {
            SuggestSlot::Fast if !m.cpu_only => match choose(vram_only) {
                p if !p.is_empty() => (vram_only, p),
                _ => (full, choose(full)),
            },
            _ => (full, choose(full)),
        };
        let fresh = picks.iter().find(|p| !taken.contains(&p.model.id.as_str()));
        let pick = fresh.or(picks.first());
        let mut s = Suggestion {
            slot,
            kind: slot_kind(slot),
            class: slot_class(slot),
            budget_vram_gib: round1(b.vram),
            budget_ram_gib: round1(full.ram),
            ..Suggestion::default()
        };
        match pick {
            None => s.note = Some(nothing_fits(pool, &m, slot, b)),
            Some(p) => {
                s.rec = Some(rec_id(&p.model.id, &p.quant.id));
                s.model = p.model.name.clone();
                s.quant = p.quant.label.clone();
                s.ctx = p.ctx;
                s.kv = p.kv.map(str::to_string);
                s.est_vram_gib = round1(p.fit.vram);
                s.est_ram_gib = round1(p.fit.ram);
                s.placement = p.fit.placement.map(str::to_string);
                let mut notes = Vec::new();
                if fresh.is_none() {
                    notes.push("The same model as a lower tier: no other model of this tier fits.".to_string());
                }
                if m.cpu_only {
                    notes.push("No GPU: it runs on the CPU from RAM, slowly.".to_string());
                }
                s.note = (!notes.is_empty()).then(|| notes.join(" "));
                taken.push(&p.model.id);
            }
        }
        out.push(s);
    }
    out
}

// ------------------------------------------------------------------------------------------- adopt

/// How a preset made from a recommendation is fitted to this machine: the caller's ctx / kv, else those of this
/// machine's suggestion of that recommendation, plus the fit margin and offload its slot needs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdoptFit {
    pub ctx: Option<u32>,
    /// KV cache type for -ctk / -ctv.
    pub kv: Option<String>,
    /// llama.cpp `-fitt`: MiB of VRAM -fit leaves free on each GPU.
    pub fit_margin_mib: Option<u32>,
    /// sd.cpp: add `--offload-to-cpu`.
    pub offload: bool,
}

/// The fit for adopting `rec` (System of class `class`, if any): the call's ctx / kv first, else the matching
/// suggestion's. System 1's margin keeps a third of the inference GPU (`[gpu] inference`, else the largest counted
/// GPU) free: max(1024, a third of its memory in MiB); System 2 / 3 leave 1024 MiB.
pub fn adopt_fit(
    hw: &HardwareInfo,
    suggestions: &[Suggestion],
    rec: &str,
    class: Option<LlmClass>,
    inference_gpu: Option<&str>,
    ctx: Option<u32>,
    kv: Option<String>,
) -> AdoptFit {
    let matching = || suggestions.iter().filter(|s| s.rec.as_deref() == Some(rec));
    let sugg = matching().find(|s| class.is_some_and(|c| s.class == Some(c))).or_else(|| matching().next());
    let gpu_gib = inference_gpu
        .and_then(normalize_gpu_id)
        .and_then(|id| hw.gpus.iter().find(|g| normalize_gpu_id(&g.id).as_deref() == Some(id.as_str())))
        .and_then(|g| g.vram_gib)
        .unwrap_or(hw.largest_gpu_gib);
    let third = if gpu_gib.is_finite() && gpu_gib > 0.0 { (gpu_gib * 1024.0 / 3.0).round() as u32 } else { 0 };
    let fit_margin_mib = match sugg.map(|s| s.slot) {
        Some(SuggestSlot::Fast) => Some(third.max(1024)),
        Some(SuggestSlot::Deep | SuggestSlot::Max) => Some(1024),
        _ => None,
    };
    AdoptFit {
        ctx: ctx.or_else(|| sugg.and_then(|s| s.ctx)),
        kv: kv.or_else(|| sugg.and_then(|s| s.kv.clone())),
        fit_margin_mib,
        offload: sugg.is_some_and(|s| s.placement.as_deref() == Some(PLACE_OFFLOAD)),
    }
}

/// Write a fit into a preset made from a recommendation: `ctx`, the -ctk / -ctv values (replaced or added), the
/// -fitt margin (when the args use -fit), --offload-to-cpu for sd.cpp. Err = one sentence.
pub fn apply_fit(spec: &mut PresetCfg, fit: &AdoptFit) -> Result<(), String> {
    if let Some(c) = fit.ctx {
        if !(256..=16 * 1024 * 1024).contains(&c) {
            return Err(format!("A context of {c} tokens is not usable (256 to 16777216)."));
        }
        spec.ctx = Some(c);
    }
    if let Some(kv) = &fit.kv {
        let kv = kv.trim().to_ascii_lowercase();
        if !KV_TYPES.contains(&kv.as_str()) {
            return Err(format!("\"{kv}\" is not a KV cache type ({}).", KV_TYPES.join(", ")));
        }
        if spec.adapter != AdapterId::LlamaCpp {
            return Err("A KV cache type applies to llama.cpp presets only.".into());
        }
        set_flag(&mut spec.args, &["-ctk", "--cache-type-k"], &kv);
        set_flag(&mut spec.args, &["-ctv", "--cache-type-v"], &kv);
    }
    if let Some(mib) = fit.fit_margin_mib {
        let fit_on = spec.args.iter().any(|a| matches!(a.split_once('=').map(|(f, _)| f).unwrap_or(a), "-fit" | "--fit"));
        if spec.adapter == AdapterId::LlamaCpp && fit_on {
            set_flag(&mut spec.args, &["-fitt", "--fit-target"], &mib.to_string());
        }
    }
    if fit.offload && spec.adapter == AdapterId::SdCpp && !spec.args.iter().any(|a| a == "--offload-to-cpu") {
        spec.args.push("--offload-to-cpu".into());
    }
    Ok(())
}

/// Set a flag's value in an arg list: every `<flag> <value>` and `<flag>=<value>` of any of `names` is replaced;
/// without one, `<names[0]> <value>` is appended.
fn set_flag(args: &mut Vec<String>, names: &[&str], value: &str) {
    let mut found = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].clone();
        if names.contains(&a.as_str()) {
            found = true;
            if i + 1 < args.len() {
                args[i + 1] = value.to_string();
            } else {
                args.push(value.to_string());
            }
            i += 2;
            continue;
        }
        if let Some((f, _)) = a.split_once('=') {
            if names.contains(&f) {
                found = true;
                args[i] = format!("{f}={value}");
            }
        }
        i += 1;
    }
    if !found {
        args.push(names[0].to_string());
        args.push(value.to_string());
    }
}
