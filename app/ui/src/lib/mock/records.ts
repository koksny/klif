// Mock records: the best values the mock machine (and its remote node) reached per model file and backend, the
// last broken records, and the climb of every record. Built relative to a clock (`now`, epoch seconds) so the
// numbers stay "recent" whenever the mock starts; the static sample uses its own fixed clock.
//
// The files, hashes and sources are real recommendation files (klif-catalog data/recommendations.toml) where one
// exists, so the strings look like a real machine's; the machine's own files (custom quants) have a hash but no
// source, and one entry has neither (a file KLIF has not hashed yet). Keys follow the core:
// "<machine>|<sha256 or file:size>|<backend>", a node's entry "<node>/<its own key>".
import type { RecordEntry, RecordEvent, RecordHistoryLine, RecordMetric, RecordModel, RecordValue, SystemKind } from '../model/types';
import { createRng, hashSeed } from './rng';

const DAY = 86400;
const KLIF = '0.3.1';
const THIS_MACHINE = 'This machine';
export const MOCK_NODE_ID = 'render-box';
export const MOCK_NODE_MACHINE = 'Render box';
/** The machine's FP32 TFLOPS total (the mock hardware) and the node's. */
const TFLOPS_LOCAL = 62.9;
const TFLOPS_NODE = 65.3;

const GPU_9070 = 'RX 9070 XT';
const GPU_5700 = 'RX 5700 XT';
const GPU_NODE = 'RX 7900 XTX';

/** *Tps / *Rtf round to 2 decimals, *S to 3 (the core's `rounded`). */
const HIGHER: Record<RecordMetric, boolean> = { decodeTps: true, prefillTps: true, ttftS: false, imageS: false, ttsRtf: true, sttRtf: true, videoS: false };
export const roundMetric = (m: RecordMetric, v: number) => {
  const p = HIGHER[m] ? 100 : 1000;
  return Math.round(v * p) / p;
};

/** One best value of a definition: `ago` = seconds before the clock it was reached; the rest is the conditions. */
type Cond = Omit<RecordValue, 'value' | 'at' | 'source' | 'gpus' | 'klifVersion' | 'tflopsFp32' | 'backendBuild' | 'preset'>;
interface Best {
  value: number;
  ago: number;
  source?: 'live' | 'bench';
  /** History steps (3..6 by default); 1 = the first value of this metric. */
  steps?: number;
  cond?: Cond;
}

interface Def {
  node?: string;
  machine: string;
  kind: SystemKind;
  model: RecordModel;
  backend: string;
  gpus: string[];
  tflops: number;
  build?: string;
  preset?: string;
  best: Partial<Record<RecordMetric, Best>>;
}

const GIB = 1024 ** 3;
const H = 3600;

const DEFS: Def[] = [
  // ---- this machine: LLMs ------------------------------------------------------------------------------
  {
    machine: THIS_MACHINE,
    kind: 'llm',
    model: {
      sha256: '3f227079003add2511437e5b1e94812e363385225bf6a9b47b0054a72bc8b01e',
      file: 'Qwen3.8-27B-UD-Q4_K_XL.gguf',
      name: 'Qwen 3.8 27B',
      quant: 'UD-Q4_K_XL',
      source: 'unsloth/Qwen3.8-27B-GGUF@4ca720788d1e01f1bff70c033e0d0028fd02e502',
      sizeBytes: 17559178144,
    },
    backend: 'HIP',
    gpus: [GPU_9070, GPU_5700],
    tflops: TFLOPS_LOCAL,
    build: 'b9112',
    best: {
      decodeTps: { value: 34.81, ago: 6.2 * DAY, cond: { ctx: 98304, kv: 'q8_0', promptTokens: 1820, genTokens: 640 } },
      prefillTps: { value: 742.63, ago: 6.2 * DAY, cond: { ctx: 98304, kv: 'q8_0', promptTokens: 18432, cachedTokens: 0 } },
      ttftS: { value: 0.419, ago: 9.4 * DAY, cond: { ctx: 98304, kv: 'q8_0', promptTokens: 4096, cachedTokens: 3584 } },
    },
  },
  {
    machine: THIS_MACHINE,
    kind: 'llm',
    model: {
      sha256: '1440b132511f9fbb94246f259f17a8403bc9348b28bbf3b7051f2a26c85730ac',
      file: 'qwen-3.8-27b-gsq-iq3_s.gguf',
      name: 'Qwen 3.8 27B',
      quant: 'GSQ-RCO IQ3_S',
      sizeBytes: Math.round(11.6 * GIB),
    },
    backend: 'HIP',
    gpus: [GPU_9070],
    tflops: TFLOPS_LOCAL,
    build: 'b9112',
    preset: 'qwen-27b',
    best: {
      decodeTps: { value: 56.42, ago: 2.1 * DAY, cond: { ctx: 98304, kv: 'q8_0', promptTokens: 2210, cachedTokens: 1980, genTokens: 912 } },
      prefillTps: { value: 1012.84, ago: 3.4 * DAY, cond: { ctx: 98304, kv: 'q8_0', promptTokens: 18432, cachedTokens: 0 } },
      ttftS: { value: 0.268, ago: 1.3 * DAY, cond: { ctx: 98304, kv: 'q8_0', promptTokens: 1210, cachedTokens: 1100 } },
    },
  },
  {
    machine: THIS_MACHINE,
    kind: 'llm',
    model: {
      sha256: 'ef728c8e0c337fd1067b947af006e38a9ef2419e56feced4fd29b4bf0636e30c',
      file: 'gemma-4-26B-A4B-it-UD-Q4_K_XL.gguf',
      name: 'Gemma 4 26B-A4B',
      quant: 'UD-Q4_K_XL',
      source: 'unsloth/gemma-4-26B-A4B-it-GGUF@c099eb48e663fd284577b04978a94ffccb261841',
      sizeBytes: 17010980576,
    },
    backend: 'HIP',
    gpus: [GPU_9070],
    tflops: TFLOPS_LOCAL,
    build: 'b9112',
    preset: 'gemma-26b',
    best: {
      // The newest record on the machine (it is also the newest of the two recent events).
      decodeTps: { value: 96.81, ago: 780, cond: { ctx: 16384, kv: 'q8_0', promptTokens: 1480, genTokens: 770 } },
      prefillTps: { value: 3120.44, ago: 4.8 * DAY, cond: { ctx: 16384, kv: 'q8_0', promptTokens: 12288, cachedTokens: 0 } },
      ttftS: { value: 0.127, ago: 4.8 * DAY, cond: { ctx: 16384, kv: 'q8_0', promptTokens: 640, cachedTokens: 512 } },
    },
  },
  {
    // The same file on the other backend: its own entry.
    machine: THIS_MACHINE,
    kind: 'llm',
    model: {
      sha256: 'ef728c8e0c337fd1067b947af006e38a9ef2419e56feced4fd29b4bf0636e30c',
      file: 'gemma-4-26B-A4B-it-UD-Q4_K_XL.gguf',
      name: 'Gemma 4 26B-A4B',
      quant: 'UD-Q4_K_XL',
      source: 'unsloth/gemma-4-26B-A4B-it-GGUF@c099eb48e663fd284577b04978a94ffccb261841',
      sizeBytes: 17010980576,
    },
    backend: 'Vulkan',
    gpus: [GPU_9070],
    tflops: TFLOPS_LOCAL,
    build: 'b9112',
    best: {
      decodeTps: { value: 81.27, ago: 11.5 * DAY, cond: { ctx: 16384, kv: 'q8_0', promptTokens: 1480, genTokens: 702 } },
      prefillTps: { value: 2214.7, ago: 11.5 * DAY, cond: { ctx: 16384, kv: 'q8_0', promptTokens: 12288, cachedTokens: 0 } },
      ttftS: { value: 0.171, ago: 11.4 * DAY, cond: { ctx: 16384, kv: 'q8_0', promptTokens: 640, cachedTokens: 512 } },
    },
  },
  {
    machine: THIS_MACHINE,
    kind: 'llm',
    model: {
      sha256: '204913539736177065144612cdf7fdc502d9acc7d9b323915dc58cd66be67534',
      file: 'qwen-3.8-flash-next-iq2_xxs.gguf',
      name: 'Qwen 3.8 Flash-Next',
      quant: 'IQ2_XXS',
      sizeBytes: Math.round(13.2 * GIB),
    },
    backend: 'HIP',
    gpus: [GPU_9070],
    tflops: TFLOPS_LOCAL,
    build: 'b9112',
    preset: 'flash-next',
    best: {
      decodeTps: { value: 12.93, ago: 15.2 * DAY, cond: { ctx: 131072, kv: 'q8_0', promptTokens: 2400, genTokens: 410 } },
      prefillTps: { value: 371.8, ago: 15.2 * DAY, cond: { ctx: 131072, kv: 'q8_0', promptTokens: 66214, cachedTokens: 0 } },
      ttftS: { value: 1.624, ago: 15.3 * DAY, cond: { ctx: 131072, kv: 'q8_0', promptTokens: 3200, cachedTokens: 2900 } },
    },
  },
  // ---- this machine: image, speech ---------------------------------------------------------------------
  {
    machine: THIS_MACHINE,
    kind: 'image',
    model: {
      sha256: '66df0ded1cca2b55b89ae15c782d7b4078a105e7504060cec0e3bb4f48280546',
      file: 'krea-2-realism-turbo-q8_0.gguf',
      name: 'Krea 2 Realism Turbo',
      quant: 'Q8_0',
      sizeBytes: Math.round(7.4 * GIB),
    },
    backend: 'HIP',
    gpus: [GPU_9070],
    tflops: TFLOPS_LOCAL,
    preset: 'krea-realism',
    best: {
      imageS: { value: 11.842, ago: 3.1 * DAY, cond: { width: 512, height: 768, steps: 8 } },
    },
  },
  {
    machine: THIS_MACHINE,
    kind: 'image',
    model: {
      sha256: '631d532e7ca71e8d90a87c71d3699761a812039d22e3370e87498d87754660fe',
      file: 'qwen-image-2.1-Q4_K_M.gguf',
      name: 'Qwen Image 2.1',
      quant: 'Q4_K_M',
      source: 'unsloth/Qwen-Image-2.1-GGUF@2c31ccd392b367a6637841a143813320a02dff55',
      sizeBytes: 4199565024,
    },
    backend: 'HIP',
    gpus: [GPU_9070],
    tflops: TFLOPS_LOCAL,
    best: {
      imageS: { value: 28.317, ago: 17.6 * DAY, cond: { width: 640, height: 928, steps: 20 } },
    },
  },
  {
    // TTS servers log no per-request timing: the only source is klif-cli bench. The first value of its metric.
    machine: THIS_MACHINE,
    kind: 'tts',
    model: {
      sha256: '5d800fd204029302c10313daeafdb31c875c7c29ae31974d0d156cc7f512d1d0',
      file: 'kokoro-82m-q8_0.gguf',
      name: 'Kokoro 82M',
      quant: 'Q8_0',
      source: 'audio-cpp/audio.cpp-gguf@69492fe6b3f35ec13e49e5d2ed388bc9c24473d4',
      sizeBytes: 189549408,
    },
    backend: 'Vulkan',
    gpus: [GPU_5700],
    tflops: TFLOPS_LOCAL,
    best: {
      ttsRtf: { value: 41.72, ago: 5400, source: 'bench', steps: 1 },
    },
  },
  // ---- the remote node "Render box" ---------------------------------------------------------------------
  {
    node: MOCK_NODE_ID,
    machine: MOCK_NODE_MACHINE,
    kind: 'image',
    model: {
      sha256: 'a79cc00120acd0aab072ebab8922c5dfece9c7b90101b617a3b080df602c38d9',
      file: 'z-image-turbo-q8_0.gguf',
      name: 'Z-Image Turbo',
      quant: 'Q8_0',
      sizeBytes: Math.round(6.9 * GIB),
    },
    backend: 'HIP',
    gpus: [GPU_NODE],
    tflops: TFLOPS_NODE,
    preset: 'z-image',
    best: {
      imageS: { value: 5.913, ago: 1.8 * DAY, cond: { width: 1024, height: 1024, steps: 8 } },
    },
  },
  {
    node: MOCK_NODE_ID,
    machine: MOCK_NODE_MACHINE,
    kind: 'image',
    model: {
      // Not hashed on that machine yet: the key is "<file>:<size>", and there is no source.
      file: 'qwen-image-edit-q4_0.gguf',
      name: 'Qwen Image Edit',
      quant: 'Q4_0',
      sizeBytes: Math.round(11.6 * GIB),
    },
    backend: 'HIP',
    gpus: [GPU_NODE],
    tflops: TFLOPS_NODE,
    preset: 'qwen-edit',
    best: {
      imageS: { value: 33.126, ago: 8.7 * DAY, cond: { width: 1024, height: 1024, steps: 20 } },
    },
  },
];

/** `<machine>|<sha256 or file:size>|<backend>`, a node's as `<node>/<its key>`. */
export function mockRecordKey(d: { node?: string; machine: string; model: RecordModel; backend: string }): string {
  const own = `${d.machine}|${d.model.sha256 ?? `${d.model.file}:${d.model.sizeBytes ?? 0}`}|${d.backend}`;
  return d.node ? `${d.node}/${own}` : own;
}

function bestValue(d: Def, b: Best, now: number): RecordValue {
  const v: RecordValue = {
    value: b.value,
    at: Math.round(now - b.ago),
    source: b.source ?? 'live',
    ...b.cond,
    gpus: [...d.gpus],
    klifVersion: KLIF,
    tflopsFp32: d.tflops,
  };
  if (d.build) v.backendBuild = d.build;
  if (d.preset && (b.source ?? 'live') === 'live') v.preset = d.preset;
  return v;
}

/** The climb to a best value: 3..6 broken records over the last weeks (or `steps`), ending exactly at the best. */
function climb(key: string, metric: RecordMetric, best: RecordValue, steps?: number): RecordHistoryLine[] {
  const rng = createRng(hashSeed(`${key}#${metric}`));
  const n = steps ?? rng.int(3, 6);
  if (n <= 1) return [{ key, metric, new: best.value, at: best.at }];
  const higher = HIGHER[metric];
  const start = best.value * (higher ? rng.range(0.6, 0.78) : rng.range(1.4, 1.85));
  const span = rng.range(13, 26) * DAY;
  // Time of step i: spread over the span, the last one at the best; values: diminishing returns with a little noise.
  const lines: RecordHistoryLine[] = [];
  let prev: number | undefined;
  for (let i = 0; i < n; i++) {
    const f = i / (n - 1);
    const at = Math.round(best.at - span * (1 - f) ** 1.15);
    const progress = i === n - 1 ? 1 : Math.min(0.97, f ** 0.75 * rng.range(0.9, 1.05));
    const v = i === n - 1 ? best.value : roundMetric(metric, start + (best.value - start) * progress);
    const line: RecordHistoryLine = { key, metric, new: v, at };
    if (prev !== undefined) line.old = prev;
    lines.push(line);
    prev = v;
  }
  return lines;
}

export interface MockRecordSet {
  entries: RecordEntry[];
  /** The last broken records, newest last (at most 8). */
  events: RecordEvent[];
  /** Every broken record of every entry, oldest first within a key and metric. */
  history: RecordHistoryLine[];
}

/** The mock machine's records as of `now`: best values, the two recent events, and the history behind them. */
export function mockRecordSet(now: number): MockRecordSet {
  const entries: RecordEntry[] = [];
  const history: RecordHistoryLine[] = [];
  for (const d of DEFS) {
    const key = mockRecordKey(d);
    const best: RecordEntry['best'] = {};
    for (const m of Object.keys(d.best) as RecordMetric[]) {
      const b = d.best[m]!;
      const rv = bestValue(d, b, now);
      best[m] = rv;
      history.push(...climb(key, m, rv, b.steps));
    }
    const e: RecordEntry = { key, machine: d.machine, kind: d.kind, model: { ...d.model }, backend: d.backend, best };
    if (d.node) e.node = d.node;
    entries.push(e);
  }
  history.sort((a, b) => a.at - b.at);
  // Recent events = the newest history lines (the real core keeps the last 8); the mock shows the two freshest.
  const events: RecordEvent[] = history.slice(-2).map((l) => ({ key: l.key, metric: l.metric, ...(l.old !== undefined ? { old: l.old } : {}), new: l.new, at: l.at }));
  return { entries, events, history };
}
