//! Recipes: the per-tier defaults, Tune patches and the repair of dependent fields.
//!
//! A recipe is a plain selection (card, backend, hardware, context ...). It does not store derived launch
//! values: the Flash-Next prompt-cache cap is applied by `repair` (sticky, like the old GUI) and again at
//! plan time (pure `min`), and every other injected flag comes from the recipe at plan time.

use klif_common::config::{Config, TierCfg};
use klif_common::vm::{Backend, Recipe, RecipePatch, SlotId};

use crate::data::{Card, Data};
use crate::{Catalog, OldLauncherDefaults, Recipes};

/// Values used where a recipe field is missing: the old launcher state, else the launcher's own defaults.
#[derive(Debug, Clone)]
pub(crate) struct Fill {
    pub cache: u32,
    pub port: u16,
    pub kv: String,
    pub vision: bool,
    pub mode: String,
}

impl Fill {
    pub(crate) fn catalog(c: &Catalog) -> Fill {
        let d = &c.data.defaults;
        Fill { cache: d.prompt_cache_mib, port: d.server_port, kv: d.cache_type.clone(), vision: d.vision, mode: d.generation_mode.clone() }
    }

    fn from_old(c: &Catalog, old: &OldLauncherDefaults) -> Fill {
        let mut f = Fill::catalog(c);
        if let Some(v) = old.prompt_cache_mib {
            f.cache = v;
        }
        if let Some(v) = old.server_port {
            f.port = v;
        }
        if let Some(v) = &old.cache_type {
            if c.data.rules.kv_values.iter().any(|k| k == v) {
                f.kv = v.clone();
            }
        }
        if let Some(v) = old.vision {
            f.vision = v;
        }
        if let Some(v) = &old.generation_mode {
            if let Some(m) = c.data.rules.generation_modes.iter().find(|m| m.eq_ignore_ascii_case(v)) {
                f.mode = m.clone();
            }
        }
        f
    }

    fn from_recipe(c: &Catalog, r: &Recipe) -> Fill {
        let mut f = Fill::catalog(c);
        if let Some(v) = r.prompt_cache_mib {
            f.cache = v;
        }
        if let Some(v) = r.port {
            f.port = v;
        }
        if let Some(v) = &r.kv_type {
            f.kv = v.clone();
        }
        if let Some(v) = r.vision {
            f.vision = v;
        }
        if let Some(v) = &r.mode {
            f.mode = v.clone();
        }
        f
    }
}

/// What the caller explicitly asked to keep when a card swap leaves no profile for the current combination.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Intent {
    pub backend: bool,
    pub hardware: bool,
}

// ---------------------------------------------------------------------------------------------
// Groups
// ---------------------------------------------------------------------------------------------

pub(crate) fn home_slot(c: &Catalog, card_id: &str) -> Option<SlotId> {
    let card = c.data.card(card_id)?;
    Some(if card.is_image {
        SlotId::Krea
    } else if card.is_flash_next() {
        SlotId::High
    } else if card.is_dense27 || card.is_bonsai {
        SlotId::Medium
    } else {
        SlotId::Low
    })
}

/// The cards the Tune drawer offers for a slot, in catalog order:
/// high = Flash-Next, medium = dense Qwen 27B and Bonsai, low = Gemma first then any other language model,
/// krea = image cards (Krea and Qwen Image).
pub(crate) fn group(c: &Catalog, slot: SlotId) -> Vec<&Card> {
    let cards = &c.data.cards;
    match slot {
        SlotId::High => cards.iter().filter(|x| x.is_flash_next()).collect(),
        SlotId::Medium => cards.iter().filter(|x| x.is_dense27 || x.is_bonsai).collect(),
        SlotId::Low => {
            let mut out: Vec<&Card> = cards.iter().filter(|x| x.is_gemma() && !x.is_image).collect();
            out.extend(cards.iter().filter(|x| !x.is_gemma() && !x.is_image));
            out
        }
        SlotId::Krea => cards.iter().filter(|x| x.is_image).collect(),
    }
}

/// The launcher preset each tier falls back to (the old launcher's own four presets).
fn default_preset_id(slot: SlotId) -> &'static str {
    match slot {
        SlotId::High => "qwen-moe",
        SlotId::Medium => "qwen-dense",
        SlotId::Low => "gemma-tavern",
        SlotId::Krea => "krea",
    }
}

// ---------------------------------------------------------------------------------------------
// Building recipes
// ---------------------------------------------------------------------------------------------

pub(crate) fn cap_for(card: &Card, ctx: u32) -> Option<u32> {
    card.cache_caps.get(&ctx).copied()
}

/// A recipe for a profile: its card, backend, hardware and context, plus the fill values.
fn base_recipe(c: &Catalog, card: &Card, backend: Backend, hardware: &str, ctx: u32, fill: &Fill) -> Recipe {
    let mut r = Recipe { card_id: card.id.clone(), backend, hardware: hardware.to_string(), ..Recipe::default() };
    if card.is_image {
        r.image_size = card.ctx_label(ctx).map(str::to_string);
    } else {
        r.ctx_tokens = Some(ctx);
    }
    r.port = Some(fill.port);
    if !card.is_sd {
        r.prompt_cache_mib = Some(fill.cache);
        r.kv_type = Some(fill.kv.clone());
        if card.is_dense27 {
            r.vision = Some(fill.vision);
        }
        if c.data.is_gen_override(&card.id) {
            r.mode = Some(fill.mode.clone());
        }
    }
    r
}

pub(crate) fn recipe_for_profile(c: &Catalog, key: &str) -> Option<Recipe> {
    let p = c.data.profile_by_key(key)?;
    let card = c.data.card(&p.card_id)?;
    let backend = Data::backend_of(&p.backend)?;
    let fill = Fill::catalog(c);
    let mut r = base_recipe(c, card, backend, &p.hardware, p.context, &fill);
    if card.is_sd {
        r.port = Some(p.server_port);
    }
    Some(r)
}

/// Apply-GuiPreset: selection, backend, hardware and context from the preset; cache and port from what the
/// preset applies (else the old state); KV from the preset (qwen-dense), Bonsai's table, else the old state.
fn preset_recipe(c: &Catalog, preset_id: &str, fill: &Fill) -> Option<Recipe> {
    let preset = c.data.presets.iter().find(|p| p.id == preset_id)?;
    let card = c.data.card(preset.card_id.as_deref()?)?;
    let backend = Data::backend_of(&preset.backend)?;
    let mut f = fill.clone();
    if let Some(v) = preset.applies_cache {
        f.cache = v;
    }
    if let Some(v) = preset.applies_port {
        f.port = v;
    }
    if let Some(kv) = &preset.applies_kv {
        f.kv = kv.clone();
    } else if card.is_bonsai {
        if let Some(kv) = card.bonsai_kv.get(&preset.context) {
            f.kv = kv.clone();
        }
    }
    Some(base_recipe(c, card, backend, &preset.hardware, preset.context, &f))
}

fn card_recipe(c: &Catalog, card_id: &str, fill: &Fill) -> Option<Recipe> {
    let card = c.data.card(card_id)?;
    let mut f = fill.clone();
    if card.is_bonsai {
        if let Some(kv) = card.bonsai_kv.get(&card.default_context) {
            f.kv = kv.clone();
        }
    }
    let backend = c.data.backend_values.first().and_then(|b| Data::backend_of(b)).unwrap_or(Backend::Hip);
    Some(base_recipe(c, card, backend, &c.data.primary_hardware, card.default_context, &f))
}

/// The recipe of a tier that has no stored recipe: the launcher preset for it with the catalog's defaults.
pub(crate) fn fallback_recipe(c: &Catalog, slot: SlotId) -> Recipe {
    let fill = Fill::catalog(c);
    let mut r = preset_recipe(c, default_preset_id(slot), &fill)
        .or_else(|| group(c, slot).first().and_then(|card| card_recipe(c, &card.id, &fill)))
        .unwrap_or_default();
    repair(c, slot, &mut r, &fill, Intent { backend: true, hardware: true });
    r
}

pub(crate) fn default_recipes(c: &Catalog, cfg: &Config, old: &OldLauncherDefaults) -> Recipes {
    let fill = Fill::from_old(c, old);
    SlotId::ALL.iter().map(|&slot| (slot, default_recipe(c, cfg, slot, &fill))).collect()
}

fn default_recipe(c: &Catalog, cfg: &Config, slot: SlotId, fill: &Fill) -> Recipe {
    let tier = cfg.tiers.get(slot);

    let mut r: Option<Recipe> = None;
    if let Some(t) = tier {
        if let Some(id) = t.preset.as_deref() {
            r = preset_recipe(c, id, fill);
            if r.is_none() {
                log::warn!("tier {}: launcher preset {id:?} not found; using the default for the tier", slot.as_str());
            }
        } else if let Some(id) = t.card.as_deref() {
            r = card_recipe(c, id, fill);
            if r.is_none() {
                log::warn!("tier {}: card {id:?} not found in the catalog; using the default for the tier", slot.as_str());
            }
        }
    }
    let mut r = r
        .or_else(|| preset_recipe(c, default_preset_id(slot), fill))
        .or_else(|| group(c, slot).first().and_then(|card| card_recipe(c, &card.id, fill)))
        .or_else(|| c.data.cards.first().and_then(|card| card_recipe(c, &card.id, fill)))
        .unwrap_or_default();

    if let Some(t) = tier {
        apply_tier_overrides(c, &mut r, t);
    }
    repair(c, slot, &mut r, fill, Intent { backend: true, hardware: true });
    r
}

fn apply_tier_overrides(c: &Catalog, r: &mut Recipe, t: &TierCfg) {
    if let Some(b) = t.backend.as_deref().and_then(Data::backend_of) {
        r.backend = b;
    }
    if let Some(h) = &t.hardware {
        r.hardware = h.clone();
    }
    if let Some(v) = t.context {
        r.ctx_tokens = Some(v);
    }
    if let Some(v) = &t.image_size {
        r.image_size = Some(v.clone());
    }
    if let Some(v) = t.kv.as_deref() {
        if c.data.rules.kv_values.iter().any(|k| k == v) {
            r.kv_type = Some(v.to_string());
        }
    }
    if let Some(v) = t.prompt_cache_mib {
        r.prompt_cache_mib = Some(v);
    }
    if let Some(v) = t.port {
        r.port = Some(v);
    }
    if let Some(v) = t.vision {
        r.vision = Some(v);
    }
    if let Some(v) = t.mode.as_deref() {
        if let Some(m) = c.data.rules.generation_modes.iter().find(|m| m.eq_ignore_ascii_case(v)) {
            r.mode = Some(m.clone());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Patches and repair
// ---------------------------------------------------------------------------------------------

pub(crate) fn apply_patch(c: &Catalog, slot: SlotId, current: &Recipe, patch: &RecipePatch) -> Recipe {
    let mut r = current.clone();
    let fill = Fill::from_recipe(c, current);
    let previous_card = r.card_id.clone();

    if let Some(id) = &patch.card_id {
        if c.data.card(id).is_some() {
            r.card_id = id.clone();
        }
    }
    if let Some(b) = patch.backend {
        r.backend = b;
    }
    if let Some(h) = &patch.hardware {
        if c.data.hardware_values.is_empty() || c.data.hardware_values.iter().any(|v| v == h) {
            r.hardware = h.clone();
        }
    }
    // A context or image size the (new) card does not offer is ignored; repair keeps the current one if the
    // card offers it, else falls back to the card's default.
    let target = c.data.card(&r.card_id);
    if let (Some(v), Some(card)) = (patch.ctx_tokens, target) {
        if card.offers(v) {
            r.ctx_tokens = Some(v);
        }
    }
    if let (Some(v), Some(card)) = (&patch.image_size, target) {
        let fast_size = c.takes_krea_settings(card) && crate::slots::FAST_KREA_SIZES.contains(&v.as_str());
        if card.ctx_by_label(v).is_some() || fast_size {
            r.image_size = Some(v.clone());
        }
    }
    if let Some(v) = &patch.kv_type {
        if c.data.rules.kv_values.iter().any(|k| k == v) {
            r.kv_type = Some(v.clone());
        }
    }
    if let Some(v) = patch.prompt_cache_mib {
        r.prompt_cache_mib = Some(v);
    }
    if let Some(v) = patch.port {
        if v >= 1024 {
            r.port = Some(v);
        }
    }
    if let Some(v) = patch.vision {
        r.vision = Some(v);
    }
    if let Some(v) = &patch.mode {
        if let Some(m) = c.data.rules.generation_modes.iter().find(|m| m.eq_ignore_ascii_case(v)) {
            r.mode = Some(m.clone());
        }
    }
    // Krea Precision / Edit: kept here, dropped by repair where the card does not take them.
    if let Some(v) = patch.precision {
        r.precision = Some(v);
    }
    if let Some(v) = patch.edit {
        r.edit = Some(v);
    }

    // A swap to Bonsai starts from its context-dependent KV type, like a preset without a CacheType does.
    if r.card_id != previous_card && patch.kv_type.is_none() {
        if let Some(card) = c.data.card(&r.card_id) {
            if card.is_bonsai {
                let ctx = r.ctx_tokens.filter(|x| card.offers(*x)).unwrap_or(card.default_context);
                if let Some(kv) = card.bonsai_kv.get(&ctx) {
                    r.kv_type = Some(kv.clone());
                }
            }
        }
    }

    let intent = Intent {
        backend: patch.backend.is_some() && patch.hardware.is_none(),
        hardware: patch.hardware.is_some() && patch.backend.is_none(),
    };
    repair(c, slot, &mut r, &fill, intent);
    r
}

/// Make a recipe consistent with the catalog: the card exists, a profile exists for its backend/hardware,
/// the context is one the card offers, KV/cache/port/vision/mode (and Krea precision/edit) are present exactly
/// where the card uses them, and the prompt cache respects the catalog's cap for the card and context.
pub(crate) fn repair(c: &Catalog, slot: SlotId, r: &mut Recipe, fill: &Fill, intent: Intent) {
    if c.data.card(&r.card_id).is_none() {
        let fallback = group(c, slot).first().map(|x| x.id.clone()).or_else(|| c.data.cards.first().map(|x| x.id.clone()));
        if let Some(id) = fallback {
            r.card_id = id;
        }
    }
    let Some(card) = c.data.card(&r.card_id) else { return };

    repair_combo(c, card, r, intent);

    // context
    if card.is_image && c.fast_krea_for(card, r).is_some() {
        // The fast starter has its own size list (slots::FAST_KREA_SIZES); 720x1024 became 768x1024.
        r.ctx_tokens = None;
        let fast = crate::slots::FAST_KREA_SIZES;
        let wanted = r.image_size.as_deref().map(|s| if s == "720x1024" { "768x1024" } else { s });
        r.image_size = match wanted {
            Some(s) if fast.contains(&s) => Some(s.to_string()),
            _ => card.ctx_label(card.default_context).map(str::to_string),
        };
    } else if card.is_image {
        r.ctx_tokens = None;
        let valid = r.image_size.as_deref().and_then(|s| card.ctx_by_label(s)).is_some();
        if !valid {
            r.image_size = card.ctx_label(card.default_context).map(str::to_string);
        } else if let Some(v) = r.image_size.as_deref().and_then(|s| card.ctx_by_label(s)).and_then(|v| card.ctx_label(v)) {
            r.image_size = Some(v.to_string()); // canonical label spelling
        }
    } else {
        r.image_size = None;
        let ctx = r.ctx_tokens.filter(|x| card.offers(*x)).unwrap_or(card.default_context);
        r.ctx_tokens = Some(ctx);
    }

    // Krea cards with the fast starter configured carry precision (default low), edit (default off) and vision
    // (default on: Qwen3-VL reads the reference images when Edit is on).
    let krea = c.takes_krea_settings(card);
    if krea {
        r.precision = Some(r.precision.unwrap_or_default());
        r.edit = Some(r.edit.unwrap_or(false));
    } else {
        r.precision = None;
        r.edit = None;
    }

    if card.is_sd {
        r.kv_type = None;
        r.prompt_cache_mib = None;
        r.vision = if krea { Some(r.vision.unwrap_or(true)) } else { None };
        r.mode = None;
        r.port = Some(pinned_port(c, card, r).unwrap_or(fill.port));
        return;
    }

    if !r.kv_type.as_ref().is_some_and(|k| c.data.rules.kv_values.iter().any(|v| v == k)) {
        r.kv_type = Some(fill.kv.clone());
    }
    let ctx = r.ctx_tokens.unwrap_or(card.default_context);
    let mut cache = r.prompt_cache_mib.unwrap_or(fill.cache);
    if let Some(cap) = cap_for(card, ctx) {
        cache = cache.min(cap); // Limit-PromptCacheForSelection (sticky: it only ever lowers the value)
    }
    r.prompt_cache_mib = Some(cache);
    r.port = Some(r.port.filter(|p| *p >= 1024).unwrap_or(fill.port));
    r.vision = if card.is_dense27 { Some(r.vision.unwrap_or(fill.vision)) } else { None };
    r.mode = if c.data.is_gen_override(&card.id) {
        let current = r.mode.as_deref().and_then(|m| c.data.rules.generation_modes.iter().find(|x| x.eq_ignore_ascii_case(m)));
        Some(current.cloned().unwrap_or_else(|| fill.mode.clone()))
    } else {
        None
    };
}

/// The port an sd-server starter pins (Krea 1234, Qwen Image 1235), from the profile of the recipe.
fn pinned_port(c: &Catalog, card: &Card, r: &Recipe) -> Option<u16> {
    let ctx = if card.is_image { r.image_size.as_deref().and_then(|s| card.ctx_by_label(s)) } else { r.ctx_tokens };
    let backend = Data::backend_str(r.backend);
    if let Some(ctx) = ctx {
        let key = format!("{}|{}|{}|{}", card.id, backend, r.hardware, ctx);
        if let Some(p) = c.data.profile_by_key(&key) {
            return Some(p.server_port);
        }
    }
    c.data.profiles.iter().find(|p| p.card_id == card.id).map(|p| p.server_port)
}

fn repair_combo(c: &Catalog, card: &Card, r: &mut Recipe, intent: Intent) {
    let Some(combos) = c.data.combos.get(&card.id) else { return };
    let want_b = Data::backend_str(r.backend);
    let want_h = r.hardware.as_str();
    if combos.iter().any(|(b, h)| b == want_b && h == want_h) {
        return;
    }
    let hw_rank = |h: &str| c.data.hardware_values.iter().position(|v| v == h).unwrap_or(usize::MAX);
    let be_rank = |b: &str| c.data.backend_values.iter().position(|v| v == b).unwrap_or(usize::MAX);
    let mut ordered: Vec<&(String, String)> = combos.iter().collect();
    ordered.sort_by_key(|(b, h)| (hw_rank(h), be_rank(b)));

    let same_backend = ordered.iter().find(|(b, _)| b == want_b).copied();
    let same_hardware = ordered.iter().find(|(_, h)| h == want_h).copied();
    // An explicit hardware choice keeps the hardware; otherwise (explicit backend, or a card swap) keep the backend.
    let pick = if intent.hardware && !intent.backend { same_hardware.or(same_backend) } else { same_backend.or(same_hardware) }
        .or_else(|| ordered.first().copied());

    if let Some((b, h)) = pick {
        if let Some(be) = Data::backend_of(b) {
            r.backend = be;
        }
        r.hardware = h.clone();
    }
}
