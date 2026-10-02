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

/**
 * Catalog availability of the model assigned to a slot (mirrors the launcher's precedence).
 * 'busy' = the slot's port is held by a process KLIF does not own.
 */
export type Availability =
  | 'ready'
  | 'unsupported'
  | 'script-missing'
  | 'model-missing'
  | 'build-required'
  | 'busy';

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
  /** Reasoning mode, e.g. "Thinking" / "Instruct" (LLM); Krea on the fast starter: "Edit · Low" / "Generate · Low". */
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
  /** One plain sentence when availability !== 'ready', e.g. "Model file not found". */
  reason?: string;
  /** Expected VRAM layers if launched with the current recipe (for the "will it fit" preview). */
  expectedVram?: VramLayer[];
  /** What is launched for this slot. Changed with Actions.setRecipe (the Tune drawer). */
  recipe?: Recipe;
  /** The choices the Tune drawer may offer for this slot, from the catalog. */
  options?: RecipeOptions;
}

/** The launch recipe of a slot. Values are catalog values; display text lives in ModelRef. */
export interface Recipe {
  /** Catalog card id, e.g. "qwen-gsq". */
  cardId: string;
  backend: Backend;
  /** Catalog hardware value, e.g. "9070", "Dual", "9950X3D". */
  hardware: string;
  /** LLM context in tokens. */
  ctxTokens?: number;
  /** Image output size, e.g. "512x768". */
  imageSize?: string;
  kvType?: 'q4_0' | 'q8_0';
  promptCacheMiB?: number;
  port?: number;
  vision?: boolean;
  /** Reasoning mode, e.g. "Thinking" / "Instruct", only where the card supports it. */
  mode?: string;
  /**
   * Krea speed/quality level (image slot, only where options.precisions is offered: the fast Krea starter).
   * low = rank-64 edit LoRA + quarter-size reference, medium = rank-64 + half-size reference,
   * high = the full identity LoRA + half-size reference. Default 'low'.
   */
  precision?: Precision;
  /**
   * Krea identity edit (image slot, only where options.editToggle is true). Off = plain generation; on = the
   * edit LoRA and reference preset per precision, with the identity LoRA applied to every request that names
   * no LoRA itself. Default false.
   */
  edit?: boolean;
}

/** The three Krea precision levels (Recipe.precision). */
export type Precision = 'low' | 'medium' | 'high';

export interface RecipeChoice<T> {
  value: T;
  label: string;
  availability: Availability;
  reason?: string;
}

export interface RecipeOptions {
  /** Models that may sit behind this slot (cards), with their availability. */
  cards: (RecipeChoice<string> & { name: string; quant: string })[];
  backends: RecipeChoice<Backend>[];
  hardware: RecipeChoice<string>[];
  contexts?: RecipeChoice<number>[];
  imageSizes?: RecipeChoice<string>[];
  kvTypes?: ('q4_0' | 'q8_0')[];
  /** Prompt cache sizes, already capped where the catalog caps them. */
  promptCacheMiB?: number[];
  ports?: number[];
  /** True when the current card supports the vision projector. */
  vision?: boolean;
  modes?: string[];
  /** Krea precision levels with a one-line hint each ("fastest · ~19 s edit 1024×768"); fast Krea starter only. */
  precisions?: (RecipeChoice<Precision> & { hint: string })[];
  /** True when the current card takes the Edit toggle (fast Krea starter only). */
  editToggle?: boolean;
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
  /**
   * The inference GPU powered down while a model is loaded (e.g. AMD ULPS / device state D3): the
   * session's allocations still exist but are paged out to system RAM, and the next request pays a
   * wake-up delay while they are restored. While dormant, `layers` show the session's ALLOCATIONS
   * (so they may sum to more than `usedGiB`, which stays the resident amount); skins draw them as
   * paged out (ghosted), never as if they were resident.
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
  /** Change a slot's launch recipe (Tune drawer). A running session keeps its recipe until restart. */
  setRecipe(slot: SlotId, patch: Partial<Recipe>): void;
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
