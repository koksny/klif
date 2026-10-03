<script lang="ts">
  // A value that decodes: on every change its characters cycle through glyphs and lock in left to right.
  let { value, ms = 260 }: { value: string; ms?: number } = $props();

  const POOL = '0123456789abcdef#%&*+=/<>';
  let shown = $state('');
  let raf = 0;
  let first = true;
  const reduced = typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  let current = '';

  $effect(() => {
    const target = value;
    // the parent re-renders at the telemetry rate: only a changed value decodes again
    if (target === current) return;
    const prev = current;
    current = target;
    cancelAnimationFrame(raf);
    if (reduced || first) {
      first = false;
      shown = target;
      return;
    }
    const t0 = performance.now();
    const step = (now: number) => {
      const k = (now - t0) / ms;
      let out = '';
      for (let i = 0; i < target.length; i++) {
        const ch = target[i];
        const lockAt = (i + 1) / (target.length + 1);
        // only the characters that changed decode again; the rest stay readable
        const same = prev.length === target.length && prev[i] === ch;
        out += same || k >= lockAt || ch === ' ' || ch === '.' ? ch : POOL[Math.floor(Math.random() * POOL.length)];
      }
      shown = out;
      if (k < 1) raf = requestAnimationFrame(step);
      else shown = target;
    };
    raf = requestAnimationFrame(step);
  });
</script>

<span class="lock">{shown}</span>

<style>
  .lock {
    white-space: pre;
  }
</style>
