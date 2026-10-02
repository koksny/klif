// Reactive owner of the current ViewModel and of the Actions handed to the skins and shell surfaces.
// Two sources:
//   - native (inside the Tauri shell): the Rust core pushes view models; actions go back over IPC;
//   - mock (any browser): the deterministic mock engine, ticked here at 2 Hz (tier aware), with scenarios.
import { MockEngine, type EngineHooks } from '../mock/engine';
import { withDefaultRecipes } from '../mock/catalog';
import { browserHost } from '../mock/host';
import { buildEngine, DEFAULT_SCENARIO, SCENARIOS, scenarioDef, SHOT_DEFAULT_SCENARIO } from '../mock/scenarios';
import { SAMPLE_VM } from '../model/sample';
import type { Actions, ViewModel } from '../model/types';
import { getTier } from '../render/scheduler';
import { IN_TAURI, loadNative, type EngineAction, type GpuReport, type NativeLink } from '../transport';
import type { SizeClass } from '../../skins/contract';
import { skinMeta } from '../../skins/registry';
import type { Params } from './params';
import { ui } from './ui.svelte';

const TICK_MS = 500;
const MOCK_KEY = 'klif-mock-key-not-a-secret';

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

/**
 * In a browser panel mode is the mini layout override (there is no window to move), so the panel flags
 * follow the layout actually shown. The desktop app reports them itself (HostInfo.panel).
 */
function browserPanel(vm: ViewModel, size: SizeClass): ViewModel {
  const active = size === 'mini';
  const p = vm.host.panel;
  if (p.available && p.active === active) return vm;
  return { ...vm, host: { ...vm.host, panel: { available: true, active, target: p.target ?? '960x640' } } };
}

class Player {
  /**
   * The view model as the core (native) or the mock engine produced it. Replaced wholesale every tick;
   * never mutated, so it is held raw (no deep proxy over 300-sample arrays).
   */
  private source = $state.raw<ViewModel>(SAMPLE_VM);
  private readonly view = $derived.by(() => (IN_TAURI ? this.source : browserPanel(this.source, ui.size)));

  /** What the skins and shell surfaces read. */
  get vm(): ViewModel {
    return this.view;
  }
  /** True when the native core drives the UI (Tauri shell). */
  readonly native: boolean = IN_TAURI;
  /** False until the native core delivered its first view model. Always true for the mock. */
  ready = $state(!IN_TAURI);
  /** Set when the native core could not be reached. */
  nativeError = $state<string | null>(null);

  scenario = $state<string>(SHOT_DEFAULT_SCENARIO);
  speed = $state(1);
  paused = $state(false);
  /** Simulated seconds since the scenario started (dev bar). */
  simT = $state(0);

  private engine: MockEngine | null = null;
  private link: NativeLink | null = null;
  private timer: ReturnType<typeof setTimeout> | 0 = 0;
  private lastWall = 0;
  private params: Params | null = null;

  private readonly hooks: EngineHooks = {
    toast: (t) => ui.toast(t),
    toggleConsole: (open) => ui.toggleConsole(open),
    togglePanel: () => ui.toggleSize(false),
    openTune: (slot) => ui.openTune(slot),
    copy: (text, toastText) => {
      void writeClipboard(text).then((ok) => ui.toast(ok ? toastText : 'Clipboard is not available here.'));
    },
    openUrl: (url) => {
      window.open(url, '_blank', 'noopener');
    },
  };

  /** Actions handed to every skin. Always safe to call. */
  readonly actions: Actions = IN_TAURI ? this.nativeActions() : this.mockActions();

  /** Call once at startup. */
  init(params: Params) {
    this.params = params;
    if (this.native) {
      void this.connectNative();
      return;
    }
    this.speed = params.speed;
    const name = params.scenario ?? (params.shot ? SHOT_DEFAULT_SCENARIO : DEFAULT_SCENARIO);
    const def = scenarioDef(name);
    if (!def) ui.toast(`Unknown scenario "${name}". Using ${DEFAULT_SCENARIO}.`);
    this.setScenario(def?.name ?? DEFAULT_SCENARIO, params.t);
    // shot=1 freezes the player so screenshots are reproducible; run=1 lets it tick anyway.
    this.paused = params.shot && !params.run;
    this.start();
  }

  // ---- native (Tauri) ---------------------------------------------------------------------------------

  private async connectNative() {
    try {
      const { connect, errorText } = await loadNative();
      this.link = await connect({
        vm: (vm) => {
          this.source = vm;
          // Panel mode moves the window onto the small screen: the layout is mini whatever the viewport.
          ui.panelActive = vm.host.panel.active;
          this.ready = true;
        },
        skin: (id) => {
          const meta = skinMeta(id);
          ui.setSkin(meta.id);
          ui.toast(`Skin: ${meta.name}`);
        },
        gpu: (g) => this.onGpu(g),
      });
      this.nativeErrorText = errorText;
    } catch (e) {
      this.nativeError = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
      console.error('[klif] native core unreachable', e);
    }
  }

  private nativeErrorText: (e: unknown) => string = (e) => String(e);

  private onGpu(g: GpuReport) {
    if (g.ok) return;
    const where = g.adapter ?? 'an unknown adapter';
    ui.toast(g.expected ? `The UI renders on ${where}, not on ${g.expected}.` : `The UI renders on ${where}.`, 8000);
  }

  /** Run a native call; failures become toasts (they are user-facing sentences from the core). */
  private call(run: (l: NativeLink) => Promise<void>, okToast?: string) {
    const l = this.link;
    if (!l) {
      ui.toast(this.nativeError ? `KLIF core unavailable: ${this.nativeError}` : 'Connecting to the KLIF core.');
      return;
    }
    run(l).then(
      () => {
        if (okToast) ui.toast(okToast);
      },
      (e: unknown) => ui.toast(this.nativeErrorText(e)),
    );
  }

  private act(action: EngineAction) {
    this.call((l) => l.act(action));
  }

  private nativeActions(): Actions {
    return {
      select: (slot) => this.act({ type: 'select', slot }),
      launch: (slot) => this.act(slot ? { type: 'launch', slot } : { type: 'launch' }),
      stop: () => this.act({ type: 'stop' }),
      restart: () => this.act({ type: 'restart' }),
      dismiss: () => this.act({ type: 'dismiss' }),
      setRecipe: (slot, patch) => this.act({ type: 'setRecipe', slot, patch }),
      openEndpoint: () => this.call((l) => l.openEndpoint()),
      copyEndpoint: () => this.call((l) => l.copyEndpoint(), 'Endpoint copied'),
      copyApiKey: () => this.call((l) => l.copyApiKey(), 'API key copied'),
      toggleConsole: (open) => ui.toggleConsole(open),
      openTune: (slot) => ui.openTune(slot ?? this.vm.selected),
      minimize: () => this.call((l) => l.minimize()),
      toggleMaximize: () => this.call((l) => l.toggleMaximize()),
      closeWindow: () => this.call((l) => l.close()),
      togglePanel: () => this.call((l) => l.togglePanel()),
    };
  }

  // ---- mock (browser) ---------------------------------------------------------------------------------

  private mockActions(): Actions {
    return {
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
      setRecipe: (slot, patch) => {
        this.live().setRecipe(slot, patch);
        this.publish();
      },
      // Window chrome belongs to the desktop host (Tauri). In the browser there is no window to control.
      minimize: () => ui.toast('Window controls work in the desktop app.'),
      toggleMaximize: () => ui.toast('Window controls work in the desktop app.'),
      closeWindow: () => ui.toast('Window controls work in the desktop app.'),
      // Panel mode in a browser = the mini layout override, the same switch as F3.
      togglePanel: () => this.hooks.togglePanel(),
    };
  }

  setScenario(name: string, t: number | null = null) {
    if (this.native) return;
    const def = scenarioDef(name) ?? scenarioDef(DEFAULT_SCENARIO)!;
    this.scenario = def.name;
    if (def.isStatic) {
      this.engine = null;
      this.source = { ...SAMPLE_VM, slots: withDefaultRecipes(SAMPLE_VM.slots), host: browserHost(this.params?.frameless ?? false) };
      this.simT = 0;
      return;
    }
    this.engine = buildEngine(def, {
      hooks: this.hooks,
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

  // ---- ticking (mock) ---------------------------------------------------------------------------------

  private publish() {
    if (!this.engine) return;
    this.source = this.engine.snapshot();
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
    this.link?.disconnect();
    this.link = null;
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
