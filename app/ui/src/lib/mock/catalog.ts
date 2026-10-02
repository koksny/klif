// Launch catalog of the mock core (browser path). Shaped like the real launcher catalog: cards (one model
// file each) x backend x hardware, one default recipe per tier. The values are placeholders; the native
// core reads the installed catalog. Figures marked "est." are not measured.
import { fmtCtx } from '../model/format';
import type {
  Availability,
  Backend,
  ModelRef,
  Recipe,
  RecipeChoice,
  RecipeOptions,
  Slot,
  SlotId,
  SlotKind,
  VramLayer,
} from '../model/types';
import { fitLayers, FIT_HEADROOM_GIB, type FitResult } from '../shell/fit';

export { fitLayers, FIT_HEADROOM_GIB, type FitResult };

export interface MockCard {
  /** Catalog card id, e.g. "qwen-gsq". */
  id: string;
  /** ModelRef.name */
  name: string;
  /** ModelRef.quant */
  quant: string;
  kind: SlotKind;
  engine: string;
  weightsGiB: number;
  /** LLM: context options in tokens. */
  contexts?: number[];
  defaultCtx?: number;
  /** KV cache GiB per context token at q8_0 (LLM). */
  kvGiBPerTokenQ8?: number;
  /** Compute buffers GiB. */
  buffersGiB: number;
  /** The card ships a vision projector. */
  hasVision?: boolean;
  /** Reasoning modes the starter accepts (only some cards take one). */
  modes?: string[];
  /** Speculative decoding built into the starter. */
  specMode?: string;
  imageSizes?: string[];
  defaultImageSize?: string;
  /** Image: VAE GiB and activation GiB at 512x768. */
  vaeGiB?: number;
  activationsGiB?: number;
  /** Image: share of the weights that stays on the GPU (the rest streams). */
  gpuWeightShare?: number;
  /** Hardware with a starter, per backend. Anything else is 'unsupported'. */
  hardware: Partial<Record<Backend, string[]>>;
  /** Non-ready state of a supported combination (a missing model file or server build). */
  state?: Partial<Record<Backend, Availability>>;
  /** Prompt cache cap in MiB at contexts >= 131072, where the catalog caps it. */
  cacheCapLongCtx?: number;
}

export interface MockHardware {
  value: string;
  label: string;
  /** ModelRef.device */
  device: string;
}

export const HARDWARE: MockHardware[] = [
  { value: '9070', label: 'RX 9070 XT', device: 'RX 9070 XT' },
  { value: '5700', label: 'RX 5700 XT', device: 'RX 5700 XT' },
  { value: 'Dual', label: 'Dual GPU', device: 'RX 9070 XT + 5700 XT' },
  { value: '9950X3D', label: '9950X3D (RAM)', device: '9950X3D' },
];

export const BACKENDS: Backend[] = ['HIP', 'Vulkan'];
export const KV_TYPES: ('q4_0' | 'q8_0')[] = ['q4_0', 'q8_0'];
export const PROMPT_CACHE_MIB = [2048, 8192, 16384, 32768];
export const LLM_PORTS = [7030, 7031, 7032, 7033, 7034, 7035];

const CTX = [16384, 32768, 65536, 98304, 131072, 262144];
const CTX_128K = CTX.filter((c) => c <= 131072);
const GPU_RAM = ['9070', 'Dual', '9950X3D'];
const IMAGE_SIZES = ['512x512', '512x768', '640x920', '720x960', '720x1024', '1024x1024'];

const qwen27 = (id: string, quant: string, weightsGiB: number, specMode: string, defaultCtx: number): MockCard => ({
  id,
  name: 'Qwen 3.8 27B',
  quant,
  kind: 'llm',
  engine: 'llama.cpp',
  weightsGiB,
  contexts: CTX,
  defaultCtx,
  kvGiBPerTokenQ8: 2.9 / 98304,
  buffersGiB: 0.9,
  hasVision: true,
  specMode,
  hardware: { HIP: GPU_RAM, Vulkan: GPU_RAM },
});

const flashNext = (id: string, name: string, quant: string, weightsGiB: number, extra: Partial<MockCard> = {}): MockCard => ({
  id,
  name,
  quant,
  kind: 'llm',
  engine: 'llama.cpp',
  weightsGiB,
  contexts: CTX_128K,
  defaultCtx: 131072,
  kvGiBPerTokenQ8: 1.6 / 131072,
  buffersGiB: 0.99,
  specMode: 'ngram',
  hardware: { HIP: ['9070', '9950X3D'], Vulkan: ['9070', '9950X3D'] },
  ...extra,
});

const gemma = (id: string, name: string, quant: string, weightsGiB: number, kvPerTok: number, buffersGiB: number, defaultCtx: number): MockCard => ({
  id,
  name,
  quant,
  kind: 'llm',
  engine: 'llama.cpp',
  weightsGiB,
  contexts: CTX,
  defaultCtx,
  kvGiBPerTokenQ8: kvPerTok,
  buffersGiB,
  hardware: { HIP: GPU_RAM, Vulkan: GPU_RAM },
});

const image = (id: string, name: string, quant: string, weightsGiB: number, extra: Partial<MockCard> = {}): MockCard => ({
  id,
  name,
  quant,
  kind: 'image',
  engine: 'sd.cpp',
  weightsGiB,
  buffersGiB: 0,
  imageSizes: IMAGE_SIZES,
  defaultImageSize: '512x768',
  vaeGiB: 0.6,
  activationsGiB: 3.2,
  gpuWeightShare: 6.1 / 7.4,
  hardware: { HIP: ['9070', 'Dual'], Vulkan: ['9070', 'Dual'] },
  ...extra,
});

export const CARDS: MockCard[] = [
  qwen27('qwen-gsq', 'GSQ-RCO IQ3_S', 11.6, 'MTP+ngram', 98304),
  qwen27('qwen-q3k', 'UD-Q3_K_XL', 12.9, 'ngram', 131072),
  qwen27('qwen-iq2', 'UD-IQ2_XXS', 8.4 /* est. */, 'ngram', 131072),
  qwen27('qwen-iq4', 'UD-IQ4_XS', 14.1 /* est. */, 'ngram', 131072),
  {
    id: 'bonsai2-pq2',
    name: 'Bonsai 2 27B',
    quant: 'PQ2_0',
    kind: 'llm',
    engine: 'llama.cpp',
    weightsGiB: 7.2, // est.
    contexts: CTX,
    defaultCtx: 262144,
    kvGiBPerTokenQ8: 2.9 / 98304, // est.
    buffersGiB: 0.9,
    hardware: { HIP: ['9070'], Vulkan: ['9070'] },
    state: { Vulkan: 'build-required' },
  },
  flashNext('qwen-fn-iq2', 'Qwen 3.8 Flash-Next', 'IQ2_XXS', 13.2, { modes: ['Thinking', 'Instruct'], cacheCapLongCtx: 2048 }),
  flashNext('qwen-fn-q2', 'Qwen 3.8 Flash-Next', 'UD-Q2_K_XL', 15.8 /* est. */, {
    modes: ['Thinking', 'Instruct'],
    contexts: [16384, 32768],
    defaultCtx: 16384,
    state: { HIP: 'model-missing', Vulkan: 'model-missing' },
  }),
  flashNext('qwen-fn-coder', 'Qwen 3.8 Flash-Next Coder', 'GSQ-RCO IQ1_M', 11.5, { cacheCapLongCtx: 2048 }),
  gemma('gemma-26b', 'Gemma 4 26B-A4B', 'Q4_0', 13.4, 0.4 / 16384, 0.8, 16384),
  gemma('gemma-26b-udxl', 'Gemma 4 26B-A4B', 'QAT UD-Q4_K_XL', 14.6 /* est. */, 0.4 / 16384, 0.8, 131072),
  gemma('gemma-12b', 'Gemma 4 12B', 'QAT Q4_0', 6.5, 0.55 / 32768, 0.6, 131072),
  gemma('gemma-31b', 'Gemma 4 31B', 'Q4_0', 16.9 /* est. */, 0.9 / 16384 /* est. */, 1.0, 131072),
  image('krea-realism', 'Krea 2 Realism Turbo', 'Q8_0', 7.4),
  image('krea-muse', 'Krea 2 Muse', 'Q8_0', 7.4),
  image('qwen-image-21', 'Qwen Image 2.1', 'Q4_0', 11.9 /* est. */, {
    imageSizes: ['512x512', '512x768', '640x928', '736x960', '736x1024', '1024x1024'],
    vaeGiB: 0.3, // est.
    activationsGiB: 2.8, // est.
    gpuWeightShare: 0.75, // est.
    hardware: { HIP: ['9070'] },
  }),
];

/** The tier defaults (the launcher's presets). */
export const DEFAULT_RECIPES: Record<SlotId, Recipe> = {
  high: { cardId: 'qwen-fn-iq2', backend: 'HIP', hardware: '9070', ctxTokens: 131072, kvType: 'q8_0', promptCacheMiB: 2048, port: 7030, mode: 'Thinking' },
  medium: { cardId: 'qwen-gsq', backend: 'HIP', hardware: '9070', ctxTokens: 98304, kvType: 'q8_0', promptCacheMiB: 16384, port: 7030, vision: true },
  low: { cardId: 'gemma-26b', backend: 'HIP', hardware: '9070', ctxTokens: 16384, kvType: 'q8_0', promptCacheMiB: 8192, port: 7030 },
  krea: { cardId: 'krea-realism', backend: 'HIP', hardware: '9070', imageSize: '512x768', port: 1234 },
};

/** Image servers keep their own pinned port. */
export const DEFAULT_PORT: Record<SlotId, number> = { high: 7030, medium: 7030, low: 7030, krea: 1234 };

export function cardById(id: string): MockCard | undefined {
  return CARDS.find((c) => c.id === id);
}

/** The card behind a ModelRef (name + quant, else the first card with that name). */
export function cardFor(model: ModelRef): MockCard | undefined {
  return CARDS.find((c) => c.name === model.name && c.quant === model.quant) ?? CARDS.find((c) => c.name === model.name);
}

function hardwareOf(value: string): MockHardware {
  return HARDWARE.find((h) => h.value === value) ?? { value, label: value, device: value };
}

const REASON: Partial<Record<Availability, string>> = {
  'model-missing': 'Model file not found.',
  'script-missing': 'Starter script not found.',
};

/** Availability of one card on one backend and hardware, with a plain reason when it is not ready. */
export function comboAvailability(card: MockCard, backend: Backend, hardware: string): { availability: Availability; reason?: string } {
  if (!(card.hardware[backend] ?? []).includes(hardware)) {
    return { availability: 'unsupported', reason: `No ${backend} starter for ${hardwareOf(hardware).label}.` };
  }
  const st = card.state?.[backend];
  if (st && st !== 'ready') {
    return { availability: st, reason: REASON[st] ?? (st === 'build-required' ? `The ${backend} server build is missing.` : undefined) };
  }
  return { availability: 'ready' };
}

function capCache(card: MockCard, ctx: number | undefined, mib: number | undefined): number | undefined {
  if (mib === undefined) return undefined;
  return card.cacheCapLongCtx && (ctx ?? 0) >= 131072 ? Math.min(mib, card.cacheCapLongCtx) : mib;
}

/**
 * Apply a Tune drawer patch. Switching the card keeps every setting the new card supports and falls back
 * to its defaults for the rest; values the card does not offer are ignored.
 */
export function applyPatch(kind: SlotKind, cur: Recipe, patch: Partial<Recipe>): Recipe {
  const next: Recipe = { ...cur };
  const switched = patch.cardId !== undefined && patch.cardId !== cur.cardId && cardById(patch.cardId)?.kind === kind;
  if (switched) next.cardId = patch.cardId!;
  const card = cardById(next.cardId);
  if (!card) return cur;
  if (patch.backend && BACKENDS.includes(patch.backend)) next.backend = patch.backend;
  if (patch.hardware && HARDWARE.some((h) => h.value === patch.hardware)) next.hardware = patch.hardware;
  if (card.kind === 'llm') {
    const ctxs = card.contexts ?? [];
    if (patch.ctxTokens !== undefined && ctxs.includes(patch.ctxTokens)) next.ctxTokens = patch.ctxTokens;
    if (next.ctxTokens === undefined || !ctxs.includes(next.ctxTokens)) next.ctxTokens = card.defaultCtx ?? ctxs[0];
    if (patch.kvType && KV_TYPES.includes(patch.kvType)) next.kvType = patch.kvType;
    next.kvType ??= 'q8_0';
    if (patch.promptCacheMiB !== undefined && PROMPT_CACHE_MIB.includes(patch.promptCacheMiB)) next.promptCacheMiB = patch.promptCacheMiB;
    next.promptCacheMiB = capCache(card, next.ctxTokens, next.promptCacheMiB ?? 8192);
    if (patch.port !== undefined && LLM_PORTS.includes(patch.port)) next.port = patch.port;
    next.port ??= 7030;
    if (card.hasVision) {
      if (patch.vision !== undefined) next.vision = patch.vision;
      next.vision ??= true;
    } else delete next.vision;
    if (card.modes?.length) {
      if (patch.mode !== undefined && card.modes.includes(patch.mode)) next.mode = patch.mode;
      if (next.mode === undefined || !card.modes.includes(next.mode)) next.mode = card.modes[0];
    } else delete next.mode;
    delete next.imageSize;
  } else {
    const sizes = card.imageSizes ?? [];
    if (patch.imageSize !== undefined && sizes.includes(patch.imageSize)) next.imageSize = patch.imageSize;
    if (next.imageSize === undefined || !sizes.includes(next.imageSize)) next.imageSize = card.defaultImageSize ?? sizes[0];
    for (const k of ['ctxTokens', 'kvType', 'promptCacheMiB', 'vision', 'mode'] as const) delete next[k];
  }
  return next;
}

/** What the recipe launches, as display text. */
export function modelFromRecipe(r: Recipe): ModelRef {
  const card = cardById(r.cardId);
  if (!card) return { name: r.cardId, quant: '', engine: '', backend: r.backend, device: hardwareOf(r.hardware).device };
  const m: ModelRef = { name: card.name, quant: card.quant, engine: card.engine, backend: r.backend, device: hardwareOf(r.hardware).device };
  if (card.kind === 'llm') {
    m.ctxTokens = r.ctxTokens;
    m.kvType = r.kvType;
    if (card.specMode) m.specMode = card.specMode;
    if (card.hasVision) m.vision = !!r.vision;
    if (r.mode) m.mode = r.mode;
  } else {
    m.imageSize = r.imageSize;
  }
  m.weightsGiB = card.weightsGiB;
  return m;
}

/** The choices the Tune drawer offers for a slot with this recipe. */
export function optionsFor(kind: SlotKind, r: Recipe): RecipeOptions {
  const card = cardById(r.cardId);
  const choice = <T,>(value: T, label: string, a: { availability: Availability; reason?: string }): RecipeChoice<T> =>
    a.reason ? { value, label, availability: a.availability, reason: a.reason } : { value, label, availability: a.availability };
  const opts: RecipeOptions = {
    cards: CARDS.filter((c) => c.kind === kind).map((c) => ({
      ...choice(c.id, `${c.name} · ${c.quant}`, comboAvailability(c, r.backend, r.hardware)),
      name: c.name,
      quant: c.quant,
    })),
    backends: BACKENDS.map((b) => choice(b, b, card ? comboAvailability(card, b, r.hardware) : { availability: 'unsupported' })),
    hardware: HARDWARE.map((h) => choice(h.value, h.label, card ? comboAvailability(card, r.backend, h.value) : { availability: 'unsupported' })),
  };
  if (!card) return opts;
  if (card.kind === 'llm') {
    opts.contexts = (card.contexts ?? []).map((c) => choice(c, `${fmtCtx(c)} tokens`, { availability: 'ready' }));
    opts.kvTypes = [...KV_TYPES];
    const cap = card.cacheCapLongCtx && (r.ctxTokens ?? 0) >= 131072 ? card.cacheCapLongCtx : Infinity;
    opts.promptCacheMiB = PROMPT_CACHE_MIB.filter((c) => c <= cap);
    opts.ports = [...LLM_PORTS];
    opts.vision = !!card.hasVision;
    if (card.modes?.length) opts.modes = [...card.modes];
  } else {
    opts.imageSizes = (card.imageSizes ?? []).map((s) => choice(s, s, { availability: 'ready' }));
  }
  return opts;
}

const r2 = (v: number) => Math.round(v * 100) / 100;
const KV_FACTOR: Record<string, number> = { q4_0: 0.53, q8_0: 1, f16: 1.88 };

/** Expected VRAM layers for a model configuration: the "will it fit" preview. */
export function expectedLayers(model: ModelRef): VramLayer[] {
  const card = cardFor(model);
  const weights = card?.weightsGiB ?? model.weightsGiB ?? 8;
  if (!card || card.kind === 'llm') {
    const ctx = model.ctxTokens ?? card?.defaultCtx ?? 16384;
    const kv = (card?.kvGiBPerTokenQ8 ?? 2.9 / 98304) * ctx * (KV_FACTOR[model.kvType ?? 'q8_0'] ?? 1);
    const layers: VramLayer[] = [
      { id: 'weights', label: 'weights', gib: r2(weights) },
      { id: 'kv', label: 'KV cache', gib: r2(kv) },
      { id: 'buffers', label: 'buffers', gib: r2(card?.buffersGiB ?? 0.9) },
    ];
    if (model.specMode && /MTP/i.test(model.specMode)) layers.push({ id: 'draft', label: 'draft', gib: 0.12 });
    return layers;
  }
  const [w, h] = (model.imageSize ?? card.defaultImageSize ?? '512x768').split('x').map(Number);
  const pixels = (w * h) / (512 * 768);
  return [
    { id: 'weights', label: 'DiT layers', gib: r2(weights * (card.gpuWeightShare ?? 0.8)) },
    { id: 'buffers', label: 'activations', gib: r2((card.activationsGiB ?? 3) * pixels) },
    { id: 'other', label: 'VAE', gib: r2(card.vaeGiB ?? 0.6) },
  ];
}

export function totalGiB(layers: VramLayer[]): number {
  return r2(layers.reduce((a, l) => a + l.gib, 0));
}

/** A slot as the core reports it for a recipe: model, availability, expected VRAM, recipe and choices. */
export function slotWithRecipe(base: Slot, recipe: Recipe): Slot {
  const card = cardById(recipe.cardId);
  const model = modelFromRecipe(recipe);
  const a = card ? comboAvailability(card, recipe.backend, recipe.hardware) : { availability: 'model-missing' as const, reason: 'Unknown card.' };
  const slot: Slot = {
    id: base.id,
    label: base.label,
    kind: base.kind,
    model,
    availability: a.availability,
    expectedVram: expectedLayers(model),
    recipe: { ...recipe },
    options: optionsFor(base.kind, recipe),
  };
  if (a.reason && a.availability !== 'ready') slot.reason = a.reason;
  return slot;
}

/** Static snapshots (the sample) get the default recipes and their choices; their models stay as they are. */
export function withDefaultRecipes(slots: Slot[]): Slot[] {
  return slots.map((s) => {
    const recipe = s.recipe ?? DEFAULT_RECIPES[s.id];
    return { ...s, recipe: { ...recipe }, options: s.options ?? optionsFor(s.kind, recipe) };
  });
}
