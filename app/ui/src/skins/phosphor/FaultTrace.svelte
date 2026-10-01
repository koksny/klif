<script lang="ts">
  // The last real trace before a fault, on a 5-minute window whose right edge is "now": the samples
  // end at the moment of failure (agoS before now), drop to the floor in red, and a dashed red line
  // runs from there to now. Static: nothing moves after a fault.
  import { niceRange, smoothSvg, type Pt } from './geom';

  let {
    values,
    agoS,
    unit,
    stepped = false,
    minSpan = 10,
  }: {
    /** One sample per second, oldest first, ending at the fault. */
    values: number[];
    agoS: number;
    /** Axis caption, e.g. "decode tok/s". */
    unit: string;
    /** Draw as steps (GiB) instead of a smoothed beam (tok/s). */
    stepped?: boolean;
    minSpan?: number;
  } = $props();

  const SPAN = 300;
  let w = $state(0);
  let h = $state(0);

  const geo = $derived.by(() => {
    if (w < 20 || h < 20) return null;
    const xr = w - 10;
    const dx = (xr - 2) / (SPAN - 1);
    const a = Math.max(0, Math.min(SPAN, agoS));
    const xF = xr - a * dx;
    const top = h * 0.12;
    const bot = h * 0.86;
    let mn = Infinity;
    let mx = -Infinity;
    for (const v of values) {
      if (v < mn) mn = v;
      if (v > mx) mx = v;
    }
    const [lo, hi] = values.length ? niceRange(Math.min(0, mn), mx, minSpan) : [0, minSpan];
    const yOf = (v: number) => top + (1 - (hi > lo ? (v - lo) / (hi - lo) : 0)) * (bot - top);
    const n = values.length;
    const pts: Pt[] = [];
    for (let i = 0; i < n; i++) {
      const x = xF - (n - 1 - i) * dx;
      if (x < 0) continue;
      pts.push([x, yOf(values[i])]);
    }
    let d = '';
    if (stepped && pts.length) {
      d = `M${pts[0][0].toFixed(1)},${pts[0][1].toFixed(1)}`;
      for (let i = 1; i < pts.length; i++) d += `H${pts[i][0].toFixed(1)}V${pts[i][1].toFixed(1)}`;
    } else {
      d = smoothSvg(pts);
    }
    const yLast = pts.length ? pts[pts.length - 1][1] : bot;
    return { xr, xF, top, bot, lo, hi, d, yLast, visible: agoS < SPAN && pts.length > 0 };
  });
</script>

<div class="ft" bind:clientWidth={w} bind:clientHeight={h}>
  {#if geo}
    <svg width={w} height={h} viewBox="0 0 {w} {h}" role="img" aria-label="{unit} until the fault">
      <line class="rule" x1="0" x2={geo.xr} y1={geo.top} y2={geo.top} />
      <line class="rule" x1="0" x2={geo.xr} y1={geo.bot} y2={geo.bot} />
      {#if geo.d}
        <path d={geo.d} class="after" />
        <path d={geo.d} class="halo" />
        <path d={geo.d} class="beam" />
      {/if}
      {#if geo.visible}
        <path d="M{geo.xF.toFixed(1)},{geo.yLast.toFixed(1)}V{geo.bot.toFixed(1)}" class="drop" />
        <line class="dash" x1={geo.xF} x2={geo.xr} y1={geo.bot} y2={geo.bot} />
        <circle cx={geo.xF} cy={geo.bot} r="9" class="fdot halo2" />
        <circle cx={geo.xF} cy={geo.bot} r="4" class="fdot" />
      {/if}
    </svg>
    <span class="tick" style="top:{(geo.top / h) * 100}%">{geo.hi}</span>
    <span class="tick lo" style="top:{(geo.bot / h) * 100}%">{geo.lo}</span>
    <span class="cap">{unit}</span>
  {/if}
</div>

<style>
  .ft {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  svg {
    position: absolute;
    inset: 0;
    display: block;
    overflow: visible;
  }
  path,
  line {
    fill: none;
    stroke-linejoin: round;
    stroke-linecap: round;
    vector-effect: non-scaling-stroke;
  }
  .rule {
    stroke: var(--ph-grat);
    stroke-dasharray: 3 5;
  }
  .after {
    stroke: var(--ph-amber);
    stroke-width: 5;
    opacity: 0.22;
  }
  .halo {
    stroke: var(--ph-brand);
    stroke-width: 6;
    opacity: 0.16;
  }
  .beam {
    stroke: var(--ph-cyan);
    stroke-width: 1.5;
  }
  .drop {
    stroke: var(--ph-danger);
    stroke-width: 2;
  }
  .dash {
    stroke: var(--ph-danger);
    stroke-width: 1.4;
    stroke-dasharray: 6 5;
  }
  .fdot {
    fill: var(--ph-danger);
  }
  .fdot.halo2 {
    opacity: 0.25;
  }
  .tick {
    position: absolute;
    left: 0;
    transform: translateY(-115%);
    font: max(10.5px, calc(12.5px * var(--k, 1))) / 1 var(--ph-ui);
    color: var(--ph-muted);
    opacity: 0.85;
    pointer-events: none;
  }
  /* the floor figure sits under its rule: the trace often runs right along it */
  .tick.lo {
    transform: translateY(20%);
  }
  .cap {
    position: absolute;
    right: calc(10px * var(--k, 1));
    top: 0;
    font: max(10.5px, calc(13px * var(--k, 1))) / 1 var(--ph-ui);
    letter-spacing: 0.05em;
    color: var(--ph-muted);
  }
</style>
