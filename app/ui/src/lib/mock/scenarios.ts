// Scenario registry. A scenario is a deterministic starting script for the mock engine plus the moment
// at which the first frame is taken (so every skin can be looked at in the state it needs).
import { SAMPLE_NOW } from '../model/sample';
import type { SystemId, ViewModel, VramLayer } from '../model/types';
import { MockEngine, type DormantOpts, type EngineHooks } from './engine';
import { browserHost } from './host';
import { hashSeed } from './rng';
import { classicWorld, emptyWorld, kindsWorld, multiWorld, type World } from './world';

export interface ScenarioDef {
  name: string;
  label: string;
  blurb: string;
  /** True for the frozen sample snapshot (no engine). */
  isStatic?: boolean;
  /** The configuration the scenario starts from (default: the operator's four Systems). */
  world?: () => World;
  /** Run the script, leaving the engine at t=0. */
  setup(e: MockEngine): void;
  /** Runs once the engine has reached its first frame (after the pre-roll): for behaviour that must start then. */
  ready?(e: MockEngine): void;
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

const liveSession = (system: SystemId, until: (vm: ViewModel) => boolean) => (vm: ViewModel) =>
  vm.session?.system === system && vm.session.phase === 'live' && until(vm);

function boot(name: string, system: SystemId, label: string, blurb: string): ScenarioDef {
  return {
    name,
    label,
    blurb,
    setup: (e) => {
      e.select(system);
      e.launch(system);
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

function live(system: SystemId, name: string, label: string, blurb: string, until: (vm: ViewModel) => boolean, world?: () => World): ScenarioDef {
  return {
    name,
    label,
    blurb,
    ...(world ? { world } : {}),
    setup: (e) => {
      e.select(system);
      e.launch(system);
    },
    startT: 600,
    startUntil: liveSession(system, until),
    searchS: 3600,
  };
}

/**
 * System 2 with the 9070 XT asleep between requests (AMD ULPS, device state D3). The allocations are
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
      e.select('s2');
      e.launch('s2', { skipBoot: true, uptimeOffsetS: 1800, dormant: DORMANT_SESSION });
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
  // ---- the 0.3 worlds: several Systems at once ----
  live(
    's1',
    'multi',
    'Multi-System (default)',
    'System 1 online/busy, System 2 offline (launching it asks to stop System 1), System 3 not set, CGI offline, an external TTS server online, and a remote node with one image System.',
    (vm) => {
      const l = vm.session?.llm;
      return !!l && l.activity === 'decode' && l.decodeTps > 20 && l.requests.length >= 6 && l.decodeHistory.length >= 240;
    },
    multiWorld,
  ),
  {
    name: 'record',
    label: 'Records: a live System breaks its record',
    blurb:
      'System 2 live in the multi world; every ~12 s of simulated time it breaks its decode record (the entry, the recent events and the history move), for the new record moment of the Records screen.',
    world: multiWorld,
    setup: (e) => {
      e.select('s2');
      e.launch('s2');
    },
    ready: (e) => e.raiseRecords(12),
    startT: 600,
    startUntil: liveSession('s2', llmDecoding),
    searchS: 3600,
  },
  {
    name: 'kinds',
    label: 'Every kind of System',
    blurb: 'LLM, image, external TTS, transcription and video Systems: the generic display for kinds without a dedicated one.',
    world: kindsWorld,
    setup: (e) => {
      e.launch('stt', { skipBoot: true, uptimeOffsetS: 900 });
      e.launch('video', { skipBoot: true, uptimeOffsetS: 400 });
      e.select('tts');
    },
    startT: 120,
    searchS: 400,
  },
  {
    name: 'empty',
    label: 'Empty: no Systems',
    blurb: 'A fresh install: no Systems in klif.toml, no models directory. The shell shows the onboarding card.',
    world: emptyWorld,
    setup: () => {},
    startT: 0,
  },
  {
    name: 'node-down',
    label: 'Remote node unreachable',
    blurb: 'The multi world with the remote node offline: its System is unreachable, last known state only.',
    world: () => multiWorld('offline'),
    setup: (e) => {
      e.launch('s1', { skipBoot: true, uptimeOffsetS: 300 });
      e.select('render-box/cgi');
    },
    startT: 60,
  },
  {
    name: 'many',
    label: 'Many Systems (strip overflow)',
    blurb: 'Twelve Systems of every status, some with long labels and a remote suffix, to check the picker overflow.',
    world: () => {
      const w = kindsWorld();
      const more = [
        { id: 's4', label: 'Experimental coder with a long label', kind: 'llm' as const },
        { id: 'cgi2', label: 'CGI portraits', kind: 'image' as const, preset: 'qwen-image' },
        { id: 's5', label: 'System 5', kind: 'llm' as const, preset: 'flash-next-q2' },
        { id: 'tts2', label: 'Voice', kind: 'tts' as const },
      ];
      w.systems.splice(4, 0, ...more);
      w.selected = 's1';
      return w;
    },
    setup: (e) => {
      e.launch('s1', { skipBoot: true, uptimeOffsetS: 300 });
    },
    startT: 60,
  },
  {
    name: 'invalid',
    label: 'Invalid preset (model missing)',
    blurb: 'System 3 points at a preset whose model file is missing: status invalid, Launch refused with the reason.',
    world: () => {
      const w = multiWorld();
      w.systems = w.systems.map((s) => (s.id === 's3' && !s.node ? { ...s, preset: 'flash-next-q2' } : s));
      w.selected = 's3';
      return w;
    },
    setup: () => {},
    startT: 0,
  },
  {
    name: 'downloads',
    label: 'Downloads running',
    blurb: 'Two recommendation downloads in progress (the Tune drawer shows percentages).',
    world: multiWorld,
    setup: (e) => {
      e.select('s3');
      e.downloadRecommendation('whisper-large-v3-turbo-q8');
      e.downloadRecommendation('kokoro-82m');
    },
    startT: 6,
  },
  // ---- the 0.2 scenarios, on the operator's four Systems (low=s1, medium=s2, high=s3, krea=cgi) ----
  {
    name: 'idle',
    label: 'Idle (no session)',
    blurb: 'Nothing running. All four Systems ready; launch one. The last session (System 2, stopped 21 min ago) is summarised.',
    setup: (e) => {
      e.select('s2');
      e.setLastSession('s2', { model: e.systemModel('s2'), uptimeS: 8047, ended: 'stopped', requests: 12, generatedTokens: 11420, decodeTps: 47.3 }, 1260);
    },
    startT: 0,
  },
  boot('boot-medium', 's2', 'Boot: System 2', 'Starting, then loading steps with weights and VRAM layers building, then live.'),
  live('s2', 'live-medium', 'Live: System 2 (Qwen 27B)', 'Requests arrive, prefill bursts, decode at real speeds, history rolling, spec acceptance varying.', llmDecoding),
  {
    name: 'prefill-high',
    label: 'Prefill: System 3 (Flash-Next)',
    blurb: 'One 66k-token prompt prefilling at ~370 tok/s for ~3 min, then decode at ~9 tok/s.',
    setup: (e) => {
      e.select('s3');
      e.launch('s3', { skipBoot: true, uptimeOffsetS: 236, big: true });
    },
    startT: 0,
    shotUntil: (vm) => {
      const p = vm.session?.llm?.prefill;
      return !!p && vm.session?.llm?.activity === 'prefill' && p.doneTokens / p.tokens >= 0.42;
    },
    searchS: 200,
  },
  live('s3', 'live-high', 'Live: System 3 (Flash-Next)', 'Flash-Next replayed from real sessions: slow prefill, ~12 tok/s decode, high spec acceptance.', (vm) => {
    const l = vm.session?.llm;
    return !!l && l.activity !== 'idle' && l.requests.length >= 3;
  }),
  {
    name: 'spill-high',
    label: 'Spill: System 3 over the edge',
    blurb:
      'Flash-Next at 128k, one long prefill: compute buffers swell until the demand passes the VRAM edge. spillMiB ramps 0 to ~900 and relaxes back; free VRAM hits 0 while it spills.',
    setup: (e) => {
      e.select('s3');
      e.launch('s3', { skipBoot: true, uptimeOffsetS: 236, big: true, spill: true });
    },
    startT: 0,
    startUntil: (vm) => vm.vram.spillMiB >= 650,
    searchS: 400,
  },
  {
    name: 'warn',
    label: 'Warn: System 2, tight VRAM',
    blurb: 'System 2 decoding while another process takes ~0.15 GiB of VRAM: free VRAM drops below warnBelowGiB (0.15) but nothing spills.',
    setup: (e) => {
      e.select('s2');
      e.launch('s2');
      // A browser or video player grabs VRAM 560 s in; the baseline ('other' layer) steps up.
      e.scheduleBaseline(560, 0.27);
    },
    startT: 600,
    startUntil: liveSession('s2', (vm) => {
      const l = vm.session?.llm;
      const free = vm.vram.totalGiB - vm.vram.usedGiB;
      return !!l && l.activity === 'decode' && l.requests.length >= 3 && free < vm.vram.warnBelowGiB && vm.vram.spillMiB === 0;
    }),
    searchS: 3600,
  },
  dormantScenario(
    'dormant',
    'Dormant: GPU asleep (System 2)',
    'The 9070 XT drives no display, so ULPS puts it in D3 ~12 s after the last request: allocations stay, Dedicated Usage falls to ~0 (paged out to RAM). Every ~40 s a request wakes it: ~4 s at 0 tok/s while VRAM is restored. Freezes while asleep.',
    gpuAsleep,
  ),
  dormantScenario(
    'waking',
    'Waking: restore in flight (System 2)',
    'The same session frozen mid-wake: a request is waiting at 0 tok/s, ~7 GiB of the VRAM restored, ~8.4 GiB still paged out.',
    gpuWaking,
    0.05,
  ),
  live('s1', 'live-low', 'Live: System 1 (Gemma)', 'Gemma 4 26B-A4B: measured ~76-92 tok/s decode; request mix scaled to a 16k context (prefill speed estimated).', (vm) => {
    const l = vm.session?.llm;
    return !!l && l.activity === 'decode' && l.requests.length >= 4;
  }),
  {
    name: 'krea',
    label: 'Live: CGI (image jobs)',
    blurb: '8-step jobs replayed from real sessions: ~13 s plain, ~27 s edit, up to ~56 s for large edits.',
    setup: (e) => {
      e.select('cgi');
      e.launch('cgi');
    },
    startT: 300,
    startUntil: liveSession('cgi', (vm) => {
      const i = vm.session?.image;
      return !!i && i.activity === 'generating' && i.step >= 4 && i.recent.length >= 5;
    }),
    searchS: 1500,
  },
  {
    name: 'fault-krea',
    label: 'Fault: CGI crash',
    blurb: 'Image server dies mid edit job (exit code -1073740791 / 0xC0000409), real crash pattern.',
    setup: (e) => {
      e.select('cgi');
      e.launch('cgi', { crashOnJob: 4 });
    },
    startT: 0,
    startUntil: (vm) => vm.session?.phase === 'fault' && (vm.session.fault?.sinceS ?? 0) >= 15,
    searchS: 1500,
  },
  {
    name: 'fault-medium',
    label: 'Fault: server exited',
    blurb: 'The llama.cpp server exits 2 s after start with a model load error.',
    setup: (e) => {
      e.select('s2');
      e.launch('s2', { failAtS: 2, failKind: 'exit' });
    },
    startT: 0,
    startUntil: (vm) => vm.session?.phase === 'fault' && (vm.session.fault?.sinceS ?? 0) >= 6,
    searchS: 60,
  },
  boot('boot-high', 's3', 'Boot: System 3', 'Flash-Next loading (real log timings; cold loads can take 30 s).'),
  boot('boot-low', 's1', 'Boot: System 1', 'Gemma loading.'),
  boot('boot-krea', 'cgi', 'Boot: CGI', 'Image server loading (durations estimated).'),
];

export const DEFAULT_SCENARIO = 'multi';
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
  const e = new MockEngine(epoch0, hashSeed(def.name) ^ ctx.seed, ctx.hooks, (def.world ?? classicWorld)());
  e.host = browserHost(ctx.frameless);
  def.setup(e);
  if (ctx.t !== null) {
    e.fastForward(ctx.t);
    def.ready?.(e);
    return e;
  }
  e.fastForward(def.startT);
  const until = ctx.shot ? (def.shotUntil ?? def.startUntil) : def.startUntil;
  if (until) {
    const limit = def.searchS ?? 900;
    const step = def.searchStepS ?? 0.5;
    while (e.t < limit && !until(e.snapshot())) e.fastForward(e.t + step);
  }
  def.ready?.(e);
  return e;
}
