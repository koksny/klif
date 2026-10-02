//! Expected VRAM of a launch ("will it fit"). An ESTIMATE for the inference GPU, not a measurement:
//! weights come from the model file on disk, the rest from small per-family coefficient tables derived from
//! the benchmark logs (`llama-server -lv 4`: `load_tensors` / `llama_kv_cache` / `sched_reserve` lines) and the
//! sd.cpp server logs. The telemetry crate replaces these numbers with the real ones once a session is up.
//!
//! Sources of the constants (all per model family, HIP/Vulkan on a 16 GiB RDNA4 card):
//! - Dense Qwen 3.8 27B (16 full-attention layers + recurrent state):
//!   KV 576 MiB at 32768 tokens with q4_0 -> 18432 B/token (q4_0), 34816 B/token (q8_0) by the block sizes
//!   (4.5 vs 8.5 bits/element); recurrent state 149.62 MiB, 448.88 MiB with MTP, 598.5 MiB with DFlash;
//!   MTP draft KV f16, 1 layer = 4096 B/token; HIP compute buffer 184.27 MiB at 32k and 616.27 MiB at 128k
//!   (linear: 40.3 MiB + 0.004394 MiB/token), Vulkan 41-130 MiB; token embeddings stay in host RAM
//!   (file size minus ~0.6 GiB is on the device).
//! - The residual between composed buffers and the Windows per-process VRAM counter is +0.30 GiB on HIP and
//!   +0.06 GiB on Vulkan (60 benchmark runs); it is added to the buffers layer.
//! - Flash-Next (125B-A6B MoE, experts mostly in host RAM): KV 408 MiB + indexer KV 153 MiB at 32k with q8_0
//!   -> 17952 B/token; recurrent state 112.57 MiB (+225 MiB with MTP); compute buffer scales with context
//!   and micro-batch (2862 MiB at 32k, 8110 MiB at 128k with ub 4096); weights on the device are about 13.4 %
//!   of the files on disk at 40 CPU-resident MoE layers, 0.85 GiB less per additional CPU layer.
//! - sd.cpp (Krea 2, Qwen Image): DiT weights = file size (the log shows `diffusion_model 7931 MB` for the
//!   7.75 GiB file), VAE 242 MB (Krea) / 676 MB (Qwen Image), activations scale with the pixel count and are
//!   NOT measured (the server log carries no VRAM figure).

use klif_common::vm::{Backend, VramLayer, VramLayerId};

use crate::data::{Card, Profile};

const MIB_PER_GIB: f64 = 1024.0;
const BYTES_PER_MIB: f64 = 1_048_576.0;

/// Fit target of the Gemma starters (`-fitt 1024` margin on a 16 GiB card): their weights are trimmed to this.
const GEMMA_FIT_BUDGET_GIB: f64 = 14.9;

/// Everything the estimate depends on.
pub(crate) struct Input<'a> {
    pub card: &'a Card,
    pub profile: &'a Profile,
    pub backend: Backend,
    /// True when the profile runs on the primary (inference) GPU; other hardware (RAM tier, dual, a second
    /// card) gets no estimate.
    pub primary_gpu: bool,
    pub kv_type: &'a str,
    pub vision: bool,
    /// UI spec label ("MTP + n-gram", "n-gram", "DFlash", "MTP").
    pub spec: Option<&'a str>,
    /// Model size on disk in GiB (all shards), when the file exists.
    pub file_gib: Option<f64>,
}

fn layer(id: VramLayerId, label: &str, gib: f64) -> VramLayer {
    VramLayer { id, label: label.to_string(), gib: (gib * 100.0).round() / 100.0 }
}

fn kv_factor(kv: &str) -> f64 {
    match kv {
        "q4_0" => 4.5 / 8.5,
        "f16" => 16.0 / 8.5,
        _ => 1.0,
    }
}

/// Parse the launcher's size text ("11.29 GiB", "10.18+0.54 GiB", "29.6 GB resident") into GiB.
pub(crate) fn parse_size_label(text: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let unit_gb = lower.contains("gb") && !lower.contains("gib");
    let mut total = 0.0;
    let mut any = false;
    for part in lower.split('+') {
        let num: String = part.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if let Ok(v) = num.parse::<f64>() {
            total += v;
            any = true;
        }
    }
    if !any {
        return None;
    }
    Some(if unit_gb { total * 1e9 / (1u64 << 30) as f64 } else { total })
}

pub(crate) fn estimate(inp: &Input<'_>) -> Option<Vec<VramLayer>> {
    if !inp.primary_gpu || inp.backend == Backend::Cpu {
        return None;
    }
    let file_gib = inp.file_gib.or_else(|| parse_size_label(&inp.card.file_size_label)).unwrap_or(8.0);
    if inp.card.is_sd {
        return Some(image(inp, file_gib));
    }
    let ctx = inp.profile.context as f64;
    let hip = inp.backend == Backend::Hip;
    let overhead_gib = if hip { 0.30 } else { 0.06 };
    let kvf = kv_factor(inp.kv_type);
    let spec = inp.spec.unwrap_or("");
    let mtp = spec.contains("MTP");
    let dflash = spec.contains("DFlash");

    if inp.card.is_dense27 || inp.card.is_bonsai {
        let host_embed = if inp.card.is_bonsai { 0.32 } else { 0.6 };
        let weights = (file_gib - host_embed).max(0.5);
        let kv = ctx * 34816.0 / BYTES_PER_MIB * kvf / MIB_PER_GIB;
        let rs = 149.62 / MIB_PER_GIB;
        let compute = if hip { (40.3 + 0.004394 * ctx) / MIB_PER_GIB } else { 0.08 };
        let mut out = vec![
            layer(VramLayerId::Weights, "weights", weights),
            layer(VramLayerId::Kv, "KV cache", kv + rs),
            layer(VramLayerId::Buffers, "buffers", compute + overhead_gib),
        ];
        if inp.vision && inp.card.is_dense27 && inp.profile.context <= 65536 && file_gib <= 12.6 {
            // Get-Qwen38VisionArgs: the mmproj goes to the GPU only at <= 64k and <= 12.6 GiB of weights.
            out.push(layer(VramLayerId::Projector, "vision projector", 0.86));
        }
        if mtp || dflash {
            // recurrent state: 149.62 MiB alone, 448.88 MiB with the MTP context, 598.5 MiB with DFlash
            let (rs_extra, draft_weights, draft_compute) = if dflash {
                (598.5 - 149.62, 0.54 * MIB_PER_GIB, 280.0)
            } else {
                (448.88 - 149.62, 0.0, 54.0 + 0.000336 * ctx)
            };
            let draft_kv = ctx * 4096.0 / BYTES_PER_MIB;
            out.push(layer(VramLayerId::Draft, "draft", (rs_extra + draft_weights + draft_kv + draft_compute) / MIB_PER_GIB));
        }
        return Some(out);
    }

    if inp.card.is_flash_next() {
        let cpu_moe = inp.profile.arg_u32("CpuMoeLayers").unwrap_or(40) as f64;
        let ubatch = inp.profile.arg_u32("UBatch").unwrap_or(4096) as f64;
        let weights = (0.134 * file_gib - 0.85 * (cpu_moe - 40.0)).clamp(2.0, 14.0);
        let kv = ctx * 17952.0 / BYTES_PER_MIB * kvf / MIB_PER_GIB;
        let rs = 112.57 / MIB_PER_GIB;
        let compute = (1113.0 + 0.05339 * ctx) * (ubatch / 4096.0) / MIB_PER_GIB;
        let mut out = vec![
            layer(VramLayerId::Weights, "weights", weights),
            layer(VramLayerId::Kv, "KV cache", kv + rs),
            layer(VramLayerId::Buffers, "buffers", compute + overhead_gib),
        ];
        if mtp {
            let draft = (225.14 + 58.0 + 34.0 + 840.0 + 0.00781 * ctx) / MIB_PER_GIB;
            out.push(layer(VramLayerId::Draft, "draft", draft));
        }
        return Some(out);
    }

    // Gemma (and any other plain llama.cpp card): coefficients are rougher, see the module comment.
    let (kv_q8_bytes, host_embed) = if inp.card.is_gemma() {
        if inp.card.size_label.starts_with("12") {
            (15_000.0, 0.5)
        } else if inp.card.size_label.starts_with("31") {
            (9_000.0, 1.2)
        } else {
            (4_500.0, 1.2)
        }
    } else {
        (20_000.0, 0.5)
    };
    let kv = ctx * kv_q8_bytes / BYTES_PER_MIB * kvf / MIB_PER_GIB;
    let compute = if hip { (60.0 + 0.002 * ctx) / MIB_PER_GIB } else { 0.1 };
    let buffers = compute + overhead_gib + 0.3;
    let mut weights = (file_gib - host_embed).max(0.5);
    if inp.card.is_gemma() {
        weights = weights.min((GEMMA_FIT_BUDGET_GIB - kv - buffers).max(1.0));
    }
    Some(vec![
        layer(VramLayerId::Weights, "weights", weights),
        layer(VramLayerId::Kv, "KV cache", kv),
        layer(VramLayerId::Buffers, "buffers", buffers),
    ])
}

fn image(inp: &Input<'_>, file_gib: f64) -> Vec<VramLayer> {
    let (w, h) = parse_size(inp.profile.arg("Size").unwrap_or(&inp.profile.context_label)).unwrap_or((512, 768));
    let pixels = (w as f64 * h as f64) / (512.0 * 768.0);
    let qwen_image = inp.card.family_id == "qwen-image";
    let (act_at_512x768, vae) = if qwen_image { (2.2, 0.63) } else { (3.2, 0.24) };
    vec![
        layer(VramLayerId::Weights, "DiT layers", file_gib),
        layer(VramLayerId::Buffers, "activations", act_at_512x768 * pixels),
        layer(VramLayerId::Other, "VAE", vae),
    ]
}

pub(crate) fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (a, b) = text.trim().split_once(['x', 'X'])?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Sum of the layers in GiB.
#[allow(dead_code)]
pub(crate) fn total_gib(layers: &[VramLayer]) -> f64 {
    layers.iter().map(|l| l.gib).sum()
}
