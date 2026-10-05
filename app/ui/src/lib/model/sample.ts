// A static snapshot that matches the approved mockups exactly (System 2 live, decoding), expressed in the 0.3
// view model: the four Systems of the operator's file (System 1/2/3, CGI) with their presets. Used as the first
// frame and by the static "sample" scenario (screenshots).
import { APP_VERSION } from '../mock/host';
import { MOCK_HARDWARE, MOCK_RECORD_EVENTS, MOCK_RECORDS, MOCK_SUGGESTIONS } from '../mock/hardware';
import {
  availabilityOf,
  buildCommand,
  expectedLayers,
  factsOf,
  gpuList,
  GPU_9070,
  localPresets,
  modelRefOf,
  paramViews,
  presetInfo,
  recommendationInfos,
} from '../mock/catalog';
import type { LlmClass, System, SystemKind, ViewModel, VramLayer } from './types';

export const SAMPLE_NOW = 1_790_000_000;

const PRESETS = localPresets();

const ctxFor = { apiKeySet: true, modelsDir: 'D:\\models' };

function system(id: string, label: string, kind: SystemKind, cls: LlmClass | undefined, presetId: string): System {
  const spec = PRESETS[presetId];
  const cmd = buildCommand(spec, ctxFor);
  const model = modelRefOf(spec, cmd, undefined, GPU_9070.name);
  const a = availabilityOf(spec, cmd);
  const layers = expectedLayers(spec, model);
  const port = cmd.port;
  const s: System = {
    id,
    label,
    kind,
    status: 'offline',
    availability: a.availability,
    model,
    preset: presetId,
    params: paramViews(spec),
    command: cmd,
    gpu: gpuList(spec, GPU_9070.id)[0],
    gpus: gpuList(spec, GPU_9070.id),
    external: false,
    exclusive: true,
    editable: true,
    controllable: true,
    conflicts: [],
    activity: 0,
    expectedVram: layers,
    endpoint: kind === 'llm' ? `http://127.0.0.1:${port}/v1` : `http://127.0.0.1:${port}/`,
  };
  if (cls) s.class = cls;
  if (factsOf(spec)) s.expectedVramSource = 'file-size';
  return s;
}

const S1 = system('s1', 'System 1', 'llm', 'fast', 'gemma-26b');
const S2 = system('s2', 'System 2', 'llm', 'deep', 'qwen-27b');
const S3 = system('s3', 'System 3', 'llm', 'max', 'flash-next');
const CGI = system('cgi', 'System CGI', 'image', undefined, 'krea-realism');

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

const MEDIUM_LAYERS: VramLayer[] = [
  { id: 'weights', label: 'weights', gib: 11.6 },
  { id: 'kv', label: 'KV cache', gib: 2.9 },
  { id: 'buffers', label: 'buffers', gib: 0.9 },
  { id: 'draft', label: 'draft', gib: 0.12 },
];

const SESSION: NonNullable<System['session']> = {
  system: 's2',
  model: S2.model,
  phase: 'live',
  uptimeS: 2 * 3600 + 14 * 60 + 7,
  endpoint: { host: '127.0.0.1', port: 7031 },
  apiKeySet: true,
  loading: null,
  fault: null,
  image: null,
  generic: null,
  preset: 'qwen-27b',
  command: S2.command,
  gpu: GPU_9070.id,
  vramGiB: 15.52,
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
};

const S2_LIVE: System = { ...S2, status: 'busy', session: SESSION, activity: 0.8 };
// The other three would have to stop first: one exclusive GPU.
const SYSTEMS: System[] = [
  { ...S1, conflicts: ['s2'], reason: 'Needs System 2 stopped: the GPU is exclusive.' },
  S2_LIVE,
  { ...S3, conflicts: ['s2'], reason: 'Needs System 2 stopped: the GPU is exclusive.' },
  { ...CGI, conflicts: ['s2'], reason: 'Needs System 2 stopped: the GPU is exclusive.' },
];

const presetInfos = Object.entries(PRESETS).map(([id, spec]) =>
  presetInfo(id, spec, buildCommand(spec, ctxFor), GPU_9070.name, {}),
);

export const SAMPLE_VM: ViewModel = {
  now: SAMPLE_NOW,
  systems: SYSTEMS,
  selected: 's2',
  session: SESSION,
  lastSession: null,
  vram: {
    id: GPU_9070.id,
    name: GPU_9070.name,
    device: GPU_9070.name,
    totalGiB: GPU_9070.totalGiB,
    usedGiB: 15.52,
    layers: [{ id: 'other', label: 'other', gib: 0 }, ...MEDIUM_LAYERS],
    spillMiB: 0,
    history: vramHistory(),
    baselineGiB: 0,
    warnBelowGiB: 0.15,
  },
  gpus: [
    {
      id: GPU_9070.id,
      name: GPU_9070.name,
      device: GPU_9070.name,
      totalGiB: GPU_9070.totalGiB,
      usedGiB: 15.52,
      layers: [{ id: 'other', label: 'other', gib: 0 }],
      spillMiB: 0,
      history: vramHistory(),
      baselineGiB: 0,
      warnBelowGiB: 0.15,
    },
  ],
  machine: { ramUsedGiB: 31.2, ramTotalGiB: 93.6, ramType: 'DDR5', cpuName: '9950X3D', cpuPct: 6 },
  host: { kind: 'browser', frameless: false, maximized: false, appVersion: APP_VERSION, panel: { available: true, active: false, target: '960x640' } },
  presets: presetInfos,
  recommendations: recommendationInfos(PRESETS, new Set()),
  hardware: MOCK_HARDWARE,
  suggestions: MOCK_SUGGESTIONS,
  records: MOCK_RECORDS,
  recordEvents: MOCK_RECORD_EVENTS,
  downloads: [],
  config: {
    path: '%APPDATA%\\KLIF\\klif.toml',
    stateDir: '%APPDATA%\\KLIF',
    dataDir: '%LOCALAPPDATA%\\KLIF',
    issues: [],
    apiKey: { source: 'file', set: true },
    modelsDir: 'D:\\models',
    onConflict: 'ask',
    recordMoment: true,
    webui: { enabled: false, host: '0.0.0.0', port: 7341, listening: false, urls: [], devices: [] },
  },
  nodes: [],
  console: [
    'srv  load_model: loading model GSQ-RCO IQ3_S',
    'main: server is listening on :7031',
    'slot 0 · task 12 · prompt 18432 tok · cache 0',
    'slot 0 · prompt done · 812 t/s',
    'slot 0 · n_past 19716 · 47.3 t/s',
  ],
};
