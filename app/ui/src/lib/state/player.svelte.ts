// Reactive owner of the current ViewModel and of the Actions handed to the skins and shell surfaces.
// Two sources:
//   - native (inside the Tauri shell): the Rust core pushes view models; actions go back over IPC;
//   - mock (any browser): the deterministic mock engine, ticked here at 2 Hz (tier aware), with scenarios.
//
// Engine actions return a Promise that rejects with the engine's sentence. The failure is ALSO shown as a toast
// here (once, for every surface), and the returned promise is pre-handled, so a caller that does not care can
// ignore it without an unhandled-rejection report; a caller that shows it inline (Tune) passes { quiet: true }
// and gets no toast.
import { MockEngine, type EngineHooks } from '../mock/engine';
import { browserHost } from '../mock/host';
import { buildEngine, DEFAULT_SCENARIO, SCENARIOS, scenarioDef, SHOT_DEFAULT_SCENARIO } from '../mock/scenarios';
import { classicWorld } from '../mock/world';
import { SAMPLE_VM } from '../model/sample';
import type { Actions, CallOpts, CommandView, ConfigApi, PresetDetail, PresetSpec, SystemId, ViewModel } from '../model/types';
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

function sentence(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

/** A rejected promise nobody has to catch (the failure is already shown as a toast). */
function failed(message: string): Promise<never> {
  const p = Promise.reject(new Error(message));
  p.catch(() => {});
  return p;
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
  /** While connecting: why the core is not up yet (another process holds the engine), else null. */
  nativeWaiting = $state<string | null>(null);

  scenario = $state<string>(SHOT_DEFAULT_SCENARIO);
  speed = $state(1);
  paused = $state(false);
  /** Simulated seconds since the scenario started (dev bar). */
  simT = $state(0);

  private engine: MockEngine | null = null;
  /** Backs ConfigApi reads while the static sample is shown (no live engine yet). */
  private readOnlyEngine: MockEngine | null = null;
  private link: NativeLink | null = null;
  private timer: ReturnType<typeof setTimeout> | 0 = 0;
  private lastWall = 0;
  private params: Params | null = null;

  private readonly hooks: EngineHooks = {
    toast: (t) => ui.toast(t),
    toggleConsole: (open) => ui.toggleConsole(open),
    togglePanel: () => ui.toggleSize(false),
    openTune: (system, opts) => ui.openTune(system, opts),
    copy: (text, toastText) => {
      void writeClipboard(text).then((ok) => ui.toast(ok ? toastText : 'Clipboard is not available here.'));
    },
    openUrl: (url) => {
      window.open(url, '_blank', 'noopener');
    },
  };

  /** Actions handed to every skin. Always safe to call. */
  readonly actions: Actions = IN_TAURI ? this.nativeActions() : this.mockActions();
  /** Config-level calls for the Tune drawer (preset detail, command preview, API key, open klif.toml / logs). */
  readonly config: ConfigApi = IN_TAURI ? this.nativeConfig() : this.mockConfig();

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
      const { connect, engineStatus, errorText } = await loadNative();
      // The first snapshot waits for the engine; meanwhile say why when another process holds it.
      const poll = setInterval(async () => {
        if (this.ready || this.nativeError) return;
        try {
          const s = await engineStatus();
          this.nativeWaiting = s.state === 'waiting' ? (s.message ?? null) : null;
        } catch {
          // The status is a convenience: the connect below reports real failures.
        }
      }, 2000);
      try {
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
      } finally {
        clearInterval(poll);
        this.nativeWaiting = null;
      }
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

  /** Run a native call; failures become a toast (unless quiet) and a rejection with the core's sentence. */
  private call(run: (l: NativeLink) => Promise<void>, okToast?: string, quiet = false): Promise<void> {
    const l = this.link;
    if (!l) {
      const m = this.nativeError ? `KLIF core unavailable: ${this.nativeError}` : 'Connecting to the KLIF core.';
      if (!quiet) ui.toast(m);
      return failed(m);
    }
    const p = run(l).then(
      () => {
        if (okToast) ui.toast(okToast);
      },
      (e: unknown) => {
        const m = this.nativeErrorText(e);
        if (!quiet) ui.toast(m);
        throw new Error(m);
      },
    );
    p.catch(() => {});
    return p;
  }

  private act(action: EngineAction, call?: CallOpts): Promise<void> {
    return this.call((l) => l.act(action), undefined, !!call?.quiet);
  }

  private nativeActions(): Actions {
    return {
      select: (system, call) => this.act({ type: 'select', system }, call),
      launch: (system, opts) =>
        this.act({ type: 'launch', ...(system ? { system } : {}), ...(opts?.stopOthers ? { stopOthers: true } : {}) }, opts),
      stop: (system, call) => this.act({ type: 'stop', ...(system ? { system } : {}) }, call),
      stopAll: (call) => this.act({ type: 'stopAll' }, call),
      restart: (system, call) => this.act({ type: 'restart', ...(system ? { system } : {}) }, call),
      dismiss: (system, call) => this.act({ type: 'dismiss', ...(system ? { system } : {}) }, call),
      usePreset: (system, preset, call) => this.act({ type: 'usePreset', system, preset }, call),
      setParam: (system, name, value, call) => this.act({ type: 'setParam', system, name, value }, call),
      savePreset: (id, preset, opts) => {
        // quiet is how the UI reports the call; it is not part of the engine action.
        const { quiet, ...rest } = opts ?? {};
        return this.act({ type: 'savePreset', id, preset, ...rest }, { quiet });
      },
      deletePreset: (id, node, call) => this.act({ type: 'deletePreset', id, ...(node ? { node } : {}) }, call),
      addSystem: (spec, call) => this.act({ type: 'addSystem', ...spec }, call),
      removeSystem: (system, call) => this.act({ type: 'removeSystem', system }, call),
      updateSystem: (system, patch, call) => this.act({ type: 'updateSystem', system, ...patch }, call),
      downloadRecommendation: (id, node, call) => this.act({ type: 'downloadRecommendation', id, ...(node ? { node } : {}) }, call),
      cancelDownload: (id, node, call) => this.act({ type: 'cancelDownload', id, ...(node ? { node } : {}) }, call),
      adoptRecommendation: (id, system, call) => this.act({ type: 'adoptRecommendation', id, ...(system ? { system } : {}) }, call),
      openEndpoint: (system) => void this.call((l) => l.openEndpoint(system)).catch(() => {}),
      copyEndpoint: (system) => void this.call((l) => l.copyEndpoint(system), 'Endpoint copied').catch(() => {}),
      copyApiKey: () => void this.call((l) => l.copyApiKey(), 'API key copied').catch(() => {}),
      toggleConsole: (open) => ui.toggleConsole(open),
      openTune: (system, opts) => ui.openTune(system ?? this.vm.selected ?? undefined, opts),
      minimize: () => void this.call((l) => l.minimize()).catch(() => {}),
      toggleMaximize: () => void this.call((l) => l.toggleMaximize()).catch(() => {}),
      closeWindow: () => void this.call((l) => l.close()).catch(() => {}),
      togglePanel: () => void this.call((l) => l.togglePanel()).catch(() => {}),
    };
  }

  private nativeConfig(): ConfigApi {
    const need = (): NativeLink => {
      if (!this.link) throw new Error(this.nativeError ? `KLIF core unavailable: ${this.nativeError}` : 'Connecting to the KLIF core.');
      return this.link;
    };
    const wrap = async <T>(run: (l: NativeLink) => Promise<T>): Promise<T> => {
      try {
        return await run(need());
      } catch (e) {
        throw new Error(this.nativeErrorText(e));
      }
    };
    return {
      presetGet: (id, node) => wrap((l) => l.presetGet(id, node)),
      commandPreview: (spec, system) => wrap((l) => l.commandPreview(spec, system)),
      setApiKey: (key) => wrap((l) => l.setApiKey(key)),
      openConfig: () => wrap((l) => l.openConfig()),
      openLogs: () => wrap((l) => l.openLogs()),
    };
  }

  // ---- mock (browser) ---------------------------------------------------------------------------------

  /** Run an engine action on the mock; refusals become a toast (unless quiet) and a rejection with the sentence. */
  private mock(run: (e: MockEngine) => void, call?: CallOpts): Promise<void> {
    try {
      run(this.live());
      this.publish();
      return Promise.resolve();
    } catch (e) {
      const m = sentence(e);
      if (!call?.quiet) ui.toast(m);
      return failed(m);
    }
  }

  /** A shell-level mock action (clipboard, URL): failures only toast. */
  private shellMock(run: (e: MockEngine) => void) {
    try {
      run(this.engine ?? this.live());
    } catch (e) {
      ui.toast(sentence(e));
    }
  }

  private mockActions(): Actions {
    return {
      select: (system, call) => this.mock((e) => e.select(system), call),
      launch: (system, opts) => this.mock((e) => e.launch(system, opts?.stopOthers ? { stopOthers: true } : {}), opts),
      stop: (system, call) => this.mock((e) => e.stop(system), call),
      stopAll: (call) => this.mock((e) => e.stopAll(), call),
      restart: (system, call) => this.mock((e) => e.restart(system), call),
      dismiss: (system, call) => this.mock((e) => e.dismiss(system), call),
      usePreset: (system, preset, call) => this.mock((e) => e.usePreset(system, preset), call),
      setParam: (system, name, value, call) => this.mock((e) => e.setParam(system, name, value), call),
      savePreset: (id, preset, opts) => {
        const { quiet, ...rest } = opts ?? {};
        return this.mock((e) => e.savePreset(id, preset, rest), { quiet });
      },
      deletePreset: (id, node, call) => this.mock((e) => e.deletePreset(id, node), call),
      addSystem: (spec, call) => this.mock((e) => e.addSystem(spec), call),
      removeSystem: (system, call) => this.mock((e) => e.removeSystem(system), call),
      updateSystem: (system, patch, call) => this.mock((e) => e.updateSystem(system, patch), call),
      downloadRecommendation: (id, node, call) => this.mock((e) => e.downloadRecommendation(id, node), call),
      cancelDownload: (id, node, call) => this.mock((e) => e.cancelDownload(id, node), call),
      adoptRecommendation: (id, system, call) => this.mock((e) => e.adoptRecommendation(id, system), call),
      openEndpoint: (system) => this.shellMock((e) => e.openEndpoint(system)),
      copyEndpoint: (system) => this.shellMock((e) => e.copyEndpoint(system)),
      copyApiKey: () => {
        if (this.engine) this.shellMock((e) => e.copyApiKey());
        else this.hooks.copy(MOCK_KEY, 'API key copied');
      },
      toggleConsole: (open) => ui.toggleConsole(open),
      openTune: (system, opts) => ui.openTune(system ?? this.vm.selected ?? undefined, opts),
      // Window chrome belongs to the desktop host (Tauri). In the browser there is no window to control.
      minimize: () => ui.toast('Window controls work in the desktop app.'),
      toggleMaximize: () => ui.toast('Window controls work in the desktop app.'),
      closeWindow: () => ui.toast('Window controls work in the desktop app.'),
      // Panel mode in a browser = the mini layout override, the same switch as F3.
      togglePanel: () => this.hooks.togglePanel(),
    };
  }

  private mockConfig(): ConfigApi {
    const read = (): MockEngine => {
      if (this.engine) return this.engine;
      this.readOnlyEngine ??= new MockEngine(SAMPLE_VM.now, 1, this.hooks, classicWorld());
      return this.readOnlyEngine;
    };
    const later = <T>(run: () => T): Promise<T> => {
      try {
        return Promise.resolve(run());
      } catch (e) {
        return Promise.reject(new Error(sentence(e)));
      }
    };
    return {
      presetGet: (id: string, node?: string): Promise<PresetDetail | null> => later(() => read().presetGet(id, node)),
      commandPreview: (spec: PresetSpec, system?: SystemId): Promise<CommandView> => later(() => read().commandPreview(spec, system)),
      // Tune shows the outcome inline (the native setApiKey does not toast either).
      setApiKey: (key) => this.mock((e) => e.setApiKey(key), { quiet: true }),
      openConfig: () => {
        ui.toast('Opens klif.toml in the desktop app.');
        return Promise.resolve();
      },
      openLogs: () => {
        ui.toast('Opens the logs folder in the desktop app.');
        return Promise.resolve();
      },
    };
  }

  setScenario(name: string, t: number | null = null) {
    if (this.native) return;
    const def = scenarioDef(name) ?? scenarioDef(DEFAULT_SCENARIO)!;
    this.scenario = def.name;
    if (def.isStatic) {
      this.engine = null;
      this.source = { ...SAMPLE_VM, host: browserHost(this.params?.frameless ?? false) };
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

  /** Static sample: any state-changing interaction leaves it for the live System 2 scenario. */
  private live(): MockEngine {
    if (!this.engine) {
      ui.toast('Leaving the static sample for a live scenario.');
      this.setScenario('live-medium');
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
