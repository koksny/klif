//! The launch plan: byte-for-byte what the old GUI's `Start-GuiLaunch` runs (see
//! `.local/research/reports/catalog.md` section 1.3 and the oracle comparison in `catalog-parity`).
//!
//! argv, in order:
//!   1. `-NoProfile -ExecutionPolicy Bypass -File <profile.script>`
//!   2. each profile argument as `-Name` then its value, in the launcher's order; for non-sd cards `Port` and
//!      `PromptCacheMiB` are dropped, and for the GenerationMode-override cards the value comes from the recipe
//!   3. non-sd only: `-PromptCacheMiB <capped cache> -Port <PORT setting>`
//!   4. dense Qwen 27B only: `-Vision on|off`
//!   5. non-sd only: `-CacheType q4_0|q8_0`

use anyhow::{anyhow, bail, Result};
use klif_common::config::Config;
use klif_common::vm::{Availability, Recipe, SlotId};
use klif_common::Secret;

use crate::chrono_like::LocalStamp;
use crate::{Catalog, EnvValue, LaunchPlan};

/// The only variable the GUI manages around the child (removed always; set for llama cards when a key exists).
const API_KEY_VAR: &str = "LLAMA_API_KEY";

/// llama.cpp reads `-lv` (log verbosity) from this variable (`common/arg.cpp`: `set_env("LLAMA_ARG_LOG_VERBOSITY")`
/// in every tree the launcher uses). Level 4 prints the `... buffer size` lines the VRAM composition needs.
/// An explicit `-lv` on a starter's command line (Flash-Next passes `-lv 4`) takes precedence over it.
const LOG_VERBOSITY_VAR: &str = "LLAMA_ARG_LOG_VERBOSITY";
const LOG_VERBOSITY_VALUE: &str = "4";

pub(crate) fn plan(
    c: &Catalog,
    cfg: &Config,
    slot: SlotId,
    recipe: &Recipe,
    api_key: Option<&Secret>,
    now_local: LocalStamp,
    check_files: bool,
) -> Result<LaunchPlan> {
    let card = c
        .data
        .card(&recipe.card_id)
        .ok_or_else(|| anyhow!("The model {:?} is no longer in the launcher catalog", recipe.card_id))?;
    let ctx = c.recipe_ctx(card, recipe);
    let profile = c.profile_for(card, recipe.backend, &recipe.hardware, ctx).ok_or_else(|| {
        anyhow!(
            "The launcher has no profile for {} on {} with this context",
            c.display_name(card),
            c.hardware_label(&recipe.hardware)
        )
    })?;
    if check_files {
        match c.profile_state(profile) {
            Availability::Ready => {}
            Availability::ScriptMissing => bail!("The starter script for {} was not found", c.display_name(card)),
            Availability::ModelMissing => bail!("The model file for {} was not found", c.display_name(card)),
            Availability::BuildRequired => bail!("The {} server binary for {} is not built", klif_backend(recipe), c.display_name(card)),
            _ => {}
        }
    }

    let sd = card.is_sd;
    let eff = c.effective(card, profile, recipe);
    let rules = &c.data.rules;

    // ---- argv ------------------------------------------------------------------------------
    let mut args: Vec<String> = rules.fixed_args.clone();
    args.push(profile.script.to_string_lossy().into_owned());

    let mut pairs: Vec<(String, String)> = profile.args.clone();
    if c.data.is_gen_override(&card.id) {
        let mode = eff.mode.clone().unwrap_or_else(|| c.data.defaults.generation_mode.clone());
        let mut hit = false;
        for pair in pairs.iter_mut() {
            if pair.0 == rules.generation_mode_arg {
                pair.1 = mode.clone();
                hit = true;
            }
        }
        if !hit {
            pairs.push((rules.generation_mode_arg.clone(), mode));
        }
    }
    for (name, value) in pairs {
        if !sd && rules.skip_when_not_sd.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            continue;
        }
        args.push(format!("-{name}"));
        args.push(value);
    }
    if !sd {
        args.push("-PromptCacheMiB".into());
        args.push(eff.cache.to_string());
        args.push("-Port".into());
        args.push(eff.port_setting.to_string());
    }
    if card.is_dense27 {
        args.push("-Vision".into());
        args.push(if eff.vision { "on" } else { "off" }.into());
    }
    if !sd {
        args.push("-CacheType".into());
        args.push(eff.kv.clone());
    }

    // ---- environment -----------------------------------------------------------------------
    let env_remove = vec![API_KEY_VAR.to_string()];
    let mut env_set: Vec<(String, EnvValue)> = Vec::new();
    if !sd {
        if let Some(key) = api_key {
            env_set.push((API_KEY_VAR.to_string(), EnvValue::Secret(key.clone())));
        }
        if cfg.telemetry.verbose_llama_logs {
            env_set.push((LOG_VERBOSITY_VAR.to_string(), EnvValue::Plain(LOG_VERBOSITY_VALUE.to_string())));
        }
    }

    // ---- working directory, session and logs -----------------------------------------------
    let cwd = profile.script.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let session_name = session_name(&rules.session_format, &now_local.format(), &card.id, eff.port);
    let logs = c.logs_dir(cfg);
    let out_log = logs.join(format!("{session_name}.out.log"));
    let err_log = logs.join(format!("{session_name}.err.log"));

    let host = if sd { cfg.net.image_host.clone() } else { cfg.net.llm_host.clone() };
    let model = c.model_ref(card, profile, recipe, &eff);
    let spec_mode = model.spec_mode.clone();
    let expected_layers = c.expected_layers(card, profile, recipe, &eff).unwrap_or_default();

    Ok(LaunchPlan {
        slot,
        kind: slot.kind(),
        card_id: card.id.clone(),
        profile_key: profile.key.clone(),
        model,
        exe: crate::export::powershell_exe(),
        args,
        cwd,
        env_remove,
        env_set,
        session_name,
        out_log,
        err_log,
        port: eff.port,
        host,
        expected_layers,
        spec_mode,
    })
}

fn klif_backend(r: &Recipe) -> &'static str {
    crate::data::Data::backend_str(r.backend)
}

/// `klif-{0}-{1}-p{2}` with the date stamp, card id and effective port, every character outside
/// `[A-Za-z0-9_.-]` replaced by `-` (the GUI's sanitiser).
fn session_name(format: &str, stamp: &str, card_id: &str, port: u16) -> String {
    let raw = format.replace("{0}", stamp).replace("{1}", card_id).replace("{2}", &port.to_string());
    raw.chars().map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') { ch } else { '-' }).collect()
}
