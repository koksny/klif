//! View-model side of the catalog: availability, display names, expected VRAM and the Tune drawer options.

use klif_common::vm::{
    Availability, Backend, CardChoice, ModelRef, Recipe, RecipeChoice, RecipeOptions, Slot, SlotId, SlotKind,
};

use crate::data::{Card, Data, Profile};
use crate::recipes::{self, cap_for, Fill};
use crate::vram;
use crate::{Catalog, LiveFacts, Recipes};

/// A recipe's launch-relevant values after defaults and the prompt-cache cap.
#[derive(Debug, Clone)]
pub(crate) struct Effective {
    /// `-PromptCacheMiB` after the catalog cap.
    pub cache: u32,
    /// The PORT setting that is injected as `-Port` (non-sd cards).
    pub port_setting: u16,
    /// `profile.serverPort` when the starter pins one, else the PORT setting.
    pub port: u16,
    pub kv: String,
    pub vision: bool,
    /// The GenerationMode the GUI would pass: the recipe's for the override cards, else the profile's own.
    pub mode: Option<String>,
}

impl Catalog {
    // ---- lookups ---------------------------------------------------------------------------

    /// The context value a recipe selects on its card (tokens, or the encoded image size).
    pub(crate) fn recipe_ctx(&self, card: &Card, r: &Recipe) -> u32 {
        if card.is_image {
            r.image_size.as_deref().and_then(|s| card.ctx_by_label(s)).unwrap_or(card.default_context)
        } else {
            r.ctx_tokens.unwrap_or(card.default_context)
        }
    }

    pub(crate) fn profile_key(card_id: &str, backend: &str, hardware: &str, ctx: u32) -> String {
        format!("{card_id}|{backend}|{hardware}|{ctx}")
    }

    pub(crate) fn profile_for(&self, card: &Card, backend: Backend, hardware: &str, ctx: u32) -> Option<&Profile> {
        self.data.profile_by_key(&Catalog::profile_key(&card.id, Data::backend_str(backend), hardware, ctx))
    }

    /// The launcher's file-based precedence (without BUSY): script, model, binary.
    pub(crate) fn profile_state(&self, p: &Profile) -> Availability {
        if !self.probe.is_file(&p.script) {
            Availability::ScriptMissing
        } else if self.probe.model_bytes(&p.model).is_none() {
            Availability::ModelMissing
        } else if !self.probe.is_file(&p.binary) {
            Availability::BuildRequired
        } else {
            Availability::Ready
        }
    }

    /// Availability of a (card, backend, hardware, context) combination with a plain-sentence reason.
    pub(crate) fn combo_state(&self, card: &Card, backend: Backend, hardware: &str, ctx: u32) -> (Availability, Option<String>) {
        match self.profile_for(card, backend, hardware, ctx) {
            None => (
                Availability::Unsupported,
                Some(format!("No launcher profile for {} on {}", Data::backend_str(backend), self.hardware_label(hardware))),
            ),
            Some(p) => {
                let av = self.profile_state(p);
                (av, reason_for(av, backend))
            }
        }
    }

    pub(crate) fn effective(&self, card: &Card, profile: &Profile, r: &Recipe) -> Effective {
        let d = &self.data.defaults;
        let mut cache = r.prompt_cache_mib.unwrap_or(d.prompt_cache_mib);
        if let Some(cap) = cap_for(card, profile.context) {
            cache = cache.min(cap);
        }
        let port_setting = r.port.unwrap_or(d.server_port);
        let port = if profile.server_port != d.server_port { profile.server_port } else { port_setting };
        let kv = r
            .kv_type
            .clone()
            .filter(|k| self.data.rules.kv_values.iter().any(|v| v == k))
            .unwrap_or_else(|| d.cache_type.clone());
        let vision = r.vision.unwrap_or(d.vision);
        let gen_arg = self.data.rules.generation_mode_arg.as_str();
        let mode = if self.data.is_gen_override(&card.id) {
            Some(r.mode.clone().unwrap_or_else(|| d.generation_mode.clone()))
        } else {
            profile.arg(gen_arg).map(str::to_string)
        };
        Effective { cache, port_setting, port, kv, vision, mode }
    }

    // ---- display ---------------------------------------------------------------------------

    pub(crate) fn hardware_label(&self, hardware: &str) -> String {
        if hardware == self.data.primary_hardware {
            self.device_name.clone()
        } else if self.data.cpu_hardware.iter().any(|h| h == hardware) {
            "CPU (RAM only)".to_string()
        } else if hardware.eq_ignore_ascii_case("dual") {
            "Dual GPU".to_string()
        } else {
            hardware.to_string()
        }
    }

    /// The device text of a ModelRef.
    fn device_label(&self, hardware: &str) -> String {
        if hardware == self.data.primary_hardware {
            self.device_name.clone()
        } else if self.data.cpu_hardware.iter().any(|h| h == hardware) {
            "CPU".to_string()
        } else if hardware.eq_ignore_ascii_case("dual") {
            format!("{} + 2nd GPU", self.device_name)
        } else {
            hardware.to_string()
        }
    }

    /// "Qwen 3.8 27B", "Qwen 3.8 Flash-Next", "Gemma 4 26B-A4B", "Krea 2 Realism Turbo", "Qwen Image 2.1".
    pub(crate) fn display_name(&self, card: &Card) -> String {
        let family = self.data.family_label(&card.family_id).unwrap_or(card.family.as_str());
        if card.is_image {
            if card.family_id == "krea" {
                card.name.clone()
            } else {
                format!("{family} {}", card.size_label)
            }
        } else if card.is_flash_next() {
            format!("{family} Flash-Next")
        } else {
            format!("{family} {}", card.size_label)
        }
    }

    /// The speculative-decoding label the starter implies: dense Qwen ngram/MTP wrappers "MTP + n-gram",
    /// Flash-Next n-gram "n-gram" (MTP below the cap: "MTP + n-gram"), DFlash "DFlash".
    pub(crate) fn spec_label(&self, card: &Card, p: &Profile) -> Option<String> {
        if card.is_image || card.is_bonsai || card.is_gemma() {
            return None;
        }
        let script = p.script_name().to_ascii_lowercase();
        let speculative = p.arg("Speculative").map(str::to_ascii_lowercase);
        if script.contains("dflash") || speculative.as_deref() == Some("dflash") {
            return Some("DFlash".into());
        }
        if script.starts_with("start-qwen38-flashnext") {
            return match speculative.as_deref() {
                Some("mtp") | Some("on") => Some("MTP + n-gram".into()),
                Some("off") => Some("n-gram".into()),
                _ => None,
            };
        }
        if card.is_dense27 {
            // Wrappers named "...Ngram.ps1" force MTP + n-gram; the single-GPU Vulkan wrapper defaults to both on;
            // the dual and RAM wrappers default to MTP only unless -Ngram on is passed.
            let spec_on = speculative.as_deref() != Some("off");
            let ngram_on = match p.arg("Ngram").map(str::to_ascii_lowercase).as_deref() {
                Some("on") => true,
                Some(_) => false,
                None => script.ends_with("ngram.ps1") || (p.backend == "Vulkan" && p.hardware == self.data.primary_hardware),
            };
            return match (spec_on, ngram_on) {
                (true, true) => Some("MTP + n-gram".into()),
                (true, false) => Some("MTP".into()),
                (false, true) => Some("n-gram".into()),
                (false, false) => None,
            };
        }
        None
    }

    /// Model size on disk in GiB (all shards of a split gguf), when the file exists.
    pub(crate) fn weights_gib(&self, p: &Profile) -> Option<f64> {
        self.probe.model_bytes(&p.model).map(|b| b as f64 / (1u64 << 30) as f64)
    }

    pub(crate) fn model_ref(&self, card: &Card, p: &Profile, r: &Recipe, eff: &Effective) -> ModelRef {
        let spec = self.spec_label(card, p);
        let mut m = ModelRef {
            name: self.display_name(card),
            quant: card.quant.clone(),
            engine: if card.is_image { "sd.cpp" } else { "llama.cpp" }.to_string(),
            backend: r.backend,
            device: self.device_label(&p.hardware),
            weights_gib: self.weights_gib(p).map(|g| (g * 100.0).round() / 100.0),
            ..ModelRef::default()
        };
        if card.is_image {
            m.image_size = Some(if p.context_label.is_empty() { card.ctx_label(p.context).unwrap_or_default().to_string() } else { p.context_label.clone() });
        } else {
            m.ctx_tokens = Some(p.context);
            m.kv_type = Some(eff.kv.clone());
            m.spec_mode = spec;
            if card.is_dense27 {
                m.vision = Some(eff.vision);
            }
            m.mode = eff.mode.clone();
        }
        m
    }

    /// Expected VRAM layers for a profile launched with the given effective settings.
    pub(crate) fn expected_layers(&self, card: &Card, p: &Profile, r: &Recipe, eff: &Effective) -> Option<Vec<klif_common::vm::VramLayer>> {
        let spec = self.spec_label(card, p);
        vram::estimate(&vram::Input {
            card,
            profile: p,
            backend: r.backend,
            primary_gpu: p.hardware == self.data.primary_hardware,
            kv_type: &eff.kv,
            vision: eff.vision,
            spec: spec.as_deref(),
            file_gib: self.weights_gib(p),
        })
    }
}

/// One plain sentence for a state that is not READY.
fn reason_for(av: Availability, backend: Backend) -> Option<String> {
    match av {
        Availability::Ready => None,
        Availability::Unsupported => Some("This combination is not supported by the launcher".to_string()),
        Availability::ScriptMissing => Some("Starter script not found".to_string()),
        Availability::ModelMissing => Some("Model file not found".to_string()),
        Availability::BuildRequired => Some(format!("{} server binary is not built", Data::backend_str(backend))),
        Availability::Busy => Some("Port is in use by another process".to_string()),
    }
}

// ---------------------------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------------------------

pub(crate) fn slots(c: &Catalog, recipes_in: &Recipes, live: &LiveFacts) -> Vec<Slot> {
    SlotId::ALL.iter().map(|&id| slot(c, id, recipes_in, live)).collect()
}

fn slot(c: &Catalog, id: SlotId, recipes_in: &Recipes, live: &LiveFacts) -> Slot {
    // A slot without a stored recipe uses the launcher preset default for the tier.
    let stored = recipes_in.get(&id).cloned().unwrap_or_else(|| recipes::fallback_recipe(c, id));
    let mut slot = Slot {
        id,
        label: id.label().to_string(),
        kind: id.kind(),
        model: ModelRef { device: c.device_name.clone(), engine: if id.kind() == SlotKind::Image { "sd.cpp" } else { "llama.cpp" }.to_string(), ..ModelRef::default() },
        availability: Availability::Unsupported,
        reason: None,
        expected_vram: None,
        recipe: None,
        options: None,
    };

    let Some(card) = c.data.card(&stored.card_id) else {
        slot.reason = Some("This model is no longer in the launcher catalog".to_string());
        slot.recipe = Some(stored);
        return slot;
    };

    // Fill what the recipe leaves out and apply the catalog cap; never change the selection itself.
    let fill = Fill::catalog(c);
    let mut r = stored;
    if r.prompt_cache_mib.is_none() && !card.is_sd {
        r.prompt_cache_mib = Some(fill.cache);
    }
    let ctx = c.recipe_ctx(card, &r);
    let hardware = r.hardware.clone();

    match c.profile_for(card, r.backend, &hardware, ctx) {
        None => {
            slot.model = ModelRef {
                name: c.display_name(card),
                quant: card.quant.clone(),
                engine: slot.model.engine.clone(),
                backend: r.backend,
                device: c.hardware_label(&hardware),
                ..ModelRef::default()
            };
            let (av, reason) = c.combo_state(card, r.backend, &hardware, ctx);
            slot.availability = av;
            slot.reason = reason;
        }
        Some(p) => {
            let eff = c.effective(card, p, &r);
            slot.model = c.model_ref(card, p, &r, &eff);
            slot.expected_vram = c.expected_layers(card, p, &r, &eff);
            let mut av = c.profile_state(p);
            let mut reason = reason_for(av, r.backend);
            if av == Availability::Ready && live.foreign_ports.contains(&eff.port) {
                av = Availability::Busy;
                reason = Some(format!("Port {} is in use by another process", eff.port));
            }
            slot.availability = av;
            slot.reason = reason;
            // The recipe the UI shows carries the capped cache, like the old GUI's persisted value.
            r.prompt_cache_mib = if card.is_sd { None } else { Some(eff.cache) };
            if card.is_sd {
                r.port = Some(eff.port);
            }
        }
    }
    slot.options = Some(options(c, id, card, &r, ctx));
    slot.recipe = Some(r);
    slot
}

// ---------------------------------------------------------------------------------------------
// Options for the Tune drawer
// ---------------------------------------------------------------------------------------------

fn options(c: &Catalog, slot: SlotId, card: &Card, r: &Recipe, ctx: u32) -> RecipeOptions {
    let hardware = r.hardware.as_str();

    // Cards that may sit behind this slot; the current card is always listed.
    let mut cards: Vec<&Card> = recipes::group(c, slot);
    if !cards.iter().any(|x| x.id == card.id) {
        cards.insert(0, card);
    }
    let card_choices = cards
        .iter()
        .map(|x| {
            let xctx = if x.is_image {
                if x.offers(ctx) { ctx } else { x.default_context }
            } else if x.offers(ctx) {
                ctx
            } else {
                x.default_context
            };
            let (availability, reason) = c.combo_state(x, r.backend, hardware, xctx);
            CardChoice { value: x.id.clone(), label: x.name.clone(), availability, reason, name: c.display_name(x), quant: x.quant.clone() }
        })
        .collect();

    let backends = c
        .data
        .backend_values
        .iter()
        .filter_map(|b| Data::backend_of(b).map(|be| (b.clone(), be)))
        .map(|(label, be)| {
            let (availability, reason) = c.combo_state(card, be, hardware, ctx);
            RecipeChoice { value: be, label, availability, reason }
        })
        .collect();

    let hardware_choices = c
        .data
        .hardware_values
        .iter()
        .map(|h| {
            let (availability, reason) = c.combo_state(card, r.backend, h, ctx);
            RecipeChoice { value: h.clone(), label: c.hardware_label(h), availability, reason }
        })
        .collect();

    let mut o = RecipeOptions { cards: card_choices, backends, hardware: hardware_choices, ..RecipeOptions::default() };

    if card.is_image {
        o.image_sizes = Some(
            card.contexts
                .iter()
                .map(|x| {
                    let (availability, reason) = c.combo_state(card, r.backend, hardware, x.value);
                    RecipeChoice { value: x.label.clone(), label: x.label.clone(), availability, reason }
                })
                .collect(),
        );
    } else {
        o.contexts = Some(
            card.contexts
                .iter()
                .map(|x| {
                    let (availability, reason) = c.combo_state(card, r.backend, hardware, x.value);
                    RecipeChoice { value: x.value, label: x.label.clone(), availability, reason }
                })
                .collect(),
        );
        o.kv_types = Some(c.data.rules.kv_values.clone());
        let cap = cap_for(card, ctx);
        let mut cache: Vec<u32> = c.data.cache_values.iter().map(|v| cap.map_or(*v, |m| (*v).min(m))).collect();
        cache.sort_unstable();
        cache.dedup();
        o.prompt_cache_mib = Some(cache);
        o.ports = Some(c.data.port_values.clone());
        o.vision = Some(card.is_dense27);
        if c.data.is_gen_override(&card.id) {
            o.modes = Some(c.data.rules.generation_modes.clone());
        }
    }
    o
}
