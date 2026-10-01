// The mock core: a deterministic state machine that produces a fresh ViewModel snapshot on demand and
// implements Actions. Time is simulated (seconds since the scenario started); the reactive wrapper
// (lib/state/player.svelte.ts) feeds it wall-clock deltas times the speed factor.
import type {
  Actions,
  Fault,
  GpuMemory,
  HostInfo,
  LastSession,
  LoadProgress,
  ModelRef,
  Phase,
  Session,
  Slot,
  SlotId,
  SystemStats,
  ViewModel,
  VramLayer,
  VramLayerId,
} from '../model/types';
import { SLOTS } from '../model/sample';
import { bootLines, bootView, planBoot, type BootLine, type BootPlan } from './boot';
import { expectedLayers, fitLayers } from './catalog';
import { ConsoleBuf, llamaTs } from './consoleBuf';
import { failSteps, makeFault, POWERSHELL_FAIL_STDERR, ROCM_CRASH_STDERR, ROCM_EXIT_CODE, type FaultKind } from './faults';
import { browserHost } from './host';
import { ImageSim } from './imageSim';
import { LlmSim } from './llmSim';
import { bootDurations, defaultPort, deviceDetail, imageConfig, llmConfig, ramFootprintGiB } from './profiles';
import { clamp, createRng, hashSeed, type Rng } from './rng';

const SUB = 0.5; // max simulation sub-step in seconds
const STOP_S = 2.4;
/** VRAM held by the driver and other processes: visible on the device with nothing loaded. */
const BASE_VRAM = 0.12;
const VRAM_TOTAL = 15.87;
const VRAM_WARN = 0.15;
const RAM_TOTAL = 93.6;
const RAM_BASE = 24.0;
const RAM_TYPE = 'DDR5';
const HIST = 300;
const MOCK_KEY = 'klif-mock-key-not-a-secret';
/** Peak extra compute-buffer demand during a long prefill in the spill scenario (GiB). */
const SWELL_PEAK_GIB = 0.93;
const BASELINE_TAU_S = 4;

const LAYER_IDS: VramLayerId[] = ['other', 'weights', 'kv', 'buffers', 'draft', 'projector'];
/** When demand exceeds the edge, the allocator demotes these to shared memory first (compute buffers, KV...). */
const DEMOTE_ORDER: VramLayerId[] = ['buffers', 'kv', 'draft', 'projector', 'weights'];

export interface EngineHooks {
  toast(text: string): void;
  toggleConsole(open?: boolean): void;
  openTune(slot?: SlotId): void;
  copy(text: string, toastText: string): void;
  openUrl(url: string): void;
}

export interface Extras {
  promptCacheMiB: number;
  port: number;
}

export interface LaunchOpts {
  /** Start already live (no boot sequence). */
  skipBoot?: boolean;
  /** Pretend the session started this long before the scenario's t=0. */
  uptimeOffsetS?: number;
  /** Die this many seconds after launch. */
  failAtS?: number;
  failKind?: FaultKind;
  /** First request is the real 66k-token prefill. */
  big?: boolean;
  /** Image: crash on the n-th job. */
  crashOnJob?: number;
  /** LLM: compute buffers swell with the prefill progress until the demand passes the VRAM edge (spill). */
  spill?: boolean;
}

/** The previous session, kept by the engine; endedAgoS is derived from the clock at snapshot time. */
export type LastRec = Omit<LastSession, 'endedAgoS'> & { endedAt: number };

interface SessionRt {
  slot: SlotId;
  model: ModelRef;
  port: number;
  apiKey: boolean;
  t0: number;
  procOffset: number;
  phase: Phase;
  phaseT: number;
  plan: BootPlan;
  lines: BootLine[];
  lineIdx: number;
  opts: LaunchOpts;
  llm: LlmSim | null;
  image: ImageSim | null;
  fault: Fault | null;
  faultAt: number;
  /** Time at which stop() was requested (uptime ends here). */
  stopT: number;
  /** Session layers (baseline NOT included), already fitted by the loader. */
  layers: VramLayer[];
  /** Eased 0..1 load of the activation buffers (image slots). */
  act: number;
  /** Extra compute-buffer demand in GiB (spill scenario). */
  swell: number;
  seed: number;
}

const r1 = (v: number) => Math.round(v * 10) / 10;
const r2 = (v: number) => Math.round(v * 100) / 100;

function cloneSlot(s: Slot): Slot {
  return { ...s, model: { ...s.model }, expectedVram: s.expectedVram?.map((l) => ({ ...l })) };
}

function idSums(layers: VramLayer[]): Record<VramLayerId, number> {
  const out: Record<VramLayerId, number> = { other: 0, weights: 0, kv: 0, buffers: 0, draft: 0, projector: 0 };
  for (const l of layers) out[l.id] += l.gib;
  return out;
}

interface Resolved {
  layers: VramLayer[];
  spillMiB: number;
}

/**
 * Residency. The device cannot hold more than totalGiB: what the layers ask for beyond the edge is demoted
 * to shared system memory (spillMiB) and the resident layers fill the edge exactly (free = 0). The first
 * layer is the baseline and is never demoted.
 */
function resolveOverflow(demand: VramLayer[], total: number): Resolved {
  const out = demand.map((l) => ({ ...l, gib: r2(l.gib) }));
  const sum = () => out.reduce((a, l) => a + l.gib, 0);
  let over = sum() - total;
  if (over <= 0.004) return { layers: out, spillMiB: 0 };
  const spillMiB = Math.round(over * 1024);
  for (const id of DEMOTE_ORDER) {
    for (let i = 1; i < out.length && over > 0; i++) {
      const l = out[i];
      if (l.id !== id) continue;
      const take = Math.min(l.gib, over);
      l.gib = r2(l.gib - take);
      over -= take;
    }
  }
  // Rounding: land exactly on the edge.
  const diff = r2(total) - sum();
  if (Math.abs(diff) > 1e-9) {
    const l = out.slice(1).find((x) => x.id === 'buffers') ?? out.slice(1).find((x) => x.gib > 0);
    if (l) l.gib = Math.max(0, r2(l.gib + diff));
  }
  return { layers: out.filter((l, i) => i === 0 || l.gib > 0.004), spillMiB };
}

export class MockEngine {
  /** Simulated seconds since the scenario started. */
  t = 0;
  selected: SlotId = 'medium';
  readonly console = new ConsoleBuf();
  readonly actions: Actions;
  /** Window facts reported to the skins (set by the scenario builder from ?frameless=1). */
  host: HostInfo = browserHost(false);

  private slots: Slot[];
  private slotsSnap: Slot[];
  private rt: SessionRt | null = null;
  private lastRec: LastRec | null = null;
  private rng: Rng;
  private seed: number;
  private sessionNo = 0;
  private pendingLaunch: SlotId | null = null;

  // VRAM
  private baseline = BASE_VRAM;
  private baselineTarget = BASE_VRAM;
  private baselineEvents: { at: number; gib: number }[] = [];
  private vramHist: number[] = Array.from({ length: HIST }, () => BASE_VRAM);
  private layerHist: Record<VramLayerId, number[]> = {
    other: Array.from({ length: HIST }, () => BASE_VRAM),
    weights: new Array<number>(HIST).fill(0),
    kv: new Array<number>(HIST).fill(0),
    buffers: new Array<number>(HIST).fill(0),
    draft: new Array<number>(HIST).fill(0),
    projector: new Array<number>(HIST).fill(0),
  };
  private layerAcc: Record<VramLayerId, number> = { other: 0, weights: 0, kv: 0, buffers: 0, draft: 0, projector: 0 };
  private vramAccT = 0;
  private layersNow: VramLayer[] = [{ id: 'other', label: 'other', gib: BASE_VRAM }];
  private sessionNow: VramLayer[] = [];
  private spillNow = 0;

  private cpu = 3;
  private ram = RAM_BASE;

  constructor(
    private readonly epoch0: number,
    seed: number,
    private readonly hooks: EngineHooks,
    private readonly extras: (slot: SlotId) => Extras,
  ) {
    this.seed = seed;
    this.rng = createRng(seed);
    this.slots = SLOTS.map(cloneSlot);
    this.slotsSnap = this.slots.map(cloneSlot);
    this.actions = {
      select: (slot) => this.select(slot),
      launch: (slot) => this.launch(slot),
      stop: () => this.stop(),
      restart: () => this.restart(),
      openEndpoint: () => this.openEndpoint(),
      copyEndpoint: () => this.copyEndpoint(),
      copyApiKey: () => this.copyApiKey(),
      toggleConsole: (open) => this.hooks.toggleConsole(open),
      openTune: (slot) => this.hooks.openTune(slot ?? this.selected),
      dismiss: () => this.dismiss(),
      minimize: () => this.windowControl(),
      toggleMaximize: () => this.windowControl(),
      closeWindow: () => this.windowControl(),
    };
  }

  // ---- public control ---------------------------------------------------------------------------

  /** Advance simulated time by dt seconds. */
  advance(dt: number) {
    let left = dt;
    let guard = 0;
    while (left > 1e-9 && guard++ < 4000) {
      const h = Math.min(SUB, left);
      this.step(h);
      left -= h;
    }
  }

  /** Run to an absolute simulated time in fixed sub-steps (deterministic pre-roll). */
  fastForward(toT: number) {
    while (this.t < toT - 1e-9) this.advance(Math.min(SUB, toT - this.t));
  }

  get running(): boolean {
    return this.rt !== null;
  }

  get phase(): Phase | null {
    return this.rt?.phase ?? null;
  }

  /** Replace the model behind a slot (Tune drawer). A running session keeps the settings it started with. */
  setSlotModel(slot: SlotId, model: ModelRef) {
    const s = this.slots.find((x) => x.id === slot);
    if (!s) return;
    s.model = { ...model };
    s.expectedVram = expectedLayers(model);
    this.slotsSnap = this.slots.map(cloneSlot);
  }

  slotModel(slot: SlotId): ModelRef {
    return { ...this.slots.find((x) => x.id === slot)!.model };
  }

  /** Scenario script: pretend a session ended `endedAgoS` seconds before t=0. */
  setLastSession(rec: Omit<LastSession, 'endedAgoS'>, endedAgoS: number) {
    this.lastRec = { ...rec, model: { ...rec.model }, endedAt: this.t - endedAgoS };
  }

  /** Scenario script: another process takes (or frees) VRAM at simulated time `atT`. */
  scheduleBaseline(atT: number, gib: number) {
    this.baselineEvents.push({ at: atT, gib });
    this.baselineEvents.sort((a, b) => a.at - b.at);
  }

  // ---- actions ----------------------------------------------------------------------------------

  select(slot: SlotId) {
    if (this.rt && this.rt.phase !== 'fault') {
      this.hooks.toast('Stop the running session to change the slot.');
      return;
    }
    if (this.slots.some((s) => s.id === slot)) this.selected = slot;
  }

  launch(slotId?: SlotId, opts: LaunchOpts = {}) {
    const id = slotId ?? this.selected;
    if (this.rt && this.rt.phase !== 'fault') {
      this.hooks.toast('A session is already running. Stop it first.');
      return;
    }
    const slot = this.slots.find((s) => s.id === id);
    if (!slot) return;
    if (slot.availability !== 'ready') {
      this.hooks.toast(`${slot.label} cannot start: ${slot.availability}.`);
      return;
    }
    // Launching over a faulted session ends that one (it becomes the "last session").
    if (this.rt) this.endFault(this.rt);
    this.selected = id;
    this.sessionNo++;
    this.pendingLaunch = null;
    const model: ModelRef = { ...slot.model };
    const kind = slot.kind;
    const x = this.extras(id);
    const sRng = createRng(hashSeed(`${this.seed}:${this.sessionNo}:${id}`));
    const d = bootDurations(model, sRng);
    // The loader trims its compute buffers to squeeze under the edge before anything spills.
    const target = fitLayers(expectedLayers(model), this.baseline, VRAM_TOTAL).layers;
    const plan = planBoot(d, kind, model, target, deviceDetail(model), x.port || defaultPort(id));
    const rt: SessionRt = {
      slot: id,
      model,
      port: plan.port,
      apiKey: kind === 'llm',
      t0: this.t - (opts.uptimeOffsetS ?? 0),
      procOffset: d.processS + d.deviceS,
      phase: 'starting',
      phaseT: this.t,
      plan,
      lines: bootLines(plan),
      lineIdx: 0,
      opts,
      llm: null,
      image: null,
      fault: null,
      faultAt: 0,
      stopT: 0,
      layers: target,
      act: 0.3,
      swell: 0,
      seed: hashSeed(`${this.seed}:${this.sessionNo}`),
    };
    this.rt = rt;
    this.console.clear();
    if (opts.skipBoot) {
      for (const l of rt.lines) this.pushBootLine(rt, l);
      rt.lineIdx = rt.lines.length;
      this.goLive(rt);
      this.refreshVram();
      this.backfillVram(rt, opts.uptimeOffsetS ?? 0);
    }
  }

  stop() {
    const s = this.rt;
    if (!s) {
      this.hooks.toast('Nothing is running.');
      return;
    }
    if (s.phase === 'fault') {
      this.dismiss();
      return;
    }
    if (s.phase === 'stopping') return;
    // Shrink from whatever is resident right now.
    s.layers = this.sessionNow.map((l) => ({ ...l }));
    s.phase = 'stopping';
    s.phaseT = this.t;
    s.stopT = this.t;
    this.console.push(this.stamp(s, 'srv', 'stop: shutting the server down'));
  }

  restart() {
    const s = this.rt;
    const slot = s?.slot ?? this.selected;
    if (!s || s.phase === 'fault') {
      this.dismiss();
      this.launch(slot);
      return;
    }
    this.pendingLaunch = slot;
    this.stop();
  }

  /** Leave a faulted session and return to the idle launcher (the fault becomes the last session). */
  dismiss() {
    const s = this.rt;
    if (!s) return;
    if (s.phase !== 'fault') {
      this.hooks.toast('Stop the running session first.');
      return;
    }
    this.endFault(s);
  }

  private endFault(s: SessionRt) {
    this.recordLast(s, 'fault', s.faultAt, s.faultAt);
    this.rt = null;
    this.pendingLaunch = null;
  }

  private windowControl() {
    this.hooks.toast('Window controls work in the desktop app.');
  }

  copyEndpoint() {
    const s = this.rt;
    if (!s) return this.hooks.toast('No session is running.');
    this.hooks.copy(s.plan.kind === 'llm' ? `http://127.0.0.1:${s.port}/v1` : `http://127.0.0.1:${s.port}/`, 'Endpoint copied');
  }

  copyApiKey() {
    const s = this.rt;
    if (!s) return this.hooks.toast('No session is running.');
    if (!s.apiKey) return this.hooks.toast('This session has no API key.');
    this.hooks.copy(MOCK_KEY, 'API key copied');
  }

  openEndpoint() {
    const s = this.rt;
    if (!s) return this.hooks.toast('No session is running.');
    if (s.phase !== 'live') return this.hooks.toast('The endpoint is not up yet.');
    this.hooks.openUrl(`http://127.0.0.1:${s.port}/`);
  }

  // ---- stepping ---------------------------------------------------------------------------------

  private step(h: number) {
    this.t += h;
    const s = this.rt;
    if (s) {
      switch (s.phase) {
        case 'starting':
        case 'loading':
          this.stepBoot(s);
          break;
        case 'live':
          this.stepLive(s, h);
          break;
        case 'stopping':
          if (this.t - s.phaseT >= STOP_S) this.finishStop(s);
          break;
      }
    }
    this.stepResources(h);
  }

  private stepBoot(s: SessionRt) {
    const e = this.t - s.t0;
    while (s.lineIdx < s.lines.length && s.lines[s.lineIdx].at <= e) this.pushBootLine(s, s.lines[s.lineIdx++]);
    if (s.opts.failAtS !== undefined && e >= s.opts.failAtS) {
      this.fail(s, s.opts.failKind ?? 'powershell');
      return;
    }
    const v = bootView(s.plan, e);
    if (v.phase === 'live') this.goLive(s);
    else s.phase = v.phase;
  }

  private goLive(s: SessionRt) {
    s.phase = 'live';
    s.phaseT = this.t;
    const rng = createRng(s.seed);
    const clock = () => ({ epoch: this.epoch0 + this.t, sessionT: this.t - s.t0 - s.procOffset });
    if (s.plan.kind === 'llm') {
      s.llm = new LlmSim(llmConfig(s.model, { big: s.opts.big }), rng, this.console, clock);
    } else {
      s.image = new ImageSim(imageConfig(s.opts.crashOnJob), rng, this.console, () => this.epoch0 + this.t);
    }
  }

  private stepLive(s: SessionRt, h: number) {
    s.llm?.step(h);
    if (s.image) {
      s.image.step(h);
      if (s.image.crashed) this.fail(s, 'rocm-crash');
    }
  }

  private fail(s: SessionRt, kind: FaultKind) {
    // A process that dies while loading reports the load steps with the failing one marked.
    const loading = s.phase === 'starting' || s.phase === 'loading';
    const steps = loading ? failSteps(bootView(s.plan, this.t - s.t0).steps) : null;
    s.phase = 'fault';
    s.phaseT = this.t;
    s.faultAt = this.t;
    s.fault = { ...makeFault(kind, 0), ...(steps ? { steps } : {}) };
    s.image?.abort();
    const lines = kind === 'rocm-crash' ? [...ROCM_CRASH_STDERR, `sd-server HIP exited with code ${ROCM_EXIT_CODE}.`] : POWERSHELL_FAIL_STDERR;
    this.console.pushAll(lines);
    this.pendingLaunch = null;
  }

  private finishStop(s: SessionRt) {
    this.console.push(this.stamp(s, 'srv', 'stopped'));
    this.recordLast(s, 'stopped', s.stopT, this.t);
    this.rt = null;
    const next = this.pendingLaunch;
    this.pendingLaunch = null;
    if (next) this.launch(next);
  }

  /** Remember how a session went, for the idle launcher's "last session" line. */
  private recordLast(s: SessionRt, ended: 'stopped' | 'fault', upEndT: number, endedAt: number) {
    const rec: LastRec = {
      slot: s.slot,
      model: { ...s.model },
      uptimeS: Math.max(0, Math.round(upEndT - s.t0)),
      ended,
      endedAt,
    };
    if (s.llm) {
      rec.requests = s.llm.totals.requests;
      rec.generatedTokens = s.llm.totals.generatedTokens;
      const m = s.llm.medianDecodeTps();
      if (m > 0) rec.decodeTps = r1(m);
    }
    if (s.image) {
      rec.images = s.image.images;
      const m = s.image.medianSeconds();
      if (m > 0) rec.secondsPerImage = r1(m);
    }
    this.lastRec = rec;
  }

  private pushBootLine(s: SessionRt, l: BootLine) {
    if (l.proc >= 0 && s.plan.kind === 'llm') this.console.push(`${llamaTs(l.proc)} ${l.level} ${l.text}`);
    else this.console.push(l.text);
  }

  private stamp(s: SessionRt, ch: string, msg: string): string {
    if (s.plan.kind === 'llm') return `${llamaTs(Math.max(0, this.t - s.t0 - s.procOffset))} I ${ch}  ${msg}`;
    return `[INFO ] ${ch} - ${msg}`;
  }

  // ---- machine resources ---------------------------------------------------------------------------

  /** What the running session asks of the GPU right now (baseline NOT included). */
  private sessionDemand(): VramLayer[] {
    const s = this.rt;
    if (!s) return [];
    switch (s.phase) {
      case 'loading':
        return bootView(s.plan, this.t - s.t0).layers;
      case 'live': {
        const out = s.layers.map((l) => ({ ...l }));
        const buf = out.find((l) => l.id === 'buffers');
        if (buf) {
          // Image slots: activation buffers swell while a job runs and shrink back between jobs.
          if (s.image) buf.gib = r2(buf.gib * (0.3 + 0.7 * s.act));
          if (s.swell > 0.002) buf.gib = r2(buf.gib + s.swell);
        }
        return out;
      }
      case 'stopping': {
        const p = clamp((this.t - s.phaseT) / STOP_S, 0, 1);
        return s.layers.map((l) => ({ ...l, gib: r2(l.gib * (1 - p)) })).filter((l) => l.gib > 0.004);
      }
      default:
        return []; // starting, fault: nothing of ours is resident
    }
  }

  /** Recompute what is resident on the device (baseline first, then the session) and what spilled. */
  private refreshVram() {
    const base: VramLayer = { id: 'other', label: 'other', gib: r2(this.baseline) };
    const res = resolveOverflow([base, ...this.sessionDemand()], VRAM_TOTAL);
    this.layersNow = res.layers;
    this.sessionNow = res.layers.slice(1);
    this.spillNow = res.spillMiB;
  }

  /** A session that is "already running" at t=0 also has a past: fill the history with its steady state. */
  private backfillVram(s: SessionRt, seconds: number) {
    const n = Math.min(HIST, Math.floor(seconds));
    if (n <= 0) return;
    const steady = resolveOverflow([{ id: 'other', label: 'other', gib: r2(this.baseline) }, ...s.layers], VRAM_TOTAL).layers;
    const sums = idSums(steady);
    let total = 0;
    for (const id of LAYER_IDS) {
      const v = r2(sums[id]);
      total += v;
      this.layerHist[id].fill(v, HIST - n);
    }
    this.vramHist.fill(r2(total), HIST - n);
  }

  private stepResources(h: number) {
    const s = this.rt;
    if (s?.image) {
      const target = s.image.load();
      s.act += (target - s.act) * (1 - Math.exp(-h / 0.8));
    }
    // Spill scenario: compute-buffer demand follows the prefill progress, then relaxes back.
    if (s && s.phase === 'live' && s.opts.spill && s.llm) {
      const f = s.llm.prefillFraction();
      const target = f >= 0 ? SWELL_PEAK_GIB * f * f : 0;
      s.swell += (target - s.swell) * (1 - Math.exp(-h / (target > s.swell ? 1.2 : 9)));
    }
    // Another process taking or freeing VRAM.
    while (this.baselineEvents.length && this.baselineEvents[0].at <= this.t) this.baselineTarget = this.baselineEvents.shift()!.gib;
    this.baseline += (this.baselineTarget - this.baseline) * (1 - Math.exp(-h / BASELINE_TAU_S));
    if (Math.abs(this.baselineTarget - this.baseline) < 0.0005) this.baseline = this.baselineTarget;

    this.refreshVram();
    this.recordVram(h);

    // CPU and RAM: smooth approach to a phase-dependent target plus a little seeded noise.
    let cpuT = 2.5;
    let ramT = RAM_BASE;
    if (s) {
      const foot = ramFootprintGiB(s.model);
      switch (s.phase) {
        case 'starting':
          cpuT = 9;
          break;
        case 'loading': {
          const v = bootView(s.plan, this.t - s.t0);
          const w = v.steps.find((x) => x.id === 'weights');
          cpuT = w?.state === 'active' ? 28 : 15;
          ramT = RAM_BASE + foot * clamp(v.fraction * 1.3, 0, 1);
          break;
        }
        case 'live': {
          ramT = RAM_BASE + foot;
          if (s.llm) cpuT = s.llm.activity === 'prefill' ? 13 : s.llm.activity === 'decode' ? 6 : 2.5;
          else if (s.image) cpuT = s.image.activity === 'generating' ? 5 : 2;
          break;
        }
        case 'stopping':
          cpuT = 10;
          ramT = RAM_BASE + foot * (1 - clamp((this.t - s.phaseT) / STOP_S, 0, 1));
          break;
      }
    }
    const a = 1 - Math.exp(-h / 1.2);
    this.cpu += (cpuT - this.cpu) * a + this.rng.gauss() * 0.35;
    this.cpu = clamp(this.cpu, 0.5, 60);
    this.ram += (ramT - this.ram) * (1 - Math.exp(-h / 1.6)) + this.rng.gauss() * 0.01;
  }

  /** 1 Hz history of the total and of every layer id (same cadence and length). */
  private recordVram(h: number) {
    const cur = idSums(this.layersNow);
    for (const id of LAYER_IDS) this.layerAcc[id] += cur[id] * h;
    this.vramAccT += h;
    while (this.vramAccT >= 1) {
      let total = 0;
      for (const id of LAYER_IDS) {
        const v = r2(this.layerAcc[id] / this.vramAccT);
        total += v;
        const arr = this.layerHist[id];
        arr.push(v);
        if (arr.length > HIST) arr.shift();
      }
      this.vramHist.push(r2(total));
      if (this.vramHist.length > HIST) this.vramHist.shift();
      this.vramAccT -= 1;
      for (const id of LAYER_IDS) this.layerAcc[id] = cur[id] * this.vramAccT;
    }
  }

  // ---- snapshot -------------------------------------------------------------------------------------

  snapshot(): ViewModel {
    const s = this.rt;
    const used = r2(this.layersNow.reduce((a, l) => a + l.gib, 0));
    const layerHistory: Partial<Record<VramLayerId, number[]>> = {};
    for (const id of LAYER_IDS) if (this.layerHist[id].some((v) => v > 0)) layerHistory[id] = this.layerHist[id].slice();
    const vram: GpuMemory = {
      device: 'RX 9070 XT',
      totalGiB: VRAM_TOTAL,
      usedGiB: used,
      layers: this.layersNow.map((l) => ({ ...l })),
      spillMiB: this.spillNow,
      history: this.vramHist.slice(),
      layerHistory,
      baselineGiB: r2(this.baseline),
      warnBelowGiB: VRAM_WARN,
    };
    const system: SystemStats = {
      ramUsedGiB: r1(this.ram),
      ramTotalGiB: RAM_TOTAL,
      ramType: RAM_TYPE,
      cpuName: '9950X3D',
      cpuPct: Math.round(this.cpu),
    };
    let session: Session | null = null;
    if (s) {
      const e = (s.phase === 'fault' ? s.faultAt : this.t) - s.t0;
      let loading: LoadProgress | null = null;
      if (s.phase === 'starting' || s.phase === 'loading') {
        const v = bootView(s.plan, e);
        loading = { steps: v.steps, fraction: r2(v.fraction), elapsedS: r1(e) };
      }
      let fault: Fault | null = null;
      if (s.fault) {
        fault = { ...s.fault, logTail: s.fault.logTail.slice(), sinceS: r1(this.t - s.faultAt) };
        if (s.fault.steps) fault.steps = s.fault.steps.map((x) => ({ ...x }));
      }
      session = {
        slot: s.slot,
        model: { ...s.model },
        phase: s.phase,
        uptimeS: Math.max(0, r1(e)),
        endpoint: { host: '127.0.0.1', port: s.port },
        apiKeySet: s.apiKey,
        loading,
        fault,
        llm: s.llm ? s.llm.toLive() : null,
        image: s.image ? s.image.toLive() : null,
      };
    }
    let lastSession: LastSession | null = null;
    if (this.lastRec) {
      const { endedAt, ...rest } = this.lastRec;
      lastSession = { ...rest, model: { ...rest.model }, endedAgoS: Math.max(0, r1(this.t - endedAt)) };
    }
    return {
      now: this.epoch0 + this.t,
      slots: this.slotsSnap,
      selected: s ? s.slot : this.selected,
      session,
      vram,
      system,
      lastSession,
      host: { ...this.host },
      console: this.console.snapshot(),
    };
  }
}
