// Shell UI state: active skin, size class, drawers, toasts, dev bar. One instance for the whole app.
import type { SystemId } from '../model/types';
import type { Tier } from '../render/scheduler';
import type { SizeClass, SkinId } from '../../skins/contract';
import { DEFAULT_SKIN, SKINS, skinMeta } from '../../skins/registry';
import { parseParams, type Params, type SizeParam } from './params';

const SKIN_KEY = 'klif.skin';

function readStoredSkin(): string | null {
  try {
    return localStorage.getItem(SKIN_KEY);
  } catch {
    return null;
  }
}

function writeStoredSkin(id: string) {
  try {
    localStorage.setItem(SKIN_KEY, id);
  } catch {
    /* storage can be blocked; the skin choice then simply is not remembered */
  }
}

export interface ToastItem {
  id: number;
  text: string;
}

class UiState {
  readonly params: Params = parseParams(typeof location === 'undefined' ? '' : location.search);

  skinId = $state<SkinId>(DEFAULT_SKIN);
  /** auto = decide from the viewport; full/mini = explicit (URL or F3). */
  sizeMode = $state<SizeParam>('auto');
  vw = $state(typeof innerWidth === 'undefined' ? 1024 : innerWidth);
  vh = $state(typeof innerHeight === 'undefined' ? 1152 : innerHeight);

  consoleOpen = $state(false);
  tuneOpen = $state(false);
  /** The System the Tune drawer shows (null: the selected one). */
  tuneSystem = $state<SystemId | null>(null);
  /** The Tune drawer is in add mode ("Add System"). Opened by the "+" of every full-size picker. */
  tuneAdd = $state(false);
  /** Old name of tuneSystem (the drawer used to be keyed by slot). */
  get tuneSlot(): SystemId | null {
    return this.tuneSystem;
  }
  set tuneSlot(id: SystemId | null) {
    this.tuneSystem = id;
  }

  devbar = $state(false);
  /** Tier currently applied (mirrors data-tier on <html>). */
  tier = $state<Tier>('ambient');
  /** null = automatic; otherwise pinned from the dev bar or ?tier=. */
  tierPin = $state<Tier | null>(null);

  toasts = $state<ToastItem[]>([]);
  private toastSeq = 0;

  /** shot=1: deterministic frame for screenshots (no dev bar, no persistence, frozen player). */
  readonly shot: boolean = this.params.shot;

  /**
   * Panel mode of the desktop app (view model: host.panel.active): the window sits on the small status
   * screen, so the layout is mini whatever the viewport says. Set by the player from every native view
   * model; it stays false in a browser, where panel mode is just the `sizeMode` override.
   */
  panelActive = $state(false);

  /** Size class the skin renders at. */
  size: SizeClass = $derived.by(() => {
    if (this.panelActive) return 'mini';
    if (this.sizeMode !== 'auto') return this.sizeMode;
    return this.vw > this.vh && this.vh <= 700 ? 'mini' : 'full';
  });

  /** mini on a viewport that is not the 960x640 panel: draw it in a centred 960x640 frame. */
  miniFramed: boolean = $derived(this.size === 'mini' && !(this.vw > this.vh && this.vh <= 700));

  constructor() {
    this.sizeMode = this.params.size;
    const wanted = this.params.skin ?? (this.shot ? null : readStoredSkin());
    this.skinId = skinMeta(wanted).id;
    this.tierPin = this.params.tier;
    if (this.params.drawer === 'console') this.consoleOpen = true;
    if (this.params.drawer === 'tune') {
      this.tuneOpen = true;
      this.tuneSystem = this.params.system;
      this.tuneAdd = this.params.add;
    }
  }

  setSkin(id: SkinId) {
    if (id === this.skinId) return;
    this.skinId = skinMeta(id).id;
    if (!this.shot) writeStoredSkin(this.skinId);
  }

  cycleSkin(dir: 1 | -1) {
    const i = SKINS.findIndex((s) => s.id === this.skinId);
    const next = SKINS[(i + dir + SKINS.length) % SKINS.length];
    this.setSkin(next.id);
    this.toast(`Skin: ${next.name}`);
  }

  skinByIndex(n: number) {
    const s = SKINS[n];
    if (s) this.setSkin(s.id);
  }

  setSizeMode(mode: SizeParam) {
    this.sizeMode = mode;
  }

  /** F3 in a browser, and panel mode there. `announce` = say which layout it switched to (a toast). */
  toggleSize(announce = true) {
    this.sizeMode = this.size === 'full' ? 'mini' : 'full';
    if (announce) this.toast(`Size: ${this.sizeMode}`);
  }

  toggleConsole(open?: boolean) {
    this.consoleOpen = open ?? !this.consoleOpen;
  }

  /** Open the Tune drawer on a System (add mode: the "Add System" form). */
  openTune(system?: SystemId, opts: { add?: boolean } = {}) {
    if (system) this.tuneSystem = system;
    this.tuneAdd = !!opts.add;
    this.tuneOpen = true;
  }

  /** Open the Tune drawer in add mode (the "+" after the last System tab). */
  openTuneAdd() {
    this.openTune(undefined, { add: true });
  }

  /**
   * Set by the Tune drawer while it holds unsaved edits: returns false to keep the drawer open (it asks the
   * user first). Not reactive on purpose.
   */
  tuneCanClose: (() => boolean) | null = null;

  closeTune() {
    if (this.tuneCanClose && !this.tuneCanClose()) return;
    this.tuneOpen = false;
    this.tuneAdd = false;
  }

  /** Escape. Drawers are hidden in mini (Tune keeps its drafts there), so mini closes nothing. */
  closeDrawers(): boolean {
    if (this.size !== 'full') return false;
    if (this.tuneOpen) {
      this.closeTune();
      return true;
    }
    if (this.consoleOpen) {
      this.consoleOpen = false;
      return true;
    }
    return false;
  }

  toast(text: string, ms = 2400) {
    const id = ++this.toastSeq;
    this.toasts = [...this.toasts.slice(-2), { id, text }];
    setTimeout(() => {
      this.toasts = this.toasts.filter((t) => t.id !== id);
    }, ms);
  }
}

export const ui = new UiState();
