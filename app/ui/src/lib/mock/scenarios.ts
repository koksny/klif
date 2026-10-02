// Scenario registry. A scenario is a deterministic starting script for the mock engine plus the moment
// at which the first frame is taken (so every skin can be looked at in the state it needs).
import { SAMPLE_NOW } from '../model/sample';
import type { SlotId, ViewModel, VramLayer } from '../model/types';
import { MockEngine, type DormantOpts, type EngineHooks } from './engine';
import { browserHost } from './host';
import { hashSeed } from './rng';

export interface ScenarioDef {
  name: string;
  label: string;
  blurb: string;
  /** True for the frozen sample snapshot (no engine). */
  isStatic?: boolean;
  /** Run the script, leaving the engine at t=0. */
  setup(e: MockEngine): void;
  /** Minimum simulated seconds before the first frame. */
  startT: number;
  /** Keep stepping from startT until this holds (bounded), for the first interactive frame. */
  startUntil?: (vm: ViewModel) => boolean;
  /** Same, for shot=1 frames; defaults to startUntil. */
  shotUntil?: (vm: ViewModel) => boolean;
  /** Upper bound of the search, simulated seconds from t=0. */
  searchS?: number;
  /** Simulated seconds per search step (default 0.5); finer when the frame to catch is brief. */
  searchStepS?: number;
}

const llmDecoding = (vm: ViewModel) => {
  const l = vm.session?.llm;
  return !!l && l.activity === 'decode' && l.decodeTps > 20 && l.requests.length >= 8 && l.decodeHistory.length >= 240;
};

const liveSession = (slot: SlotId, until: (vm: ViewModel) => boolean) => (vm: ViewModel) =>
  vm.session?.slot === slot && vm.session.phase === 'live' && until(vm);

function boot(slot: SlotId, label: string, blurb: string): ScenarioDef {
  return {
    name: `boot-${slot}`,
    label,
    blurb,
    setup: (e) => {
      e.select(slot);
      e.launch(slot);
    },
    startT: 0,
    shotUntil: (vm) => {
      const s = vm.session;
      if (!s || s.phase !== 'loading' || !s.loading) return false;
      const w = s.loading.steps.find((x) => x.id === 'weights');
      return w?.state === 'active' && s.loading.fraction >= 0.4;
    },
    searchS: 60,
  };
}

function live(slot: SlotId, name: string, label: string, blurb: string, until: (vm: ViewModel) => boolean): ScenarioDef {
  return {
    name,
    label,
    blurb,
    setup: (e) => {
      e.select(slot);
      e.launch(slot);
    },
    startT: 600,
    startUntil: liveSession(slot, until),
    searchS: 3600,
  };
}

/**
 * Agent Medium with the 9070 XT asleep between requests (AMD ULPS, device state D3). The allocations are
 * what the real session held (llama-server, 98k context); the device drives no display, so ~12 s after
 * the last request the driver pages the VRAM out to system RAM and the next request waits ~4 s for it.
 */
const DORMANT_SESSION: DormantOpts = {
  layers: [
    { id: 'weights', label: 'weights', gib: 10.87 },
    { id: 'kv', label: 'KV cache', gib: 3.62 },
    { id: 'buffers', label: 'buffers', gib: 0.46 },
    { id: 'draft', label: 'draft', gib: 0.45 },
  ] satisfies VramLayer[],
  periodS: 40,
  firstInS: 4,
};

/** The GPU is fully asleep (VRAM paged out, nothing resident) and has been for a few seconds. */
const gpuAsleep = (vm: ViewModel) => {
  const d = vm.vram.dormant;
  const l = vm.session?.llm;
  return vm.session?.phase === 'live' && !!d && vm.vram.usedGiB < 0.05 && d.sinceS >= 9 && !!l && l.activity === 'idle' && l.requests.length >= 3;
};

/** Mid-wake: a request is waiting at 0 tok/s while about 7 GiB of the VRAM have been restored (~8.4 still paged out). */
const gpuWaking = (vm: ViewModel) => {
  const d = vm.vram.dormant;
  const l = vm.session?.llm;
  return (
    vm.session?.phase === 'live' &&
    !!d &&
    !!l &&
    l.activity === 'prefill' &&
    l.prefill?.doneTokens === 0 &&
    l.requests.length >= 2 &&
    vm.vram.usedGiB >= 6.9 &&
    vm.vram.usedGiB < 7.2
  );
};

function dormantScenario(name: string, label: string, blurb: string, until: (vm: ViewModel) => boolean, searchStepS?: number): ScenarioDef {
  return {
    name,
    label,
    blurb,
    setup: (e) => {
      e.select('medium');
      e.launch('medium', { skipBoot: true, uptimeOffsetS: 1800, dormant: DORMANT_SESSION });
    },
    startT: 0,
    startUntil: until,
    searchS: 400,
    ...(searchStepS ? { searchStepS } : {}),
  };
}

export const SCENARIOS: ScenarioDef[] = [
  {
    name: 'sample',
    label: 'Sample (static)',
    blurb: 'The static snapshot from the approved mockups. Deterministic; used for screenshots.',
    isStatic: true,
    setup: () => {},
    startT: 0,
  },
  {
    name: 'idle',
    label: 'Idle (no session)',
    blurb: 'Nothing running. All four slots ready; launch one. The last session (Agent Medium, stopped 21 min ago) is summarised.',
    setup: (e) => {
      e.select('medium');
      e.setLastSession(
        { slot: 'medium', model: e.slotModel('medium'), uptimeS: 8047, ended: 'stopped', requests: 12, generatedTokens: 11420, decodeTps: 47.3 },
        1260,
      );
    },
    startT: 0,
  },
  boot('medium', 'Boot: Agent Medium', 'Starting, then loading steps with weights and VRAM layers building, then live.'),
  live('medium', 'live-medium', 'Live: Agent Medium (Qwen 27B)', 'Requests arrive, prefill bursts, decode at real speeds, history rolling, spec acceptance varying.', llmDecoding),
  {
    name: 'prefill-high',
    label: 'Prefill: Agent High (Flash-Next)',
    blurb: 'One 66k-token prompt prefilling at ~370 tok/s for ~3 min, then decode at ~9 tok/s.',
    setup: (e) => {
      e.select('high');
      e.launch('high', { skipBoot: true, uptimeOffsetS: 236, big: true });
    },
    startT: 0,
    shotUntil: (vm) => {
      const p = vm.session?.llm?.prefill;
      return !!p && vm.session?.llm?.activity === 'prefill' && p.doneTokens / p.tokens >= 0.42;
    },
    searchS: 200,
  },
  live('high', 'live-high', 'Live: Agent High (Flash-Next)', 'Flash-Next replayed from real sessions: slow prefill, ~12 tok/s decode, high spec acceptance.', (vm) => {
    const l = vm.session?.llm;
    return !!l && l.activity !== 'idle' && l.requests.length >= 3;
  }),
  {
    name: 'spill-high',
    label: 'Spill: Agent High over the edge',
    blurb:
      'Flash-Next at 128k, one long prefill: compute buffers swell until the demand passes the VRAM edge. spillMiB ramps 0 to ~900 and relaxes back; free VRAM hits 0 while it spills.',
    setup: (e) => {
      e.select('high');
      e.launch('high', { skipBoot: true, uptimeOffsetS: 236, big: true, spill: true });
    },
    startT: 0,
    startUntil: (vm) => vm.vram.spillMiB >= 650,
    searchS: 400,
  },
  {
    name: 'warn',
    label: 'Warn: Agent Medium, tight VRAM',
    blurb:
      'Agent Medium decoding while another process takes ~0.15 GiB of VRAM: free VRAM drops below warnBelowGiB (0.15) but nothing spills.',
    setup: (e) => {
      e.select('medium');
      e.launch('medium');
      // A browser or video player grabs VRAM 560 s in; the baseline ('other' layer) steps up.
      e.scheduleBaseline(560, 0.27);
    },
    startT: 600,
    startUntil: liveSession('medium', (vm) => {
      const l = vm.session?.llm;
      const free = vm.vram.totalGiB - vm.vram.usedGiB;
      return !!l && l.activity === 'decode' && l.requests.length >= 3 && free < vm.vram.warnBelowGiB && vm.vram.spillMiB === 0;
    }),
    searchS: 3600,
  },
  dormantScenario(
    'dormant',
    'Dormant: GPU asleep (Agent Medium)',
    'The 9070 XT drives no display, so ULPS puts it in D3 ~12 s after the last request: allocations stay, Dedicated Usage falls to ~0 (paged out to RAM). Every ~40 s a request wakes it: ~4 s at 0 tok/s while VRAM is restored. Freezes while asleep.',
    gpuAsleep,
  ),
  dormantScenario(
    'waking',
    'Waking: restore in flight (Agent Medium)',
    'The same session frozen mid-wake: a request is waiting at 0 tok/s, ~7 GiB of the VRAM restored, ~8.4 GiB still paged out.',
    gpuWaking,
    0.05,
  ),
  live('low', 'live-low', 'Live: Agent Low (Gemma)', 'Gemma 4 26B-A4B: measured ~76-92 tok/s decode; request mix scaled to a 16k context (prefill speed estimated).', (vm) => {
    const l = vm.session?.llm;
    return !!l && l.activity === 'decode' && l.requests.length >= 4;
  }),
  {
    name: 'krea',
    label: 'Live: Krea (image jobs)',
    blurb: '8-step jobs replayed from real sessions: ~13 s plain, ~27 s edit, up to ~56 s for large edits.',
    setup: (e) => {
      e.select('krea');
      e.launch('krea');
    },
    startT: 300,
    startUntil: liveSession('krea', (vm) => {
      const i = vm.session?.image;
      return !!i && i.activity === 'generating' && i.step >= 4 && i.recent.length >= 5;
    }),
    searchS: 1500,
  },
  {
    name: 'fault-krea',
    label: 'Fault: Krea crash',
    blurb: 'Image server dies mid edit job (exit code -1073740791 / 0xC0000409), real crash pattern.',
    setup: (e) => {
      e.select('krea');
      e.launch('krea', { crashOnJob: 4 });
    },
    startT: 0,
    startUntil: (vm) => vm.session?.phase === 'fault' && (vm.session.fault?.sinceS ?? 0) >= 15,
    searchS: 1500,
  },
  {
    name: 'fault-medium',
    label: 'Fault: starter died',
    blurb: 'The launcher script dies 2 s after start with a PowerShell error.',
    setup: (e) => {
      e.select('medium');
      e.launch('medium', { failAtS: 2, failKind: 'powershell' });
    },
    startT: 0,
    startUntil: (vm) => vm.session?.phase === 'fault' && (vm.session.fault?.sinceS ?? 0) >= 6,
    searchS: 60,
  },
  boot('high', 'Boot: Agent High', 'Flash-Next loading (real log timings; cold loads can take 30 s).'),
  boot('low', 'Boot: Agent Low', 'Gemma loading.'),
  boot('krea', 'Boot: Krea', 'Image server loading (durations estimated).'),
];

export const DEFAULT_SCENARIO = 'live-medium';
export const SHOT_DEFAULT_SCENARIO = 'sample';

export function scenarioDef(name: string | null | undefined): ScenarioDef | null {
  return SCENARIOS.find((s) => s.name === name) ?? null;
}

export interface BuildCtx {
  hooks: EngineHooks;
  /** shot=1: fixed epoch so snapshots are reproducible. */
  shot: boolean;
  /** ?frameless=1: the skin must draw its own window controls. */
  frameless: boolean;
  /** Explicit simulated time to start at (URL t=). */
  t: number | null;
  seed: number;
}

/** Create the engine for a scenario and fast-forward it to its first frame. */
export function buildEngine(def: ScenarioDef, ctx: BuildCtx): MockEngine {
  const epoch0 = ctx.shot ? SAMPLE_NOW : Math.floor(Date.now() / 1000);
  const e = new MockEngine(epoch0, hashSeed(def.name) ^ ctx.seed, ctx.hooks);
  e.host = browserHost(ctx.frameless);
  def.setup(e);
  if (ctx.t !== null) {
    e.fastForward(ctx.t);
    return e;
  }
  e.fastForward(def.startT);
  const until = ctx.shot ? (def.shotUntil ?? def.startUntil) : def.startUntil;
  if (until) {
    const limit = def.searchS ?? 900;
    const step = def.searchStepS ?? 0.5;
    while (e.t < limit && !until(e.snapshot())) e.fastForward(e.t + step);
  }
  return e;
}
