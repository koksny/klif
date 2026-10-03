// The mock's catalog: presets as klif.toml writes them (PresetSpec), the facts a real core derives from model
// files (sizes, shape), commands resolved the way the core does (placeholders, params, managed env), and a few
// fictional recommendations. Paths are fictional (D:\llama.cpp, D:\models); nothing here is a promise.
import { fmtCtx } from '../model/format';
import type {
  AdapterId,
  Availability,
  CommandView,
  EnvView,
  HealthCheck,
  Issue,
  ModelArch,
  ModelRef,
  ParamView,
  PresetInfo,
  PresetSpec,
  RecommendationInfo,
  SystemKind,
  VramLayer,
} from '../model/types';
import { fitLayers, FIT_HEADROOM_GIB, type FitResult } from '../shell/fit';

export { fitLayers, FIT_HEADROOM_GIB, type FitResult };

export const MODELS_DIR = 'D:\\models';
export const LLAMA = 'D:\\llama.cpp\\llama-server.exe';
export const SD_SERVER = 'D:\\stable-diffusion.cpp\\sd-server.exe';
export const WHISPER = 'D:\\whisper.cpp\\whisper-server.exe';
export const COMFY_PY = 'D:\\ComfyUI\\python_embeded\\python.exe';
export const MOCK_KEY_MASK = '\u2022\u2022\u2022\u2022';

/** GPUs of the mock machine. */
export const GPU_9070 = { id: '1002:7550', name: 'RX 9070 XT', totalGiB: 15.87 };
export const GPU_5700 = { id: '1002:731f', name: 'RX 5700 XT', totalGiB: 7.86 };
export const GPU_NODE = { id: '1002:744c', name: 'RX 7900 XTX', totalGiB: 23.9 };

// ------------------------------------------------------------------------------------------------ facts

export type Family = 'qwen27' | 'flashnext' | 'gemma' | 'krea' | 'generic';

/** What a real core reads from a model file / the log: size and shape. Keyed by the model path in the preset. */
export interface ModelFacts {
  name: string;
  quant: string;
  weightsGiB: number;
  /** LLM: KV cache GiB per context token at q8_0. */
  kvGiBPerTokQ8?: number;
  buffersGiB: number;
  specMode?: string;
  arch?: ModelArch;
  /** Image: VAE GiB, activations GiB at 512x768 and the share of the weights that stays on the GPU. */
  vaeGiB?: number;
  activationsGiB?: number;
  gpuWeightShare?: number;
  family: Family;
}

const QWEN27_ARCH: ModelArch = { layers: 64, experts: 0, expertsUsed: 0, sharedExperts: 0, heads: 24, kvHeads: 4, embd: 5120, vocab: 248320, params: '26.9 B' };
const FLASH_ARCH: ModelArch = { layers: 48, experts: 512, expertsUsed: 10, sharedExperts: 1, heads: 24, kvHeads: 2, embd: 2560, vocab: 248320, params: '176.94 B' };
const GEMMA_ARCH: ModelArch = { layers: 30, experts: 128, expertsUsed: 8, sharedExperts: 1, heads: 16, kvHeads: 8, embd: 2816, vocab: 262144, params: '25.2 B' };

export const FACTS: Record<string, ModelFacts> = {
  [`${MODELS_DIR}\\gemma-4-26b-a4b-q4_0.gguf`]: {
    name: 'Gemma 4 26B-A4B',
    quant: 'Q4_0',
    weightsGiB: 13.4,
    kvGiBPerTokQ8: 0.4 / 16384,
    buffersGiB: 0.8,
    arch: GEMMA_ARCH,
    family: 'gemma',
  },
  [`${MODELS_DIR}\\qwen-3.8-27b-gsq-iq3_s.gguf`]: {
    name: 'Qwen 3.8 27B',
    quant: 'GSQ-RCO IQ3_S',
    weightsGiB: 11.6,
    kvGiBPerTokQ8: 2.9 / 98304,
    buffersGiB: 0.9,
    specMode: 'MTP+ngram',
    arch: QWEN27_ARCH,
    family: 'qwen27',
  },
  [`${MODELS_DIR}\\qwen-3.8-flash-next-iq2_xxs.gguf`]: {
    name: 'Qwen 3.8 Flash-Next',
    quant: 'IQ2_XXS',
    weightsGiB: 13.2,
    kvGiBPerTokQ8: 1.6 / 131072,
    buffersGiB: 0.99,
    specMode: 'ngram',
    arch: FLASH_ARCH,
    family: 'flashnext',
  },
  [`${MODELS_DIR}\\krea-2-realism-turbo-q8_0.gguf`]: {
    name: 'Krea 2 Realism Turbo',
    quant: 'Q8_0',
    weightsGiB: 7.4,
    buffersGiB: 0,
    vaeGiB: 0.6,
    activationsGiB: 3.2,
    gpuWeightShare: 6.1 / 7.4,
    family: 'krea',
  },
  [`${MODELS_DIR}\\qwen-image-2.1-q4_0.gguf`]: {
    name: 'Qwen Image 2.1',
    quant: 'Q4_0',
    weightsGiB: 11.9,
    buffersGiB: 0,
    vaeGiB: 0.3,
    activationsGiB: 2.8,
    gpuWeightShare: 0.75,
    family: 'krea',
  },
  [`${MODELS_DIR}\\kokoro-82m.onnx`]: { name: 'Kokoro 82M', quant: 'fp16', weightsGiB: 0.2, buffersGiB: 0.1, family: 'generic' },
  [`${MODELS_DIR}\\ggml-large-v3-turbo-q8_0.bin`]: { name: 'Whisper large-v3 turbo', quant: 'Q8_0', weightsGiB: 0.87, buffersGiB: 0.4, family: 'generic' },
  [`${MODELS_DIR}\\wan-2.2-ti2v-5b-q8_0.gguf`]: { name: 'Wan 2.2 TI2V 5B', quant: 'Q8_0', weightsGiB: 5.4, buffersGiB: 1.6, family: 'generic' },
  [`${MODELS_DIR}\\qwen-3.8-flash-next-ud-q2_k_xl.gguf`]: {
    name: 'Qwen 3.8 Flash-Next',
    quant: 'UD-Q2_K_XL',
    weightsGiB: 15.8,
    kvGiBPerTokQ8: 1.6 / 131072,
    buffersGiB: 0.99,
    specMode: 'ngram',
    arch: FLASH_ARCH,
    family: 'flashnext',
  },
  'E:\\render\\models\\z-image-turbo-q8_0.gguf': {
    name: 'Z-Image Turbo',
    quant: 'Q8_0',
    weightsGiB: 6.9,
    buffersGiB: 0,
    vaeGiB: 0.4,
    activationsGiB: 3.6,
    gpuWeightShare: 1,
    family: 'krea',
  },
  'E:\\render\\models\\qwen-image-edit-q4_0.gguf': {
    name: 'Qwen Image Edit',
    quant: 'Q4_0',
    weightsGiB: 11.6,
    buffersGiB: 0,
    vaeGiB: 0.3,
    activationsGiB: 3.2,
    gpuWeightShare: 1,
    family: 'krea',
  },
};

/** Model files that do not exist on the mock machine (the preset then reads "model missing"). */
export const MISSING_FILES = new Set<string>([`${MODELS_DIR}\\qwen-3.8-flash-next-ud-q2_k_xl.gguf`]);

// ----------------------------------------------------------------------------------------------- presets

const llamaArgs = (...extra: string[]): string[] => [
  '-m', '{model}', '-c', '{ctx}', '-ctk', 'q8_0', '-ctv', 'q8_0', '-ngl', '99', '--flash-attn', 'on',
  '--host', '{host}', '--port', '{port}', ...extra,
];

const onOff = (flag: string[], label = 'Reasoning') => ({
  label,
  default: 'on',
  choices: { on: { args: [...flag, 'on'] }, off: { label: 'Off', args: [...flag, 'off'] } },
});

/** The presets of this machine, as `[presets.<id>]` in klif.toml. Fictional paths. */
export function localPresets(): Record<string, PresetSpec> {
  return {
    'gemma-26b': {
      name: 'Gemma 4 26B-A4B',
      adapter: 'llama.cpp',
      command: LLAMA,
      args: llamaArgs('{p.reasoning}'),
      cwd: 'D:\\llama.cpp',
      env: { HIP_VISIBLE_DEVICES: '0', PATH: 'D:\\ROCm\\bin;{env:PATH}' },
      port: 7030,
      model: `${MODELS_DIR}\\gemma-4-26b-a4b-q4_0.gguf`,
      ctx: 16384,
      gpu: GPU_9070.id,
      model_name: 'Gemma 4 26B-A4B',
      quant: 'Q4_0',
      backend: 'HIP',
      recommended: 'gemma-4-26b-a4b-q4_0',
      params: { reasoning: onOff(['--reasoning']) },
    },
    'qwen-27b': {
      name: 'Qwen 3.8 27B',
      adapter: 'llama.cpp',
      command: LLAMA,
      args: llamaArgs('{p.vision}', '{p.reasoning}', '--spec-type', 'mtp,ngram'),
      cwd: 'D:\\llama.cpp',
      env: { HIP_VISIBLE_DEVICES: '0', PATH: 'D:\\ROCm\\bin;{env:PATH}' },
      port: 7031,
      model: `${MODELS_DIR}\\qwen-3.8-27b-gsq-iq3_s.gguf`,
      mmproj: `${MODELS_DIR}\\qwen-3.8-27b-mmproj-f16.gguf`,
      ctx: 98304,
      gpu: GPU_9070.id,
      model_name: 'Qwen 3.8 27B',
      quant: 'GSQ-RCO IQ3_S',
      backend: 'HIP',
      recommended: 'qwen-3.8-27b-gsq-iq3',
      params: {
        vision: {
          label: 'Vision',
          default: 'on',
          choices: { on: { args: ['--mmproj', '{mmproj}'] }, off: { label: 'Off', args: [] } },
        },
        reasoning: onOff(['--reasoning']),
      },
    },
    'flash-next': {
      name: 'Qwen 3.8 Flash-Next',
      adapter: 'llama.cpp',
      command: LLAMA,
      args: llamaArgs('{p.mode}', '--cache-ram', '2048'),
      cwd: 'D:\\llama.cpp',
      env: { HIP_VISIBLE_DEVICES: '0', PATH: 'D:\\ROCm\\bin;{env:PATH}' },
      port: 7032,
      model: `${MODELS_DIR}\\qwen-3.8-flash-next-iq2_xxs.gguf`,
      ctx: 131072,
      gpu: GPU_9070.id,
      model_name: 'Qwen 3.8 Flash-Next',
      quant: 'IQ2_XXS',
      backend: 'HIP',
      recommended: 'qwen-3.8-flash-next-iq2',
      params: {
        mode: {
          label: 'Mode',
          default: 'Thinking',
          choices: {
            Thinking: { label: 'Thinking', args: ['--reasoning', 'on'] },
            Instruct: { label: 'Instruct', args: ['--reasoning', 'off'] },
          },
        },
      },
    },
    'flash-next-q2': {
      name: 'Flash-Next UD-Q2_K_XL',
      adapter: 'llama.cpp',
      command: LLAMA,
      args: llamaArgs('--cache-ram', '2048'),
      cwd: 'D:\\llama.cpp',
      env: { HIP_VISIBLE_DEVICES: '0', PATH: 'D:\\ROCm\\bin;{env:PATH}' },
      port: 7032,
      model: `${MODELS_DIR}\\qwen-3.8-flash-next-ud-q2_k_xl.gguf`,
      ctx: 16384,
      gpu: GPU_9070.id,
      model_name: 'Qwen 3.8 Flash-Next',
      quant: 'UD-Q2_K_XL',
      backend: 'HIP',
    },
    'krea-realism': {
      name: 'Krea 2 Realism Turbo',
      adapter: 'sd.cpp',
      command: SD_SERVER,
      args: ['--diffusion-model', '{model}', '-W', '{p.size.W}', '-H', '{p.size.H}', '--steps', '8', '--listen-ip', '{host}', '--listen-port', '{port}', '{p.edit}'],
      cwd: 'D:\\stable-diffusion.cpp',
      env: { HIP_VISIBLE_DEVICES: '0' },
      port: 1234,
      model: `${MODELS_DIR}\\krea-2-realism-turbo-q8_0.gguf`,
      gpu: GPU_9070.id,
      model_name: 'Krea 2 Realism Turbo',
      quant: 'Q8_0',
      backend: 'HIP',
      recommended: 'krea-2-realism-turbo-q8',
      params: {
        size: {
          label: 'Image size',
          default: '512x768',
          choices: {
            '512x768': { vars: { W: '512', H: '768' } },
            '768x1024': { vars: { W: '768', H: '1024' } },
            '1024x1024': { vars: { W: '1024', H: '1024' } },
          },
        },
        edit: {
          label: 'Mode',
          default: 'off',
          choices: { off: { label: 'Generate', args: [] }, on: { label: 'Edit', args: ['--mode', 'edit'] } },
        },
      },
    },
    'qwen-image': {
      name: 'Qwen Image 2.1',
      adapter: 'sd.cpp',
      command: SD_SERVER,
      args: ['--diffusion-model', '{model}', '-W', '640', '-H', '928', '--steps', '20', '--listen-ip', '{host}', '--listen-port', '{port}'],
      cwd: 'D:\\stable-diffusion.cpp',
      env: { HIP_VISIBLE_DEVICES: '0' },
      port: 1235,
      model: `${MODELS_DIR}\\qwen-image-2.1-q4_0.gguf`,
      gpu: GPU_9070.id,
      model_name: 'Qwen Image 2.1',
      quant: 'Q4_0',
      backend: 'HIP',
    },
    'kokoro-tts': {
      name: 'Kokoro (running elsewhere)',
      adapter: 'openai',
      kind: 'tts',
      endpoint: 'http://127.0.0.1:8880',
      health: '/health',
      api_key: false,
      model_name: 'Kokoro 82M',
      quant: 'fp16',
      backend: 'CPU',
      notes: 'An external server: KLIF only watches it.',
    },
    'whisper-stt': {
      name: 'whisper.cpp large-v3 turbo',
      adapter: 'generic',
      kind: 'stt',
      command: WHISPER,
      args: ['-m', '{model}', '--host', '{host}', '--port', '{port}'],
      cwd: 'D:\\whisper.cpp',
      port: 7050,
      health: '/',
      model: `${MODELS_DIR}\\ggml-large-v3-turbo-q8_0.bin`,
      gpu: GPU_5700.id,
      model_name: 'Whisper large-v3 turbo',
      quant: 'Q8_0',
      backend: 'Vulkan',
    },
    'comfy-video': {
      name: 'ComfyUI video (Wan 2.2)',
      adapter: 'generic',
      kind: 'video',
      command: COMFY_PY,
      args: ['main.py', '--listen', '{host}', '--port', '{port}'],
      cwd: 'D:\\ComfyUI',
      port: 8188,
      health: '/',
      model: `${MODELS_DIR}\\wan-2.2-ti2v-5b-q8_0.gguf`,
      gpu: GPU_9070.id,
      model_name: 'Wan 2.2 TI2V 5B',
      quant: 'Q8_0',
      backend: 'HIP',
    },
  };
}

/** The presets of the remote node "render-box" (masked as the node serves them). */
export function nodePresets(): Record<string, PresetSpec> {
  return {
    'z-image': {
      name: 'Z-Image Turbo',
      adapter: 'sd.cpp',
      command: 'E:\\render\\sd-server.exe',
      args: ['--diffusion-model', '{model}', '-W', '1024', '-H', '1024', '--steps', '8', '--listen-ip', '{host}', '--listen-port', '{port}'],
      cwd: 'E:\\render',
      port: 1234,
      model: 'E:\\render\\models\\z-image-turbo-q8_0.gguf',
      gpu: GPU_NODE.id,
      model_name: 'Z-Image Turbo',
      quant: 'Q8_0',
      backend: 'HIP',
    },
    'qwen-edit': {
      name: 'Qwen Image Edit',
      adapter: 'sd.cpp',
      command: 'E:\\render\\sd-server.exe',
      args: ['--diffusion-model', '{model}', '--listen-ip', '{host}', '--listen-port', '{port}'],
      cwd: 'E:\\render',
      port: 1236,
      model: 'E:\\render\\models\\qwen-image-edit-q4_0.gguf',
      gpu: GPU_NODE.id,
      model_name: 'Qwen Image Edit',
      quant: 'Q4_0',
      backend: 'HIP',
    },
  };
}

// ----------------------------------------------------------------------------------------- resolution

const ADAPTER_PORT: Record<AdapterId, number | null> = { 'llama.cpp': 7030, 'sd.cpp': 1234, vllm: 8000, openai: 8080, generic: null };
const ADAPTER_KIND: Record<AdapterId, SystemKind | null> = { 'llama.cpp': 'llm', 'sd.cpp': 'image', vllm: 'llm', openai: 'llm', generic: null };
const SECRET_FLAGS = new Set(['--api-key', '--hf-token', '-hft', '--token', '--password']);
const SECRET_ENV = /(KEY|TOKEN|SECRET|PASSWORD)/i;

export function presetKind(spec: PresetSpec): SystemKind | null {
  return spec.kind ?? ADAPTER_KIND[spec.adapter ?? 'llama.cpp'];
}

export function adapterOf(spec: PresetSpec): AdapterId {
  return spec.adapter ?? 'llama.cpp';
}

export function isExternal(spec: PresetSpec): boolean {
  return !!spec.endpoint;
}

/** A model file exists on the mock machine when it is a known one, or sits in a models folder (and is not flagged missing). */
export function modelExists(path: string): boolean {
  if (MISSING_FILES.has(path)) return false;
  return !!FACTS[path] || /^[A-Z]:[\\/](models|render[\\/]models)[\\/]/i.test(path);
}

export function factsOf(spec: PresetSpec): ModelFacts | undefined {
  if (!spec.model) return undefined;
  const known = FACTS[spec.model];
  if (known) return known;
  if (!modelExists(spec.model)) return undefined;
  // A file the mock has no facts for: a small generic model named after the file.
  const base = spec.model.slice(Math.max(spec.model.lastIndexOf('\\'), spec.model.lastIndexOf('/')) + 1).replace(/\.[^.]+$/, '');
  return { name: base, quant: '', weightsGiB: 4, kvGiBPerTokQ8: 1 / 16384, buffersGiB: 0.5, family: 'generic' };
}

/** CommandLineToArgvW quoting, so the displayed line pastes into a shell. */
export function quoteArg(a: string): string {
  if (a !== '' && !/[\s"]/.test(a)) return a;
  let out = '"';
  let backslashes = 0;
  for (const ch of a) {
    if (ch === '\\') {
      backslashes++;
    } else if (ch === '"') {
      out += '\\'.repeat(backslashes * 2 + 1) + '"';
      backslashes = 0;
    } else {
      out += '\\'.repeat(backslashes) + ch;
      backslashes = 0;
    }
  }
  return out + '\\'.repeat(backslashes * 2) + '"';
}

export function renderLine(program: string, args: string[]): string {
  return [program, ...args].map(quoteArg).join(' ');
}

/** FNV-1a, 8 hex digits: a stable stand-in for the core's command hash. */
export function hashText(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, '0');
}

export interface CmdCtx {
  /** The System's param selection (choice values by param name). */
  params?: Record<string, string>;
  /** [security] api_key is set (managed key row). */
  apiKeySet: boolean;
  modelsDir?: string;
  verboseLlama?: boolean;
}

/** The literal value after one of the flags (a `{placeholder}` is not a literal). */
function literalAfter(args: string[], flags: string[]): string | undefined {
  for (let i = 0; i < args.length - 1; i++) if (flags.includes(args[i]) && !args[i + 1].includes('{')) return args[i + 1];
  return undefined;
}

/** Selected value of a param (the System's choice, else the preset default, else the first choice). */
function chosen(spec: PresetSpec, name: string, sel?: Record<string, string>): string | undefined {
  const p = spec.params?.[name];
  if (!p) return undefined;
  const v = sel?.[name];
  if (v !== undefined && p.choices[v]) return v;
  if (p.default !== undefined && p.choices[p.default]) return p.default;
  return Object.keys(p.choices)[0];
}

/** The ParamView list of a preset with a System's selection. */
export function paramViews(spec: PresetSpec, sel?: Record<string, string>): ParamView[] {
  return Object.entries(spec.params ?? {}).map(([name, p]) => ({
    name,
    label: p.label ?? name,
    value: chosen(spec, name, sel) ?? '',
    choices: Object.entries(p.choices).map(([value, c]) => ({ value, label: c.label ?? value })),
  }));
}

/** Expand placeholders in one string; unknown / missing ones are reported as issues. */
function expand(text: string, spec: PresetSpec, ctx: CmdCtx, vals: Record<string, string>, issues: Issue[], field: string): string {
  return text.replace(/\{([A-Za-z_][\w.]*|env:[A-Za-z_]\w*)\}/g, (m, key: string) => {
    if (key.startsWith('env:')) return `%${key.slice(4)}%`;
    if (key.startsWith('p.')) {
      const [, name, v] = key.split('.');
      const choice = spec.params?.[name]?.choices[chosen(spec, name, ctx.params) ?? ''];
      if (!choice) {
        issues.push({ level: 'error', field, text: `Unknown param ${name} in {${key}}.` });
        return '';
      }
      const val = v ? choice.vars?.[v] : undefined;
      if (val === undefined) issues.push({ level: 'error', field, text: `Param ${name} has no value ${v ?? ''}.` });
      return val ?? '';
    }
    const val = vals[key];
    if (val === undefined || val === '') {
      issues.push({ level: 'error', field, text: vals[key] === undefined ? `Unknown placeholder ${m}.` : `Missing value for ${m}.` });
      return '';
    }
    return val;
  });
}

/** Resolve a preset into the command it would run, secrets masked (what the core's CommandView carries). */
export function buildCommand(spec: PresetSpec, ctx: CmdCtx): CommandView {
  const adapter = adapterOf(spec);
  const issues: Issue[] = [];
  const ext = spec.endpoint;
  const vals: Record<string, string> = {
    model: spec.model ?? '',
    mmproj: spec.mmproj ?? '',
    ctx: spec.ctx !== undefined ? String(spec.ctx) : '',
    host: spec.host ?? literalAfter(spec.args ?? [], ['--host', '--listen-ip']) ?? '127.0.0.1',
    models_dir: ctx.modelsDir ?? '',
    state_dir: '%APPDATA%\\KLIF',
    data_dir: '%LOCALAPPDATA%\\KLIF',
    // The session stamp (yyyyMMdd-HHmmss-fff): previews and the hash keep it literal, as the core does.
    stamp: '{stamp}',
  };
  const literalPort = literalAfter(spec.args ?? [], ['--port', '--listen-port']);
  const portNum = spec.port ?? (literalPort && /^\d+$/.test(literalPort) ? Number(literalPort) : undefined) ?? ADAPTER_PORT[adapter] ?? 0;
  vals.port = String(portNum);

  let program = '';
  let args: string[] = [];
  let cwd = '';
  const env: EnvView[] = [];

  if (ext) {
    try {
      const u = new URL(ext);
      vals.host = u.hostname;
      vals.port = u.port || '80';
      if (u.protocol === 'https:') {
        issues.push({
          level: 'error',
          field: 'endpoint',
          text: `endpoint "${ext}": https:// endpoints are not supported yet; KLIF probes external servers over plain http://. Use http://host:port.`,
        });
      }
    } catch {
      issues.push({ level: 'error', field: 'endpoint', text: 'The endpoint is not a valid URL.' });
    }
    if (spec.command) issues.push({ level: 'error', field: 'command', text: 'An external server has no command: clear it or clear the endpoint.' });
  } else {
    if (!spec.command) issues.push({ level: 'error', field: 'command', text: 'No program: set a command (or an endpoint for an external server).' });
    program = spec.command ?? '';
    if (/\.(bat|cmd)$/i.test(program)) {
      issues.push({ level: 'error', field: 'command', text: 'A .bat / .cmd file cannot be launched directly: run cmd.exe with /c and the file as arguments.' });
    }
    cwd = spec.cwd ?? (program.includes('\\') ? program.slice(0, program.lastIndexOf('\\')) : '');
    for (const raw of spec.args ?? []) {
      const whole = /^\{p\.([A-Za-z_]\w*)\}$/.exec(raw);
      if (whole) {
        const p = spec.params?.[whole[1]];
        const choice = p?.choices[chosen(spec, whole[1], ctx.params) ?? ''];
        if (!choice) {
          issues.push({ level: 'error', field: 'args', text: `Unknown param ${whole[1]} in {p.${whole[1]}}.` });
          continue;
        }
        for (const a of choice.args ?? []) args.push(expand(a, spec, ctx, vals, issues, 'args'));
        continue;
      }
      args.push(expand(raw, spec, ctx, vals, issues, 'args'));
    }
    cwd = expand(cwd, spec, ctx, vals, issues, 'cwd');
    // Secret flags: mask the value after them.
    args = args.map((a, i) => (i > 0 && SECRET_FLAGS.has(args[i - 1]) ? MOCK_KEY_MASK : a));
    // Env: managed rows first (adapter telemetry), then the preset's own (with choice env merged).
    const own: Record<string, string> = { ...(spec.env ?? {}) };
    for (const name of Object.keys(spec.params ?? {})) Object.assign(own, spec.params?.[name].choices[chosen(spec, name, ctx.params) ?? '']?.env ?? {});
    const managed: [string, string | null][] = [];
    if (spec.managed !== false && adapter === 'llama.cpp') {
      if (ctx.verboseLlama !== false) managed.push(['LLAMA_ARG_LOG_VERBOSITY', '4']);
      managed.push(['LLAMA_ARG_LOG_PREFIX', '1'], ['LLAMA_ARG_LOG_TIMESTAMPS', '1']);
    }
    if (spec.api_key !== false && (adapter === 'llama.cpp' || adapter === 'vllm') && ctx.apiKeySet) {
      managed.push([adapter === 'vllm' ? 'VLLM_API_KEY' : 'LLAMA_API_KEY', null]);
    }
    for (const [name, value] of managed) {
      const overridden = name in own;
      env.push({ name, value: value ?? undefined, secret: value === null, managed: true, removed: false, overridden });
    }
    for (const [name, value] of Object.entries(own)) {
      const secret = SECRET_ENV.test(name);
      const shown = expand(value, spec, ctx, vals, issues, `env.${name}`);
      env.push({ name, ...(secret ? {} : { value: shown }), secret, managed: false, removed: false, overridden: false });
    }
    for (const name of spec.env_remove ?? []) env.push({ name, secret: false, managed: false, removed: true, overridden: false });

    if (spec.model && !modelExists(spec.model)) {
      issues.push({ level: 'error', field: 'model', text: `Model file not found: ${spec.model}` });
    }
    if (spec.mmproj && MISSING_FILES.has(spec.mmproj)) issues.push({ level: 'error', field: 'mmproj', text: `File not found: ${spec.mmproj}` });
  }
  if (!ext && spec.port !== undefined && literalPort && /^\d+$/.test(literalPort) && Number(literalPort) !== spec.port) {
    issues.push({ level: 'error', field: 'port', text: `port = ${spec.port} but the arguments say ${literalPort}.` });
  }
  if (adapter === 'generic' && !spec.kind) issues.push({ level: 'error', field: 'kind', text: 'A generic preset needs a kind.' });
  if (adapter === 'generic' && !ext && !spec.port && !literalPort && !spec.health) {
    issues.push({ level: 'error', field: 'port', text: 'A generic preset needs a port (or a health check).' });
  }
  const host = vals.host;
  if (!ext && host !== '127.0.0.1' && host !== 'localhost' && !host.startsWith('[::1]') && !ctx.apiKeySet) {
    issues.push({ level: 'error', field: 'host', text: `The server listens on ${host} but no API key is set.` });
  }
  if (adapter === 'llama.cpp' && spec.managed !== false && (spec.args ?? []).includes('--log-disable')) {
    issues.push({ level: 'warn', field: 'args', text: '--log-disable hides the lines KLIF reads: load steps and VRAM will be missing.' });
  }
  const health: HealthCheck = !spec.health ? { type: 'auto' } : spec.health === 'tcp' ? { type: 'tcp' } : { type: 'http', path: spec.health };
  const display = ext ? ext : renderLine(program, args);
  const view: CommandView = {
    program,
    args,
    cwd,
    env,
    port: Number(vals.port) || 0,
    host,
    health,
    adapter,
    display,
    issues,
    hash: hashText(`${display}|${cwd}|${env.map((e) => `${e.name}=${e.value ?? ''}`).join(';')}`),
  };
  if (ext) view.external = ext;
  return view;
}

// ------------------------------------------------------------------------------------------ derived

/** Availability of a preset from the issues of its command. */
export function availabilityOf(spec: PresetSpec, cmd: CommandView): { availability: Availability; reason?: string } {
  const errs = cmd.issues.filter((i) => i.level === 'error');
  if (!errs.length) return { availability: 'ready' };
  const first = errs[0];
  if (first.field === 'model' || first.field === 'mmproj') return { availability: 'model-missing', reason: first.text };
  if (first.field === 'command') return { availability: 'exe-missing', reason: first.text };
  return { availability: 'invalid', reason: first.text };
}

function kvTypeOf(args: string[]): string | undefined {
  return literalAfter(args, ['-ctk', '--cache-type-k']);
}

/** The ModelRef of a preset with the System's param selection. */
export function modelRefOf(spec: PresetSpec, cmd: CommandView, sel: Record<string, string> | undefined, gpuName: string): ModelRef {
  const facts = factsOf(spec);
  const adapter = adapterOf(spec);
  const kind = presetKind(spec) ?? 'llm';
  const m: ModelRef = {
    name: spec.model_name ?? facts?.name ?? spec.name ?? 'Unnamed model',
    quant: spec.quant ?? facts?.quant ?? '',
    engine: adapter,
    backend: spec.backend ?? '',
    device: spec.device ?? gpuName,
  };
  if (facts?.weightsGiB) m.weightsGiB = facts.weightsGiB;
  if (facts?.arch) m.arch = facts.arch;
  if (kind === 'llm') {
    if (spec.ctx) m.ctxTokens = spec.ctx;
    const kv = kvTypeOf(cmd.args);
    if (kv) m.kvType = kv;
    if (facts?.specMode) m.specMode = facts.specMode;
    if (spec.mmproj && cmd.args.includes('--mmproj')) m.vision = true;
    else if (spec.mmproj) m.vision = false;
    const mode = chosen(spec, 'mode', sel);
    if (mode) m.mode = mode;
  } else if (kind === 'image') {
    const size = chosen(spec, 'size', sel);
    if (size) m.imageSize = size;
    else {
      const w = literalAfter(cmd.args, ['-W', '--width']);
      const h = literalAfter(cmd.args, ['-H', '--height']);
      if (w && h) m.imageSize = `${w}x${h}`;
    }
    if (chosen(spec, 'edit', sel) === 'on') m.mode = 'Edit';
  }
  return m;
}

const r2 = (v: number) => Math.round(v * 100) / 100;
const KV_FACTOR: Record<string, number> = { q4_0: 0.53, q8_0: 1, f16: 1.88 };

/** Expected VRAM layers for a preset: the "will it fit" preview (measured/file-size style numbers). */
export function expectedLayers(spec: PresetSpec, model: ModelRef): VramLayer[] {
  const facts = factsOf(spec);
  const kind = presetKind(spec) ?? 'llm';
  const weights = facts?.weightsGiB ?? model.weightsGiB ?? 0;
  if (kind === 'llm') {
    const ctx = model.ctxTokens ?? 16384;
    const kv = (facts?.kvGiBPerTokQ8 ?? 2.9 / 98304) * ctx * (KV_FACTOR[model.kvType ?? 'q8_0'] ?? 1);
    const layers: VramLayer[] = [
      { id: 'weights', label: 'weights', gib: r2(weights) },
      { id: 'kv', label: 'KV cache', gib: r2(kv) },
      { id: 'buffers', label: 'buffers', gib: r2(facts?.buffersGiB ?? 0.9) },
    ];
    if (model.specMode && /MTP/i.test(model.specMode)) layers.push({ id: 'draft', label: 'draft', gib: 0.12 });
    return layers;
  }
  if (kind === 'image') {
    const [w, h] = (model.imageSize ?? '512x768').split('x').map(Number);
    const pixels = ((w || 512) * (h || 768)) / (512 * 768);
    return [
      { id: 'weights', label: 'DiT layers', gib: r2(weights * (facts?.gpuWeightShare ?? 0.8)) },
      { id: 'buffers', label: 'activations', gib: r2((facts?.activationsGiB ?? 3) * pixels) },
      { id: 'other', label: 'VAE', gib: r2(facts?.vaeGiB ?? 0.6) },
    ];
  }
  return [
    { id: 'weights', label: 'weights', gib: r2(weights) },
    { id: 'buffers', label: 'buffers', gib: r2(facts?.buffersGiB ?? 0.3) },
  ];
}

export function totalGiB(layers: VramLayer[]): number {
  return r2(layers.reduce((a, l) => a + l.gib, 0));
}

/** "gpu = a,b" comma list -> ids (first is the fit display). */
export function gpuList(spec: PresetSpec, fallback: string): string[] {
  const v = (spec.gpu ?? fallback).split(',').map((x) => x.trim()).filter(Boolean);
  return v.length ? v : [fallback];
}

export function presetInfo(
  id: string,
  spec: PresetSpec,
  cmd: CommandView,
  gpuName: string,
  benches: Record<string, PresetInfo['bench']>,
  node?: string,
): PresetInfo {
  const a = availabilityOf(spec, cmd);
  const model = modelRefOf(spec, cmd, undefined, gpuName);
  const info: PresetInfo = {
    id,
    name: spec.name ?? id,
    adapter: adapterOf(spec),
    kind: presetKind(spec) ?? 'llm',
    model,
    availability: a.availability,
    external: isExternal(spec),
  };
  if (a.reason) info.reason = a.reason;
  if (spec.recommended) info.recommended = spec.recommended;
  if (benches[id]) info.bench = benches[id];
  if (spec.gpu) info.gpu = spec.gpu;
  if (node) info.node = node;
  info.specHash = specHash(maskSpec(spec), undefined);
  return info;
}

/** Secret env values never leave the core: a preset served to the UI has them replaced by the mask. */
export function maskSpec(spec: PresetSpec): PresetSpec {
  const out: PresetSpec = JSON.parse(JSON.stringify(spec));
  if (out.env) for (const k of Object.keys(out.env)) if (SECRET_ENV.test(k)) out.env[k] = MOCK_KEY_MASK;
  if (out.args) out.args = out.args.map((a, i, all) => (i > 0 && SECRET_FLAGS.has(all[i - 1]) ? MOCK_KEY_MASK : a));
  return out;
}

export function specHash(spec: PresetSpec, params: Record<string, string> | undefined): string {
  return hashText(JSON.stringify([spec, params ?? {}]));
}

// ----------------------------------------------------------------------------------- recommendations

interface RecDef {
  id: string;
  kind: SystemKind;
  class?: 'fast' | 'deep' | 'max';
  name: string;
  adapter: AdapterId;
  hfRepo: string;
  quant: string;
  license: string;
  hardwareClass: string;
  minVramGiB?: number;
  files: { name: string; role: 'model' | 'mmproj' | 'other'; gib: number }[];
  measured?: RecommendationInfo['measured'];
  notes?: string;
  /** The local model path the downloaded file lands at (matches FACTS when the model is already installed). */
  installedAs?: string;
}

const REV = (n: number) => (n.toString(16).repeat(8) + 'c0ffee1234567890abcdef1234567890abcdef12').slice(0, 40);

const MEASURED = (decodeTps: number): RecommendationInfo['measured'] => ({
  decodeTps,
  hardware: 'RX 9070 XT, 16 GB',
  backend: 'HIP',
  date: '2026-09',
});

export const REC_DEFS: RecDef[] = [
  {
    id: 'gemma-4-26b-a4b-q4_0',
    kind: 'llm',
    class: 'fast',
    name: 'Gemma 4 26B-A4B',
    adapter: 'llama.cpp',
    hfRepo: 'example-org/gemma-4-26b-a4b-GGUF',
    quant: 'Q4_0',
    license: 'Gemma terms',
    hardwareClass: '16 GB',
    minVramGiB: 15,
    files: [{ name: 'gemma-4-26b-a4b-q4_0.gguf', role: 'model', gib: 13.4 }],
    measured: MEASURED(84),
    installedAs: `${MODELS_DIR}\\gemma-4-26b-a4b-q4_0.gguf`,
  },
  {
    id: 'gemma-4-12b-qat-q4_0',
    kind: 'llm',
    class: 'fast',
    name: 'Gemma 4 12B',
    adapter: 'llama.cpp',
    hfRepo: 'example-org/gemma-4-12b-qat-GGUF',
    quant: 'QAT Q4_0',
    license: 'Gemma terms',
    hardwareClass: '8 GB',
    minVramGiB: 8,
    files: [{ name: 'gemma-4-12b-qat-q4_0.gguf', role: 'model', gib: 6.5 }],
  },
  {
    id: 'qwen-3.8-27b-gsq-iq3',
    kind: 'llm',
    class: 'deep',
    name: 'Qwen 3.8 27B',
    adapter: 'llama.cpp',
    hfRepo: 'example-org/qwen-3.8-27b-GGUF',
    quant: 'GSQ-RCO IQ3_S',
    license: 'Apache-2.0',
    hardwareClass: '16 GB',
    minVramGiB: 15.5,
    files: [
      { name: 'qwen-3.8-27b-gsq-iq3_s.gguf', role: 'model', gib: 11.6 },
      { name: 'qwen-3.8-27b-mmproj-f16.gguf', role: 'mmproj', gib: 0.9 },
    ],
    measured: MEASURED(47),
    installedAs: `${MODELS_DIR}\\qwen-3.8-27b-gsq-iq3_s.gguf`,
  },
  {
    id: 'qwen-3.8-14b-q4',
    kind: 'llm',
    class: 'deep',
    name: 'Qwen 3.8 14B',
    adapter: 'llama.cpp',
    hfRepo: 'example-org/qwen-3.8-14b-GGUF',
    quant: 'Q4_K_M',
    license: 'Apache-2.0',
    hardwareClass: '12 GB',
    minVramGiB: 10.5,
    files: [{ name: 'qwen-3.8-14b-q4_k_m.gguf', role: 'model', gib: 8.9 }],
  },
  {
    id: 'qwen-3.8-flash-next-iq2',
    kind: 'llm',
    class: 'max',
    name: 'Qwen 3.8 Flash-Next',
    adapter: 'llama.cpp',
    hfRepo: 'example-org/qwen-3.8-flash-next-GGUF',
    quant: 'IQ2_XXS',
    license: 'Apache-2.0',
    hardwareClass: '16 GB',
    minVramGiB: 15.8,
    files: [{ name: 'qwen-3.8-flash-next-iq2_xxs.gguf', role: 'model', gib: 13.2 }],
    measured: MEASURED(12),
    notes: 'Fills the card at 128k context.',
    installedAs: `${MODELS_DIR}\\qwen-3.8-flash-next-iq2_xxs.gguf`,
  },
  {
    id: 'krea-2-realism-turbo-q8',
    kind: 'image',
    name: 'Krea 2 Realism Turbo',
    adapter: 'sd.cpp',
    hfRepo: 'example-org/krea-2-realism-turbo-GGUF',
    quant: 'Q8_0',
    license: 'Non-commercial',
    hardwareClass: '12 GB+',
    minVramGiB: 10,
    files: [{ name: 'krea-2-realism-turbo-q8_0.gguf', role: 'model', gib: 7.4 }],
    measured: { secondsPerImage: 13, hardware: 'RX 9070 XT, 16 GB', backend: 'HIP', date: '2026-09' },
    installedAs: `${MODELS_DIR}\\krea-2-realism-turbo-q8_0.gguf`,
  },
  {
    id: 'qwen-image-2.1-q4',
    kind: 'image',
    name: 'Qwen Image 2.1',
    adapter: 'sd.cpp',
    hfRepo: 'example-org/qwen-image-2.1-GGUF',
    quant: 'Q4_0',
    license: 'Apache-2.0',
    hardwareClass: '16 GB',
    minVramGiB: 14,
    files: [{ name: 'qwen-image-2.1-q4_0.gguf', role: 'model', gib: 11.9 }],
    installedAs: `${MODELS_DIR}\\qwen-image-2.1-q4_0.gguf`,
  },
  {
    id: 'kokoro-82m',
    kind: 'tts',
    name: 'Kokoro 82M',
    adapter: 'openai',
    hfRepo: 'example-org/kokoro-82m-onnx',
    quant: 'fp16',
    license: 'Apache-2.0',
    hardwareClass: 'CPU',
    files: [{ name: 'kokoro-82m.onnx', role: 'model', gib: 0.2 }],
    notes: 'Needs an OpenAI-compatible speech server in front of it.',
  },
  {
    id: 'piper-voices-en',
    kind: 'tts',
    name: 'Piper English voices',
    adapter: 'generic',
    hfRepo: 'example-org/piper-voices-en',
    quant: 'medium',
    license: 'MIT',
    hardwareClass: 'CPU',
    files: [{ name: 'en_US-lessac-medium.onnx', role: 'model', gib: 0.06 }],
  },
  {
    id: 'whisper-large-v3-turbo-q8',
    kind: 'stt',
    name: 'Whisper large-v3 turbo',
    adapter: 'generic',
    hfRepo: 'example-org/whisper-large-v3-turbo-GGML',
    quant: 'Q8_0',
    license: 'MIT',
    hardwareClass: '4 GB+',
    minVramGiB: 2,
    files: [{ name: 'ggml-large-v3-turbo-q8_0.bin', role: 'model', gib: 0.87 }],
    installedAs: `${MODELS_DIR}\\ggml-large-v3-turbo-q8_0.bin`,
  },
  {
    id: 'whisper-small-en',
    kind: 'stt',
    name: 'Whisper small (English)',
    adapter: 'generic',
    hfRepo: 'example-org/whisper-small-en-GGML',
    quant: 'Q5_1',
    license: 'MIT',
    hardwareClass: 'CPU',
    files: [{ name: 'ggml-small.en-q5_1.bin', role: 'model', gib: 0.18 }],
  },
  {
    id: 'ltx-video-2b-q8',
    kind: 'video',
    name: 'LTX Video 2B',
    adapter: 'generic',
    hfRepo: 'example-org/ltx-video-2b-GGUF',
    quant: 'Q8_0',
    license: 'Open weights',
    hardwareClass: '12 GB',
    minVramGiB: 8,
    files: [{ name: 'ltx-video-2b-q8_0.gguf', role: 'model', gib: 2.3 }],
  },
  {
    id: 'wan-2.2-ti2v-5b-q8',
    kind: 'video',
    name: 'Wan 2.2 TI2V 5B',
    adapter: 'generic',
    hfRepo: 'example-org/wan-2.2-ti2v-5b-GGUF',
    quant: 'Q8_0',
    license: 'Apache-2.0',
    hardwareClass: '16 GB',
    minVramGiB: 12,
    files: [{ name: 'wan-2.2-ti2v-5b-q8_0.gguf', role: 'model', gib: 5.4 }],
    installedAs: `${MODELS_DIR}\\wan-2.2-ti2v-5b-q8_0.gguf`,
  },
];

export const GIB = 1024 ** 3;

/** Recommendations as listed. `installed` = a download finished or the file is already referenced by a preset. */
export function recommendationInfos(presets: Record<string, PresetSpec>, downloaded: Set<string>): RecommendationInfo[] {
  return REC_DEFS.map((d) => {
    const existing = Object.entries(presets).find(([, p]) => d.installedAs && p.model === d.installedAs && !MISSING_FILES.has(p.model))?.[0];
    const info: RecommendationInfo = {
      id: d.id,
      kind: d.kind,
      name: d.name,
      adapter: d.adapter,
      hfRepo: d.hfRepo,
      revision: REV(d.id.length),
      files: d.files.map((f) => ({ name: f.name, role: f.role, sizeBytes: Math.round(f.gib * GIB) })),
      quant: d.quant,
      license: d.license,
      hardwareClass: d.hardwareClass,
      installed: !!existing || downloaded.has(d.id),
    };
    if (d.class) info.class = d.class;
    if (d.minVramGiB !== undefined) info.minVramGiB = d.minVramGiB;
    if (d.measured) info.measured = d.measured;
    if (d.notes) info.notes = d.notes;
    if (existing) info.existing = existing;
    return info;
  });
}

/** A preset made from a recommendation (what Adopt writes): command / cwd / env come from the System's current preset. */
export function presetFromRecommendation(rec: RecDef, base: PresetSpec | undefined): { id: string; spec: PresetSpec } {
  const id = rec.id.replace(/[^a-z0-9-]/g, '-').slice(0, 40);
  const modelFile = rec.files.find((f) => f.role === 'model')!;
  const mm = rec.files.find((f) => f.role === 'mmproj');
  const model = `${MODELS_DIR}\\${modelFile.name}`;
  const args =
    rec.adapter === 'llama.cpp' || rec.adapter === 'vllm' || rec.adapter === 'openai'
      ? ['-m', '{model}', '-c', '{ctx}', '--host', '{host}', '--port', '{port}', '-ngl', '99']
      : rec.adapter === 'sd.cpp'
        ? ['--diffusion-model', '{model}', '--listen-ip', '{host}', '--listen-port', '{port}']
        : ['-m', '{model}', '--host', '{host}', '--port', '{port}'];
  const spec: PresetSpec = {
    name: rec.name,
    adapter: rec.adapter,
    kind: rec.kind,
    command: base?.command ?? '',
    args,
    model,
    model_name: rec.name,
    quant: rec.quant,
    recommended: rec.id,
  };
  if (base?.cwd) spec.cwd = base.cwd;
  if (base?.env) spec.env = { ...base.env };
  if (base?.backend) spec.backend = base.backend;
  if (mm) spec.mmproj = `${MODELS_DIR}\\${mm.name}`;
  if (base?.port) spec.port = base.port;
  if (rec.kind === 'llm') spec.ctx = base?.ctx ?? 16384;
  if (base?.gpu) spec.gpu = base.gpu;
  // The downloaded file is real now: the mock knows its size.
  if (!FACTS[model]) {
    FACTS[model] = {
      name: rec.name,
      quant: rec.quant,
      weightsGiB: modelFile.gib,
      kvGiBPerTokQ8: rec.kind === 'llm' ? 0.6 / 16384 : undefined,
      buffersGiB: rec.kind === 'llm' ? 0.7 : 0.3,
      family: 'generic',
    };
  }
  return { id, spec };
}

export function recDef(id: string): RecDef | undefined {
  return REC_DEFS.find((d) => d.id === id);
}

export type { RecDef };

/** Context windows offered in text. */
export const ctxText = (tokens: number) => `${fmtCtx(tokens)} tokens`;
