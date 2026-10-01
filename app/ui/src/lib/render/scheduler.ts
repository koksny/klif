// One requestAnimationFrame loop for the whole app, with frame-rate tiers.
//
// Tiers (driven by the shell from visibility, focus and idleness; later by the native host):
//   off     - window hidden/minimized/occluded: no frames at all
//   calm    - visible but the user is away: no per-frame callbacks; values still update at 1 Hz
//   ambient - visible, unfocused (the normal desktop-widget state): <= 30 fps
//   live    - focused/hovered or during launch/stop sequences: <= 60 fps
//
// Skins with canvas animation MUST use onFrame() instead of their own rAF loop.

export type Tier = 'off' | 'calm' | 'ambient' | 'live';

const TIER_FPS: Record<Tier, number> = { off: 0, calm: 0, ambient: 30, live: 60 };

type FrameCb = (timeMs: number, dtMs: number) => void;

interface Sub {
  cb: FrameCb;
  /** Optional per-subscriber cap, never above the tier cap. */
  maxFps: number;
  last: number;
}

const subs = new Set<Sub>();
let tier: Tier = 'ambient';
let raf = 0;
const tierListeners = new Set<(t: Tier) => void>();

function loop(now: number) {
  raf = 0;
  const cap = TIER_FPS[tier];
  if (cap <= 0 || subs.size === 0) return;
  for (const s of subs) {
    const fps = Math.min(cap, s.maxFps);
    const minGap = 1000 / fps - 1;
    if (now - s.last >= minGap) {
      const dt = s.last ? Math.min(now - s.last, 250) : 1000 / fps;
      s.last = now;
      s.cb(now, dt);
    }
  }
  raf = requestAnimationFrame(loop);
}

function kick() {
  if (!raf && TIER_FPS[tier] > 0 && subs.size > 0) raf = requestAnimationFrame(loop);
}

/** Subscribe to frames. Returns an unsubscribe function (call it from the component's cleanup). */
export function onFrame(cb: FrameCb, opts: { maxFps?: number } = {}): () => void {
  const sub: Sub = { cb, maxFps: opts.maxFps ?? 60, last: 0 };
  subs.add(sub);
  kick();
  return () => {
    subs.delete(sub);
  };
}

export function getTier(): Tier {
  return tier;
}

export function setTier(next: Tier) {
  if (next === tier) return;
  tier = next;
  for (const l of tierListeners) l(tier);
  if (TIER_FPS[tier] <= 0 && raf) {
    cancelAnimationFrame(raf);
    raf = 0;
  }
  kick();
}

/** Listen for tier changes (e.g. to pause CSS animations). Returns an unsubscribe function. */
export function onTier(listener: (t: Tier) => void): () => void {
  tierListeners.add(listener);
  return () => tierListeners.delete(listener);
}
