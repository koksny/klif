// A static snapshot that matches the approved mockups exactly (SYSTEM 2 live, decoding).
// Used as the first frame and by skins while the mock player is not wired in.
import type { ModelRef, Slot, ViewModel, VramLayer } from './types';

export const MODELS: Record<'high' | 'medium' | 'low' | 'krea', ModelRef> = {
  high: {
    name: 'Qwen 3.8 Flash-Next',
    quant: 'IQ2_XXS',
    engine: 'llama.cpp',
    backend: 'HIP',
    device: 'RX 9070 XT',
    ctxTokens: 131072,
    kvType: 'q8_0',
    specMode: 'ngram',
    mode: 'Thinking',
    weightsGiB: 13.2,
  },
  medium: {
    name: 'Qwen 3.8 27B',
    quant: 'GSQ-RCO IQ3_S',
    engine: 'llama.cpp',
    backend: 'HIP',
    device: 'RX 9070 XT',
    ctxTokens: 98304,
    kvType: 'q8_0',
    specMode: 'MTP+ngram',
    vision: true,
    weightsGiB: 11.6,
  },
  low: {
    name: 'Gemma 4 26B-A4B',
    quant: 'Q4_0',
    engine: 'llama.cpp',
    backend: 'HIP',
    device: 'RX 9070 XT',
    ctxTokens: 16384,
    kvType: 'q8_0',
    weightsGiB: 13.4,
  },
  krea: {
    name: 'Krea 2 Realism Turbo',
    quant: 'Q8_0',
    engine: 'sd.cpp',
    backend: 'HIP',
    device: 'RX 9070 XT',
    imageSize: '512x768',
    weightsGiB: 7.4,
  },
};

const MEDIUM_LAYERS: VramLayer[] = [
  { id: 'weights', label: 'weights', gib: 11.6 },
  { id: 'kv', label: 'KV cache', gib: 2.9 },
  { id: 'buffers', label: 'buffers', gib: 0.9 },
  { id: 'draft', label: 'draft', gib: 0.12 },
];

export const SLOTS: Slot[] = [
  {
    id: 'low',
    label: 'SYSTEM 1',
    kind: 'llm',
    model: MODELS.low,
    availability: 'ready',
    expectedVram: [
      { id: 'weights', label: 'weights', gib: 13.4 },
      { id: 'kv', label: 'KV cache', gib: 0.4 },
      { id: 'buffers', label: 'buffers', gib: 0.8 },
    ],
  },
  { id: 'medium', label: 'SYSTEM 2', kind: 'llm', model: MODELS.medium, availability: 'ready', expectedVram: MEDIUM_LAYERS },
  {
    id: 'high',
    label: 'SYSTEM 3',
    kind: 'llm',
    model: MODELS.high,
    availability: 'ready',
    expectedVram: [
      { id: 'weights', label: 'weights', gib: 13.2 },
      { id: 'kv', label: 'KV cache', gib: 1.6 },
      { id: 'buffers', label: 'buffers', gib: 0.99 },
    ],
  },
  {
    id: 'krea',
    label: 'SYSTEM CGI',
    kind: 'image',
    model: MODELS.krea,
    availability: 'ready',
    expectedVram: [
      { id: 'weights', label: 'DiT layers', gib: 6.1 },
      { id: 'buffers', label: 'activations', gib: 3.2 },
      { id: 'other', label: 'VAE', gib: 0.6 },
    ],
  },
];

/** A gently varying 5-minute decode history ending at 47.3 (deterministic, no Math.random). */
function history(): number[] {
  const out: number[] = [];
  for (let i = 0; i < 300; i++) {
    const t = i / 299;
    const v = 41 + 5 * t + 1.6 * Math.sin(i * 0.21) + 0.9 * Math.sin(i * 0.057 + 1.3);
    out.push(Math.round(v * 10) / 10);
  }
  out[299] = 47.3;
  return out;
}

function vramHistory(): number[] {
  const out: number[] = [];
  for (let i = 0; i < 300; i++) out.push(15.52 - (i < 20 ? (20 - i) * 0.01 : 0));
  return out;
}

export const SAMPLE_NOW = 1_790_000_000;

export const SAMPLE_VM: ViewModel = {
  now: SAMPLE_NOW,
  slots: SLOTS,
  selected: 'medium',
  session: {
    slot: 'medium',
    model: MODELS.medium,
    phase: 'live',
    uptimeS: 2 * 3600 + 14 * 60 + 7,
    endpoint: { host: '127.0.0.1', port: 7030 },
    apiKeySet: true,
    loading: null,
    fault: null,
    llm: {
      activity: 'decode',
      decodeTps: 47.3,
      decodeHistory: history(),
      prefill: { tokens: 18432, doneTokens: 18432, tps: 812, elapsedS: 22.7, etaS: 0 },
      generatedTokens: 1284,
      context: { usedTokens: 19716, totalTokens: 98304 },
      spec: { acceptancePct: 71, mode: 'MTP + n-gram' },
      requests: [
        { id: 5, at: SAMPLE_NOW - 900, promptTokens: 6120, cachedTokens: 4096, prefillS: 3.1, generatedTokens: 512, decodeS: 11.2 },
        { id: 6, at: SAMPLE_NOW - 780, promptTokens: 8400, cachedTokens: 6100, prefillS: 3.4, generatedTokens: 760, decodeS: 16.9 },
        { id: 7, at: SAMPLE_NOW - 640, promptTokens: 9900, cachedTokens: 8300, prefillS: 2.4, generatedTokens: 1020, decodeS: 22.1 },
        { id: 8, at: SAMPLE_NOW - 500, promptTokens: 11800, cachedTokens: 9800, prefillS: 3.0, generatedTokens: 640, decodeS: 13.8 },
        { id: 9, at: SAMPLE_NOW - 380, promptTokens: 13200, cachedTokens: 11700, prefillS: 2.6, generatedTokens: 1410, decodeS: 30.5 },
        { id: 10, at: SAMPLE_NOW - 240, promptTokens: 15100, cachedTokens: 13100, prefillS: 3.2, generatedTokens: 880, decodeS: 18.4 },
        { id: 11, at: SAMPLE_NOW - 120, promptTokens: 16600, cachedTokens: 15000, prefillS: 2.7, generatedTokens: 1190, decodeS: 25.0 },
        { id: 12, at: SAMPLE_NOW - 30, promptTokens: 18432, cachedTokens: 0, prefillS: 22.7, generatedTokens: 1284, decodeS: 27.1 },
      ],
      totals: { requests: 12, promptTokens: 152_300, generatedTokens: 11_420 },
    },
    image: null,
  },
  vram: {
    device: 'RX 9070 XT',
    totalGiB: 15.87,
    usedGiB: 15.52,
    layers: MEDIUM_LAYERS,
    spillMiB: 0,
    history: vramHistory(),
    baselineGiB: 0,
    warnBelowGiB: 0.15,
  },
  system: { ramUsedGiB: 31.2, ramTotalGiB: 93.6, ramType: 'DDR5', cpuName: '9950X3D', cpuPct: 6 },
  lastSession: null,
  host: { kind: 'browser', frameless: false, maximized: false, appVersion: '0.2.0', panel: { available: true, active: false, target: '960x640' } },
  console: [
    'srv  load_model: loading model GSQ-RCO IQ3_S',
    'main: server is listening on :7030',
    'slot 0 · task 12 · prompt 18432 tok · cache 0',
    'slot 0 · prompt done · 812 t/s',
    'slot 0 · n_past 19716 · 47.3 t/s',
  ],
};
