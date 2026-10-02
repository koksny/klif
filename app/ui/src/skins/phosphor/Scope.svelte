<script lang="ts">
  // Decode-speed trace: the real 1 Hz history drawn as a phosphor beam.
  // - New samples enter at the right; the trace glides left over ~0.65 s when a sample arrives.
  // - Afterglow: earlier frames persist on a 1/3-resolution canvas and fade (tau 0.38 s). Upscaling
  //   that small canvas is the bloom, so there is no blur filter anywhere.
  // - Frames come from the shared scheduler (<= 30 fps) and stop entirely once the glow has settled.
  // - dim (GPU asleep): the live beam is off; only the amber afterglow and a faint trace remain.
  import { untrack } from 'svelte';
  import { getTier, onFrame, onTier, type Tier } from '../../lib/render/scheduler';
  import { niceRange, newSamples, sameSeries, smoothCanvas } from './geom';
  import { P, prefersReducedMotion } from './palette';

  let {
    history,
    variant = 'full',
    dim = false,
  }: {
    /** One sample per second, oldest first, up to 300. */
    history: number[];
    variant?: 'full' | 'mini';
    /** Afterglow only: no bright beam, no hot core (the GPU is asleep, nothing is being traced live). */
    dim?: boolean;
  } = $props();

  const SLOTS = 300;
  const GLOW_DIV = 3;
  const TAU_MS = 380;
  const SCROLL_MS = 650;
  const FPS = 24;
  const reduced = prefersReducedMotion();

  let canvas: HTMLCanvasElement | undefined = $state();
  let w = $state(0);
  let h = $state(0);
  /** Target scale, also printed as the two scale labels. */
  let range = $state<[number, number]>([0, 10]);

  // Drawing state lives outside Svelte reactivity on purpose.
  let ctx: CanvasRenderingContext2D | null = null;
  let glow: HTMLCanvasElement | null = null;
  let gctx: CanvasRenderingContext2D | null = null;
  let data: number[] = [];
  let prev: number[] | null = null;
  let shift = 0;
  let shiftFrom = 0;
  let shiftStart = 0;
  let lo = 0;
  let hi = 10;
  let lastMotion = 0;
  let unsub: (() => void) | null = null;
  const xs = new Float32Array(SLOTS + 8);
  const ys = new Float32Array(SLOTS + 8);

  const pads = $derived(variant === 'mini' ? { top: 0.12, bot: 0.86, end: 14 } : { top: 0.1, bot: 0.86, end: 12 });

  const canAnimate = (t: Tier = getTier()) => !reduced && (t === 'ambient' || t === 'live');

  function yOf(v: number): number {
    const t = hi > lo ? (v - lo) / (hi - lo) : 0.5;
    return h * pads.top + (1 - t) * h * (pads.bot - pads.top);
  }

  /** Fills xs/ys with the visible samples; returns [count, dotY]. */
  function layout(): [number, number] {
    const n = data.length;
    if (n === 0) return [0, yOf(0)];
    const xr = w - pads.end;
    const dx = (xr - 2) / (SLOTS - 1);
    let m = 0;
    for (let i = 0; i < n; i++) {
      const x = xr - (n - 1 - i - shift) * dx;
      if (x < -dx * 2) continue;
      xs[m] = x;
      ys[m] = yOf(data[i]);
      m++;
    }
    // The dot rides the right edge at the value interpolated where the trace crosses it.
    const f = Math.max(0, n - 1 - shift);
    const a = Math.floor(f);
    const b = Math.min(n - 1, a + 1);
    const v = data[a] + (data[b] - data[a]) * (f - a);
    return [m, yOf(v)];
  }

  function draw(dtMs: number, fresh: boolean) {
    if (!ctx || !gctx || !glow || w < 4 || h < 4) return;
    const c = ctx;
    const g = gctx;
    const xr = w - pads.end;
    c.clearRect(0, 0, w, h);

    // Scale rules (the two printed values).
    c.save();
    c.strokeStyle = P.grat;
    c.lineWidth = 1;
    c.setLineDash([3, 5]);
    for (const y of [h * pads.top, h * pads.bot]) {
      c.beginPath();
      c.moveTo(0, Math.round(y) + 0.5);
      c.lineTo(xr, Math.round(y) + 0.5);
      c.stroke();
    }
    c.restore();

    const [n, dotY] = layout();

    // Afterglow buffer: fade what was there, stamp the current beam.
    const gw = glow.width;
    const gh = glow.height;
    if (fresh) {
      g.clearRect(0, 0, gw, gh);
    } else {
      const keep = Math.exp(-dtMs / TAU_MS);
      g.globalCompositeOperation = 'destination-out';
      g.fillStyle = `rgba(0,0,0,${(1 - keep).toFixed(3)})`;
      g.fillRect(0, 0, gw, gh);
      g.globalCompositeOperation = 'source-over';
    }
    if (n > 1) {
      g.save();
      g.beginPath();
      g.rect(0, 0, xr / GLOW_DIV, gh);
      g.clip();
      g.strokeStyle = P.amber;
      g.lineJoin = 'round';
      g.lineWidth = variant === 'mini' ? 2.4 : 1.7;
      // Fresh stamp = the steady state of per-frame stamping, so settling never pops.
      g.globalAlpha = fresh ? 0.77 : 0.22;
      smoothCanvas(g, xs, ys, n, 1 / GLOW_DIV);
      g.stroke();
      g.restore();
    }

    c.save();
    c.beginPath();
    c.rect(0, 0, xr + 0.5, h);
    c.clip();
    c.globalCompositeOperation = 'lighter';
    c.globalAlpha = dim ? (variant === 'mini' ? 0.6 : 0.5) : variant === 'mini' ? 1 : 0.85;
    c.imageSmoothingEnabled = true;
    c.drawImage(glow, 0, 0, w, h);
    c.globalCompositeOperation = 'source-over';
    if (n > 1 && dim) {
      // Afterglow only: the beam itself is gone, a ghost of its path stays.
      c.lineJoin = 'round';
      c.lineCap = 'round';
      smoothCanvas(c, xs, ys, n);
      c.strokeStyle = P.brand;
      c.globalAlpha = 0.2;
      c.lineWidth = variant === 'mini' ? 1.6 : 1.1;
      c.stroke();
    } else if (n > 1) {
      c.lineJoin = 'round';
      c.lineCap = 'round';
      smoothCanvas(c, xs, ys, n);
      c.strokeStyle = P.brand;
      c.globalAlpha = 0.16;
      c.lineWidth = variant === 'mini' ? 8 : 6;
      c.stroke();
      c.strokeStyle = P.cyan;
      c.globalAlpha = 0.38;
      c.lineWidth = variant === 'mini' ? 3.6 : 2.8;
      c.stroke();
      c.globalAlpha = 1;
      c.lineWidth = variant === 'mini' ? 1.8 : 1.4;
      c.stroke();
      c.strokeStyle = P.hot;
      c.globalAlpha = 0.5;
      c.lineWidth = 0.6;
      c.stroke();
    }
    c.restore();

    if (data.length > 0) {
      const r = variant === 'mini' ? 5 : 3.6;
      c.fillStyle = dim ? P.amber : P.cyan;
      c.globalAlpha = dim ? 0.14 : 0.22;
      c.beginPath();
      c.arc(xr, dotY, r * 2.4, 0, Math.PI * 2);
      c.fill();
      c.globalAlpha = dim ? 0.55 : 1;
      c.fillStyle = dim ? P.amber : P.hot;
      c.beginPath();
      c.arc(xr, dotY, r, 0, Math.PI * 2);
      c.fill();
    }
  }

  function stop() {
    unsub?.();
    unsub = null;
  }

  function settleNow() {
    stop();
    shift = 0;
    lo = range[0];
    hi = range[1];
    draw(0, true);
  }

  function frame(now: number, dt: number) {
    let moving = false;
    if (shift > 0) {
      const t = (now - shiftStart) / SCROLL_MS;
      if (t >= 1) shift = 0;
      else {
        const e = t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
        shift = shiftFrom * (1 - e);
      }
      moving = true;
    }
    const [tlo, thi] = range;
    if (Math.abs(tlo - lo) > 0.01 || Math.abs(thi - hi) > 0.01) {
      const a = 1 - Math.exp(-dt / 200);
      lo += (tlo - lo) * a;
      hi += (thi - hi) * a;
      moving = true;
    } else {
      lo = tlo;
      hi = thi;
    }
    if (moving) lastMotion = now;
    draw(dt, false);
    // Once the beam has stood still for a few glow time-constants the picture is final: stop.
    if (!moving && now - lastMotion > TAU_MS * 5) settleNow();
  }

  function start() {
    if (unsub || !canAnimate()) return;
    lastMotion = performance.now();
    unsub = onFrame(frame, { maxFps: FPS });
  }

  function ingest(hist: number[]) {
    if (sameSeries(prev, hist)) return;
    const k = newSamples(prev, hist);
    prev = hist.slice();
    data = prev;
    let mn = Infinity;
    let mx = -Infinity;
    for (const v of data) {
      if (v < mn) mn = v;
      if (v > mx) mx = v;
    }
    range = data.length ? niceRange(mn, mx, 10) : [0, 10];
    if (!canAnimate()) return settleNow(); // reduced motion or calm/off tier: final picture only
    if (k < 0) {
      // First frame or an unrelated history: no glide, fresh phosphor.
      shift = 0;
      if (!unsub) {
        lo = range[0];
        hi = range[1];
      }
      draw(0, true);
      return;
    }
    if (k === 0) {
      // Only the newest sample was refined: keep gliding/glowing, no new shift.
      start();
      return;
    }
    shiftFrom = shift + k;
    shift = shiftFrom;
    shiftStart = performance.now();
    start();
  }

  // Canvas sizing: box x devicePixelRatio (cap 2); the glow buffer is a third of the CSS size.
  $effect(() => {
    if (!canvas || w < 2 || h < 2) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    ctx = canvas.getContext('2d');
    ctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
    glow ??= document.createElement('canvas');
    glow.width = Math.max(2, Math.ceil(w / GLOW_DIV));
    glow.height = Math.max(2, Math.ceil(h / GLOW_DIV));
    gctx = glow.getContext('2d');
    untrack(() => draw(0, true));
  });

  $effect(() => {
    const hist = history;
    untrack(() => ingest(hist));
  });

  // Dim on/off while the beam is at rest: repaint (while it moves, the next frame does it).
  $effect(() => {
    void dim;
    untrack(() => {
      if (ctx && !unsub) draw(0, true);
    });
  });

  $effect(() => {
    const off = onTier((t) => {
      if (!canAnimate(t)) settleNow();
    });
    return () => {
      off();
      stop();
    };
  });
</script>

<div class="scope {variant}" bind:clientWidth={w} bind:clientHeight={h}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
  {#if variant === 'full'}
    <span class="tick" style="top:{pads.top * 100}%">{range[1]}</span>
    <span class="tick" style="top:{pads.bot * 100}%">{range[0]}</span>
  {/if}
</div>

<style>
  .scope {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
  .tick {
    position: absolute;
    left: 0;
    transform: translateY(-115%);
    font: max(10.5px, calc(12.5px * var(--k, 1))) / 1 var(--ph-ui);
    color: var(--ph-muted);
    opacity: 0.8;
    pointer-events: none;
  }
</style>
