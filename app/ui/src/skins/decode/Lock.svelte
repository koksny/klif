<script lang="ts">
  // A value that decodes: on every change its characters cycle through glyphs and lock in left to right.
  // Driven by the shared frame scheduler; with no frames (calm/off tier) or reduced motion it just shows the value.
  import { getTier, onFrame } from '../../lib/render/scheduler';

  let { value, ms = 260 }: { value: string; ms?: number } = $props();

  const POOL = '0123456789abcdef#%&*+=/<>';
  const reduced = typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  let shown = $state('');
  let first = true;
  let current = '';
  let unsub: (() => void) | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function halt() {
    unsub?.();
    unsub = null;
    clearTimeout(timer);
  }

  $effect(() => {
    const target = value;
    // the parent re-renders at the telemetry rate: only a changed value decodes again
    if (target === current) return;
    const prev = current;
    current = target;
    halt();
    const tier = getTier();
    if (reduced || first || tier === 'off' || tier === 'calm') {
      first = false;
      shown = target;
      return;
    }
    const t0 = performance.now();
    unsub = onFrame((now) => {
      const k = (now - t0) / ms;
      if (k >= 1) {
        halt();
        shown = target;
        return;
      }
      let out = '';
      for (let i = 0; i < target.length; i++) {
        const ch = target[i];
        const lockAt = (i + 1) / (target.length + 1);
        // only the characters that changed decode again; the rest stay readable
        const same = prev.length === target.length && prev[i] === ch;
        out += same || k >= lockAt || ch === ' ' || ch === '.' ? ch : POOL[Math.floor(Math.random() * POOL.length)];
      }
      shown = out;
    });
    // the scheduler may stop delivering frames mid-way (tier change): the value still lands
    timer = setTimeout(() => {
      halt();
      shown = current;
    }, ms + 150);
  });

  $effect(() => halt);
</script>

<span class="lock">{shown}</span>

<style>
  .lock {
    white-space: pre;
  }
</style>
