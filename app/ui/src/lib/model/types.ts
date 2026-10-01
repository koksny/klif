// KLIF view model: the single contract between the core (or the mock) and every skin.
//
// Rules:
// - Skins only READ this. They never invent a number that is not here.
// - Every field is a measurement or a configured fact; nothing is decorative.
// - Units are in field names (GiB, MiB, S = seconds, Tps = tokens per second, Pct = 0..100).
// - The view model is replaced wholesale at the telemetry rate (1-2 Hz). Skins interpolate between
//   snapshots themselves (see lib/render), never by polling.

/** The four job slots. LLM slots are tiers, not models: the model behind a tier is swappable. */
export type SlotId = 'high' | 'medium' | 'low' | 'krea';

export type SlotKind = 'llm' | 'image';

/** Catalog availability of the model assigned to a slot (mirrors the launcher's precedence). */
export type Availability =
  | 'ready'
  | 'unsupported'
  | 'script-missing'
  | 'model-missing'
  | 'build-required';

export type Backend = 'HIP' | 'Vulkan' | 'CPU';

/** What runs behind a slot. All strings are display-ready. */
export interface ModelRef {
  /** "Qwen 3.8 27B" */
  name: string;
  /** "GSQ-RCO IQ3_S" */
  quant: string;
  /** Inference engine family, e.g. "llama.cpp", "sd.cpp". Engine-agnostic on purpose. */
  engine: string;
  backend: Backend;
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
  /** Reasoning mode, e.g. "Thinking" / "Instruct" (LLM only). */
  mode?: string;
  /** Default output size for image models, e.g. "512x768". */
  imageSize?: string;
  /** Weights size on disk in GiB, used to preview fit before launch. */
  weightsGiB?: number;
}

export interface Slot {
  id: SlotId;
  /** "AGENT HIGH" */
  label: string;
  kind: SlotKind;
  model: ModelRef;
  availability: Availability;
  /** Expected VRAM layers if launched with the current recipe (for the "will it fit" preview). */
  expectedVram?: VramLayer[];
}

/** Lifecycle of the one running session. */
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

/** Summary of the previous session, shown by the idle launcher. */
export interface LastSession {
  slot: SlotId;
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
}

export interface Session {
  slot: SlotId;
  model: ModelRef;
  phase: Phase;
  /** Seconds since the session started. */
  uptimeS: number;
  endpoint: { host: string; port: number };
  apiKeySet: boolean;
  loading: LoadProgress | null;
  fault: Fault | null;
  llm: LlmLive | null;
  image: ImageLive | null;
}

export type VramLayerId = 'weights' | 'kv' | 'buffers' | 'draft' | 'projector' | 'other';

export interface VramLayer {
  id: VramLayerId;
  /** "KV cache" */
  label: string;
  gib: number;
}

/** The inference GPU's memory: the "cliff". */
export interface GpuMemory {
  /** "RX 9070 XT" */
  device: string;
  /** Usable dedicated memory: the cliff edge. */
  totalGiB: number;
  usedGiB: number;
  /** Composition of usedGiB, bottom layer first. Sums to usedGiB (within rounding). */
  layers: VramLayer[];
  /** Allocations demoted to shared system memory: material that went over the edge. */
  spillMiB: number;
  /** One sample per second of usedGiB, oldest first, up to 300 samples. */
  history: number[];
  /** Optional per-layer history (same cadence and length as history), for skins that draw it over time. */
  layerHistory?: Partial<Record<VramLayerId, number[]>>;
  /**
   * VRAM held by things other than KLIF's session (driver, other processes). Part of usedGiB as the
   * 'other' layer while a session runs; used for the idle fit preview: spare = total - baseline - expected.
   */
  baselineGiB: number;
  /** Headroom below which the UI may warn (configured, not measured). */
  warnBelowGiB: number;
}

export interface SystemStats {
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
  /** "0.2.0" */
  appVersion: string;
}

export interface ViewModel {
  /** Seconds since epoch at the moment the snapshot was taken. */
  now: number;
  slots: Slot[];
  /** The slot the launcher has selected (the running one while a session exists). */
  selected: SlotId;
  session: Session | null;
  vram: GpuMemory;
  system: SystemStats;
  /** The previous session (idle launcher summary), if any. */
  lastSession: LastSession | null;
  host: HostInfo;
  /** Last console lines, ANSI-stripped, newest last, up to 200. */
  console: string[];
}

/** Everything a skin can ask the shell to do. Skins never talk to the core directly. */
export interface Actions {
  select(slot: SlotId): void;
  launch(slot?: SlotId): void;
  stop(): void;
  restart(): void;
  openEndpoint(): void;
  copyEndpoint(): void;
  copyApiKey(): void;
  toggleConsole(open?: boolean): void;
  openTune(slot?: SlotId): void;
  /** Leave a fault (or finished) session and return to the idle launcher. */
  dismiss(): void;
  /** Window chrome; only meaningful when vm.host.frameless. Skins mark their header with
   *  data-tauri-drag-region so the window can be dragged by it. */
  minimize(): void;
  toggleMaximize(): void;
  closeWindow(): void;
}
