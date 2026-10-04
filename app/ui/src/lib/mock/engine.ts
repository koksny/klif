// The mock core: a deterministic state machine that produces a fresh ViewModel snapshot on demand and implements
// the engine actions. 0.3 shape: N configured Systems (local ones from "klif.toml", Systems of remote nodes,
// external servers) running at the same time, each with its own session, sharing GPUs. Time is simulated
// (seconds since the scenario started); the reactive wrapper (lib/state/player.svelte.ts) feeds it wall-clock
// deltas times the speed factor. Engine actions throw Error(<one sentence>) when the real core would refuse.
import type {
  CommandView,
  ConfigInfo,
  DownloadInfo,
  Fault,
  GpuMemory,
  HostInfo,
  LastSession,
  LlmClass,
  LoadProgress,
  MachineStats,
  ModelRef,
  NodeView,
  Phase,
  PresetDetail,
  PresetInfo,
  PresetSpec,
  RecordEntry,
  RecordEvent,
  RecordHistoryLine,
  RecordMetric,
  RecordModel,
  RecordValue,
  Session,
  System,
  SystemId,
  SystemKind,
  SystemStatus,
  ViewModel,
  VramLayer,
  VramLayerId,
} from '../model/types';
import { specNumberProblem } from '../model/presets';
import { bootLines, bootView, planBoot, type BootLine, type BootPlan, type LogStyle } from './boot';
import {
  adapterOf,
  availabilityOf,
  buildCommand,
  expectedLayers,
  factsOf,
  fitLayers,
  gpuList,
  isExternal,
  maskSpec,
  modelRefOf,
  paramViews,
  presetFromRecommendation,
  presetInfo,
  presetKind,
  recDef,
  recommendationInfos,
  specHash,
  totalGiB,
  GIB,
  MOCK_KEY_MASK,
  type CmdCtx,
} from './catalog';
import { ConsoleBuf, llamaTs } from './consoleBuf';
import { EXIT_FAIL_STDERR, failSteps, makeFault, ROCM_CRASH_STDERR, ROCM_EXIT_CODE, type FaultKind } from './faults';
import { GenericSim } from './genericSim';
import { GpuPower, type PowerCfg } from './gpupower';
import { browserHost } from './host';
import { ImageSim } from './imageSim';
import { LlmSim } from './llmSim';
import { bootDurations, familyOf, imageConfig, llmConfig, ramFootprintGiB } from './profiles';
import { clamp, createRng, hashSeed, type Rng } from './rng';
import type { GpuDef, World, WorldNode } from './world';
import { MOCK_HARDWARE, MOCK_SUGGESTIONS } from './hardware';
import { mockRecordSet, roundMetric } from './records';

const SUB = 0.5; // max simulation sub-step in seconds
const STOP_S = 2.4;
/** VRAM held by the driver and other processes: visible on the device with nothing loaded. */
const BASE_VRAM = 0.12;
const VRAM_WARN = 0.15;
const RAM_TOTAL = 93.6;
const RAM_BASE = 24.0;
const RAM_TYPE = 'DDR5';
const HIST = 300;
const MOCK_KEY = 'klif-mock-key-not-a-secret';
/** Peak extra compute-buffer demand during a long prefill in the spill scenario (GiB). */
const SWELL_PEAK_GIB = 0.93;
const BASELINE_TAU_S = 4;
/** Simulated download speed (bytes per second). */
const DOWNLOAD_BPS = 420 * 1024 * 1024;
/** The core keeps the last 8 broken records for the "new record" moment. */
const MAX_RECORD_EVENTS = 8;

const LAYER_IDS: VramLayerId[] = ['other', 'weights', 'kv', 'buffers', 'draft', 'projector'];
/** When demand exceeds the edge, the allocator demotes these to shared memory first (compute buffers, KV...). */
const DEMOTE_ORDER: VramLayerId[] = ['buffers', 'kv', 'draft', 'projector', 'weights'];

export interface EngineHooks {
  toast(text: string): void;
  toggleConsole(open?: boolean): void;
  /** Panel mode in a browser: switch the layout override between the mini panel and the full window. */
  togglePanel(): void;
  openTune(system?: SystemId, opts?: { add?: boolean }): void;
  openRecords(): void;
  copy(text: string, toastText: string): void;
  openUrl(url: string): void;
}

/** "A", "A and B", "A, B and C" (the core's conflicts::names). */
function names(labels: string[]): string {
  if (labels.length <= 1) return labels[0] ?? '';
  return `${labels.slice(0, -1).join(', ')} and ${labels[labels.length - 1]}`;
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
  /**
   * LLM: the GPU powers down (AMD ULPS, device state D3) ~12 s after the last request and the next request
   * waits ~4 s while the VRAM is restored from system RAM. See gpupower.ts and GpuMemory.dormant.
   */
  dormant?: DormantOpts;
  /** Stop the Systems that must stop first (Launch with conflicts). */
  stopOthers?: boolean;
}

export interface DormantOpts {
  /** The session's allocations on the device (baseline NOT included), as measured on the real machine. */
  layers: VramLayer[];
  /** Seconds between request arrivals (default 40). */
  periodS?: number;
  /** The first request arrives this long after t=0 (default 4). */
  firstInS?: number;
  power?: Partial<PowerCfg>;
}

/** The previous session of a System, kept by the engine; endedAgoS is derived from the clock at snapshot time. */
export type LastRec = Omit<LastSession, 'endedAgoS'> & { endedAt: number };

interface SessionRt {
  sys: MSys;
  model: ModelRef;
  cmd: CommandView;
  preset: string;
  gpu: GpuSim;
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
  generic: GenericSim | null;
  fault: Fault | null;
  faultAt: number;
  /** Time at which stop() was requested (uptime ends here). */
  stopT: number;
  /** Session layers (baseline NOT included), already fitted by the loader. */
  layers: VramLayer[];
  /** Eased 0..1 load of the activation buffers (image Systems). */
  act: number;
  /** Extra compute-buffer demand in GiB (spill scenario). */
  swell: number;
  /** The GPU's power state (dormant scenarios), else null. */
  power: GpuPower | null;
  seed: number;
  /** Peak VRAM this session committed. */
  peakGiB: number;
}

interface Pending {
  opts: LaunchOpts;
  /** Systems that must be gone before this one starts. */
  waitFor: SystemId[];
}

/** One configured System (a tab). */
interface MSys {
  /** Full id: "s1", or "<node>/<id>" for a System of a remote node. */
  id: SystemId;
  /** Id on its own node. */
  local: string;
  label: string;
  kind: SystemKind;
  cls?: LlmClass;
  preset?: string;
  params: Record<string, string>;
  exclusive: boolean;
  node?: string;
  rt: SessionRt | null;
  last: LastRec | null;
  /** An external server's simulation (the preset has an endpoint). */
  ext: GenericSim | null;
  /** The ring buffer of the last session's console (kept after it ends). */
  console: ConsoleBuf;
  pending: Pending | null;
  /** Relaunch with these options once the stop finished (Restart). */
  relaunch: LaunchOpts | null;
}

const r1 = (v: number) => Math.round(v * 10) / 10;
const r2 = (v: number) => Math.round(v * 100) / 100;

function idSums(layers: VramLayer[]): Record<VramLayerId, number> {
  const out: Record<VramLayerId, number> = { other: 0, weights: 0, kv: 0, buffers: 0, draft: 0, projector: 0 };
  for (const l of layers) out[l.id] += l.gib;
  return out;
}

interface Tagged {
  l: VramLayer;
  owner: SystemId | null;
}

/**
 * Residency. The device cannot hold more than totalGiB: what the layers ask for beyond the edge is demoted
 * to shared system memory (spillMiB) and the resident layers fill the edge exactly (free = 0). The first
 * layer is the baseline and is never demoted.
 */
function resolveOverflow(demand: Tagged[], total: number): { items: Tagged[]; spillMiB: number } {
  const out = demand.map((d) => ({ owner: d.owner, l: { ...d.l, gib: r2(d.l.gib) } }));
  const sum = () => out.reduce((a, d) => a + d.l.gib, 0);
  let over = sum() - total;
  if (over <= 0.004) return { items: out, spillMiB: 0 };
  const spillMiB = Math.round(over * 1024);
  for (const id of DEMOTE_ORDER) {
    for (let i = 1; i < out.length && over > 0; i++) {
      const d = out[i];
      if (d.l.id !== id) continue;
      const take = Math.min(d.l.gib, over);
      d.l.gib = r2(d.l.gib - take);
      over -= take;
    }
  }
  // Rounding: land exactly on the edge.
  const diff = r2(total) - sum();
  if (Math.abs(diff) > 1e-9) {
    const d = out.slice(1).find((x) => x.l.id === 'buffers') ?? out.slice(1).find((x) => x.l.gib > 0);
    if (d) d.l.gib = Math.max(0, r2(d.l.gib + diff));
  }
  return { items: out.filter((d, i) => i === 0 || d.l.gib > 0.004), spillMiB };
}

/** One GPU: its baseline (what other processes hold), the sessions on it, the history the skins draw. */
class GpuSim {
  baseline = BASE_VRAM;
  baselineTarget = BASE_VRAM;
  events: { at: number; gib: number }[] = [];
  hist: number[] = Array.from({ length: HIST }, () => BASE_VRAM);
  layerHist: Record<VramLayerId, number[]> = {
    other: Array.from({ length: HIST }, () => BASE_VRAM),
    weights: new Array<number>(HIST).fill(0),
    kv: new Array<number>(HIST).fill(0),
    buffers: new Array<number>(HIST).fill(0),
    draft: new Array<number>(HIST).fill(0),
    projector: new Array<number>(HIST).fill(0),
  };
  layerAcc: Record<VramLayerId, number> = { other: 0, weights: 0, kv: 0, buffers: 0, draft: 0, projector: 0 };
  accT = 0;
  /** What is resident right now (baseline first), each layer tagged with the System that owns it. */
  resident: Tagged[] = [{ l: { id: 'other', label: 'other', gib: BASE_VRAM }, owner: null }];
  /** What the sessions ask for, before dormancy (their allocations). */
  alloc: Tagged[] = [{ l: { id: 'other', label: 'other', gib: BASE_VRAM }, owner: null }];
  spillMiB = 0;
  power: GpuPower | null = null;

  constructor(
    readonly def: GpuDef,
    readonly node?: string,
  ) {}

  get id() {
    return this.def.id;
  }
  get total() {
    return this.def.totalGiB;
  }

  usedGiB(): number {
    return r2(this.resident.reduce((a, d) => a + d.l.gib, 0));
  }

  /** What the baseline plus every other session on this device occupies (for fitting a new session). */
  occupiedBy(except: MSys | null): number {
    return this.alloc.reduce((a, d) => a + (d.owner === (except?.id ?? '\u0000') ? 0 : d.l.gib), 0);
  }

  refresh(sessions: SessionRt[], demandOf: (s: SessionRt) => VramLayer[]) {
    const demand: Tagged[] = [{ l: { id: 'other', label: 'other', gib: r2(this.baseline) }, owner: null }];
    for (const s of sessions) for (const l of demandOf(s)) demand.push({ l, owner: s.sys.id });
    const res = resolveOverflow(demand, this.total);
    this.alloc = res.items;
    this.spillMiB = res.spillMiB;
    const live = sessions.find((s) => s.phase === 'live' && s.power);
    this.power = live?.power ?? null;
    if (this.power && this.power.dormant) {
      const scaled = this.power.resident(res.items.map((d) => d.l));
      this.resident = res.items.map((d, i) => ({ owner: d.owner, l: scaled[i] }));
    } else {
      this.resident = res.items;
    }
  }

  record(h: number) {
    const cur = idSums(this.resident.map((d) => d.l));
    for (const id of LAYER_IDS) this.layerAcc[id] += cur[id] * h;
    this.accT += h;
    while (this.accT >= 1) {
      let total = 0;
      for (const id of LAYER_IDS) {
        const v = r2(this.layerAcc[id] / this.accT);
        total += v;
        const arr = this.layerHist[id];
        arr.push(v);
        if (arr.length > HIST) arr.shift();
      }
      this.hist.push(r2(total));
      if (this.hist.length > HIST) this.hist.shift();
      this.accT -= 1;
      for (const id of LAYER_IDS) this.layerAcc[id] = cur[id] * this.accT;
    }
  }

  stepBaseline(t: number, h: number) {
    while (this.events.length && this.events[0].at <= t) this.baselineTarget = this.events.shift()!.gib;
    this.baseline += (this.baselineTarget - this.baseline) * (1 - Math.exp(-h / BASELINE_TAU_S));
    if (Math.abs(this.baselineTarget - this.baseline) < 0.0005) this.baseline = this.baselineTarget;
  }

  /** A session that is "already running" at t=0 also has a past: fill the history with its steady state. */
  backfill(layers: VramLayer[], seconds: number) {
    const n = Math.min(HIST, Math.floor(seconds));
    if (n <= 0) return;
    const steady = resolveOverflow([{ l: { id: 'other', label: 'other', gib: r2(this.baseline) }, owner: null }, ...layers.map((l) => ({ l, owner: 'x' }))], this.total).items;
    const sums = idSums(steady.map((d) => d.l));
    let total = 0;
    for (const id of LAYER_IDS) {
      const v = r2(sums[id]);
      total += v;
      this.layerHist[id].fill(v, HIST - n);
    }
    this.hist.fill(r2(total), HIST - n);
  }

  memory(layersFor: SystemId | null): GpuMemory {
    const used = this.usedGiB();
    const sessionSum = this.resident.reduce((a, d) => a + (d.owner ? d.l.gib : 0), 0);
    const other: VramLayer = { id: 'other', label: 'other', gib: r2(Math.max(0, used - sessionSum)) };
    const mine = layersFor ? this.resident.filter((d) => d.owner === layersFor).map((d) => ({ ...d.l })) : [];
    const layerHistory: Partial<Record<VramLayerId, number[]>> = {};
    for (const id of LAYER_IDS) if (this.layerHist[id].some((v) => v > 0)) layerHistory[id] = this.layerHist[id].slice();
    const dormantNow = this.power && this.power.dormant;
    return {
      id: this.def.id,
      name: this.def.name,
      device: this.def.name,
      totalGiB: this.total,
      usedGiB: used,
      layers: [other, ...mine],
      spillMiB: this.spillMiB,
      history: this.hist.slice(),
      layerHistory,
      baselineGiB: r2(this.baseline),
      warnBelowGiB: VRAM_WARN,
      dormant: dormantNow ? this.power!.view(0) : null,
    };
  }
}

interface Download {
  id: string;
  file: string;
  total: number;
  done: number;
  state: DownloadInfo['state'];
  error?: string;
  endedAt?: number;
}

export class MockEngine {
  /** Simulated seconds since the scenario started. */
  t = 0;
  selected: SystemId;
  /** Window facts reported to the skins (set by the scenario builder from ?frameless=1). */
  host: HostInfo = browserHost(false);

  private systems: MSys[];
  /** Local presets, then each node's by node id. */
  private presets: Record<string, PresetSpec>;
  private nodePresets = new Map<string, Record<string, PresetSpec>>();
  private nodes: WorldNode[];
  private gpus: GpuSim[] = [];
  private modelsDir?: string;
  private apiKeySet: boolean;
  private onConflict: ConfigInfo['onConflict'];
  /** `[ui] record_moment` of the mock machine. */
  private recordMoment = true;
  private downloaded: Set<string>;
  private downloads: Download[] = [];
  private rng: Rng;
  private seed: number;
  private sessionNo = 0;
  private cfgRev = 1;
  private fileHash = new Map<string, string>();

  private cpu = 3;
  private ram = RAM_BASE;
  private nodeCpu = 4;

  // Records: this machine's and the node's best values, the last broken ones and the climb behind them. Entries are
  // replaced, never mutated, so a published snapshot keeps the values it was taken with.
  private records: RecordEntry[] = [];
  private recordEvents: RecordEvent[] = [];
  private recordLines: RecordHistoryLine[] = [];
  private recordsRev = 1;
  /** Scenario `record`: a running LLM System breaks its decode record every `every` simulated seconds. */
  private recordRaise: { every: number; next: number } | null = null;

  constructor(
    private readonly epoch0: number,
    seed: number,
    private readonly hooks: EngineHooks,
    world: World,
  ) {
    this.seed = seed;
    this.rng = createRng(seed);
    this.presets = JSON.parse(JSON.stringify(world.presets)) as Record<string, PresetSpec>;
    this.nodes = world.nodes.map((n) => ({ ...n, gpus: n.gpus.map((g) => ({ ...g })) }));
    for (const n of this.nodes) this.nodePresets.set(n.id, JSON.parse(JSON.stringify(n.presets)) as Record<string, PresetSpec>);
    this.modelsDir = world.modelsDir;
    this.apiKeySet = world.apiKeySet;
    this.onConflict = world.onConflict;
    this.downloaded = new Set(world.downloaded);
    for (const g of world.gpus) this.gpus.push(new GpuSim(g));
    for (const n of this.nodes) for (const g of n.gpus) this.gpus.push(new GpuSim(g, n.id));
    this.systems = world.systems.map((w) => ({
      id: w.node ? `${w.node}/${w.id}` : w.id,
      local: w.id,
      label: w.label,
      kind: w.kind,
      cls: w.class,
      preset: w.preset,
      params: { ...(w.params ?? {}) },
      exclusive: !!w.exclusive,
      node: w.node,
      rt: null,
      last: null,
      ext: null,
      console: new ConsoleBuf(),
      pending: null,
      relaunch: null,
    }));
    this.selected = world.selected && this.systems.some((s) => s.id === world.selected) ? world.selected : (this.systems[0]?.id ?? '');
    for (const s of this.systems) this.syncExternal(s);
    // A fresh install has no records yet.
    if (world.records !== false) {
      const set = mockRecordSet(epoch0);
      this.records = set.entries;
      this.recordEvents = set.events;
      this.recordLines = set.history;
    }
  }

  // ---- public control (scenarios, player) -----------------------------------------------------------

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
    return this.systems.some((s) => s.rt !== null);
  }

  /** Phase of the selected System's session (scenario predicates). */
  get phase(): Phase | null {
    return this.sysOrThrow(this.selected).rt?.phase ?? null;
  }

  /** The ModelRef a System would run now (scenario scripts). */
  systemModel(id: SystemId): ModelRef {
    const s = this.sysOrThrow(id);
    return { ...this.resolve(s).model };
  }

  /** Scenario script: pretend a session ended `endedAgoS` seconds before t=0. */
  setLastSession(system: SystemId, rec: Omit<LastSession, 'endedAgoS' | 'system'>, endedAgoS: number) {
    const s = this.sysOrThrow(system);
    s.last = { ...rec, system, model: { ...rec.model }, endedAt: this.t - endedAgoS };
  }

  /** Scenario script: another process takes (or frees) VRAM on the first local GPU at simulated time `atT`. */
  scheduleBaseline(atT: number, gib: number) {
    const g = this.gpus.find((x) => !x.node) ?? this.gpus[0];
    g.events.push({ at: atT, gib });
    g.events.sort((a, b) => a.at - b.at);
  }

  /** Scenario script: make a node reachable / unreachable. */
  setNodeState(node: string, state: WorldNode['state'], error?: string) {
    const n = this.nodes.find((x) => x.id === node);
    if (!n) return;
    n.state = state;
    n.error = state === 'online' ? undefined : (error ?? 'Connection timed out.');
    n.latencyMs = state === 'online' ? 4 : undefined;
    // A node that is not online publishes no records.
    this.recordsRev++;
  }

  /** Scenario `record`: from now on a running LLM System breaks its decode record every `everyS` simulated seconds. */
  raiseRecords(everyS = 12) {
    this.recordRaise = { every: everyS, next: this.t + everyS };
  }

  // ---- lookups ---------------------------------------------------------------------------------------

  private sys(id: SystemId | undefined): MSys | undefined {
    return this.systems.find((s) => s.id === (id ?? this.selected));
  }

  private sysOrThrow(id: SystemId | undefined): MSys {
    const s = this.sys(id);
    if (!s) throw new Error(id ? `There is no System "${id}".` : 'No System is selected.');
    return s;
  }

  private presetMap(node?: string): Record<string, PresetSpec> {
    return node ? (this.nodePresets.get(node) ?? {}) : this.presets;
  }

  private specOf(s: MSys): PresetSpec | undefined {
    return s.preset ? this.presetMap(s.node)[s.preset] : undefined;
  }

  private nodeOfSys(s: MSys): WorldNode | undefined {
    return s.node ? this.nodes.find((n) => n.id === s.node) : undefined;
  }

  private unreachable(s: MSys): boolean {
    const n = this.nodeOfSys(s);
    return !!n && n.state !== 'online';
  }

  private ctx(s: MSys | null, node?: string): CmdCtx {
    const n = s?.node ?? node;
    return { params: s?.params, apiKeySet: n ? false : this.apiKeySet, modelsDir: n ? undefined : this.modelsDir };
  }

  private gpuFor(s: MSys, spec: PresetSpec | undefined): GpuSim {
    const pool = this.gpus.filter((g) => g.node === s.node);
    const want = spec ? gpuList(spec, pool[0]?.id ?? '')[0] : pool[0]?.id;
    return pool.find((g) => g.id === want) ?? pool[0] ?? this.gpus[0];
  }

  /** The command, model, availability and expected layers of a System's preset, resolved now. */
  private resolve(s: MSys) {
    const spec = this.specOf(s);
    const gpu = this.gpuFor(s, spec);
    if (!spec) {
      const model: ModelRef = { name: '', quant: '', engine: '', backend: '', device: gpu.def.name };
      return { spec: undefined, cmd: undefined, model, avail: { availability: 'ready' as const, reason: undefined as string | undefined }, layers: [] as VramLayer[], gpu };
    }
    const cmd = buildCommand(spec, this.ctx(s));
    const model = modelRefOf(spec, cmd, s.params, gpu.def.name);
    const avail = availabilityOf(spec, cmd);
    return { spec, cmd, model, avail, layers: isExternal(spec) ? [] : expectedLayers(spec, model), gpu };
  }

  // ---- actions ---------------------------------------------------------------------------------------

  select(system: SystemId) {
    if (!this.sys(system)) throw new Error(`There is no System "${system}".`);
    this.selected = system;
  }

  launch(system?: SystemId, opts: LaunchOpts = {}) {
    const s = this.sysOrThrow(system);
    this.requireControl(s);
    if (s.pending) throw new Error(`${s.label} is about to launch.`);
    if (s.rt && s.rt.phase !== 'fault') throw new Error(`${s.label} is already running.`);
    const r = this.resolve(s);
    if (!r.spec) throw new Error(`${s.label} has no preset. Choose one in Tune.`);
    if (isExternal(r.spec)) throw new Error('External server: start it where it runs.');
    if (r.avail.availability !== 'ready') throw new Error(`${s.label} cannot start: ${r.avail.reason ?? r.avail.availability}`);
    const conflicts = this.conflictsFor(s).ids;
    if (conflicts.length) {
      const names = conflicts.map((c) => this.sysOrThrow(c).label);
      if (conflicts.some((c) => this.sysOrThrow(c).pending)) throw new Error(`${names.join(', ')} is about to launch.`);
      const busy = conflicts.map((c) => this.sysOrThrow(c)).filter((c) => c.rt?.phase === 'live' && this.activityOf(c) > 0);
      const auto = this.onConflict === 'stop' && busy.length === 0;
      if (!opts.stopOthers && !auto) throw new Error(`${names.join(' and ')} must stop first. Use "Stop ${names.join(' & ')} & launch".`);
      for (const c of conflicts) {
        const o = this.sysOrThrow(c);
        if (o.rt?.phase === 'fault') this.endFault(o);
        else if (o.rt && o.rt.phase !== 'stopping') this.beginStop(o);
      }
      s.pending = { opts: { ...opts, stopOthers: false }, waitFor: conflicts };
      this.followLaunch(s);
      return;
    }
    this.startSession(s, opts);
  }

  stop(system?: SystemId) {
    const s = this.sysOrThrow(system);
    this.requireControl(s);
    if (s.pending) {
      s.pending = null;
      return;
    }
    if (!s.rt) throw new Error(`${s.label} is not running.`);
    if (s.rt.phase === 'fault') {
      this.endFault(s);
      return;
    }
    if (s.rt.phase === 'stopping') return;
    s.relaunch = null;
    this.beginStop(s);
  }

  stopAll() {
    let n = 0;
    for (const s of this.systems) {
      if (s.node || !s.rt) continue;
      if (s.rt.phase === 'fault') this.endFault(s);
      else if (s.rt.phase !== 'stopping') this.beginStop(s);
      s.relaunch = null;
      s.pending = null;
      n++;
    }
    if (!n) throw new Error('Nothing is running.');
  }

  /**
   * Restart = stop, then launch again with the same options (native order of refusals). It never stops another
   * System (SPEC 16.8): a port or exclusive-GPU conflict refuses it before anything is touched, and the relaunch
   * re-checks those rules (native fire()) instead of stopping anyone.
   */
  restart(system?: SystemId) {
    const s = this.sysOrThrow(system);
    this.requireControl(s);
    const spec = this.specOf(s);
    if (spec && isExternal(spec)) throw new Error(`${s.label} is an external server; restart it where it runs.`);
    if (s.pending) throw new Error(`${s.label} is about to launch.`);
    if (!s.rt) throw new Error(`${s.label} is not running.`);
    if (!spec) throw new Error(`${s.label} has no preset to restart with.`);
    const hard = this.hardConflicts(s);
    if (hard.size) {
      const labels = [...hard.keys()].map((id) => this.sysOrThrow(id).label);
      const whys = [...new Set(hard.values())];
      const n = names(labels);
      throw new Error(`${s.label} cannot restart while ${n} ${labels.length === 1 ? 'is' : 'are'} running (${whys.join(', ')}). Stop ${n} first.`);
    }
    const opts: LaunchOpts = { ...s.rt.opts, skipBoot: false, uptimeOffsetS: 0, stopOthers: false };
    if (s.rt.phase === 'fault') {
      this.endFault(s);
      this.fire(s, opts);
      return;
    }
    s.relaunch = opts;
    if (s.rt.phase !== 'stopping') this.beginStop(s);
  }

  /** Leave a faulted session and return the System to offline (the fault becomes the last session). */
  dismiss(system?: SystemId) {
    const s = this.sysOrThrow(system);
    this.requireControl(s);
    if (!s.rt) throw new Error(`${s.label} has nothing to dismiss.`);
    if (s.rt.phase !== 'fault') throw new Error(`Stop ${s.label} first.`);
    this.endFault(s);
  }

  usePreset(system: SystemId, preset: string) {
    const s = this.sysOrThrow(system);
    this.requireControl(s);
    const spec = this.presetMap(s.node)[preset];
    if (!spec) throw new Error(`There is no preset "${preset}".`);
    const kind = presetKind(spec);
    if (kind !== s.kind) throw new Error(`"${preset}" is a ${kind ?? 'generic'} preset and ${s.label} is a ${s.kind} System.`);
    s.preset = preset;
    s.params = {};
    this.syncExternal(s);
  }

  setParam(system: SystemId, name: string, value: string) {
    const s = this.sysOrThrow(system);
    this.requireControl(s);
    const spec = this.specOf(s);
    const p = spec?.params?.[name];
    if (!p) throw new Error(`${s.preset ?? 'This preset'} has no param "${name}".`);
    if (!p.choices[value]) throw new Error(`"${value}" is not a choice of ${name}.`);
    s.params = { ...s.params, [name]: value };
  }

  savePreset(id: string, preset: PresetSpec, opts: { selectFor?: SystemId; secretsFrom?: string; baseHash?: string; node?: string } = {}) {
    const node = this.routeNode(opts.node, opts.selectFor);
    this.requireEdit(node);
    if (!/^[a-z0-9][a-z0-9_-]{0,63}$/.test(id)) throw new Error('A preset id uses a-z, 0-9, - and _ (up to 64 characters, starting with a letter or digit).');
    if (/^(con|prn|aux|nul|com\d|lpt\d)$/i.test(id)) throw new Error(`"${id}" is a reserved Windows name.`);
    const bad = specNumberProblem(preset);
    if (bad) throw new Error(bad);
    const map = this.presetMap(node);
    const old = map[id];
    if (old && opts.baseHash && specHash(maskSpec(old), undefined) !== opts.baseHash) throw new Error('This preset changed on disk since you opened it. Reload it first.');
    const next: PresetSpec = JSON.parse(JSON.stringify(preset)) as PresetSpec;
    // "••••" = keep the stored secret (from the preset it was copied from, else the one being replaced).
    const from = map[opts.secretsFrom ?? id];
    for (const [k, v] of Object.entries(next.env ?? {})) {
      if (v === MOCK_KEY_MASK) {
        // The mask is not a secret: it means "keep the stored value" (also on a node; Tune refuses clear secrets).
        if (from?.env?.[k] !== undefined) next.env![k] = from.env[k];
        else delete next.env![k];
      }
    }
    map[id] = next;
    this.cfgRev++;
    if (opts.selectFor) this.usePreset(opts.selectFor, id);
    for (const s of this.systems) this.syncExternal(s);
  }

  deletePreset(id: string, node?: string) {
    this.requireEdit(node);
    const map = this.presetMap(node);
    if (!map[id]) throw new Error(`There is no preset "${id}".`);
    const users = this.systems.filter((s) => s.node === node && s.preset === id);
    if (users.length) throw new Error(`Preset "${id}" is used by ${users.map((u) => u.label).join(', ')}.`);
    delete map[id];
    this.cfgRev++;
  }

  addSystem(spec: { id?: string; label?: string; kind: SystemKind; class?: LlmClass; preset?: string; node?: string }) {
    this.requireEdit(spec.node);
    const mine = this.systems.filter((s) => s.node === spec.node);
    const id = spec.id ?? this.freeId(spec.kind, spec.class, mine);
    if (!/^[a-z0-9][a-z0-9_-]{0,31}$/.test(id)) throw new Error('A System id uses a-z, 0-9, - and _ (up to 32 characters).');
    if (mine.some((s) => s.local === id)) throw new Error(`There is already a System "${id}".`);
    const label = spec.label?.trim() || this.defaultLabel(spec.kind, spec.class, mine);
    if (spec.preset) {
      const p = this.presetMap(spec.node)[spec.preset];
      if (!p) throw new Error(`There is no preset "${spec.preset}".`);
      if (presetKind(p) !== spec.kind) throw new Error(`"${spec.preset}" is not a ${spec.kind} preset.`);
    }
    const sys: MSys = {
      id: spec.node ? `${spec.node}/${id}` : id,
      local: id,
      label,
      kind: spec.kind,
      cls: spec.class,
      preset: spec.preset,
      params: {},
      exclusive: false,
      node: spec.node,
      rt: null,
      last: null,
      ext: null,
      console: new ConsoleBuf(),
      pending: null,
      relaunch: null,
    };
    // Local Systems keep file order; remote ones come after them, grouped by node.
    const lastLocal = this.systems.map((s) => !s.node).lastIndexOf(true);
    if (spec.node) this.systems.push(sys);
    else this.systems.splice(lastLocal + 1, 0, sys);
    this.syncExternal(sys);
    this.cfgRev++;
    if (!spec.node) this.selected = sys.id;
  }

  removeSystem(system: SystemId) {
    const s = this.sysOrThrow(system);
    this.requireEdit(s.node);
    if (s.rt || s.pending) throw new Error(`${s.label} is running. Stop it first.`);
    this.systems = this.systems.filter((x) => x !== s);
    if (this.selected === s.id) this.selected = this.systems[0]?.id ?? '';
    this.cfgRev++;
  }

  updateSystem(system: SystemId, patch: { label?: string; moveTo?: number; exclusive?: boolean }) {
    const s = this.sysOrThrow(system);
    this.requireEdit(s.node);
    if (patch.label !== undefined) {
      const l = patch.label.trim();
      if (!l) throw new Error('A System needs a label.');
      s.label = l;
    }
    if (patch.exclusive !== undefined) s.exclusive = patch.exclusive;
    if (patch.moveTo !== undefined) {
      const group = this.systems.filter((x) => x.node === s.node);
      const idx = clamp(Math.round(patch.moveTo), 0, group.length - 1);
      group.splice(group.indexOf(s), 1);
      group.splice(idx, 0, s);
      const rest = this.systems.filter((x) => x.node !== s.node);
      this.systems = s.node ? [...rest, ...group] : [...group, ...rest];
    }
    this.cfgRev++;
  }

  downloadRecommendation(id: string, node?: string) {
    if (node) throw new Error('Downloads for a remote node happen on that node.');
    const rec = recDef(id);
    if (!rec) throw new Error(`There is no recommendation "${id}".`);
    if (!this.modelsDir) throw new Error('Set [paths] models_dir in klif.toml first: downloads are refused until it is set.');
    if (this.downloads.some((d) => d.id === id && (d.state === 'running' || d.state === 'verifying'))) throw new Error('Already downloading.');
    this.downloads = this.downloads.filter((d) => d.id !== id);
    for (const f of rec.files) this.downloads.push({ id, file: f.name, total: Math.round(f.gib * GIB), done: 0, state: 'running' });
  }

  cancelDownload(id: string, node?: string) {
    if (node) throw new Error('Downloads for a remote node happen on that node.');
    let n = 0;
    for (const d of this.downloads) {
      if (d.id === id && (d.state === 'running' || d.state === 'verifying')) {
        d.state = 'cancelled';
        d.endedAt = this.t;
        n++;
      }
    }
    if (!n) throw new Error('That download is not running.');
  }

  adoptRecommendation(id: string, system?: SystemId, fit?: { ctx?: number; kv?: string }) {
    const rec = recDef(id);
    if (!rec) throw new Error(`There is no recommendation "${id}".`);
    const s = this.sysOrThrow(system);
    this.requireEdit(s.node);
    if (s.node) throw new Error('Recommendations are for this machine; open Tune on that node.');
    if (s.kind !== rec.kind) throw new Error(`"${rec.name}" is a ${rec.kind} model and ${s.label} is a ${s.kind} System.`);
    const info = recommendationInfos(this.presets, this.downloaded).find((r) => r.id === id);
    if (!info?.installed) throw new Error('Download it first.');
    if (info.existing) {
      this.usePreset(s.id, info.existing);
      return;
    }
    const cur = this.specOf(s);
    const base =
      cur && adapterOf(cur) === rec.adapter && !isExternal(cur) ? cur : Object.values(this.presets).find((p) => adapterOf(p) === rec.adapter && !isExternal(p) && p.command);
    const made = presetFromRecommendation(rec, base);
    // A suggestion's context and KV type (as the engine writes them: ctx, and -ctk / -ctv replaced or added).
    if (fit?.ctx !== undefined) made.spec.ctx = fit.ctx;
    if (fit?.kv && rec.adapter === 'llama.cpp') {
      const args = [...(made.spec.args ?? [])];
      for (const flag of ['-ctk', '-ctv']) {
        const i = args.indexOf(flag);
        if (i >= 0 && i + 1 < args.length) args[i + 1] = fit.kv;
        else args.push(flag, fit.kv);
      }
      made.spec.args = args;
    }
    let pid = made.id;
    for (let n = 2; this.presets[pid]; n++) pid = `${made.id}-${n}`;
    this.presets[pid] = made.spec;
    this.cfgRev++;
    this.usePreset(s.id, pid);
  }

  /**
   * Remove a junk record. `key` is the key as that machine knows it (a node's entry: without the "<node>/" prefix,
   * `node` set; the prefixed form is accepted too). A node must grant "edit".
   */
  /** Action UpdateSettings: `[ui] record_moment`. */
  updateSettings(patch: { recordMoment?: boolean }) {
    if (patch.recordMoment !== undefined) this.recordMoment = patch.recordMoment;
  }

  forgetRecord(key: string, node?: string) {
    // A node the world does not model (the records fiction of the browser mock) is treated as granting edit.
    if (node && this.nodes.some((n) => n.id === node)) this.requireEdit(node);
    const k = key.trim();
    const published = node && !k.startsWith(`${node}/`) ? `${node}/${k}` : k;
    const e = this.records.find((r) => r.key === published && r.node === node);
    if (!e) throw new Error(node ? `There is no record "${k}" on ${this.nodes.find((n) => n.id === node)?.name ?? node}.` : `There is no record "${k}" on this machine.`);
    this.records = this.records.filter((r) => r !== e);
    this.recordEvents = this.recordEvents.filter((ev) => ev.key !== e.key);
    this.recordLines = this.recordLines.filter((l) => l.key !== e.key);
    this.recordsRev++;
  }

  // ---- shell-ish actions the player forwards ----------------------------------------------------------

  endpointOf(system?: SystemId): string | null {
    const s = this.sys(system);
    if (!s) return null;
    const r = this.resolve(s);
    if (!r.cmd) return null;
    const node = this.nodeOfSys(s);
    const host = node ? node.address.split(':')[0] : r.cmd.host === '0.0.0.0' ? '127.0.0.1' : r.cmd.host;
    if (r.cmd.external) return s.kind === 'llm' ? `${r.cmd.external.replace(/\/$/, '')}/v1` : r.cmd.external;
    return s.kind === 'llm' ? `http://${host}:${r.cmd.port}/v1` : `http://${host}:${r.cmd.port}/`;
  }

  copyEndpoint(system?: SystemId) {
    const s = this.sysOrThrow(system);
    const url = this.endpointOf(s.id);
    if (!url) throw new Error('No endpoint: this System has no preset.');
    this.hooks.copy(url, 'Endpoint copied');
  }

  openEndpoint(system?: SystemId) {
    const s = this.sysOrThrow(system);
    if (!s.rt && !s.ext) throw new Error(`${s.label} is not running.`);
    if (s.rt && s.rt.phase !== 'live') throw new Error('The endpoint is not up yet.');
    const url = this.endpointOf(s.id);
    if (url) this.hooks.openUrl(url.replace(/\/v1$/, '/'));
  }

  copyApiKey() {
    if (!this.apiKeySet) throw new Error('No API key is set.');
    this.hooks.copy(MOCK_KEY, 'API key copied');
  }

  // ---- ConfigApi -------------------------------------------------------------------------------------

  presetGet(id: string, node?: string): PresetDetail | null {
    const spec = this.presetMap(node)[id];
    if (!spec) return null;
    const cmd = buildCommand(spec, this.ctx(null, node));
    const gpu = this.gpus.find((g) => g.node === node)?.def.name ?? '';
    const masked = maskSpec(spec);
    return { id, spec: masked, command: cmd, info: presetInfo(id, spec, cmd, gpu, {}, node), specHash: specHash(masked, undefined) };
  }

  /** The climb of one record (klif_records_history): every broken record of `key`, oldest first. */
  recordsHistory(key: string, metric?: RecordMetric): RecordHistoryLine[] {
    const k = key.trim();
    const e = this.records.find((r) => r.key === k);
    if (!e) throw new Error(`There is no record "${k}" on this machine.`);
    const node = e.node ? this.nodes.find((n) => n.id === e.node) : undefined;
    if (node && node.state !== 'online') throw new Error(`${node.name} is not reachable.`);
    return this.recordLines
      .filter((l) => l.key === k && l.metric !== null && (!metric || l.metric === metric))
      .sort((a, b) => a.at - b.at)
      .map((l) => ({ ...l }));
  }

  commandPreview(spec: PresetSpec, system?: SystemId): CommandView {
    const bad = specNumberProblem(spec);
    if (bad) throw new Error(bad);
    const s = system ? this.sys(system) : undefined;
    return buildCommand(spec, { ...this.ctx(s ?? null), params: s?.params });
  }

  setApiKey(key: string | null) {
    this.apiKeySet = key !== null && key !== '';
    this.cfgRev++;
  }

  // ---- internals: sessions ---------------------------------------------------------------------------

  private requireControl(s: MSys) {
    if (this.unreachable(s)) throw new Error(`${this.nodeOfSys(s)?.name ?? 'The node'} is not reachable.`);
    const n = this.nodeOfSys(s);
    if (n && !n.allow.includes('launch') && !n.allow.includes('edit')) throw new Error(`${n.name} does not allow launching.`);
  }

  private requireEdit(node: string | undefined) {
    if (!node) return;
    const n = this.nodes.find((x) => x.id === node);
    if (!n) throw new Error(`There is no node "${node}".`);
    if (n.state !== 'online') throw new Error(`${n.name} is not reachable.`);
    if (!n.allow.includes('edit')) throw new Error(`${n.name} does not allow editing: add allow = ["edit"] to its [node] section.`);
  }

  /** The node an action targets: `node` if present, else the prefix of a System id, else local. */
  private routeNode(node: string | undefined, system?: SystemId): string | undefined {
    const pre = system && system.includes('/') ? system.slice(0, system.indexOf('/')) : undefined;
    if (node && pre && node !== pre) throw new Error(`The action names node "${node}" but the System lives on "${pre}".`);
    return node ?? pre;
  }

  private freeId(kind: SystemKind, cls: LlmClass | undefined, mine: MSys[]): string {
    const base = kind === 'image' ? 'cgi' : kind === 'llm' ? '' : kind;
    if (kind === 'llm') {
      for (let n = 1; ; n++) if (!mine.some((s) => s.local === `s${n}`)) return `s${n}`;
    }
    if (!mine.some((s) => s.local === base)) return base;
    for (let n = 2; ; n++) if (!mine.some((s) => s.local === `${base}${n}`)) return `${base}${n}`;
  }

  /**
   * The label a new System gets when none is typed (native default_system_label): llm fast/deep/max -> System
   * 1/2/3, llm without class -> the next free "System N", image -> System CGI, tts/stt/video -> System TTS/STT/
   * Video; taken (case-insensitive, on that machine) -> "<label> (2)", "(3)"...
   */
  private defaultLabel(kind: SystemKind, cls: LlmClass | undefined, mine: MSys[]): string {
    const taken = (l: string) => mine.some((s) => s.label.trim().toLowerCase() === l.toLowerCase());
    if (kind === 'llm' && !cls) {
      for (let n = 1; ; n++) if (!taken(`System ${n}`)) return `System ${n}`;
    }
    const base =
      kind === 'llm'
        ? { fast: 'System 1', deep: 'System 2', max: 'System 3' }[cls!]
        : { image: 'System CGI', tts: 'System TTS', stt: 'System STT', video: 'System Video', music: 'System Music' }[kind];
    if (!taken(base)) return base;
    for (let n = 2; ; n++) if (!taken(`${base} (${n})`)) return `${base} (${n})`;
  }

  /** (Re)create or drop the simulation of an external server after its preset changed. */
  private syncExternal(s: MSys) {
    const spec = this.specOf(s);
    const want = !!spec && isExternal(spec) && !this.unreachable(s);
    if (want && !s.ext) {
      const r = createRng(hashSeed(`${this.seed}:ext:${s.id}`));
      s.ext = new GenericSim(s.kind, r, null, spec?.model_name, 3);
    } else if (!want && s.ext) {
      s.ext = null;
    }
  }

  private activityOf(s: MSys): number {
    const rt = s.rt;
    if (!rt) return s.ext ? s.ext.activity : 0;
    if (rt.phase === 'starting' || rt.phase === 'loading') return 0.35;
    if (rt.phase !== 'live') return 0;
    if (rt.llm) return rt.llm.activity === 'idle' ? 0 : rt.llm.activity === 'prefill' ? 1 : 0.8;
    if (rt.image) return rt.image.activity === 'generating' ? 1 : 0;
    if (rt.generic) return rt.generic.activity;
    return 0;
  }

  private statusOf(s: MSys, avail: string): { status: SystemStatus; reason?: string } {
    if (this.unreachable(s)) return { status: 'unreachable', reason: `${this.nodeOfSys(s)?.name ?? 'Its node'} is not reachable.` };
    const rt = s.rt;
    if (rt) {
      switch (rt.phase) {
        case 'starting':
        case 'loading':
          return { status: 'starting' };
        case 'live':
          return { status: this.activityOf(s) > 0 ? 'busy' : 'online' };
        case 'stopping':
          return { status: 'stopping' };
        case 'fault':
          return { status: 'fault', reason: rt.fault?.title };
      }
    }
    // A pending launch ("Stop System 1 & launch" while System 1 stops): starting, no session (native compose).
    if (s.pending) {
      const left = s.pending.waitFor
        .map((id) => this.sys(id))
        .filter((x): x is MSys => !!x?.rt)
        .map((x) => x.label);
      return { status: 'starting', reason: left.length ? `Waiting for ${names(left)} to stop.` : 'Starting.' };
    }
    if (!s.preset || !this.specOf(s)) return { status: 'not-set', reason: 'No preset is set. Choose one in Tune.' };
    const spec = this.specOf(s)!;
    if (isExternal(spec)) {
      if (!s.ext) return { status: 'offline', reason: 'Not answering.' };
      return { status: s.ext.busy ? 'busy' : 'online' };
    }
    if (avail !== 'ready') {
      const r = this.resolve(s);
      return { status: 'invalid', reason: r.avail.reason };
    }
    return { status: 'offline' };
  }

  private beginStop(s: MSys) {
    const rt = s.rt;
    if (!rt) return;
    // Shrink from whatever is resident right now (nothing much, if the GPU is asleep).
    rt.layers = rt.gpu.resident.filter((d) => d.owner === s.id).map((d) => ({ ...d.l }));
    rt.phase = 'stopping';
    rt.phaseT = this.t;
    rt.stopT = this.t;
    s.console.push(this.stamp(rt, 'srv', 'stop: shutting the server down'));
  }

  private endFault(s: MSys) {
    if (!s.rt) return;
    this.recordLast(s.rt, 'fault', s.rt.faultAt, s.rt.faultAt);
    s.rt = null;
    s.relaunch = null;
  }

  private finishStop(s: MSys) {
    const rt = s.rt!;
    s.console.push(this.stamp(rt, 'srv', 'stopped'));
    this.recordLast(rt, 'stopped', rt.stopT, this.t);
    s.rt = null;
    const next = s.relaunch;
    s.relaunch = null;
    if (next) {
      try {
        this.fire(s, next);
      } catch (e) {
        this.hooks.toast(e instanceof Error ? e.message : String(e));
      }
    }
  }

  /** Start a restarted System again (native fire()): ports and exclusive GPUs re-checked, VRAM not, nobody stopped. */
  private fire(s: MSys, opts: LaunchOpts) {
    const r = this.resolve(s);
    if (!r.spec) throw new Error(`${s.label} has no preset. Choose one in Tune.`);
    if (r.avail.availability !== 'ready') throw new Error(`${s.label} cannot start: ${r.avail.reason ?? r.avail.availability}`);
    const hard = this.hardConflicts(s);
    if (hard.size) {
      const labels = [...hard.keys()].map((id) => this.sysOrThrow(id).label);
      throw new Error(`${s.label} cannot launch: ${names(labels)} ${labels.length === 1 ? 'is' : 'are'} running meanwhile.`);
    }
    this.startSession(s, { ...opts, stopOthers: false });
  }

  /** After the Systems a pending launch waited for are gone, start it. */
  private followLaunch(s: MSys) {
    const p = s.pending;
    if (!p) return;
    if (p.waitFor.some((id) => this.sys(id)?.rt)) return;
    s.pending = null;
    try {
      this.startSession(s, p.opts);
    } catch (e) {
      this.hooks.toast(e instanceof Error ? e.message : String(e));
    }
  }

  private startSession(s: MSys, opts: LaunchOpts) {
    const r = this.resolve(s);
    if (!r.spec || !r.cmd) return;
    // Launching over a faulted session ends that one (it becomes the "last session").
    if (s.rt) this.endFault(s);
    if (!this.sys(this.selected)?.rt) this.selected = s.id;
    this.sessionNo++;
    const kind = s.kind;
    const gpu = r.gpu;
    const sRng = createRng(hashSeed(`${this.seed}:${this.sessionNo}:${s.id}`));
    const d = bootDurations(r.model, sRng, kind);
    // The loader trims its compute buffers to squeeze under the edge before anything spills.
    const target = fitLayers(r.layers, gpu.occupiedBy(s), gpu.total).layers;
    const style: LogStyle = kind === 'llm' ? 'llama' : kind === 'image' ? 'sd' : 'generic';
    const plan = planBoot(d, kind, style, r.model, target, 'ROCm · gfx1201', r.cmd.port);
    const rt: SessionRt = {
      sys: s,
      model: r.model,
      cmd: r.cmd,
      preset: s.preset ?? '',
      gpu,
      port: plan.port,
      apiKey: r.cmd.env.some((e) => e.managed && e.secret),
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
      generic: null,
      fault: null,
      faultAt: 0,
      stopT: 0,
      layers: opts.dormant ? opts.dormant.layers.map((l) => ({ ...l })) : target,
      act: 0.3,
      swell: 0,
      power: opts.dormant ? new GpuPower(opts.dormant.power) : null,
      seed: hashSeed(`${this.seed}:${this.sessionNo}`),
      peakGiB: 0,
    };
    s.rt = rt;
    s.console.clear();
    if (opts.skipBoot) {
      for (const l of rt.lines) this.pushBootLine(rt, l);
      rt.lineIdx = rt.lines.length;
      this.goLive(rt);
      this.refreshGpus();
      gpu.backfill(rt.layers, opts.uptimeOffsetS ?? 0);
    }
  }

  private recordLast(s: SessionRt, ended: 'stopped' | 'fault', upEndT: number, endedAt: number) {
    const rec: LastRec = {
      system: s.sys.id,
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
    if (s.generic) rec.requests = s.generic.requestsTotal;
    s.sys.last = rec;
  }

  private pushBootLine(s: SessionRt, l: BootLine) {
    if (l.proc >= 0 && s.plan.style === 'llama') s.sys.console.push(`${llamaTs(l.proc)} ${l.level} ${l.text}`);
    else s.sys.console.push(l.text);
  }

  private stamp(s: SessionRt, ch: string, msg: string): string {
    if (s.plan.style === 'llama') return `${llamaTs(Math.max(0, this.t - s.t0 - s.procOffset))} I ${ch}  ${msg}`;
    if (s.plan.style === 'sd') return `[INFO ] ${ch} - ${msg}`;
    return `${ch}: ${msg}`;
  }

  // ---- internals: stepping ----------------------------------------------------------------------------

  private step(h: number) {
    this.t += h;
    for (const s of this.systems) {
      const rt = s.rt;
      if (rt) {
        switch (rt.phase) {
          case 'starting':
          case 'loading':
            this.stepBoot(rt);
            break;
          case 'live':
            this.stepLive(rt, h);
            break;
          case 'stopping':
            if (this.t - rt.phaseT >= STOP_S) this.finishStop(s);
            break;
        }
      } else if (s.ext) {
        s.ext.step(h);
      }
      if (s.pending) this.followLaunch(s);
    }
    this.stepDownloads(h);
    this.stepResources(h);
    this.stepRecords();
  }

  /** Scenario `record`: break the running System's decode record on schedule. */
  private stepRecords() {
    const r = this.recordRaise;
    if (!r || this.t < r.next) return;
    r.next = this.t + r.every;
    this.raiseDecodeRecord();
  }

  /** A live local LLM System (the selected one first) beats its decode record: entry, event and history move. */
  private raiseDecodeRecord() {
    const live = this.systems.filter((x) => !x.node && x.kind === 'llm' && x.rt?.phase === 'live' && x.rt.llm);
    const sys = live.find((x) => x.id === this.selected) ?? live[0];
    const rt = sys?.rt;
    if (!sys || !rt || !rt.llm) return;
    const spec = this.specOf(sys);
    const file = (spec?.model ?? '').split(/[\\/]/).pop() || rt.model.name;
    const backend = rt.model.backend;
    const local = (e: RecordEntry) => !e.node && e.kind === 'llm' && e.backend.toLowerCase() === backend.toLowerCase();
    // The entry of the file this System runs, else of the same model (and quant) on this backend.
    let entry =
      this.records.find((e) => local(e) && e.model.file.toLowerCase() === file.toLowerCase()) ??
      this.records.find((e) => local(e) && e.model.name === rt.model.name && (!rt.model.quant || e.model.quant === rt.model.quant)) ??
      this.records.find((e) => local(e) && e.model.name === rt.model.name);
    const at = Math.round(this.epoch0 + this.t);
    const rng = createRng(hashSeed(`${this.seed}:record:${this.recordLines.length}`));
    const old = entry?.best.decodeTps?.value;
    const base = old ?? Math.max(20, rt.llm.medianDecodeTps());
    const next = Math.max(roundMetric('decodeTps', base * (1 + rng.range(0.006, 0.022))), r2(base) + 0.01);
    const best: RecordValue = {
      value: next,
      at,
      source: 'live',
      ...(rt.model.ctxTokens ? { ctx: rt.model.ctxTokens } : {}),
      ...(rt.model.kvType ? { kv: rt.model.kvType } : {}),
      promptTokens: rng.int(900, 3200),
      genTokens: rng.int(300, 1400),
      gpus: spec ? gpuList(spec, rt.gpu.id) : [rt.gpu.id],
      backendBuild: 'b9112',
      ...(sys.preset ? { preset: sys.preset } : {}),
      klifVersion: '0.3.1',
      tflopsFp32: MOCK_HARDWARE.tflopsFp32,
    };
    if (entry) {
      const prev = entry;
      entry = { ...prev, best: { ...prev.best, decodeTps: best } };
      this.records = this.records.map((e) => (e === prev ? entry! : e));
    } else {
      const sizeBytes = Math.round((spec ? (factsOf(spec)?.weightsGiB ?? 0) : 0) * GIB) || undefined;
      const model: RecordModel = { file, name: rt.model.name, ...(rt.model.quant ? { quant: rt.model.quant } : {}), ...(sizeBytes ? { sizeBytes } : {}) };
      entry = { key: `This machine|${file}:${sizeBytes ?? 0}|${backend}`, machine: 'This machine', kind: 'llm', model, backend, best: { decodeTps: best } };
      this.records = [...this.records, entry];
    }
    const line: RecordHistoryLine = { key: entry.key, metric: 'decodeTps', ...(old !== undefined ? { old } : {}), new: next, at };
    const lines: RecordHistoryLine[] = [line];
    // Every other turn the same request also beats prefill and time to first token: one request, several records.
    if (this.recordLines.length % 2 === 1) {
      let cur: RecordEntry = entry;
      for (const [metric, factor] of [
        ['prefillTps', 1 + rng.range(0.01, 0.04)],
        ['ttftS', 1 - rng.range(0.03, 0.09)],
      ] as const) {
        const prevV = cur.best[metric]?.value;
        const v = roundMetric(metric, (prevV ?? (metric === 'ttftS' ? 0.4 : 1500)) * factor);
        const prevEntry: RecordEntry = cur;
        const nextEntry: RecordEntry = { ...prevEntry, best: { ...prevEntry.best, [metric]: { ...best, value: v } } };
        this.records = this.records.map((e) => (e === prevEntry ? nextEntry : e));
        cur = nextEntry;
        lines.push({ key: cur.key, metric, ...(prevV !== undefined ? { old: prevV } : {}), new: v, at });
      }
    }
    this.recordLines = [...this.recordLines, ...lines];
    this.recordEvents = [...this.recordEvents, ...lines.map((l) => ({ ...l }))].slice(-MAX_RECORD_EVENTS);
    this.recordsRev++;
  }

  private stepBoot(s: SessionRt) {
    const e = this.t - s.t0;
    while (s.lineIdx < s.lines.length && s.lines[s.lineIdx].at <= e) this.pushBootLine(s, s.lines[s.lineIdx++]);
    if (s.opts.failAtS !== undefined && e >= s.opts.failAtS) {
      this.fail(s, s.opts.failKind ?? 'exit');
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
    const console_ = s.sys.console;
    if (s.sys.kind === 'llm') {
      const cfg = llmConfig(s.model, { big: s.opts.big });
      const d = s.opts.dormant;
      if (d) {
        cfg.paced = { periodS: d.periodS ?? 40, jitterS: 2, firstInS: d.firstInS ?? 4, maxNewTokens: 1800, maxGenTokens: 280 };
      }
      s.llm = new LlmSim(cfg, rng, console_, clock);
      if (s.power) {
        const power = s.power;
        s.llm.blocked = () => power.dormant;
      }
    } else if (s.sys.kind === 'image') {
      s.image = new ImageSim(imageConfig(s.opts.crashOnJob), rng, console_, () => this.epoch0 + this.t);
    } else {
      s.generic = new GenericSim(s.sys.kind, rng, console_, s.model.name);
    }
  }

  private stepLive(s: SessionRt, h: number) {
    s.llm?.step(h);
    s.generic?.step(h);
    s.power?.step(h, this.t, !!s.llm && s.llm.activity !== 'idle');
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
    const lines = kind === 'rocm-crash' ? [...ROCM_CRASH_STDERR, `sd-server HIP exited with code ${ROCM_EXIT_CODE}.`] : EXIT_FAIL_STDERR;
    s.sys.console.pushAll(lines);
    s.sys.relaunch = null;
    s.sys.pending = null;
  }

  private stepDownloads(h: number) {
    for (const d of this.downloads) {
      if (d.state === 'running') {
        d.done = Math.min(d.total, d.done + DOWNLOAD_BPS * h);
        if (d.done >= d.total) {
          d.state = 'verifying';
          d.endedAt = this.t;
        }
      } else if (d.state === 'verifying' && this.t - (d.endedAt ?? 0) >= 1.5) {
        d.state = 'done';
        d.endedAt = this.t;
        if (this.downloads.filter((x) => x.id === d.id).every((x) => x.state === 'done')) this.downloaded.add(d.id);
      }
    }
    this.downloads = this.downloads.filter((d) => !((d.state === 'done' || d.state === 'cancelled' || d.state === 'failed') && this.t - (d.endedAt ?? 0) > 6));
  }

  /** What a session asks of its GPU right now (baseline NOT included). */
  private sessionDemand(s: SessionRt): VramLayer[] {
    switch (s.phase) {
      case 'loading':
        return bootView(s.plan, this.t - s.t0).layers;
      case 'live': {
        const out = s.layers.map((l) => ({ ...l }));
        const buf = out.find((l) => l.id === 'buffers');
        if (buf) {
          // Image Systems: activation buffers swell while a job runs and shrink back between jobs.
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

  private refreshGpus() {
    for (const g of this.gpus) {
      const sessions = this.systems.filter((x) => x.rt && x.rt.gpu === g).map((x) => x.rt!);
      g.refresh(sessions, (s) => this.sessionDemand(s));
      for (const s of sessions) {
        const mine = g.alloc.filter((d) => d.owner === s.sys.id).reduce((a, d) => a + d.l.gib, 0);
        s.peakGiB = Math.max(s.peakGiB, mine);
      }
    }
  }

  private stepResources(h: number) {
    for (const s of this.systems) {
      const rt = s.rt;
      if (rt?.image) {
        const target = rt.image.load();
        rt.act += (target - rt.act) * (1 - Math.exp(-h / 0.8));
      }
      // Spill scenario: compute-buffer demand follows the prefill progress, then relaxes back.
      if (rt && rt.phase === 'live' && rt.opts.spill && rt.llm) {
        const f = rt.llm.prefillFraction();
        const target = f >= 0 ? SWELL_PEAK_GIB * f * f : 0;
        rt.swell += (target - rt.swell) * (1 - Math.exp(-h / (target > rt.swell ? 1.2 : 9)));
      }
    }
    for (const g of this.gpus) g.stepBaseline(this.t, h);
    this.refreshGpus();
    for (const g of this.gpus) g.record(h);

    // CPU and RAM of this machine: smooth approach to a phase-dependent target plus a little seeded noise.
    let cpuT = 2.5;
    let ramT = RAM_BASE;
    for (const sys of this.systems) {
      const s = sys.rt;
      if (!s || sys.node) continue;
      const foot = ramFootprintGiB(s.model, sys.kind);
      switch (s.phase) {
        case 'starting':
          cpuT = Math.max(cpuT, 9);
          break;
        case 'loading': {
          const v = bootView(s.plan, this.t - s.t0);
          const w = v.steps.find((x) => x.id === 'weights');
          cpuT = Math.max(cpuT, w?.state === 'active' ? 28 : 15);
          ramT += foot * clamp(v.fraction * 1.3, 0, 1);
          break;
        }
        case 'live': {
          ramT += foot;
          // What the driver paged out of VRAM sits in system RAM until the next request restores it.
          if (s.power) ramT += s.power.cfg.pagedOutGiB * (1 - s.power.res);
          if (s.llm) cpuT = Math.max(cpuT, s.llm.activity === 'prefill' ? 13 : s.llm.activity === 'decode' ? 6 : 2.5);
          else if (s.image) cpuT = Math.max(cpuT, s.image.activity === 'generating' ? 5 : 2);
          else if (s.generic) cpuT = Math.max(cpuT, s.generic.busy ? 7 : 2);
          break;
        }
        case 'stopping':
          cpuT = Math.max(cpuT, 10);
          ramT += foot * (1 - clamp((this.t - s.phaseT) / STOP_S, 0, 1));
          break;
      }
    }
    const a = 1 - Math.exp(-h / 1.2);
    this.cpu += (cpuT - this.cpu) * a + this.rng.gauss() * 0.35;
    this.cpu = clamp(this.cpu, 0.5, 60);
    this.ram += (ramT - this.ram) * (1 - Math.exp(-h / 1.6)) + this.rng.gauss() * 0.01;
    this.nodeCpu = clamp(this.nodeCpu + this.rng.gauss() * 0.4, 1, 12);
  }

  // ---- internals: conflicts ---------------------------------------------------------------------------

  /** A System holds its GPU when it has a session with live processes or a pending launch. */
  private holds(h: MSys): boolean {
    return (!!h.rt && h.rt.phase !== 'fault') || !!h.pending;
  }

  private reservation(h: MSys): number {
    const committed = h.rt ? h.rt.layers.reduce((a, l) => a + l.gib, 0) : 0;
    const starting = !h.rt || h.rt.phase === 'starting' || h.rt.phase === 'loading';
    if (!starting) return committed;
    return Math.max(committed, totalGiB(this.resolve(h).layers));
  }

  /** The other holders on t's machine (never externals): the members of t's conflicts. */
  private holdersOf(t: MSys): MSys[] {
    return this.systems.filter((h) => h !== t && h.node === t.node && this.holds(h) && h.preset && !isExternal(this.specOf(h) ?? {}));
  }

  /** Rules (a) port and (b) exclusive GPU against the other holders, in tab order (no VRAM rule). */
  private hardConflicts(t: MSys, r = this.resolve(t)): Map<SystemId, string> {
    const out = new Map<SystemId, string>();
    if (!r.cmd) return out;
    const holders = this.holdersOf(t);
    const overlap = (a: string, b: string) => a === b || a === '0.0.0.0' || b === '0.0.0.0' || a === '::' || b === '::';
    // (a) port
    for (const h of holders) {
      const hc = this.resolve(h).cmd;
      if (hc && hc.port === r.cmd.port && overlap(hc.host, r.cmd.host)) out.set(h.id, `port ${hc.port} is in use`);
    }
    // (b) exclusive
    for (const h of holders) {
      if (this.resolve(h).gpu !== r.gpu) continue;
      if (t.exclusive || h.exclusive) out.set(h.id, out.get(h.id) ?? 'the GPU is exclusive');
    }
    return new Map(this.systems.filter((x) => out.has(x.id)).map((x) => [x.id, out.get(x.id)!]));
  }

  /** Running Systems that must stop before `t` can launch (and why). */
  private conflictsFor(t: MSys): { ids: SystemId[]; reason?: string } {
    if (!t.preset || this.holds(t) || (t.rt && t.rt.phase !== 'fault')) return { ids: [] };
    const r = this.resolve(t);
    if (!r.spec || !r.cmd || isExternal(r.spec) || this.unreachable(t)) return { ids: [] };
    const holders = this.holdersOf(t);
    const out = this.hardConflicts(t, r);
    // (c) VRAM
    const gpu = r.gpu;
    const need = totalGiB(r.layers);
    const onGpu = holders.filter((h) => this.resolve(h).gpu === gpu);
    const foreign = Math.max(0, gpu.usedGiB() - gpu.resident.reduce((a, d) => a + (d.owner ? d.l.gib : 0), 0));
    const sum = (list: MSys[]) => list.reduce((a, h) => a + this.reservation(h), 0);
    let free = gpu.total - foreign - sum(onGpu);
    if (need > free - VRAM_WARN) {
      const sorted = [...onGpu].sort((a, b) => this.reservation(b) - this.reservation(a));
      if (need > gpu.total - foreign - VRAM_WARN) return { ids: [], reason: 'Does not fit even with everything stopped.' };
      for (const h of sorted) {
        if (need <= free - VRAM_WARN) break;
        if (!out.has(h.id)) out.set(h.id, 'not enough VRAM');
        free += this.reservation(h);
      }
    }
    // Keep tab order.
    const ids = this.systems.filter((x) => out.has(x.id)).map((x) => x.id);
    const first = ids[0];
    return { ids, reason: first ? `Needs ${this.sysOrThrow(first).label}${ids.length > 1 ? ' and others' : ''} stopped: ${out.get(first)}.` : undefined };
  }

  // ---- snapshot -------------------------------------------------------------------------------------

  private sessionView(s: MSys): Session | null {
    const rt = s.rt;
    const spec = this.specOf(s);
    if (!rt) {
      if (!s.ext || !spec) return null;
      // An external server that answers: a live session with what probes can tell.
      const cmd = buildCommand(spec, this.ctx(s));
      return {
        system: s.id,
        model: modelRefOf(spec, cmd, s.params, ''),
        phase: 'live',
        uptimeS: 0,
        endpoint: { host: cmd.host, port: cmd.port },
        apiKeySet: false,
        loading: null,
        fault: null,
        llm: null,
        image: null,
        generic: s.ext.toLive(),
        preset: s.preset,
        command: cmd,
      };
    }
    const e = (rt.phase === 'fault' ? rt.faultAt : this.t) - rt.t0;
    let loading: LoadProgress | null = null;
    if (rt.phase === 'starting' || rt.phase === 'loading') {
      const v = bootView(rt.plan, e);
      loading = { steps: v.steps, fraction: r2(v.fraction), elapsedS: r1(e) };
    }
    let fault: Fault | null = null;
    if (rt.fault) {
      fault = { ...rt.fault, logTail: rt.fault.logTail.slice(), sinceS: r1(this.t - rt.faultAt) };
      if (rt.fault.steps) fault.steps = rt.fault.steps.map((x) => ({ ...x }));
    }
    const committed = rt.gpu.alloc.filter((d) => d.owner === s.id).reduce((a, d) => a + d.l.gib, 0);
    const node = this.nodeOfSys(s);
    const host = node && (rt.cmd.host === '127.0.0.1' || rt.cmd.host === '0.0.0.0') ? node.address.split(':')[0] : rt.cmd.host;
    return {
      system: s.id,
      model: { ...rt.model },
      phase: rt.phase,
      uptimeS: Math.max(0, r1(e)),
      endpoint: { host, port: rt.port },
      apiKeySet: rt.apiKey,
      loading,
      fault,
      llm: rt.llm ? rt.llm.toLive() : null,
      image: rt.image ? rt.image.toLive() : null,
      generic: rt.generic ? rt.generic.toLive() : null,
      preset: rt.preset,
      command: rt.cmd,
      gpu: rt.gpu.id,
      vramGiB: r2(committed),
    };
  }

  private lastView(s: MSys): LastSession | null {
    if (!s.last) return null;
    const { endedAt, ...rest } = s.last;
    return { ...rest, model: { ...rest.model }, endedAgoS: Math.max(0, r1(this.t - endedAt)) };
  }

  private systemView(s: MSys, all: MSys[]): System {
    const r = this.resolve(s);
    const st = this.statusOf(s, r.avail.availability);
    const node = this.nodeOfSys(s);
    const spec = r.spec;
    const unreachable = st.status === 'unreachable';
    // A fault is no holder: Launch over it is refused for conflicts like an offline launch, so it publishes them
    // too (native compose, review R7); its reason stays the fault title.
    const conflicts =
      (!s.rt || s.rt.phase === 'fault') && !unreachable ? this.conflictsFor(s) : { ids: [] as SystemId[], reason: undefined as string | undefined };
    const session = unreachable ? null : this.sessionView(s);
    const view: System = {
      id: s.id,
      label: s.label,
      kind: s.kind,
      status: st.status,
      availability: r.avail.availability,
      model: unreachable ? { ...r.model, name: r.model.name } : r.model,
      params: spec && !unreachable ? paramViews(spec, s.params) : [],
      gpus: spec && !unreachable ? gpuList(spec, r.gpu.id) : [],
      external: !!spec && isExternal(spec),
      exclusive: s.exclusive,
      editable: node ? node.allow.includes('edit') : true,
      controllable: node ? node.state === 'online' && (node.allow.includes('launch') || node.allow.includes('edit')) : true,
      conflicts: conflicts.ids.slice(),
      activity: unreachable ? 0 : r2(this.activityOf(s)),
    };
    if (s.cls) view.class = s.cls;
    if (s.node) view.node = s.node;
    const reason = st.reason ?? (st.status === 'offline' ? conflicts.reason : undefined);
    if (reason) view.reason = reason;
    if (s.preset) view.preset = s.preset;
    if (r.cmd && !unreachable) view.command = r.cmd;
    if (r.layers.length && !unreachable) {
      view.expectedVram = r.layers.map((l) => ({ ...l }));
      view.expectedVramSource = factsOf(spec!) ? 'file-size' : undefined;
      if (!view.expectedVramSource) delete view.expectedVramSource;
    }
    if (spec && !unreachable) view.gpu = view.gpus[0];
    const ep = unreachable ? null : this.endpointOf(s.id);
    if (ep) view.endpoint = ep;
    if (session) view.session = session;
    const last = unreachable ? null : this.lastView(s);
    if (last) view.lastSession = last;
    // Cross-System check: two Systems on one host:port cannot run at the same time.
    if (view.command && !view.external) {
      const clash = all.find((o) => o !== s && o.node === s.node && o.preset && (this.resolve(o).cmd?.port === view.command!.port) && !isExternal(this.specOf(o) ?? {}));
      if (clash) {
        view.command = {
          ...view.command,
          issues: [...view.command.issues, { level: 'warn', field: 'port', text: `Shares port ${view.command.port} with ${clash.label}: they cannot run at the same time.` }],
        };
      }
    }
    return view;
  }

  private memoryOf(g: GpuSim, forSystem: SystemId | null): GpuMemory {
    const m = g.memory(forSystem);
    if (m.dormant) m.dormant = g.power!.view(this.t);
    return m;
  }

  snapshot(): ViewModel {
    const ordered = [...this.systems.filter((s) => !s.node), ...this.systems.filter((s) => s.node)];
    const systems = ordered.map((s) => this.systemView(s, ordered));
    const selSys = this.sys(this.selected) ?? ordered[0];
    const selView = selSys ? systems.find((s) => s.id === selSys.id) : undefined;
    const localGpus = this.gpus.filter((g) => !g.node);
    const defaultGpu = localGpus[0];
    // An unreachable node reports no GPU: the convenience falls back to this machine's inference GPU.
    const selGpu = selSys && !this.unreachable(selSys) ? this.gpuFor(selSys, this.specOf(selSys)) : defaultGpu;
    const vram = this.memoryOf(selGpu, selSys && !this.unreachable(selSys) ? selSys.id : null);
    const machine: MachineStats = {
      ramUsedGiB: r1(this.ram),
      ramTotalGiB: RAM_TOTAL,
      ramType: RAM_TYPE,
      cpuName: '9950X3D',
      cpuPct: Math.round(this.cpu),
    };
    const localPresets = this.presetList(undefined);
    const nodes: NodeView[] = this.nodes.map((n) => {
      const gpus = this.gpus.filter((g) => g.node === n.id).map((g) => this.memoryOf(g, null));
      const online = n.state === 'online';
      const nv: NodeView = {
        id: n.id,
        name: n.name,
        address: n.address,
        state: n.state,
        allow: n.allow.slice(),
        gpus: online ? gpus : [],
        presets: online ? this.presetList(n.id) : [],
      };
      if (online) {
        nv.version = '0.3.1';
        nv.latencyMs = n.latencyMs ?? 4;
        nv.machine = { ramUsedGiB: 22.4, ramTotalGiB: n.ramTotalGiB, ramType: 'DDR5', cpuName: n.cpuName, cpuPct: Math.round(this.nodeCpu) };
      }
      if (n.error) nv.error = n.error;
      return nv;
    });
    const config: ConfigInfo = {
      path: '%APPDATA%\\KLIF\\klif.toml',
      stateDir: '%APPDATA%\\KLIF',
      dataDir: '%LOCALAPPDATA%\\KLIF',
      issues: [],
      apiKey: { source: 'file', set: this.apiKeySet },
      onConflict: this.onConflict,
      recordMoment: this.recordMoment,
    };
    if (this.modelsDir) config.modelsDir = this.modelsDir;
    const downloads: DownloadInfo[] = this.downloads.map((d) => {
      const info: DownloadInfo = { id: d.id, file: d.file, doneBytes: Math.round(d.done), totalBytes: d.total, state: d.state };
      if (d.error) info.error = d.error;
      return info;
    });
    return {
      now: this.epoch0 + this.t,
      systems,
      selected: selView ? selView.id : null,
      session: selView?.session ?? null,
      lastSession: selView?.lastSession ?? null,
      console: selSys ? selSys.console.snapshot() : [],
      vram,
      gpus: localGpus.map((g) => this.memoryOf(g, null)).map((m) => ({ ...m, layers: m.layers.slice(0, 1) })),
      machine,
      host: { ...this.host },
      presets: localPresets,
      recommendations: recommendationInfos(this.presets, this.downloaded),
      hardware: MOCK_HARDWARE,
      suggestions: MOCK_SUGGESTIONS,
      records: this.recordView().records,
      recordEvents: this.recordView().events,
      recordsRev: this.recordsRev,
      downloads,
      config,
      nodes,
    };
  }

  private recView: { rev: number; records: RecordEntry[]; events: RecordEvent[] } | null = null;

  /**
   * What the view model publishes: this machine's records and those of the nodes that publish them. A node the
   * world models is hidden while it is not online (the real core drops its entries); a node the world does not
   * model is simply shown (the browser mock keeps one fiction for every scenario).
   */
  private recordView(): { records: RecordEntry[]; events: RecordEvent[] } {
    if (this.recView && this.recView.rev === this.recordsRev) return this.recView;
    const hidden = new Set(this.nodes.filter((n) => n.state !== 'online').map((n) => n.id));
    const records = this.records.filter((e) => !e.node || !hidden.has(e.node));
    const keys = new Set(records.map((e) => e.key));
    const events = this.recordEvents.filter((ev) => keys.has(ev.key));
    this.recView = { rev: this.recordsRev, records, events };
    return this.recView;
  }

  private presetList(node: string | undefined): PresetInfo[] {
    const gpu = this.gpus.find((g) => g.node === node)?.def.name ?? '';
    return Object.entries(this.presetMap(node)).map(([id, spec]) => presetInfo(id, spec, buildCommand(spec, this.ctx(null, node)), gpu, {}, node));
  }

  /** Configuration revision (bumped on every change of klif.toml in the mock). */
  get revision(): number {
    return this.cfgRev;
  }

  /** Kind of a System (scenario predicates). */
  kindOf(id: SystemId): SystemKind | undefined {
    return this.sys(id)?.kind;
  }

  /** Family of the model a System runs (dev bar / scenarios). */
  familyOfSystem(id: SystemId) {
    const s = this.sysOrThrow(id);
    return familyOf(this.resolve(s).model, s.kind);
  }
}
