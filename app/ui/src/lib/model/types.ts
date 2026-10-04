// KLIF view model: the single contract between the core (or the mock) and every skin.
// Mirrored 1:1 in crates/klif-common/src/vm.rs (serde camelCase); change both together.
//
// Rules:
// - Skins only READ this. They never invent a number that is not here.
// - Every field is a measurement or a configured fact; nothing is decorative.
// - Units are in field names (GiB, MiB, S = seconds, Tps = tokens per second, Pct = 0..100).
// - The view model is replaced wholesale at the telemetry rate (1-2 Hz). Skins interpolate between
//   snapshots themselves (see lib/render), never by polling.
//
// 0.3: KLIF manages N user-defined Systems (klif.toml [systems.<id>], tabs in file order) running concurrently,
// each with its own session, plus Systems on remote KLIF nodes ("<node>/<id>") and external servers. A System runs
// a PRESET ([presets.<id>]) whose command is shown exactly (CommandView). Skins switch on `kind`, never on ids.

/** A System id: a local id from `[systems.<id>]` ("s1", "cgi", "tts"...) or "<node>/<id>" for a remote System. */
export type SystemId = string;

export type SystemKind = 'llm' | 'image' | 'tts' | 'stt' | 'video' | 'music';

/** LLM System class: fast = System 1, deep = System 2, max = System 3. */
export type LlmClass = 'fast' | 'deep' | 'max';

/**
 * A System's tab status.
 * not-set: no preset · invalid: preset/kind errors, exe/model missing, port held by a foreign process (reason says
 * which) · offline: ready, not running (external: not answering) · starting: boot/loading · online: live idle ·
 * busy: live and working · stopping · fault · unreachable: its remote node is down/unauthorized/incompatible.
 */
export type SystemStatus =
  | 'not-set'
  | 'invalid'
  | 'offline'
  | 'starting'
  | 'online'
  | 'busy'
  | 'stopping'
  | 'fault'
  | 'unreachable';

/** Launch availability of a preset (or of a System's active preset). 'busy' = its port is held by a foreign process. */
export type Availability = 'ready' | 'unsupported' | 'invalid' | 'exe-missing' | 'model-missing' | 'busy';

/** The server family a preset launches (telemetry, defaults, managed env). */
export type AdapterId = 'llama.cpp' | 'sd.cpp' | 'vllm' | 'openai' | 'audiocpp' | 'generic';

/** How KLIF decides the server is ready. 'auto' = the adapter's default chain. */
export type HealthCheck = { type: 'auto' } | { type: 'http'; path: string } | { type: 'tcp' };

/** What runs behind a System. All strings are display-ready. */
export interface ModelRef {
  /** "Qwen 3.8 27B" */
  name: string;
  /** "IQ3_S" ("" unknown) */
  quant: string;
  /** Adapter label, e.g. "llama.cpp", "sd.cpp", "vllm", "openai", "audiocpp", "generic". */
  engine: string;
  /** Display text: "HIP", "Vulkan", "CUDA", "Metal", "CPU" or free text; "" when unknown. Compare case-insensitively. */
  backend: string;
  /** "RX 9070 XT" */
  device: string;
  /** Context window in tokens (LLM only). */
  ctxTokens?: number;
  /** KV cache type, e.g. "q8_0" (LLM only). */
  kvType?: string;
  /** Speculative decoding mode, e.g. "MTP+ngram" (LLM only). */
  specMode?: string;
  /** Vision projector loaded (LLM only). */
  vision?: boolean;
  /** Reasoning mode, e.g. "Thinking" / "Instruct" (LLM); "Edit" (image). */
  mode?: string;
  /** Default output size for image models, e.g. "512x768". */
  imageSize?: string;
  /** Weights size on disk in GiB, used to preview fit before launch. */
  weightsGiB?: number;
  /** Transformer shape as the server reported it when it loaded (llama.cpp print_info); kept per model after that. */
  arch?: ModelArch;
}

/** The model's shape from the server log: what the Loom skin draws. */
export interface ModelArch {
  /** Transformer blocks (n_layer). */
  layers: number;
  /** Routed experts per layer (n_expert); 0 for a dense model. */
  experts: number;
  /** Experts each token is routed to (n_expert_used). */
  expertsUsed: number;
  /** Always-on shared experts (1 when the model has a shared expert FFN). */
  sharedExperts: number;
  /** Attention heads and KV heads (n_head, n_head_kv). */
  heads?: number;
  kvHeads?: number;
  /** Hidden size (n_embd). */
  embd?: number;
  /** Vocabulary size (n_vocab). */
  vocab?: number;
  /** "176.94 B" (model params). */
  params?: string;
}

// ------------------------------------------------------------------------------------------ commands

/** One validation finding. Errors block Launch (status invalid); warnings never do. */
export interface Issue {
  level: 'error' | 'warn';
  /** The field it is about: "command", "args", "env.HIP_PATH", "port", "presets.my-id", "systems.s1", "file"... */
  field?: string;
  /** One plain sentence. */
  text: string;
}

/** One environment row of a command as shown (never the value of a secret). */
export interface EnvView {
  name: string;
  /** Absent for a secret (shown as ••••). */
  value?: string;
  secret: boolean;
  /** Set by KLIF (telemetry / API key), not by the preset. */
  managed: boolean;
  /** An inherited variable the preset removes. */
  removed: boolean;
  /** A managed variable the preset's own env overrides. */
  overridden: boolean;
}

/** Exactly what a preset launches for a System, secrets masked. */
export interface CommandView {
  /** Resolved program path (or the command as written when it cannot be resolved). "" for an external preset. */
  program: string;
  /** Final argument tokens; values after secret flags are "••••". */
  args: string[];
  cwd: string;
  env: EnvView[];
  port: number;
  host: string;
  health: HealthCheck;
  adapter: AdapterId;
  /** The exact command line (secrets masked, {env:X} shown as %X%). */
  display: string;
  issues: Issue[];
  /** Hash of what runs (program/args/env/cwd). Compare Session.command.hash with System.command.hash. */
  hash: string;
  /** The endpoint URL of an external preset (KLIF never starts or stops it). */
  external?: string;
}

export interface ParamOption {
  value: string;
  label: string;
}

/** A preset param with the System's current selection (segmented control / select in Tune). */
export interface ParamView {
  name: string;
  label: string;
  value: string;
  choices: ParamOption[];
}

// ------------------------------------------------------------------------------------------- presets

/** The latest benchmark of a preset. */
export interface BenchSummary {
  /** Seconds since epoch of the run. */
  at: number;
  runs: number;
  loadS?: number;
  ttftS?: number;
  prefillTps?: number;
  decodeTps?: number;
  secondsPerImage?: number;
  /** TTS: audio seconds per wall second. */
  ttsRtf?: number;
  /** STT: audio seconds per wall second. */
  sttRtf?: number;
  /** Music: seconds of music per wall second. */
  musicRtf?: number;
  peakVramGiB?: number;
  spillMiB?: number;
  backendBuild?: string;
  klifVersion: string;
  /** Recorded for a different preset hash (the command changed since). */
  stale: boolean;
}

/** A preset as listed. */
export interface PresetInfo {
  id: string;
  name: string;
  adapter: AdapterId;
  kind: SystemKind;
  model: ModelRef;
  availability: Availability;
  reason?: string;
  /** The recommendation it was made from. */
  recommended?: string;
  bench?: BenchSummary;
  /** The GPU it runs on: "VEN:DEV" or "cpu". */
  gpu?: string;
  /** An external server (endpoint): watched, never started or stopped. */
  external: boolean;
  /** The node it lives on (absent = this machine). vm.presets are local; remote ones are vm.nodes[n].presets. */
  node?: string;
  /** = PresetDetail.specHash of the stored spec: a clean Tune draft reloads when it changes. */
  specHash?: string;
}

/** `choices.<value>` of a param (klif.toml snake_case). */
export interface ParamChoiceSpec {
  label?: string;
  vars?: Record<string, string>;
  args?: string[];
  env?: Record<string, string>;
}

/** `[presets.<id>.params.<NAME>]`. */
export interface ParamSpec {
  label?: string;
  default?: string;
  choices: Record<string, ParamChoiceSpec>;
}

/**
 * `[presets.<id>]` exactly as in klif.toml (snake_case keys, mirrors Rust PresetCfg). Absent fields take their
 * defaults: adapter 'llama.cpp', managed TRUE, api_key TRUE. Secret env values arrive as "••••" and may be sent back
 * as "••••" (= keep the stored value; resolved against secretsFrom / the id).
 */
export interface PresetSpec {
  name?: string;
  adapter?: AdapterId;
  /** Default: image for sd.cpp (video: set it), llm for llama.cpp/vllm/openai, tts for audiocpp (music: set it); REQUIRED for generic. */
  kind?: SystemKind;
  /** Absolute path, or a name on PATH (.exe/.com only). Empty for an external preset. */
  command?: string;
  args?: string[];
  cwd?: string;
  env?: Record<string, string>;
  env_remove?: string[];
  port?: number;
  host?: string;
  /** External server URL (command must be empty). */
  endpoint?: string;
  /** "/path" = HTTP, "tcp" = TCP listen; absent = the adapter's default chain. */
  health?: string;
  model?: string;
  mmproj?: string;
  ctx?: number;
  /** "VEN:DEV", "VEN:DEV#n", "cpu", or a comma list; default [gpu] inference. Display/telemetry only. */
  gpu?: string;
  /** Default true: KLIF adds the adapter's telemetry env. */
  managed?: boolean;
  /** Default true: KLIF injects its API key into the adapter's key env var. */
  api_key?: boolean;
  model_name?: string;
  quant?: string;
  backend?: string;
  device?: string;
  notes?: string;
  recommended?: string;
  params?: Record<string, ParamSpec>;
}

/** One preset in full, for the Tune editor (klif_preset_get). */
export interface PresetDetail {
  id: string;
  spec: PresetSpec;
  command: CommandView;
  info: PresetInfo;
  /** Hash of the stored spec as served (every field, masked): send it back as SavePreset's baseHash. Empty from a
   *  node that does not report it. */
  specHash: string;
}

// ------------------------------------------------------------------------- recommendations/downloads

export interface RecFile {
  /** Path of the file in the Hugging Face repo. */
  name: string;
  role: 'model' | 'mmproj' | 'other';
  sizeBytes?: number;
}

/** Measured on one machine (a starting point, not a promise). */
export interface Measured {
  decodeTps?: number;
  prefillTps?: number;
  secondsPerImage?: number;
  hardware: string;
  backend: string;
  date: string;
}

/** A model recommendation for a kind (and LLM class) of System. Shown with the disclaimer. */
export interface RecommendationInfo {
  id: string;
  kind: SystemKind;
  class?: LlmClass;
  name: string;
  adapter: AdapterId;
  hfRepo: string;
  /** Commit sha. */
  revision: string;
  files: RecFile[];
  quant: string;
  license: string;
  hardwareClass: string;
  minVramGiB?: number;
  measured?: Measured;
  notes?: string;
  /** Files present in the models dir (or already referenced by a preset). */
  installed: boolean;
  /** Preset id already using these files. */
  existing?: string;
}

export type DownloadState = 'running' | 'verifying' | 'done' | 'failed' | 'cancelled';

/** One file of a recommendation being downloaded (one entry per (id, file)). */
export interface DownloadInfo {
  /** Recommendation id. */
  id: string;
  file: string;
  doneBytes: number;
  totalBytes?: number;
  state: DownloadState;
  error?: string;
}

// -------------------------------------------------------------------------------------------- config

export interface ApiKeyInfo {
  /** "file" | "env:NAME" | "none" */
  source: string;
  set: boolean;
}

/** `[launch] on_conflict`: what Launch does when other Systems must stop first. */
export type OnConflict = 'ask' | 'stop';

export interface ConfigInfo {
  /** The klif.toml in use (absent: defaults, no file yet). */
  path?: string;
  stateDir: string;
  dataDir: string;
  issues: Issue[];
  apiKey: ApiKeyInfo;
  /** `[paths] models_dir` (absent: downloads are refused until it is set). */
  modelsDir?: string;
  onConflict: OnConflict;
  /** `[ui] record_moment`: show the "new record" moment (default on). */
  recordMoment: boolean;
}

// ------------------------------------------------------------------------------------------- systems

/** One System (a tab): its configuration, what it would run, and its live state. */
export interface System {
  id: SystemId;
  /** "System 1" */
  label: string;
  kind: SystemKind;
  class?: LlmClass;
  /** The remote node it lives on (absent = this machine). Display name via vm.nodes. */
  node?: string;
  status: SystemStatus;
  /** One plain sentence for not-set / invalid / fault / unreachable (and conflicts). */
  reason?: string;
  /** Launch availability of the active preset. */
  availability: Availability;
  model: ModelRef;
  /** The active preset id. */
  preset?: string;
  /** The active preset's params with this System's selection. */
  params: ParamView[];
  /** What Launch would run now. */
  command?: CommandView;
  bench?: BenchSummary;
  /** Expected VRAM layers if launched now (the "will it fit" preview). */
  expectedVram?: VramLayer[];
  expectedVramSource?: 'measured' | 'file-size';
  /** The first GPU it runs on ("VEN:DEV", "VEN:DEV#n" or "cpu"): the fit display. */
  gpu?: string;
  /** Every GPU it runs on. */
  gpus: string[];
  /** Its preset is an external server (watched only; Launch/Stop are refused). */
  external: boolean;
  /** Needs the whole GPU. */
  exclusive: boolean;
  /** False for remote Systems whose node does not grant "edit" (and ghost sessions). */
  editable: boolean;
  /** May be launched / stopped from here (local: true; remote: the node grants "launch"). */
  controllable: boolean;
  /** Running Systems that must stop before this one can launch: Launch shows "Stop <labels> & launch". */
  conflicts: SystemId[];
  /** The base URL clients use (LLM: ".../v1"). */
  endpoint?: string;
  session?: Session;
  lastSession?: LastSession;
  /** 0..1: how hard it works right now, for the tab pulse. */
  activity: number;
}

// ------------------------------------------------------------------------------------------- session

/** Lifecycle of a session. */
export type Phase = 'starting' | 'loading' | 'live' | 'stopping' | 'fault';

export type LoadStepId = 'process' | 'device' | 'weights' | 'kv' | 'warmup' | 'ready';

export interface LoadStep {
  id: LoadStepId;
  /** "Load weights" */
  label: string;
  /** 'failed' only appears inside Fault.steps (the step that died). */
  state: 'done' | 'active' | 'pending' | 'failed';
  /** "11.6 / 11.6 GiB", "ROCm · gfx1201" */
  detail?: string;
}

export interface LoadProgress {
  steps: LoadStep[];
  /** 0..1 overall, derived from the steps and the weights bytes. */
  fraction: number;
  elapsedS: number;
}

export interface Fault {
  /** One plain sentence: what happened. */
  title: string;
  /** Process exit code, if known. */
  exitCode?: number;
  /** "0xC0000409" style rendering of the exit code, if useful. */
  exitCodeHex?: string;
  /** Last lines of the error log, ANSI-stripped. */
  logTail: string[];
  /** Seconds since the fault. */
  sinceS: number;
  /** If the session died while loading: the load steps, with the failing one marked 'failed'. */
  steps?: LoadStep[];
}

/** Summary of a System's previous session, shown while it is offline. */
export interface LastSession {
  system: SystemId;
  model: ModelRef;
  /** How long it ran. */
  uptimeS: number;
  /** Seconds since it ended. */
  endedAgoS: number;
  /** How it ended. */
  ended: 'stopped' | 'fault';
  /** LLM sessions. */
  requests?: number;
  generatedTokens?: number;
  /** Median decode speed over the session (LLM). */
  decodeTps?: number;
  /** Image sessions. */
  images?: number;
  /** Median seconds per image (image). */
  secondsPerImage?: number;
}

export interface RequestRecord {
  id: number;
  /** Seconds since epoch (ms precision not needed). */
  at: number;
  promptTokens: number;
  /** Prompt tokens served from the prompt cache. */
  cachedTokens: number;
  prefillS: number;
  generatedTokens: number;
  decodeS: number;
  /** The best decode speed of one full window the server measured (llama.cpp tg_3s, >= 64 tokens). */
  peakDecodeTps?: number;
}

export interface LlmLive {
  /** What the server is doing right now. */
  activity: 'idle' | 'prefill' | 'decode';
  /** Smoothed decode speed of the current or last request. */
  decodeTps: number;
  /** One sample per second, oldest first, up to 300 samples (5 minutes). 0 while idle. */
  decodeHistory: number[];
  /**
   * The current (or last) request's prompt processing.
   * While activity === 'prefill' every skin shows THIS as its hero (progress, tok/s, ETA), never "0.0 tok/s".
   */
  prefill: {
    /** Tokens that must be processed (prompt minus prompt-cache hits). */
    tokens: number;
    doneTokens: number;
    /** Prompt tokens reused from the prompt cache for this request. */
    cachedTokens?: number;
    tps: number;
    elapsedS: number;
    /** Remaining seconds while prefill is active; 0 when done. */
    etaS: number;
  } | null;
  /** Tokens generated in the current (or last) request. */
  generatedTokens: number;
  context: { usedTokens: number; totalTokens: number };
  /**
   * Speculative decoding acceptance for the current (or last) request. Non-null whenever speculation is
   * configured; acceptancePct is 0 until the first drafted token. `active` = drafting right now.
   */
  spec: { acceptancePct: number; mode: string; active?: boolean } | null;
  /** Most recent requests, oldest first, up to 12. */
  requests: RequestRecord[];
  /** Lifetime totals of this session. */
  totals: { requests: number; promptTokens: number; generatedTokens: number };
}

export interface ImageJob {
  /** Seconds since epoch when it finished. */
  at: number;
  seconds: number;
  width: number;
  height: number;
  edit: boolean;
  /** Sampling steps, when the log showed them. */
  steps?: number;
  /** A video job (sd.cpp `generate_video WxHxT`): its frame count. */
  frames?: number;
}

export interface ImageLive {
  activity: 'idle' | 'generating';
  /** Current sampling step (1-based) while generating. */
  step: number;
  steps: number;
  /** Seconds per iteration of the current job. */
  sPerIt: number;
  /** Elapsed seconds of the current job. */
  elapsedS: number;
  width: number;
  height: number;
  edit: boolean;
  /** Finished jobs this session, oldest first, up to 24. */
  recent: ImageJob[];
  imagesThisSession: number;
  /** Frames of the video job in flight (or the last one), for sd.cpp video. */
  frames?: number;
}

/** What KLIF can tell about a server without a dedicated parser (tts / stt / video, generic, openai). */
export interface GenericLive {
  requestsInFlight?: number;
  requestsTotal?: number;
  /** Seconds since the last log line / request activity. */
  lastActivityS?: number;
  /** The model id the server reports (/v1/models). */
  modelId?: string;
  /** Whether the model's weights are resident (audio.cpp /v1/models `loaded`); absent: the server does not say. */
  modelLoaded?: boolean;
}

export interface Session {
  system: SystemId;
  model: ModelRef;
  phase: Phase;
  /** Seconds since the session started. */
  uptimeS: number;
  endpoint: { host: string; port: number };
  apiKeySet: boolean;
  loading: LoadProgress | null;
  fault: Fault | null;
  /** The live part is ONE of llm / image / generic (the others are null), by kind. Exception: a video System on
   *  sd.cpp has generic AND image (steps, the job in flight, recent jobs with their frames). */
  llm: LlmLive | null;
  image: ImageLive | null;
  generic: GenericLive | null;
  /** The preset it was launched from. */
  preset?: string;
  /** What actually ran ("differs from the running session" when hash ≠ System.command.hash). */
  command?: CommandView;
  /** The GPU it runs on. */
  gpu?: string;
  /** VRAM committed by the session's processes. */
  vramGiB?: number;
}

// ---------------------------------------------------------------------------------------- machine

export type VramLayerId = 'weights' | 'kv' | 'buffers' | 'draft' | 'projector' | 'other';

export interface VramLayer {
  id: VramLayerId;
  /** "KV cache" */
  label: string;
  gib: number;
}

/** A GPU's memory: the "cliff". */
export interface GpuMemory {
  /** "VEN:DEV" (e.g. "1002:7550"), "VEN:DEV#n" or "cpu". */
  id: string;
  /** "RX 9070 XT" */
  name: string;
  /** Same as name (kept for 0.2 skins). */
  device: string;
  /** Usable dedicated memory: the cliff edge. */
  totalGiB: number;
  usedGiB: number;
  /**
   * Composition of usedGiB, bottom layer first. vm.vram: [other, ...the selected session's layers];
   * vm.gpus[*]: [other] only.
   */
  layers: VramLayer[];
  /** Allocations demoted to shared system memory: material that went over the edge. */
  spillMiB: number;
  /** One sample per second of usedGiB, oldest first, up to 300 samples. */
  history: number[];
  /** Optional per-layer history (same cadence and length as history), for skins that draw it over time. */
  layerHistory?: Partial<Record<VramLayerId, number[]>>;
  /** VRAM held by things other than KLIF's sessions (driver, other processes): spare = total - baseline - expected. */
  baselineGiB: number;
  /** Headroom below which the UI may warn (configured, not measured). */
  warnBelowGiB: number;
  /**
   * The GPU powered down while a model is loaded (e.g. AMD ULPS / device state D3): the sessions' allocations
   * still exist but are paged out to system RAM, and the next request pays a wake-up delay. While dormant,
   * `layers` show the ALLOCATIONS (they may sum to more than `usedGiB`); skins draw them as paged out (ghosted).
   */
  dormant?: {
    /** Session allocations not resident in VRAM right now. */
    pagedOutGiB: number;
    /** Seconds since the GPU went dormant. */
    sinceS: number;
    /** Device power state if known, e.g. "D3". */
    powerState?: string;
  } | null;
}

/** This machine (or a node): RAM and CPU. */
export interface MachineStats {
  ramUsedGiB: number;
  ramTotalGiB: number;
  /** "DDR5", if known. */
  ramType?: string;
  /** "9950X3D" */
  cpuName: string;
  cpuPct: number;
}

/** Facts about the host window, so skins can draw their own window chrome when frameless. */
export interface HostInfo {
  kind: 'browser' | 'tauri';
  /** True when the native title bar is hidden and the skin must draw window controls. */
  frameless: boolean;
  maximized: boolean;
  /** "0.3.1" */
  appVersion: string;
  /**
   * Panel mode = the read-only mini layout on the small status screen (e.g. a 3.5" 960x640 monitor).
   * In the desktop app, entering it moves the window onto that screen and fills it; leaving restores
   * the previous window. In a browser it just switches the layout.
   */
  panel: {
    /** True when a small screen to move to was found (desktop app) or always in a browser. */
    available: boolean;
    active: boolean;
    /** "960x640" or the monitor's name, for the button tooltip. */
    target?: string;
  };
}

// ----------------------------------------------------------------------------------------- hardware

/** One GPU or CPU of a machine with its theoretical peak FP32 throughput (vm.rs ComputeDevice). */
export interface ComputeDevice {
  /** "VEN:DEV", "VEN:DEV#n" or "cpu". */
  id: string;
  /** "RX 9070 XT", "Ryzen 9 9950X3D". */
  name: string;
  kind: 'gpu' | 'cpu';
  integrated: boolean;
  /** In the memory pool and the TFLOPS total. */
  counted: boolean;
  vramGiB?: number;
  /** The part of vramGiB that is system memory (a counted integrated GPU), already inside ramTotalGiB. */
  sharedGiB?: number;
  /** "GDDR6", "HBM3", "LPDDR5X (unified)". */
  memoryType?: string;
  tflopsFp32?: number;
  tflopsSource: 'table' | 'config' | 'computed' | 'unknown';
  /** "64 CU, 2970 MHz" / "16 cores, AVX-512, 4.3 GHz". */
  detail?: string;
}

/** A machine's static compute and memory facts. */
export interface HardwareInfo {
  gpus: ComputeDevice[];
  cpu?: ComputeDevice;
  ramTotalGiB: number;
  /** Sum of the counted GPUs' memory. */
  vramPoolGiB: number;
  /** The largest single counted GPU. */
  largestGpuGiB: number;
  /** The pool is unified memory (only an integrated GPU). */
  unified: boolean;
  /** Sum over counted devices with a known number. */
  tflopsFp32: number;
  /** Counted devices without a number ("+?"). */
  tflopsUnknown: number;
}

// -------------------------------------------------------------------------------------- suggestions

export type SuggestSlot = 'fast' | 'deep' | 'max' | 'image' | 'tts' | 'stt' | 'video' | 'music';

/** The suggested model for one slot on this machine (an estimate; rec absent = nothing fits). */
export interface Suggestion {
  slot: SuggestSlot;
  kind: SystemKind;
  class?: LlmClass;
  /** Recommendation id to download / adopt. */
  rec?: string;
  model: string;
  quant: string;
  ctx?: number;
  /** KV cache type: "f16", "q8_0". */
  kv?: string;
  estVramGiB: number;
  estRamGiB: number;
  budgetVramGiB: number;
  budgetRamGiB: number;
  /** "experts in RAM". */
  placement?: string;
  note?: string;
}

// ------------------------------------------------------------------------------------------ records

/** *Tps / *Rtf: higher is better; *S: lower is better. */
export type RecordMetric = 'decodeTps' | 'prefillTps' | 'ttftS' | 'imageS' | 'ttsRtf' | 'sttRtf' | 'videoS' | 'musicRtf';

/** The exact model file a record belongs to. */
export interface RecordModel {
  /** SHA-256 of the model file; absent until hashed. */
  sha256?: string;
  file: string;
  name: string;
  quant?: string;
  /** "repo@sha" when the file matches a known recommendation file. */
  source?: string;
  sizeBytes?: number;
}

/** One best value and the conditions it was reached under. */
export interface RecordValue {
  value: number;
  /** Epoch seconds. */
  at: number;
  source: 'live' | 'bench';
  /** The context the server was launched with. */
  ctx?: number;
  promptTokens?: number;
  cachedTokens?: number;
  genTokens?: number;
  kv?: string;
  width?: number;
  height?: number;
  steps?: number;
  /** Video frames of the job (videoS). */
  frames?: number;
  gpus: string[];
  backendBuild?: string;
  preset?: string;
  klifVersion: string;
  /** The machine's FP32 TFLOPS total at the time. */
  tflopsFp32?: number;
}

/** Best values of one model file on one backend on one machine. */
export interface RecordEntry {
  key: string;
  /** Remote node id; absent = this machine. */
  node?: string;
  /** `[node] name`, else "This machine". */
  machine: string;
  kind: SystemKind;
  model: RecordModel;
  /** "HIP", "Vulkan", "CUDA", "CPU", "Metal", "" unknown. */
  backend: string;
  best: Partial<Record<RecordMetric, RecordValue>>;
}

/** A broken record (the last few, newest last). */
export interface RecordEvent {
  key: string;
  metric: RecordMetric | null;
  /** The previous best; absent for the first value. */
  old?: number;
  new: number;
  at: number;
}

/**
 * One line of a record's climb, as `klif_records_history` returns it (Vec<RecordEvent>, oldest first): every time
 * the record `key` was broken. A node's entry comes back keyed "<node>/<key>". `metric` is null only for a line
 * written without one (older files); the first value of a metric has no `old`.
 */
export type RecordHistoryLine = RecordEvent;

// -------------------------------------------------------------------------------------------- nodes

export type NodeState = 'connecting' | 'online' | 'offline' | 'unauthorized' | 'incompatible';

/** A remote KLIF node (`[nodes.<id>]`) as seen from here. */
export interface NodeView {
  id: string;
  name: string;
  address: string;
  state: NodeState;
  /** The node's KLIF version. */
  version?: string;
  latencyMs?: number;
  /** Rights it grants us besides view: "launch", "edit". */
  allow: string[];
  gpus: GpuMemory[];
  machine?: MachineStats;
  hardware?: HardwareInfo;
  /** Its presets (masked), node = this node's id. */
  presets: PresetInfo[];
  error?: string;
}

// ---------------------------------------------------------------------------------------- view model

export interface ViewModel {
  /** Seconds since epoch at the moment the snapshot was taken. */
  now: number;
  /** Local Systems in file order, then ghost sessions, then remote Systems by node. Empty = onboarding. */
  systems: System[];
  /** The selected tab (null: no Systems). */
  selected: SystemId | null;
  // Conveniences so skins keep working, all derived from the SELECTED System:
  /** The selected System's session. */
  session: Session | null;
  /** The selected System's previous session. */
  lastSession: LastSession | null;
  /** The selected System's console: last lines, ANSI-stripped, newest last, up to 200. */
  console: string[];
  /** The selected System's GPU, else [gpu] inference. */
  vram: GpuMemory;
  /** Every local GPU KLIF measures. */
  gpus: GpuMemory[];
  machine: MachineStats;
  host: HostInfo;
  /** Local presets (remote ones: nodes[n].presets). */
  presets: PresetInfo[];
  recommendations: RecommendationInfo[];
  /** This machine's compute and memory. */
  hardware: HardwareInfo;
  /** The suggested model per slot for this machine (estimates). */
  suggestions: Suggestion[];
  /** Best values per model file and backend, this machine and the nodes. */
  records: RecordEntry[];
  /** The last broken records, newest last. */
  recordEvents: RecordEvent[];
  /** Changes whenever records / recordEvents change (absent: unknown). */
  recordsRev?: number;
  downloads: DownloadInfo[];
  config: ConfigInfo;
  nodes: NodeView[];
}

// ------------------------------------------------------------------------------------------- actions

/** The right a network peer needs for an action or protocol method. */
export type Right = 'view' | 'launch' | 'edit' | 'local-only';

/**
 * The engine actions, exactly as serialized (serde shape of klif_common::vm::Action). `system` absent = the
 * selected System. Actions on "<node>/<id>" Systems (or with `node`) are forwarded to that node.
 */
export type EngineAction =
  | { type: 'select'; system: SystemId }
  | { type: 'launch'; system?: SystemId; stopOthers?: boolean }
  | { type: 'stop'; system?: SystemId }
  | { type: 'stopAll' }
  | { type: 'restart'; system?: SystemId }
  | { type: 'dismiss'; system?: SystemId }
  | { type: 'usePreset'; system: SystemId; preset: string }
  | { type: 'setParam'; system: SystemId; name: string; value: string }
  | {
      type: 'savePreset';
      id: string;
      preset: PresetSpec;
      selectFor?: SystemId;
      secretsFrom?: string;
      baseHash?: string;
      node?: string;
    }
  | { type: 'deletePreset'; id: string; node?: string }
  | { type: 'addSystem'; id?: string; label?: string; kind: SystemKind; class?: LlmClass; preset?: string; node?: string }
  | { type: 'removeSystem'; system: SystemId }
  | { type: 'updateSystem'; system: SystemId; label?: string; moveTo?: number; exclusive?: boolean }
  | { type: 'downloadRecommendation'; id: string; node?: string }
  | { type: 'cancelDownload'; id: string; node?: string }
  | { type: 'adoptRecommendation'; id: string; system?: SystemId; ctx?: number; kv?: string }
  /** Remove a junk record (the key as that machine knows it; node = a remote node's entry). */
  | { type: 'forgetRecord'; key: string; node?: string }
  | { type: 'updateSettings'; recordMoment?: boolean };
// Serde shape: Action::ForgetRecord { key, node? } (klif_common::vm), tagged "forgetRecord"; needs the node's "edit"
// right when it is a remote entry.

/**
 * Everything a skin can ask the shell to do. Skins never talk to the core directly. Engine actions return a
 * Promise that rejects with the engine's sentence (shown inline / as a toast); shell actions are fire-and-forget.
 */
/**
 * How the UI reports one engine action. By default a refusal is ALSO shown as a toast (every surface gets it
 * once); `quiet` skips that toast for a caller that shows the sentence itself (Tune, inline). Never sent to the
 * engine.
 */
export interface CallOpts {
  quiet?: boolean;
}

export interface Actions {
  // ---- engine actions ----
  select(system: SystemId, call?: CallOpts): Promise<void>;
  /** With conflicts the control reads "Stop <labels> & launch" and passes stopOthers: true. */
  launch(system?: SystemId, opts?: { stopOthers?: boolean } & CallOpts): Promise<void>;
  stop(system?: SystemId, call?: CallOpts): Promise<void>;
  stopAll(call?: CallOpts): Promise<void>;
  restart(system?: SystemId, call?: CallOpts): Promise<void>;
  /** Leave a fault and return the System to offline. */
  dismiss(system?: SystemId, call?: CallOpts): Promise<void>;
  usePreset(system: SystemId, preset: string, call?: CallOpts): Promise<void>;
  setParam(system: SystemId, name: string, value: string, call?: CallOpts): Promise<void>;
  savePreset(
    id: string,
    preset: PresetSpec,
    opts?: { selectFor?: SystemId; secretsFrom?: string; baseHash?: string; node?: string } & CallOpts,
  ): Promise<void>;
  deletePreset(id: string, node?: string, call?: CallOpts): Promise<void>;
  addSystem(spec: { id?: string; label?: string; kind: SystemKind; class?: LlmClass; preset?: string; node?: string }, call?: CallOpts): Promise<void>;
  removeSystem(system: SystemId, call?: CallOpts): Promise<void>;
  updateSystem(system: SystemId, patch: { label?: string; moveTo?: number; exclusive?: boolean }, call?: CallOpts): Promise<void>;
  downloadRecommendation(id: string, node?: string, call?: CallOpts): Promise<void>;
  cancelDownload(id: string, node?: string, call?: CallOpts): Promise<void>;
  /**
   * A preset from a recommendation (set on `system`). `fit` = the context and KV type of a suggestion card; without
   * them the engine uses this machine's suggestion of that recommendation, else its defaults.
   */
  adoptRecommendation(id: string, system?: SystemId, opts?: { fit?: { ctx?: number; kv?: string } } & CallOpts): Promise<void>;
  /**
   * Remove a junk record (RecordEntry.key as that machine knows it: for a remote entry drop the "<node>/" prefix and
   * pass the node). A remote node must grant "edit".
   */
  forgetRecord(key: string, node?: string, call?: CallOpts): Promise<void>;
  /** This machine's display settings (`[ui]` in klif.toml). */
  updateSettings(patch: { recordMoment?: boolean }, call?: CallOpts): Promise<void>;
  // ---- shell actions ----
  openEndpoint(system?: SystemId): void;
  copyEndpoint(system?: SystemId): void;
  copyApiKey(): void;
  toggleConsole(open?: boolean): void;
  /** Open the Tune drawer on a System, or in add mode ("Add System"). */
  openTune(system?: SystemId, opts?: { add?: boolean }): void;
  /** Open the Records screen (the best each model reached). Full-size skins put a button for it next to Tune. */
  openRecords(): void;
  /** Window chrome; only meaningful when vm.host.frameless. Skins mark their header with
   *  data-tauri-drag-region so the window can be dragged by it. */
  minimize(): void;
  toggleMaximize(): void;
  closeWindow(): void;
  /**
   * Enter/leave panel mode (see HostInfo.panel). Full-size skins show a button for it next to their
   * window controls; the mini layout shows a small "back" control only on hover/tap.
   */
  togglePanel(): void;
}

/**
 * Config-level calls for the Tune drawer and the Records screen (`player.config`). Native: klif_preset_get,
 * klif_command_preview, klif_set_api_key, klif_open_config, klif_open_logs, klif_records_history, klif_save_image.
 * Mock: computed by the mock catalog. Errors reject with a sentence.
 */
export interface ConfigApi {
  /** One preset in full (secrets masked); `node` = a remote node's preset. */
  presetGet(id: string, node?: string): Promise<PresetDetail | null>;
  /**
   * The climb of one record: every time `key` (a RecordEntry.key as published, "<node>/<key>" for a remote entry)
   * was broken, oldest first, optionally of one metric. Rejects with a sentence (no such record; the node could
   * not answer).
   */
  recordsHistory(key: string, metric?: RecordMetric): Promise<RecordHistoryLine[]>;
  /**
   * Save a rendered PNG or GIF (the export card of a record). Native: into the user's Pictures folder, subfolder
   * KLIF, never overwriting; resolves with the full path. Browser (mock): downloads the file; resolves null.
   */
  saveImage(fileName: string, data: Blob): Promise<string | null>;
  /** The command an unsaved preset spec would run for `system` (debounced preview in Tune). */
  commandPreview(spec: PresetSpec, system?: SystemId): Promise<CommandView>;
  /** Set (string) or clear (null) the API key natively; the key is never echoed back. */
  setApiKey(key: string | null): Promise<void>;
  openConfig(): Promise<void>;
  openLogs(): Promise<void>;
}
