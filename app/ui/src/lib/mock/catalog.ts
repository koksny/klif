// Static model catalog for the mock launcher and the Tune drawer. Placeholder lists for now: the
// real core will supply what is installed. Figures marked "est." are not measured.
import type { ModelRef, SlotId, SlotKind, VramLayer } from '../model/types';

export interface CatalogQuant {
  quant: string;
  weightsGiB: number;
}

export interface CatalogModel {
  name: string;
  kind: SlotKind;
  engine: string;
  quants: CatalogQuant[];
  /** LLM: context options in tokens. */
  ctxOptions?: number[];
  defaultCtx?: number;
  /** KV cache GiB per context token at q8_0 (LLM). */
  kvGiBPerTokenQ8?: number;
  /** Compute buffers GiB. */
  buffersGiB: number;
  /** The model ships a vision projector. */
  hasVision?: boolean;
  modes?: string[];
  defaultMode?: string;
  specModes?: string[];
  defaultSpec?: string;
  imageSizes?: string[];
  defaultImageSize?: string;
  /** Image: VAE GiB and activation GiB at 512x768. */
  vaeGiB?: number;
  activationsGiB?: number;
  /** Image: share of the weights that stays on the GPU (the rest streams). */
  gpuWeightShare?: number;
}

const CTX = [8192, 16384, 32768, 65536, 98304, 131072, 196608, 262144];

export const CATALOG: CatalogModel[] = [
  {
    name: 'Qwen 3.8 27B',
    kind: 'llm',
    engine: 'llama.cpp',
    quants: [
      { quant: 'IQ3_XXS', weightsGiB: 10.4 },
      { quant: 'GSQ-RCO IQ3_S', weightsGiB: 11.6 },
      { quant: 'Q3_K_M', weightsGiB: 12.9 },
    ],
    ctxOptions: CTX,
    defaultCtx: 98304,
    kvGiBPerTokenQ8: 2.9 / 98304,
    buffersGiB: 0.9,
    hasVision: true,
    modes: ['Thinking', 'Instruct'],
    defaultMode: undefined,
    specModes: ['MTP+ngram', 'ngram', 'off'],
    defaultSpec: 'MTP+ngram',
  },
  {
    name: 'Qwen 3.8 Flash-Next',
    kind: 'llm',
    engine: 'llama.cpp',
    quants: [
      { quant: 'GSQ-RCO IQ1_M', weightsGiB: 11.5 },
      { quant: 'IQ2_XXS', weightsGiB: 13.2 },
    ],
    ctxOptions: CTX,
    defaultCtx: 131072,
    kvGiBPerTokenQ8: 1.6 / 131072,
    buffersGiB: 0.99,
    modes: ['Thinking', 'Instruct'],
    defaultMode: 'Thinking',
    specModes: ['ngram', 'off'],
    defaultSpec: 'ngram',
  },
  {
    name: 'Gemma 4 26B-A4B',
    kind: 'llm',
    engine: 'llama.cpp',
    quants: [{ quant: 'Q4_0', weightsGiB: 13.4 }],
    ctxOptions: CTX.filter((c) => c <= 131072),
    defaultCtx: 16384,
    kvGiBPerTokenQ8: 0.4 / 16384,
    buffersGiB: 0.8,
    hasVision: false,
    modes: ['Instruct'],
    specModes: ['off'],
  },
  {
    name: 'Gemma 4 12B',
    kind: 'llm',
    engine: 'llama.cpp',
    quants: [{ quant: 'Q4_0', weightsGiB: 6.5 }],
    ctxOptions: CTX.filter((c) => c <= 131072),
    defaultCtx: 32768,
    kvGiBPerTokenQ8: 0.55 / 32768,
    buffersGiB: 0.6,
    modes: ['Instruct'],
    specModes: ['off'],
  },
  {
    name: 'Krea 2 Realism Turbo',
    kind: 'image',
    engine: 'sd.cpp',
    quants: [
      { quant: 'Q4_0', weightsGiB: 4.6 },
      { quant: 'Q8_0', weightsGiB: 7.4 },
    ],
    buffersGiB: 0,
    imageSizes: ['512x768', '768x512', '768x768', '720x1024', '1024x768'],
    defaultImageSize: '512x768',
    vaeGiB: 0.6,
    activationsGiB: 3.2,
    gpuWeightShare: 6.1 / 7.4,
  },
  {
    name: 'Krea 2 Muse',
    kind: 'image',
    engine: 'sd.cpp',
    quants: [
      { quant: 'Q4_0', weightsGiB: 4.6 },
      { quant: 'Q8_0', weightsGiB: 7.4 },
    ],
    buffersGiB: 0,
    imageSizes: ['512x768', '768x512', '768x768', '720x1024', '1024x768'],
    defaultImageSize: '512x768',
    vaeGiB: 0.6,
    activationsGiB: 3.2,
    gpuWeightShare: 6.1 / 7.4,
  },
];

export const KV_TYPES = ['q4_0', 'q8_0', 'f16'] as const;
export const BACKENDS = ['HIP', 'Vulkan'] as const;
export const PROMPT_CACHE_MIB = [0, 2048, 4096, 8192] as const;

const KV_FACTOR: Record<string, number> = { q4_0: 0.53, q8_0: 1, f16: 1.88 };

export function catalogFor(name: string): CatalogModel | undefined {
  return CATALOG.find((m) => m.name === name);
}

export function catalogByKind(kind: SlotKind): CatalogModel[] {
  return CATALOG.filter((m) => m.kind === kind);
}

const r2 = (v: number) => Math.round(v * 100) / 100;

/** Expected VRAM layers for a model configuration: the "will it fit" preview. */
export function expectedLayers(model: ModelRef): VramLayer[] {
  const cat = catalogFor(model.name);
  const q = cat?.quants.find((x) => x.quant === model.quant);
  const weights = q?.weightsGiB ?? model.weightsGiB ?? 8;
  if (!cat || cat.kind === 'llm') {
    const ctx = model.ctxTokens ?? cat?.defaultCtx ?? 16384;
    const kv = (cat?.kvGiBPerTokenQ8 ?? 2.9 / 98304) * ctx * (KV_FACTOR[model.kvType ?? 'q8_0'] ?? 1);
    const layers: VramLayer[] = [
      { id: 'weights', label: 'weights', gib: r2(weights) },
      { id: 'kv', label: 'KV cache', gib: r2(kv) },
      { id: 'buffers', label: 'buffers', gib: r2(cat?.buffersGiB ?? 0.9) },
    ];
    if (model.specMode && /MTP/i.test(model.specMode)) layers.push({ id: 'draft', label: 'draft', gib: 0.12 });
    return layers;
  }
  const [w, h] = (model.imageSize ?? cat.defaultImageSize ?? '512x768').split('x').map(Number);
  const pixels = (w * h) / (512 * 768);
  return [
    { id: 'weights', label: 'DiT layers', gib: r2(weights * (cat.gpuWeightShare ?? 0.8)) },
    { id: 'buffers', label: 'activations', gib: r2((cat.activationsGiB ?? 3) * pixels) },
    { id: 'other', label: 'VAE', gib: r2(cat.vaeGiB ?? 0.6) },
  ];
}

export function totalGiB(layers: VramLayer[]): number {
  return r2(layers.reduce((a, l) => a + l.gib, 0));
}

/** Build a ModelRef from a catalog entry with sensible defaults, keeping device/backend of `prev`. */
export function modelFromCatalog(cat: CatalogModel, prev: ModelRef): ModelRef {
  const q = cat.quants[cat.quants.length > 1 ? 1 : 0];
  const ref: ModelRef = {
    name: cat.name,
    quant: q.quant,
    engine: cat.engine,
    backend: prev.backend,
    device: prev.device,
    weightsGiB: q.weightsGiB,
  };
  if (cat.kind === 'llm') {
    ref.ctxTokens = cat.defaultCtx;
    ref.kvType = prev.kvType ?? 'q8_0';
    if (cat.defaultSpec && cat.defaultSpec !== 'off') ref.specMode = cat.defaultSpec;
    if (cat.hasVision) ref.vision = true;
    if (cat.defaultMode) ref.mode = cat.defaultMode;
  } else {
    ref.imageSize = cat.defaultImageSize;
  }
  return ref;
}

export const DEFAULT_PORT: Record<SlotId, number> = { high: 7030, medium: 7030, low: 7030, krea: 1234 };

/** Headroom the loader tries to keep free below the edge (llama.cpp style auto-fit). */
export const FIT_HEADROOM_GIB = 0.05;

export interface FitResult {
  /** The session layers after the loader trimmed its compute buffers (baseline NOT included). */
  layers: VramLayer[];
  /** How much the compute buffers were trimmed to fit, GiB. */
  trimmedGiB: number;
  /** What still does not fit after trimming, GiB. Anything above 0 spills to shared system memory. */
  overGiB: number;
}

/**
 * "Will it fit": the real loaders shrink their compute buffers (up to half) to squeeze under the edge
 * before they let anything spill. Shared by the mock launch and the Tune drawer preview so both agree.
 */
export function fitLayers(layers: VramLayer[], baselineGiB: number, totalGiB: number): FitResult {
  const out = layers.map((l) => ({ ...l }));
  const sum = () => out.reduce((a, l) => a + l.gib, 0) + baselineGiB;
  const want = sum() - (totalGiB - FIT_HEADROOM_GIB);
  let trimmed = 0;
  if (want > 0) {
    const b = out.find((l) => l.id === 'buffers');
    if (b) {
      trimmed = r2(Math.min(want, b.gib * 0.5));
      b.gib = r2(b.gib - trimmed);
    }
  }
  return { layers: out, trimmedGiB: trimmed, overGiB: Math.max(0, r2(sum() - totalGiB)) };
}
