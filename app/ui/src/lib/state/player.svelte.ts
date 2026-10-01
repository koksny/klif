// Reactive wrapper around the mock engine: owns the current ViewModel, drives the 2 Hz telemetry
// tick (tier aware), switches scenarios, and exposes Actions to the skins and the shell surfaces.
import { DEFAULT_PORT } from '../mock/catalog';
import { MockEngine, type EngineHooks, type Extras } from '../mock/engine';
import { browserHost } from '../mock/host';
import { buildEngine, DEFAULT_SCENARIO, SCENARIOS, scenarioDef, SHOT_DEFAULT_SCENARIO } from '../mock/scenarios';
import { SAMPLE_VM } from '../model/sample';
import type { Actions, ModelRef, SlotId, ViewModel } from '../model/types';
import { getTier } from '../render/scheduler';
import type { Params } from './params';
import { ui } from './ui.svelte';

const TICK_MS = 500;
const MOCK_KEY = 'klif-mock-key-not-a-secret';

function defaultExtras(): Record<SlotId, Extras> {
  return {
    high: { promptCacheMiB: 8192, port: DEFAULT_PORT.high },
    medium: { promptCacheMiB: 8192, port: DEFAULT_PORT.medium },
    low: { promptCacheMiB: 8192, port: DEFAULT_PORT.low },
    krea: { promptCacheMiB: 0, port: DEFAULT_PORT.krea },
  };
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    try {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand('copy');
      ta.remove();
      return ok;
    } catch {
      return false;
    }
  }
}

class Player {
  /** Replaced wholesale every tick; never mutated, so it is held raw (no deep proxy over 300-sample arrays). */
  vm = $state.raw<ViewModel>(SAMPLE_VM);
  scenario = $state<string>(SHOT_DEFAULT_SCENARIO);
  speed = $state(1);
  paused = $state(false);
  /** Simulated seconds since the scenario started (dev bar). */
  simT = $state(0);
  /** Per-slot settings that are not part of ModelRef (Tune drawer). */
  extras = $state<Record<SlotId, Extras>>(defaultExtras());

  private engine: MockEngine | null = null;
  private timer: ReturnType<typeof setTimeout> | 0 = 0;
  private lastWall = 0;
  private params: Params | null = null;

  private readonly hooks: EngineHooks = {
    toast: (t) => ui.toast(t),
    toggleConsole: (open) => ui.toggleConsole(open),
    openTune: (slot) => ui.openTune(slot),
    copy: (text, toastText) => {
      void writeClipboard(text).then((ok) => ui.toast(ok ? toastText : 'Clipboard is not available here.'));
    },
    openUrl: (url) => {
      window.open(url, '_blank', 'noopener');
    },
  };

  /** Actions handed to every skin. Always safe to call; the static sample reacts too. */
  readonly actions: Actions = {
    select: (slot) => {
      this.live().select(slot);
      this.publish();
    },
    launch: (slot) => {
      this.live().launch(slot);
      this.publish();
    },
    stop: () => {
      if (!this.engine) return ui.toast('The static sample has no running session to stop.');
      this.engine.stop();
      this.publish();
    },
    restart: () => {
      if (!this.engine) return ui.toast('The static sample cannot restart.');
      this.engine.restart();
      this.publish();
    },
    openEndpoint: () => {
      if (this.engine) this.engine.openEndpoint();
      else this.hooks.openUrl('http://127.0.0.1:7030/');
    },
    copyEndpoint: () => {
      if (this.engine) this.engine.copyEndpoint();
      else this.hooks.copy('http://127.0.0.1:7030/v1', 'Endpoint copied');
    },
    copyApiKey: () => {
      if (this.engine) this.engine.copyApiKey();
      else this.hooks.copy(MOCK_KEY, 'API key copied');
    },
    toggleConsole: (open) => ui.toggleConsole(open),
    openTune: (slot) => ui.openTune(slot ?? this.vm.selected),
    dismiss: () => {
      if (!this.engine) return ui.toast('The static sample has nothing to dismiss.');
      this.engine.dismiss();
      this.publish();
    },
    // Window chrome belongs to the desktop host (Tauri). In the browser there is no window to control.
    minimize: () => ui.toast('Window controls work in the desktop app.'),
    toggleMaximize: () => ui.toast('Window controls work in the desktop app.'),
    closeWindow: () => ui.toast('Window controls work in the desktop app.'),
  };

  /** Call once at startup. */
  init(params: Params) {
    this.params = params;
    this.speed = params.speed;
    const name = params.scenario ?? (params.shot ? SHOT_DEFAULT_SCENARIO : DEFAULT_SCENARIO);
    const def = scenarioDef(name);
    if (!def) ui.toast(`Unknown scenario "${name}". Using ${DEFAULT_SCENARIO}.`);
    this.setScenario(def?.name ?? DEFAULT_SCENARIO, params.t);
    // shot=1 freezes the player so screenshots are reproducible; run=1 lets it tick anyway.
    this.paused = params.shot && !params.run;
    this.start();
  }

  setScenario(name: string, t: number | null = null) {
    const def = scenarioDef(name) ?? scenarioDef(DEFAULT_SCENARIO)!;
    this.scenario = def.name;
    if (def.isStatic) {
      this.engine = null;
      this.vm = { ...SAMPLE_VM, host: browserHost(this.params?.frameless ?? false) };
      this.simT = 0;
      return;
    }
    this.engine = buildEngine(def, {
      hooks: this.hooks,
      extras: (s) => this.extras[s],
      shot: ui.shot,
      frameless: this.params?.frameless ?? false,
      t,
      seed: this.params?.seed ?? 0,
    });
    this.publish();
  }

  /** Static sample: any state-changing interaction leaves it for the idle scenario. */
  private live(): MockEngine {
    if (!this.engine) {
      ui.toast('Leaving the static sample for the idle scenario.');
      this.setScenario('idle');
    }
    return this.engine!;
  }

  // ---- Tune drawer API ------------------------------------------------------------------------------

  applyModel(slot: SlotId, model: ModelRef) {
    this.live().setSlotModel(slot, model);
    this.publish();
  }

  setExtras(slot: SlotId, patch: Partial<Extras>) {
    this.extras = { ...this.extras, [slot]: { ...this.extras[slot], ...patch } };
  }

  // ---- ticking ----------------------------------------------------------------------------------------

  private publish() {
    if (!this.engine) return;
    this.vm = this.engine.snapshot();
    this.simT = this.engine.t;
  }

  private start() {
    if (this.timer) return;
    this.lastWall = performance.now();
    const loop = () => {
      const tier = getTier();
      const now = performance.now();
      const wallDt = Math.min(1.5, Math.max(0, (now - this.lastWall) / 1000));
      this.lastWall = now;
      if (this.engine && !this.paused && tier !== 'off') {
        this.engine.advance(wallDt * this.speed);
        this.publish();
      }
      // calm: one telemetry update per second; otherwise the normal 2 Hz.
      this.timer = setTimeout(loop, tier === 'calm' ? TICK_MS * 2 : TICK_MS);
    };
    this.timer = setTimeout(loop, TICK_MS);
  }

  stopTimer() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = 0;
  }

  /** Dev bar: single step while paused. */
  stepOnce(seconds = 0.5) {
    if (!this.engine) return;
    this.engine.advance(seconds);
    this.publish();
  }

  get scenarios() {
    return SCENARIOS;
  }
}

export const player = new Player();
