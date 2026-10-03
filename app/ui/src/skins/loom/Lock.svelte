<script lang="ts">
  // A figure that settles: on a change, only the digits that changed roll through other digits and lock in left
  // to right. Every other character (decimal point, slash, units, dashes) stays fixed; nothing but digits ever
  // shows. Frames come from the shared scheduler; with none (calm or hidden window) or reduced motion it just
  // switches.
  import { onDestroy } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import { getTier, onFrame, onTier } from '../../lib/render/scheduler';

  let { value, ms = 180 }: { value: string; ms?: number } = $props();

  const isDigit = (ch: string) => ch >= '0' && ch <= '9';
  let shown = $state('');
  let current = '';
  let first = true;
  let off: (() => void) | null = null;

  const stop = () => {
    off?.();
    off = null;
  };
  const flowing = (t = getTier()) => t === 'ambient' || t === 'live';

  $effect(() => {
    const target = value;
    // the parent re-renders at the telemetry rate: only a changed value rolls again
    if (target === current) return;
    const prev = current;
    current = target;
    stop();
    if (first || prefersReducedMotion.current || !flowing()) {
      first = false;
      shown = target;
      return;
    }
    const same = prev.length === target.length;
    const t0 = performance.now();
    off = onFrame((now) => {
      const k = (now - t0) / ms;
      let out = '';
      for (let i = 0; i < target.length; i++) {
        const ch = target[i];
        const settled = !isDigit(ch) || (same && prev[i] === ch) || k >= (i + 1) / (target.length + 1);
        out += settled ? ch : String(Math.floor(Math.random() * 10));
      }
      shown = out;
      if (k >= 1) {
        shown = target;
        stop();
      }
    });
  });

  // frames stopping mid-roll (the window went calm or hidden) must not leave it unsettled
  const offTier = onTier((t) => {
    if (off && !flowing(t)) {
      stop();
      shown = current;
    }
  });

  onDestroy(() => {
    stop();
    offTier();
  });
</script>

<span class="lock">{shown}</span>

<style>
  .lock {
    white-space: pre;
    font-variant-numeric: tabular-nums;
  }
</style>
