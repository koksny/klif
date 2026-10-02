<script lang="ts">
  // The die floor plan. Static linework is rendered once to an offscreen canvas; the data layer
  // (token tiles, GDDR6 gauge, prompt cache) is redrawn on data changes and, only while tiles are
  // glowing or the stream is running, via the shared scheduler at <= 30 fps.
  import { onMount, untrack } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import { getTier, onFrame } from '../../lib/render/scheduler';
  import type { LlmLive } from '../../lib/model/types';
  import { GEOM, Pen, drawDynamic, drawStatic, type Variant } from './die';
  import { TokenStream } from './stream';

  interface Props {
    variant: Variant;
    llm: LlmLive | null;
    /** True only while an LLM session is live (tiles never light otherwise). */
    live: boolean;
    usedGiB: number;
    totalGiB: number;
    cacheFrac: number | null;
    /** Progress 0..1 of a non-token job (image sampling) that fills the tiles, or null. */
    jobFill?: number | null;
    /** Full only: what one tile means right now, one string per callout line. */
    caption?: string[];
    /** GPU dormant (vram.dormant): GiB of the session's allocations paged out to system RAM, else null. */
    pagedOutGiB?: number | null;
    /** Extra CSS transform scale applied by an ancestor (mini stage), for a crisp backing store. */
    pxScale?: number;
    label?: string;
  }
  let { variant, llm, live, usedGiB, totalGiB, cacheFrac, jobFill = null, caption = [], pagedOutGiB = null, pxScale = 1, label = '' }: Props = $props();

  let canvas: HTMLCanvasElement;
  let cssW = $state(0);
  let cssH = $state(0);
  let fontsReady = $state(false);
  let mounted = $state(false);

  const stream = new TokenStream();
  const geom = $derived(GEOM[variant]);
  const perBlock = $derived(totalGiB > 0 ? totalGiB / 8 : 0);
  const captionKey = $derived(caption.join('|'));
  /** The linework only depends on whether the GPU is dormant; the amount is drawn with the data. */
  const dormant = $derived(pagedOutGiB !== null);

  let staticLayer: HTMLCanvasElement | null = null;
  let pen: Pen | null = null;
  let staticPen: Pen | null = null;
  let unsub: (() => void) | null = null;

  function rebuild() {
    if (!canvas || cssW <= 0 || cssH <= 0) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1) * pxScale;
    const cw = Math.max(1, Math.round(cssW * dpr));
    const ch = Math.max(1, Math.round(cssH * dpr));
    if (canvas.width !== cw) canvas.width = cw;
    if (canvas.height !== ch) canvas.height = ch;
    const vh = geom.H - geom.vy;
    const s = Math.min(cw / geom.W, ch / vh);
    const ox = Math.round((cw - geom.W * s) / 2);
    const oy = Math.round((ch - vh * s) / 2 - geom.vy * s);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    pen = new Pen(ctx, s, ox, oy, dpr);

    staticLayer ??= document.createElement('canvas');
    staticLayer.width = cw;
    staticLayer.height = ch;
    const sctx = staticLayer.getContext('2d');
    if (!sctx) return;
    sctx.clearRect(0, 0, cw, ch);
    staticPen = new Pen(sctx, s, ox, oy, dpr);
    drawStatic(staticPen, geom, { perBlockGiB: perBlock, caption, dormant });
  }

  function draw(now: number) {
    if (!pen || !staticLayer) return;
    const { ctx } = pen;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(staticLayer, 0, 0);
    drawDynamic(pen, geom, {
      usedGiB,
      perBlockGiB: perBlock,
      cacheFrac,
      glow: (t) => stream.brightness(t, now),
      pagedOutGiB,
    });
  }

  /** True while the scheduler is delivering frames (ambient/live). In calm/off it is not. */
  const framesFlowing = () => {
    const t = getTier();
    return t === 'ambient' || t === 'live';
  };

  function stopLoop() {
    unsub?.();
    unsub = null;
  }

  function ensureLoop() {
    if (unsub || prefersReducedMotion.current) return;
    unsub = onFrame(
      (t) => {
        const alive = stream.step(t);
        draw(t);
        if (!alive) stopLoop();
      },
      { maxFps: 30 },
    );
  }

  onMount(() => {
    mounted = true;
    const fam = '"JetBrains Mono Variable"';
    Promise.all([document.fonts.load(`500 12px ${fam}`), document.fonts.load(`700 12px ${fam}`)])
      .catch(() => undefined)
      .then(() => (fontsReady = true));
    return stopLoop;
  });

  // Geometry / size / fonts -> rebuild the static layer.
  $effect(() => {
    void cssW;
    void cssH;
    void pxScale;
    void fontsReady;
    void perBlock;
    void captionKey;
    void dormant;
    void geom;
    if (!mounted) return;
    untrack(() => {
      rebuild();
      draw(performance.now());
    });
  });

  // Telemetry snapshot -> token stream.
  $effect(() => {
    const now = performance.now();
    stream.update(llm, now, live, jobFill);
    if (!mounted) return;
    untrack(() => {
      if (unsub && framesFlowing()) return; // the frame loop picks the new snapshot up next frame
      stream.step(now);
      draw(now);
      if (stream.active(now)) ensureLoop();
    });
  });

  // Gauges.
  $effect(() => {
    void usedGiB;
    void cacheFrac;
    void pagedOutGiB;
    if (mounted) untrack(() => (!unsub || !framesFlowing()) && draw(performance.now()));
  });
</script>

<div class="die" bind:clientWidth={cssW} bind:clientHeight={cssH} role="img" aria-label={label}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
</div>

<style>
  .die {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
</style>
