// The skins' slot keys: three buttons in the control row for the selected System's conversation slots. A click loads
// the slot, holding it (HOLD_MS) saves to it; the press fills the key as it is held. While the System is stopped,
// holding a saved slot launches the System and loads that slot once it is up. Each skin draws the buttons in
// its own style and adds `use:slotKey`; the shared states (hold fill, saved and active marks, a running job) come
// from slotKeys.css through data attributes, in currentColor, so they follow the skin.
import type { Action } from 'svelte/action';
import { canLaunch, launchCtl } from '../../model/systems';
import type { System } from '../../model/types';
import { player } from '../../state/player.svelte';
import { ui } from '../../state/ui.svelte';
import './slotKeys.css';

/** How long a key is held to save. */
export const HOLD_MS = 800;
/** A press released before this loads; a longer press that is let go early does nothing (a save given up). */
const TAP_MS = 300;

export interface SlotKey {
  n: number;
  saved: boolean;
  /** Saved with another launch command: it cannot be loaded into this one. */
  stale: boolean;
  active: boolean;
  job?: 'save' | 'load';
  /** The key is enabled: the System runs and its server can save slots (a job only makes it wait), or it is
   *  stopped and holding the key launches it with this slot. */
  ready: boolean;
  canLoad: boolean;
  canSave: boolean;
  /** Stopped: holding launches the System (stopping the ones that conflict, `stopOthers`) and loads this slot. */
  canLaunch: boolean;
  stopOthers: boolean;
  title: string;
  /** The data attributes the shared CSS reads; spread them on the button. */
  attrs: Record<string, string | undefined>;
}

const fmtTokens = (n?: number) => (n === undefined ? '' : `${n.toLocaleString('en-US').replace(/,/g, ' ')} tokens`);
const fmtWhen = (s: number) =>
  new Date(s * 1000).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });

/** The three keys of a System with slots on (none otherwise). */
export function slotKeys(s: System | undefined | null): SlotKey[] {
  const v = s?.slots;
  if (!s || !v) return [];
  const running = s.status === 'online' || s.status === 'busy';
  const ready = running && s.controllable && v.supported;
  const free = ready && !v.job;
  const ctl = canLaunch(s) && v.supported ? launchCtl(player.vm, s) : null;
  const launchWords = ctl?.stopOthers ? `stop ${ctl.names.join(' and ')}, launch ${s.label}` : `launch ${s.label}`;
  return v.entries.map((e) => {
    const saved = e.savedAt !== undefined;
    const stale = saved && !e.compatible;
    const launchable = !!ctl?.enabled && saved && !stale;
    const job = v.job?.slot === e.n ? v.job.op : undefined;
    const name = e.name ? `“${e.name}”` : `Slot ${e.n}`;
    const what = saved
      ? `${name}: ${[fmtTokens(e.tokens), fmtWhen(e.savedAt!)].filter(Boolean).join(', ')}${stale ? ' (saved with another command)' : ''}`
      : `${name}: empty`;
    const how = !v.supported
      ? 'Its server cannot save slots.'
      : !running
        ? launchable
          ? `Hold to ${launchWords} and load it.`
          : `Launch ${s.label} to save or load.`
        : v.job
          ? `${v.job.op === 'save' ? 'Saving' : 'Loading'} slot ${v.job.slot}…`
          : saved && !stale
            ? 'Click to load, hold to save over it.'
            : 'Hold to save.';
    return {
      n: e.n,
      saved,
      stale,
      active: v.active === e.n,
      job,
      ready: ready || launchable,
      canLoad: free && saved && !stale,
      canSave: free,
      canLaunch: launchable,
      stopOthers: !!ctl?.stopOthers,
      title: `${what}${v.active === e.n ? ' · active' : ''}\n${how}`,
      attrs: {
        'data-slotkey': String(e.n),
        'data-saved': saved ? '' : undefined,
        'data-stale': stale ? '' : undefined,
        'data-active': v.active === e.n ? '' : undefined,
        'data-job': job,
      },
    };
  });
}

interface KeyParam {
  system: System;
  key: SlotKey;
}

async function act(f: () => Promise<void>) {
  try {
    await f();
  } catch (e) {
    ui.toast(e instanceof Error ? e.message : String(e), 4000);
  }
}

/** Click loads, hold saves (or, stopped, launches and loads); Enter and Space work the same way. */
export const slotKey: Action<HTMLElement, KeyParam> = (node, initial) => {
  let p = initial;
  let start = 0;
  let raf = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let fired = false;
  let pressing = false;

  const setHold = (f: number) => node.style.setProperty('--hold', String(f));

  // The fill is drawn per frame; the save fires on a timer, so a busy page with few frames still saves on time.
  function frame() {
    setHold(Math.min(1, (performance.now() - start) / HOLD_MS));
    raf = requestAnimationFrame(frame);
  }

  function fire() {
    fired = true;
    stop();
    const { system, key } = p;
    if (key.canLaunch) {
      void act(() => player.actions.launch(system.id, { loadSlot: key.n, ...(key.stopOthers ? { stopOthers: true } : {}) }));
    } else if (key.canSave) {
      void act(() => player.actions.saveSlot(system.id, key.n));
    }
  }

  function stop() {
    pressing = false;
    cancelAnimationFrame(raf);
    clearTimeout(timer);
    delete node.dataset.holding;
    setHold(0);
  }

  function down() {
    if (pressing || !(p.key.canSave || p.key.canLaunch)) return;
    pressing = true;
    fired = false;
    start = performance.now();
    node.dataset.holding = '';
    raf = requestAnimationFrame(frame);
    timer = setTimeout(fire, HOLD_MS);
  }

  function up(cancel = false) {
    if (!pressing) return;
    stop();
    setHold(0);
    if (fired || cancel) return;
    const { system, key } = p;
    if (performance.now() - start >= TAP_MS) return;
    if (key.canLoad) void act(() => player.actions.loadSlot(system.id, key.n));
    else if (key.canLaunch) ui.toast(`Hold ${key.n} to launch ${system.label} and load that slot.`);
  }

  const onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0) return;
    try {
      node.setPointerCapture(e.pointerId);
    } catch {
      // not an active pointer (a synthetic event): the press still counts
    }
    down();
  };
  const onPointerUp = () => up();
  const onPointerCancel = () => up(true);
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    if (!e.repeat) down();
  };
  const onKeyUp = (e: KeyboardEvent) => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    up();
  };
  const onBlur = () => up(true);
  // The button's own click (from the keyboard or a pointer) is handled above.
  const onClick = (e: MouseEvent) => e.preventDefault();
  const onContext = (e: Event) => e.preventDefault();

  node.addEventListener('pointerdown', onPointerDown);
  node.addEventListener('pointerup', onPointerUp);
  node.addEventListener('pointercancel', onPointerCancel);
  node.addEventListener('keydown', onKeyDown);
  node.addEventListener('keyup', onKeyUp);
  node.addEventListener('blur', onBlur);
  node.addEventListener('click', onClick);
  node.addEventListener('contextmenu', onContext);

  return {
    update(next) {
      p = next;
      if (pressing && !(p.key.canSave || p.key.canLaunch)) up(true);
    },
    destroy() {
      cancelAnimationFrame(raf);
      clearTimeout(timer);
      node.removeEventListener('pointerdown', onPointerDown);
      node.removeEventListener('pointerup', onPointerUp);
      node.removeEventListener('pointercancel', onPointerCancel);
      node.removeEventListener('keydown', onKeyDown);
      node.removeEventListener('keyup', onKeyUp);
      node.removeEventListener('blur', onBlur);
      node.removeEventListener('click', onClick);
      node.removeEventListener('contextmenu', onContext);
    },
  };
};
