//! What a preset's args (and env equivalents such as `LLAMA_ARG_*`) say, per adapter: port, host, ctx,
//! model, KV type, vision, mode, speculation, image size, metrics, CPU offload, telemetry breakers. Feeds
//! ModelRef, the effective port/host rules (SPEC 2.3) and the validation issues. Owner: package B.
//!
//! Flag tables (names compared after llama.cpp's own normalisation: in `--long_flag` an `_` counts as `-`;
//! `--flag=value` is read like `--flag value`):
//! - llama.cpp: `--port` (LLAMA_ARG_PORT), `--host` (LLAMA_ARG_HOST), `-c/--ctx-size` (LLAMA_ARG_CTX_SIZE),
//!   `-m/--model` (LLAMA_ARG_MODEL), `-mm/--mmproj` (LLAMA_ARG_MMPROJ) → vision, `--no-mmproj`
//!   (LLAMA_ARG_NO_MMPROJ), `-ctk/--cache-type-k` (LLAMA_ARG_CACHE_TYPE_K), `--reasoning on|off`,
//!   `--reasoning-budget 0` (LLAMA_ARG_THINK_BUDGET), `--chat-template-kwargs` thinking keys
//!   (LLAMA_CHAT_TEMPLATE_KWARGS) → mode Thinking / Instruct, `--spec-type` (draft-mtp → MTP, ngram-* → ngram,
//!   draft-dflash → DFlash) and `-md/--model-draft` (LLAMA_ARG_MODEL_DRAFT) → spec mode, `--metrics`
//!   (LLAMA_ARG_ENDPOINT_METRICS), CPU offload `-cmoe/--cpu-moe` (LLAMA_ARG_CPU_MOE), `-ncmoe/--n-cpu-moe N>0`
//!   (LLAMA_ARG_N_CPU_MOE), `-ot/--override-tensor …=CPU`, `-ngl 0` (LLAMA_ARG_N_GPU_LAYERS); telemetry
//!   breakers `--no-log-prefix`, `--no-log-timestamps`, `--log-disable`, `--log-jsonl`, `-lv/--verbosity` < 4 and
//!   the env forms LLAMA_ARG_LOG_PREFIX/TIMESTAMPS = false, LLAMA_ARG_LOG_JSONL = true, LLAMA_ARG_LOG_VERBOSITY < 4.
//! - sd.cpp: `--listen-port`, `--listen-ip`, `--diffusion-model` / `-m/--model`, `-W/--width` + `-H/--height` →
//!   "WxH", `--llm_vision` → vision, `--ref-image-args` / `--default-lora` → mode "Edit", `--offload-to-cpu`,
//!   component weights (`--vae`, `--clip_l`, `--clip_g`, `--t5xxl`, `--llm`, `--llm_vision`).
//! - vllm: `--port`, `--host`, `--max-model-len`, `--model` or the token after `serve`, `--kv-cache-dtype`,
//!   `--speculative-config` (method), `--quantization`, `--cpu-offload-gb N>0`; /metrics always.
//! - openai / generic: `--port`, `--host` (the common convention; KLIF cannot know more).
//!
//! In llama.cpp the environment is read before argv, so an argument wins over its env equivalent.

use klif_common::vm::AdapterId;
use std::collections::BTreeMap;

/// Facts read from final (placeholder-expanded) args and env. `None` = not stated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArgFacts {
    pub port: Option<u16>,
    pub host: Option<String>,
    pub ctx: Option<u32>,
    pub model: Option<String>,
    pub mmproj: Option<String>,
    pub kv_type: Option<String>,
    pub vision: Option<bool>,
    /// "Thinking" / "Instruct" (llama.cpp), "Edit" (sd.cpp).
    pub mode: Option<String>,
    /// "MTP", "ngram", "DFlash", "draft", "MTP+ngram".
    pub spec_mode: Option<String>,
    /// "WxH" (sd.cpp).
    pub image_size: Option<String>,
    /// /metrics is enabled (llama.cpp `--metrics` / LLAMA_ARG_ENDPOINT_METRICS, vllm always).
    pub metrics: bool,
    /// Weights are (partly) kept in system RAM (`--cpu-moe`, `-ncmoe`, `--n-cpu-moe`, `-ot`, `--override-tensor`).
    pub cpu_offload: bool,
    /// The port value exactly as written (also when it is a placeholder such as `{port}` or not a number).
    /// `Some` = the args/env name the port at all.
    pub port_text: Option<String>,
    /// The host value exactly as written (`Some` = the args/env name the host at all).
    pub host_text: Option<String>,
    /// llama.cpp `-md/--model-draft`.
    pub draft_model: Option<String>,
    /// sd.cpp component model files (VAE, text encoders...), for the file-size VRAM estimate.
    pub extra_weights: Vec<String>,
    /// vllm `--quantization`.
    pub quant: Option<String>,
    /// llama.cpp settings that break KLIF's log telemetry, as (field, sentence) pairs.
    pub log_breakers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum F {
    Port,
    Host,
    Ctx,
    Model,
    Mmproj,
    NoMmproj,
    Kv,
    Reasoning,
    ReasoningBudget,
    TemplateKwargs,
    SpecType,
    ModelDraft,
    Metrics,
    CpuMoe,
    NCpuMoe,
    OverrideTensor,
    Ngl,
    Verbosity,
    NoLogPrefix,
    NoLogTimestamps,
    LogDisable,
    LogJsonl,
    // sd.cpp
    DiffusionModel,
    Width,
    Height,
    LlmVision,
    Edit,
    OffloadToCpu,
    ExtraWeights,
    // vllm
    SpecConfig,
    Quantization,
    CpuOffloadGb,
}

/// One flag: its spellings (normalised), whether it takes a value, what it means, its env equivalent.
struct Flag {
    names: &'static [&'static str],
    value: bool,
    fact: F,
    env: Option<&'static str>,
}

const fn fl(names: &'static [&'static str], value: bool, fact: F, env: Option<&'static str>) -> Flag {
    Flag { names, value, fact, env }
}

const LLAMA: &[Flag] = &[
    fl(&["--port"], true, F::Port, Some("LLAMA_ARG_PORT")),
    fl(&["--host"], true, F::Host, Some("LLAMA_ARG_HOST")),
    fl(&["-c", "--ctx-size"], true, F::Ctx, Some("LLAMA_ARG_CTX_SIZE")),
    fl(&["-m", "--model"], true, F::Model, Some("LLAMA_ARG_MODEL")),
    fl(&["-mm", "--mmproj"], true, F::Mmproj, Some("LLAMA_ARG_MMPROJ")),
    fl(&["--no-mmproj"], false, F::NoMmproj, Some("LLAMA_ARG_NO_MMPROJ")),
    fl(&["-ctk", "--cache-type-k"], true, F::Kv, Some("LLAMA_ARG_CACHE_TYPE_K")),
    fl(&["--reasoning"], true, F::Reasoning, None),
    fl(&["--reasoning-budget"], true, F::ReasoningBudget, Some("LLAMA_ARG_THINK_BUDGET")),
    fl(&["--chat-template-kwargs"], true, F::TemplateKwargs, Some("LLAMA_CHAT_TEMPLATE_KWARGS")),
    fl(&["--spec-type"], true, F::SpecType, None),
    fl(&["-md", "--model-draft"], true, F::ModelDraft, Some("LLAMA_ARG_MODEL_DRAFT")),
    fl(&["--metrics"], false, F::Metrics, Some("LLAMA_ARG_ENDPOINT_METRICS")),
    fl(&["-cmoe", "--cpu-moe"], false, F::CpuMoe, Some("LLAMA_ARG_CPU_MOE")),
    fl(&["-ncmoe", "--n-cpu-moe"], true, F::NCpuMoe, Some("LLAMA_ARG_N_CPU_MOE")),
    fl(&["-ot", "--override-tensor"], true, F::OverrideTensor, None),
    fl(&["-ngl", "--gpu-layers", "--n-gpu-layers"], true, F::Ngl, Some("LLAMA_ARG_N_GPU_LAYERS")),
    fl(&["-lv", "--verbosity", "--log-verbosity"], true, F::Verbosity, Some("LLAMA_ARG_LOG_VERBOSITY")),
    fl(&["--no-log-prefix"], false, F::NoLogPrefix, None),
    fl(&["--no-log-timestamps"], false, F::NoLogTimestamps, None),
    fl(&["--log-disable"], false, F::LogDisable, None),
    fl(&["--log-jsonl"], false, F::LogJsonl, Some("LLAMA_ARG_LOG_JSONL")),
];

/// Env-only llama.cpp switches (their flag forms are the positive `--log-prefix` / `--log-timestamps`).
const LLAMA_ENV_FALSE_BREAKERS: &[(&str, &str)] =
    &[("LLAMA_ARG_LOG_PREFIX", "log prefixes"), ("LLAMA_ARG_LOG_TIMESTAMPS", "log timestamps")];

const SDCPP: &[Flag] = &[
    fl(&["--listen-port"], true, F::Port, None),
    fl(&["--listen-ip"], true, F::Host, None),
    fl(&["--diffusion-model"], true, F::DiffusionModel, None),
    fl(&["-m", "--model"], true, F::Model, None),
    fl(&["-W", "--width"], true, F::Width, None),
    fl(&["-H", "--height"], true, F::Height, None),
    fl(&["--llm-vision"], true, F::LlmVision, None),
    fl(&["--ref-image-args", "--default-lora"], true, F::Edit, None),
    fl(&["--offload-to-cpu"], false, F::OffloadToCpu, None),
    fl(&["--vae", "--clip-l", "--clip-g", "--t5xxl", "--llm"], true, F::ExtraWeights, None),
];

const VLLM: &[Flag] = &[
    fl(&["--port"], true, F::Port, None),
    fl(&["--host"], true, F::Host, None),
    fl(&["--max-model-len"], true, F::Ctx, None),
    fl(&["--model"], true, F::Model, None),
    fl(&["--kv-cache-dtype"], true, F::Kv, None),
    fl(&["--speculative-config"], true, F::SpecConfig, None),
    fl(&["-q", "--quantization"], true, F::Quantization, None),
    fl(&["--cpu-offload-gb"], true, F::CpuOffloadGb, None),
];

const GENERIC: &[Flag] = &[fl(&["--port"], true, F::Port, None), fl(&["--host"], true, F::Host, None)];

fn table(adapter: AdapterId) -> &'static [Flag] {
    match adapter {
        AdapterId::LlamaCpp => LLAMA,
        AdapterId::SdCpp => SDCPP,
        AdapterId::Vllm => VLLM,
        AdapterId::OpenAi | AdapterId::Generic => GENERIC,
    }
}

/// `--long_flag` → `--long-flag` (llama.cpp normalises underscores in long flags; sd.cpp spells some with `_`).
fn norm(flag: &str) -> String {
    if flag.starts_with("--") {
        flag.replace('_', "-")
    } else {
        flag.to_string()
    }
}

/// llama.cpp's boolean env parsing: true / 1 / on / enabled, false / 0 / off / disabled.
fn env_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "enabled" | "yes" => Some(true),
        "0" | "false" | "off" | "disabled" | "no" => Some(false),
        _ => None,
    }
}

fn env_get<'a>(env: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    env.get(name)
        .or_else(|| if cfg!(windows) { env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v) } else { None })
        .map(String::as_str)
}

/// One observation: the flag's fact and value, and where it came from ("args" or "env.NAME").
struct Seen {
    fact: F,
    value: Option<String>,
    field: String,
    /// The spelling as written (for sentences).
    name: String,
}

/// Read the facts of `args` + `env` for `adapter`.
pub fn from_args(adapter: AdapterId, args: &[String], env: &BTreeMap<String, String>) -> ArgFacts {
    let flags = table(adapter);
    let mut seen: Vec<Seen> = Vec::new();

    // Env first (llama.cpp reads it before argv; argv wins because it is applied later).
    for f in flags {
        if let Some(name) = f.env {
            if let Some(v) = env_get(env, name) {
                let value = if f.value {
                    Some(v.to_string())
                } else {
                    // A switch given by env: only "true" counts as set.
                    match env_bool(v) {
                        Some(true) => None,
                        _ => continue,
                    }
                };
                seen.push(Seen { fact: f.fact, value, field: format!("env.{name}"), name: name.to_string() });
            }
        }
    }

    // Args.
    let mut vllm_serve_model: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let tok = args[i].as_str();
        if adapter == AdapterId::Vllm && tok == "serve" {
            if let Some(next) = args.get(i + 1).filter(|n| !n.starts_with('-')) {
                vllm_serve_model = Some(next.clone());
                i += 2;
                continue;
            }
        }
        if !tok.starts_with('-') || tok.len() < 2 {
            i += 1;
            continue;
        }
        let (flag, inline) = match tok.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (tok, None),
        };
        let n = norm(flag);
        match flags.iter().find(|f| f.names.contains(&n.as_str())) {
            Some(f) if f.value => {
                let value = match inline {
                    Some(v) => Some(v),
                    None => {
                        let v = args.get(i + 1).cloned();
                        if v.is_some() {
                            i += 1;
                        }
                        v
                    }
                };
                seen.push(Seen { fact: f.fact, value, field: "args".into(), name: flag.to_string() });
            }
            Some(f) => seen.push(Seen { fact: f.fact, value: inline, field: "args".into(), name: flag.to_string() }),
            None => {}
        }
        i += 1;
    }

    let mut out = ArgFacts::default();
    let mut spec_parts: Vec<String> = Vec::new();
    let mut spec_type_given = false;
    let mut width: Option<String> = None;
    let mut height: Option<String> = None;
    let mut diffusion_model: Option<String> = None;
    let mut reasoning: Option<String> = None;
    let mut kwargs_mode: Option<String> = None;
    let mut budget_mode: Option<String> = None;

    for s in &seen {
        let v = s.value.as_deref().map(str::trim);
        match s.fact {
            F::Port => {
                out.port_text = v.map(str::to_string);
                out.port = v.and_then(|v| v.parse::<u16>().ok());
            }
            F::Host => {
                out.host_text = v.map(str::to_string);
                out.host = v.filter(|v| !v.contains('{') && !v.is_empty()).map(str::to_string);
            }
            F::Ctx => out.ctx = v.and_then(|v| v.parse::<u32>().ok()),
            F::Model => out.model = v.map(str::to_string),
            F::DiffusionModel => diffusion_model = v.map(str::to_string),
            F::Mmproj => {
                out.mmproj = v.map(str::to_string);
                out.vision = Some(true);
            }
            F::NoMmproj => {
                out.mmproj = None;
                out.vision = Some(false);
            }
            F::Kv => out.kv_type = v.map(str::to_string),
            F::Reasoning => reasoning = v.map(str::to_ascii_lowercase),
            F::ReasoningBudget => {
                budget_mode = match v.and_then(|v| v.parse::<i64>().ok()) {
                    Some(0) => Some("Instruct".into()),
                    _ => None,
                }
            }
            F::TemplateKwargs => {
                if let Some(t) = v.and_then(thinking_from_kwargs) {
                    kwargs_mode = Some(if t { "Thinking" } else { "Instruct" }.into());
                }
            }
            F::SpecType => {
                spec_type_given = true;
                for part in v.unwrap_or("").split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    push_unique(&mut spec_parts, spec_label(part));
                }
            }
            F::ModelDraft => out.draft_model = v.map(str::to_string),
            F::Metrics => out.metrics = true,
            F::CpuMoe => out.cpu_offload = true,
            F::NCpuMoe => {
                if v.and_then(|v| v.parse::<i64>().ok()).unwrap_or(1) > 0 {
                    out.cpu_offload = true;
                }
            }
            F::OverrideTensor => {
                if v.is_none_or(|v| v.to_ascii_lowercase().contains("cpu")) {
                    out.cpu_offload = true;
                }
            }
            F::Ngl => {
                if v.and_then(|v| v.parse::<i64>().ok()) == Some(0) {
                    out.cpu_offload = true;
                }
            }
            F::Verbosity => {
                if let Some(n) = v.and_then(|v| v.parse::<i64>().ok()) {
                    if n < 4 {
                        out.log_breakers.push((
                            s.field.clone(),
                            format!("{} {n} hides the llama.cpp log lines KLIF reads (VRAM composition needs 4 or more).", s.name),
                        ));
                    }
                }
            }
            F::NoLogPrefix | F::NoLogTimestamps | F::LogDisable | F::LogJsonl => {
                let what = match s.fact {
                    F::NoLogPrefix => "removes the log prefixes",
                    F::NoLogTimestamps => "removes the log timestamps",
                    F::LogDisable => "turns the log off",
                    _ => "switches the log to JSON lines",
                };
                out.log_breakers.push((
                    s.field.clone(),
                    format!("{} {what}, so KLIF's llama.cpp telemetry (phases, speed, VRAM) stops working.", s.name),
                ));
            }
            F::Width => width = v.map(str::to_string),
            F::Height => height = v.map(str::to_string),
            F::LlmVision => out.vision = Some(true),
            F::Edit => out.mode = Some("Edit".into()),
            F::OffloadToCpu => out.cpu_offload = true,
            F::ExtraWeights => {
                if let Some(v) = v.filter(|v| !v.is_empty()) {
                    out.extra_weights.push(v.to_string());
                }
            }
            F::SpecConfig => {
                spec_type_given = true;
                if let Some(label) = v.and_then(vllm_spec_label) {
                    push_unique(&mut spec_parts, label);
                }
            }
            F::Quantization => out.quant = v.map(str::to_string),
            F::CpuOffloadGb => {
                if v.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) > 0.0 {
                    out.cpu_offload = true;
                }
            }
        }
    }

    // sd.cpp: --llm_vision also names a weights file.
    if adapter == AdapterId::SdCpp {
        for s in &seen {
            if s.fact == F::LlmVision {
                if let Some(v) = s.value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                    out.extra_weights.push(v.to_string());
                }
            }
        }
        if diffusion_model.is_some() {
            out.model = diffusion_model;
        }
        if let (Some(w), Some(h)) = (&width, &height) {
            out.image_size = Some(format!("{w}x{h}"));
        }
    }
    if adapter == AdapterId::Vllm {
        if out.model.is_none() {
            out.model = vllm_serve_model;
        }
        out.metrics = true;
    }
    if adapter == AdapterId::LlamaCpp {
        // Env-only breakers.
        for (name, what) in LLAMA_ENV_FALSE_BREAKERS {
            if let Some(v) = env_get(env, name) {
                if env_bool(v) == Some(false) {
                    out.log_breakers.push((
                        format!("env.{name}"),
                        format!("{name}={v} removes the {what}, so KLIF's llama.cpp telemetry (phases, speed, VRAM) stops working."),
                    ));
                }
            }
        }
        out.mode = match reasoning.as_deref() {
            Some("on") | Some("true") | Some("1") | Some("enabled") => Some("Thinking".into()),
            Some("off") | Some("false") | Some("0") | Some("disabled") => Some("Instruct".into()),
            _ => kwargs_mode.or(budget_mode),
        };
        if out.draft_model.is_some() && !spec_type_given {
            push_unique(&mut spec_parts, "draft".into());
        }
    }
    if !spec_parts.is_empty() {
        out.spec_mode = Some(spec_parts.join("+"));
    }
    out
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !s.is_empty() && !v.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
        v.push(s);
    }
}

/// llama.cpp `--spec-type` value → display label.
fn spec_label(v: &str) -> String {
    let l = v.to_ascii_lowercase();
    if l.contains("mtp") {
        "MTP".into()
    } else if l.contains("ngram") {
        "ngram".into()
    } else if l.contains("dflash") {
        "DFlash".into()
    } else if l.starts_with("draft") {
        "draft".into()
    } else if l == "none" || l == "off" {
        String::new()
    } else {
        v.to_string()
    }
}

/// vLLM `--speculative-config` JSON → display label.
fn vllm_spec_label(v: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(v).ok()?;
    let method = json.get("method").and_then(|m| m.as_str()).map(str::to_ascii_lowercase);
    Some(match method.as_deref() {
        Some(m) if m.contains("mtp") => "MTP".into(),
        Some(m) if m.contains("ngram") => "ngram".into(),
        Some(m) if m.contains("eagle") => "EAGLE".into(),
        Some(m) if m.contains("medusa") => "Medusa".into(),
        Some(m) if m.contains("draft") => "draft".into(),
        Some(m) => m.to_string(),
        None if json.get("model").is_some() => "draft".into(),
        None => return None,
    })
}

/// `{"enable_thinking": false}` (also `thinking`, `reasoning`) → Some(false).
fn thinking_from_kwargs(v: &str) -> Option<bool> {
    let json: serde_json::Value = serde_json::from_str(v).ok()?;
    let obj = json.as_object()?;
    ["enable_thinking", "thinking", "reasoning", "enable_reasoning"].iter().find_map(|k| match obj.get(*k)? {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) => env_bool(s),
        _ => None,
    })
}
