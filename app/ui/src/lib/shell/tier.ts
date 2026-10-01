// Tier driver: turns window state into a scheduler tier.
//   pinned (?tier= or the dev bar) -> that tier
//   hidden / minimized             -> off
//   host reports the system idle   -> calm   (explicit signal only, see setSystemIdle)
//   focused or pointer inside      -> live
//   otherwise                      -> ambient
// An always-on desktop widget sits visible and unfocused for hours, so "nobody touched the window" is NOT a
// reason to drop its ambient animation: calm only ever comes from the host (or a pin), never from a timer.
// The tier is mirrored as data-tier on <html> (app.css pauses CSS animations for calm and off).
import { getTier, onTier, setTier, type Tier } from '../render/scheduler';
import { ui } from '../state/ui.svelte';

let recompute: (() => void) | null = null;
let systemIdle = false;

/** Re-evaluate now (the dev bar calls this after pinning or unpinning a tier). */
export function refreshTier() {
  recompute?.();
}

/**
 * Host signal (stub for the Tauri shell): the machine is idle (screen off, locked, session away). While true
 * the tier is 'calm': no per-frame callbacks, telemetry values still update. The browser never calls this.
 */
export function setSystemIdle(idle: boolean) {
  systemIdle = idle;
  recompute?.();
}

export function installTierDriver(): () => void {
  const root = document.documentElement;
  let focused = document.hasFocus();
  let inside = false;

  const compute = (): Tier => {
    if (ui.tierPin) return ui.tierPin;
    if (document.visibilityState === 'hidden') return 'off';
    if (systemIdle) return 'calm';
    return focused || inside ? 'live' : 'ambient';
  };
  const apply = () => setTier(compute());
  recompute = apply;

  const mirror = (t: Tier) => {
    root.dataset.tier = t;
    ui.tier = t;
  };
  const offTier = onTier(mirror);
  mirror(getTier());

  // Pointer activity marks "pointer inside" even where mouseenter on <html> does not fire.
  const onPointer = () => {
    if (inside) return;
    inside = true;
    apply();
  };
  const onVis = () => apply();
  const onFocus = () => {
    focused = true;
    apply();
  };
  const onBlur = () => {
    focused = false;
    apply();
  };
  const onEnter = () => {
    inside = true;
    apply();
  };
  const onLeave = () => {
    inside = false;
    apply();
  };

  const pointerEvents = ['pointermove', 'pointerdown'] as const;
  for (const ev of pointerEvents) window.addEventListener(ev, onPointer, { passive: true, capture: true });
  document.addEventListener('visibilitychange', onVis);
  window.addEventListener('focus', onFocus);
  window.addEventListener('blur', onBlur);
  root.addEventListener('mouseenter', onEnter);
  root.addEventListener('mouseleave', onLeave);
  apply();

  return () => {
    for (const ev of pointerEvents) window.removeEventListener(ev, onPointer, { capture: true });
    document.removeEventListener('visibilitychange', onVis);
    window.removeEventListener('focus', onFocus);
    window.removeEventListener('blur', onBlur);
    root.removeEventListener('mouseenter', onEnter);
    root.removeEventListener('mouseleave', onLeave);
    offTier();
    recompute = null;
  };
}
