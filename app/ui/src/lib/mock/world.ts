// What a scenario starts from: the Systems in "klif.toml", the presets, the GPUs, the remote nodes. A world is
// configuration only; the engine simulates what runs.
import type { LlmClass, NodeState, OnConflict, PresetSpec, SystemKind } from '../model/types';
import { GPU_5700, GPU_9070, GPU_NODE, localPresets, MODELS_DIR, nodePresets } from './catalog';

export interface WorldSystem {
  /** Node-local id ("s1", "cgi", "tts"). */
  id: string;
  label: string;
  kind: SystemKind;
  class?: LlmClass;
  preset?: string;
  params?: Record<string, string>;
  exclusive?: boolean;
  /** The remote node it lives on. */
  node?: string;
}

export interface GpuDef {
  id: string;
  name: string;
  totalGiB: number;
}

export interface WorldNode {
  id: string;
  name: string;
  address: string;
  state: NodeState;
  allow: string[];
  gpus: GpuDef[];
  presets: Record<string, PresetSpec>;
  cpuName: string;
  ramTotalGiB: number;
  error?: string;
  latencyMs?: number;
}

export interface World {
  systems: WorldSystem[];
  presets: Record<string, PresetSpec>;
  gpus: GpuDef[];
  nodes: WorldNode[];
  modelsDir?: string;
  apiKeySet: boolean;
  onConflict: OnConflict;
  /** Recommendation ids whose files are "downloaded". */
  downloaded: string[];
  /** Selected System at the start. */
  selected: string;
  /** False: no records yet (a fresh install). Default true: the mock machine's records (records.ts). */
  records?: boolean;
}

const SYS = {
  s1: { id: 's1', label: 'System 1', kind: 'llm', class: 'fast', preset: 'gemma-26b' },
  s2: { id: 's2', label: 'System 2', kind: 'llm', class: 'deep', preset: 'qwen-27b' },
  s3: { id: 's3', label: 'System 3', kind: 'llm', class: 'max', preset: 'flash-next' },
  cgi: { id: 'cgi', label: 'System CGI', kind: 'image', preset: 'krea-realism' },
} as const satisfies Record<string, WorldSystem>;

const GPUS: GpuDef[] = [GPU_9070, GPU_5700];

/**
 * The four Systems of the operator's own file: one GPU, all four exclusive, so launching one while another
 * runs asks to stop it first (today's one-at-a-time behaviour). The classic scenarios use it.
 */
export function classicWorld(): World {
  return {
    systems: [SYS.s1, SYS.s2, SYS.s3, SYS.cgi].map((s) => ({ ...s, exclusive: true })),
    presets: localPresets(),
    gpus: [GPU_9070],
    nodes: [],
    modelsDir: MODELS_DIR,
    apiKeySet: true,
    onConflict: 'ask',
    downloaded: [],
    selected: 's2',
  };
}

function renderBox(state: NodeState): WorldNode {
  return {
    id: 'render-box',
    name: 'Render box',
    address: '192.0.2.10:7340',
    state,
    allow: ['launch'],
    gpus: [GPU_NODE],
    presets: nodePresets(),
    cpuName: '7950X',
    ramTotalGiB: 64,
    ...(state === 'online' ? { latencyMs: 4 } : { error: 'Connection timed out.' }),
  };
}

/** The default scenario: Systems that share a GPU, an external TTS server, a remote image node. */
export function multiWorld(nodeState: NodeState = 'online'): World {
  return {
    systems: [
      { ...SYS.s1 },
      { ...SYS.s2 },
      { id: 's3', label: 'System 3', kind: 'llm', class: 'max' },
      { ...SYS.cgi },
      { id: 'tts', label: 'System TTS', kind: 'tts', preset: 'kokoro-tts' },
      { id: 'cgi', label: 'Z-Image', kind: 'image', preset: 'z-image', node: 'render-box' },
    ],
    presets: localPresets(),
    gpus: GPUS,
    nodes: [renderBox(nodeState)],
    modelsDir: MODELS_DIR,
    apiKeySet: true,
    onConflict: 'ask',
    downloaded: [],
    selected: 's1',
  };
}

/** Every kind: the multi world plus a transcription and a video System (generic displays). */
export function kindsWorld(): World {
  const w = multiWorld();
  w.systems.splice(5, 0, { id: 'stt', label: 'System STT', kind: 'stt', preset: 'whisper-stt' }, { id: 'video', label: 'System Video', kind: 'video', preset: 'comfy-video' });
  w.selected = 'tts';
  return w;
}

/** A fresh install: no klif.toml systems yet (the onboarding card), no models directory. */
export function emptyWorld(): World {
  return {
    systems: [],
    presets: {},
    gpus: [GPU_9070],
    nodes: [],
    apiKeySet: false,
    onConflict: 'ask',
    downloaded: [],
    selected: '',
    records: false,
  };
}
