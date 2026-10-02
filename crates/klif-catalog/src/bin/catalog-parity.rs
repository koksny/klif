//! Parity helper (verification, not a test suite): prints, as JSON lines, the launch plan this crate builds
//! for every reachable launcher profile under the same four settings sets the research verifier uses
//! (`.local/research/Verify-KlifCatalog.ps1`), plus the plans of the four tier defaults.
//!
//! The verifier compares these lines with the old GUI's own code (argv, cwd, session/log names, whether
//! LLAMA_API_KEY is set). Secret values are never printed: only environment variable NAMES and a boolean
//! `keyOk` that says whether the value equalled the trimmed dummy key the settings set supplied.
//!
//! Usage: catalog-parity [--saved-cache N] [--saved-port N] [--saved-vision on|off] [--saved-kv q8_0]
//!                       [--saved-mode Thinking] [--dump]
//! `--dump` prints, instead of the parity lines, the tier default recipes, the view-model slots built from them
//! (with port 7030 reported busy) and the result of a few Tune patches, as pretty JSON, for eyeballing.
//! The config is found the usual way (KLIF_CONFIG, .local/klif.toml upward from the working directory).

use klif_catalog::chrono_like::LocalStamp;
use klif_catalog::{Catalog, EnvValue, OldLauncherDefaults};
use klif_common::config::Config;
use klif_common::vm::{Backend, RecipePatch, SlotId};
use klif_common::Secret;
use serde_json::{json, Value};

struct Set {
    name: &'static str,
    cache: u32,
    port: u16,
    vision: bool,
    kv: String,
    mode: String,
    key: &'static str,
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let saved_cache: u32 = arg_value(&args, "--saved-cache").and_then(|v| v.parse().ok()).unwrap_or(2048);
    let saved_port: u16 = arg_value(&args, "--saved-port").and_then(|v| v.parse().ok()).unwrap_or(7030);
    let saved_vision = arg_value(&args, "--saved-vision").map(|v| v.eq_ignore_ascii_case("on")).unwrap_or(true);
    let saved_kv = arg_value(&args, "--saved-kv").unwrap_or_else(|| "q8_0".into());
    let saved_mode = arg_value(&args, "--saved-mode").unwrap_or_else(|| "Thinking".into());

    let cfg = Config::load()?;
    let started = std::time::Instant::now();
    let catalog = Catalog::load(&cfg)?;
    eprintln!("catalog origin: {:?}; warnings: {}; loaded in {:.2} s", catalog.origin(), catalog.warnings().len(), started.elapsed().as_secs_f64());

    if args.iter().any(|a| a == "--dump") {
        return dump(&catalog, &cfg, &args);
    }

    // Same instant as the verifier's fixed clock.
    let stamp = LocalStamp { year: 2026, month: 10, day: 2, hour: 12, minute: 34, second: 56, millis: 789 };

    let sets = [
        Set { name: "saved-state", cache: saved_cache, port: saved_port, vision: saved_vision, kv: saved_kv.clone(), mode: saved_mode.clone(), key: "" },
        Set { name: "alt-instruct", cache: 32768, port: 7033, vision: false, kv: "q4_0".into(), mode: "Instruct".into(), key: "dummy-key-ABC" },
        Set { name: "alt-thinking", cache: 8192, port: 7035, vision: true, kv: "q4_0".into(), mode: "Thinking".into(), key: "  padded-key  " },
        Set { name: "big-cache-q8", cache: 16384, port: 7031, vision: false, kv: "q8_0".into(), mode: "Instruct".into(), key: "" },
    ];

    let keys = catalog.reachable_profile_keys();
    let mut runs = 0usize;
    let mut failures = 0usize;
    for set in &sets {
        for key in &keys {
            let Some(mut recipe) = catalog.recipe_for_profile(key) else {
                failures += 1;
                println!("{}", json!({"set": set.name, "key": key, "error": "no recipe for profile"}));
                continue;
            };
            let slot = catalog.home_slot(&recipe.card_id).unwrap_or(SlotId::Medium);
            // Every field the settings set defines, whether or not the card uses it: plan() must ignore the rest.
            recipe.prompt_cache_mib = Some(set.cache);
            recipe.port = Some(set.port);
            recipe.vision = Some(set.vision);
            recipe.kv_type = Some(set.kv.clone());
            recipe.mode = Some(set.mode.clone());
            let secret = Secret::new(set.key);
            match catalog.plan_unchecked(&cfg, slot, &recipe, secret.as_ref(), stamp) {
                Ok(plan) => {
                    runs += 1;
                    println!("{}", plan_json(Some(set.name), key, &plan, set.key));
                }
                Err(e) => {
                    failures += 1;
                    println!("{}", json!({"set": set.name, "key": key, "error": format!("{e:#}")}));
                }
            }
        }
    }

    // The four tier defaults, built the way the engine builds them (config tiers, old-launcher values).
    let old = OldLauncherDefaults {
        cache_type: Some(saved_kv),
        vision: Some(saved_vision),
        generation_mode: Some(saved_mode),
        prompt_cache_mib: Some(saved_cache),
        server_port: Some(saved_port),
    };
    let defaults = catalog.default_recipes(&cfg, &old);
    for slot in SlotId::ALL {
        let Some(recipe) = defaults.get(&slot) else { continue };
        match catalog.plan_unchecked(&cfg, slot, recipe, None, stamp) {
            Ok(plan) => {
                let mut v = plan_json(None, &plan.profile_key, &plan, "");
                v["slot"] = json!(slot.as_str());
                v["recipe"] = serde_json::to_value(recipe)?;
                println!("{v}");
            }
            Err(e) => {
                failures += 1;
                println!("{}", json!({"slot": slot.as_str(), "error": format!("{e:#}")}));
            }
        }
    }

    eprintln!("plans built: {runs} ({} profiles x {} settings sets); failures: {failures}", keys.len(), sets.len());
    Ok(())
}

fn plan_json(set: Option<&str>, key: &str, plan: &klif_catalog::LaunchPlan, input_key: &str) -> Value {
    let env_set_names: Vec<&str> = plan.env_set.iter().map(|(n, _)| n.as_str()).collect();
    // Did LLAMA_API_KEY carry exactly the trimmed key that was supplied? (compared here, never printed)
    let expected = input_key.trim();
    let key_value = plan.env_set.iter().find(|(n, _)| n == "LLAMA_API_KEY").map(|(_, v)| match v {
        EnvValue::Secret(s) => s.expose().to_string(),
        EnvValue::Plain(s) => s.clone(),
    });
    let key_ok = match (&key_value, expected.is_empty() || plan.model.engine == "sd.cpp") {
        (None, true) => true,
        (Some(v), false) => v == expected,
        _ => false,
    };
    let layers: Vec<Value> = plan.expected_layers.iter().map(|l| json!([format!("{:?}", l.id).to_lowercase(), l.gib])).collect();
    json!({
        "set": set,
        "key": key,
        "argv": plan.args,
        "exe": plan.exe.file_name().map(|n| n.to_string_lossy().to_string()),
        "cwd": plan.cwd.to_string_lossy(),
        "session": plan.session_name,
        "out": plan.out_log.to_string_lossy(),
        "err": plan.err_log.to_string_lossy(),
        "envRemove": plan.env_remove,
        "envSet": env_set_names,
        "keyOk": key_ok,
        "port": plan.port,
        "host": plan.host,
        "spec": plan.spec_mode,
        "layers": layers,
    })
}

fn dump(catalog: &Catalog, cfg: &Config, args: &[String]) -> anyhow::Result<()> {
    let old = OldLauncherDefaults {
        cache_type: Some(arg_value(args, "--saved-kv").unwrap_or_else(|| "q8_0".into())),
        vision: Some(true),
        generation_mode: Some("Thinking".into()),
        prompt_cache_mib: Some(2048),
        server_port: Some(7030),
    };
    let recipes = catalog.default_recipes(cfg, &old);
    println!("== default recipes");
    for (slot, r) in &recipes {
        println!("{}: {}", slot.as_str(), serde_json::to_string(r)?);
    }
    let busy: u16 = arg_value(args, "--busy").and_then(|v| v.parse().ok()).unwrap_or(1235);
    let live = klif_catalog::LiveFacts { foreign_ports: vec![busy] };
    let t = std::time::Instant::now();
    let slots = catalog.slots(&recipes, &live);
    eprintln!("slots() took {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);
    let t = std::time::Instant::now();
    for _ in 0..100 {
        let _ = catalog.slots(&recipes, &live);
    }
    eprintln!("slots() x100 (warm cache) took {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);
    println!("== slots (port {busy} reported busy)");
    for s in &slots {
        println!("{}", serde_json::to_string_pretty(s)?);
    }
    println!("== plan() with the file-existence gate");
    let stamp = LocalStamp { year: 2026, month: 10, day: 2, hour: 12, minute: 34, second: 56, millis: 789 };
    for (slot, r) in &recipes {
        match catalog.plan(cfg, *slot, r, None, stamp) {
            Ok(p) => println!("{}: ok  port={} host={} session={} env_set={:?} layers={}", slot.as_str(), p.port, p.host, p.session_name, p.env_set.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), p.expected_layers.len()),
            Err(e) => println!("{}: refused: {e:#}", slot.as_str()),
        }
    }
    println!("== patches");
    let cases: Vec<(SlotId, &str, RecipePatch)> = vec![
        (SlotId::High, "card -> qwen-fn-q2 (offers only 16k/32k)", RecipePatch { card_id: Some("qwen-fn-q2".into()), ..Default::default() }),
        (SlotId::High, "card -> qwen-fn-coder, ctx 131072, cache 8192 (cap 2048)", RecipePatch { card_id: Some("qwen-fn-coder".into()), ctx_tokens: Some(131072), prompt_cache_mib: Some(8192), ..Default::default() }),
        (SlotId::High, "mode -> Instruct", RecipePatch { mode: Some("Instruct".into()), ..Default::default() }),
        (SlotId::Medium, "backend -> Vulkan", RecipePatch { backend: Some(Backend::Vulkan), ..Default::default() }),
        (SlotId::Medium, "hardware -> Dual", RecipePatch { hardware: Some("Dual".into()), ..Default::default() }),
        (SlotId::Medium, "card -> bonsai2-pq2 (kv from table, no vision)", RecipePatch { card_id: Some("bonsai2-pq2".into()), ctx_tokens: Some(262144), ..Default::default() }),
        (SlotId::Medium, "card -> qwen-iq3-dflash + hardware Dual (not offered)", RecipePatch { card_id: Some("qwen-iq3-dflash".into()), hardware: Some("Dual".into()), ..Default::default() }),
        (SlotId::Medium, "ctx 12345 (not offered), vision off", RecipePatch { ctx_tokens: Some(12345), vision: Some(false), ..Default::default() }),
        (SlotId::Low, "card -> gemma-12b, ctx 262144, kv q4_0", RecipePatch { card_id: Some("gemma-12b".into()), ctx_tokens: Some(262144), kv_type: Some("q4_0".into()), ..Default::default() }),
        (SlotId::Krea, "card -> qwen-image-21", RecipePatch { card_id: Some("qwen-image-21".into()), ..Default::default() }),
        (SlotId::Krea, "image size 720x1024, hardware Dual", RecipePatch { image_size: Some("720x1024".into()), hardware: Some("Dual".into()), ..Default::default() }),
    ];
    for (slot, what, patch) in cases {
        let cur = &recipes[&slot];
        let next = catalog.apply_patch(slot, cur, &patch);
        println!("{} | {what}
   {}", slot.as_str(), serde_json::to_string(&next)?);
    }
    Ok(())
}
