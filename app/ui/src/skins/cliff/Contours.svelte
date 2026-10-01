<script lang="ts">
  // Background sea-chart contours: generated once per size to a canvas, then drifted by a
  // transform at <= 15 fps through the shared scheduler (pauses with the tier, off for reduced motion).
  import { onFrame } from '../../lib/render/scheduler';
  import { contourCanvas } from './paint';
  import { prefersReducedMotion } from './util';

  const M = 30; // overscan margin so the drift never shows an edge

  let w = $state(0);
  let h = $state(0);
  let host: HTMLDivElement | undefined = $state();
  let current: HTMLCanvasElement | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function render(W: number, H: number) {
    if (!host) return;
    const scale = Math.min(1.25, window.devicePixelRatio || 1);
    const c = contourCanvas(W + 2 * M, H + 2 * M, scale);
    c.className = 'topo';
    c.style.cssText = `position:absolute;left:${-M}px;top:${-M}px;width:${W + 2 * M}px;height:${H + 2 * M}px;will-change:transform;`;
    if (current) {
      c.style.transform = current.style.transform;
      current.replaceWith(c);
    } else host.appendChild(c);
    current = c;
  }

  $effect(() => {
    const W = w;
    const H = h;
    if (!W || !H) return;
    clearTimeout(timer);
    if (!current) render(W, H);
    else timer = setTimeout(() => render(W, H), 250);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (prefersReducedMotion()) return;
    const t0 = performance.now();
    return onFrame(
      (now) => {
        if (!current) return;
        const s = (now - t0) / 1000;
        const dx = Math.sin((s / 170) * Math.PI * 2) * 22;
        const dy = Math.sin((s / 230) * Math.PI * 2 + 1.1) * 16;
        current.style.transform = `translate3d(${dx.toFixed(2)}px, ${dy.toFixed(2)}px, 0)`;
      },
      { maxFps: 15 },
    );
  });
</script>

<div class="contours" bind:this={host} bind:clientWidth={w} bind:clientHeight={h} aria-hidden="true"></div>

<style>
  .contours {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
    z-index: 0;
  }
</style>
