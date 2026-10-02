<script lang="ts">
  // Request timeline: one pulse glyph per request, oldest at the left.
  // Glyph width is time on one shared scale for all cells: the amber step is prefill (prefillS),
  // the cyan plateau is decode (decodeS). Heights are fixed glyph form (prefill low, decode high);
  // only widths carry data. The lead-in and tail are plain baseline (rule colour), so no colour is
  // spent on anything that is not a measured duration. Hover a glyph for its numbers.
  // fault: the requests are left-aligned and the row ends in a red marker and a dashed line.
  // No requests: the empty cells only (the panel caption says why).
  import type { RequestRecord } from '../../lib/model/types';
  import { fmtInt } from '../../lib/model/format';

  let {
    requests,
    variant = 'full',
    minCells = 8,
    fault = false,
  }: { requests: RequestRecord[]; variant?: 'full' | 'mini'; minCells?: number; fault?: boolean } = $props();

  let w = $state(0);
  let h = $state(0);
  const mini = $derived(variant === 'mini');

  const cells = $derived(fault ? Math.max(minCells, requests.length + 2) : Math.max(minCells, requests.length));
  const maxS = $derived(Math.max(1, ...requests.map((r) => r.prefillS + r.decodeS)));

  const glyphs = $derived.by(() => {
    if (w < 10 || h < 6) return [];
    const cw = w / cells;
    const lead = mini ? cw * 0.28 : Math.min(18, cw * 0.14);
    const tail = mini ? cw * 0.28 : Math.min(12, cw * 0.1);
    const usable = Math.max(4, cw - lead - tail);
    const kx = usable / maxS;
    const yb = h - (mini ? 3 : 4);
    const yp = yb - (h - 8) * (mini ? 0.3 : 0.38);
    const yd = mini ? h * 0.22 : 5;
    const offset = fault ? 0 : cells - requests.length;
    return requests.map((r, i) => {
      const x0 = (offset + i) * cw;
      const xa = x0 + lead;
      const xp = xa + Math.max(1.5, r.prefillS * kx);
      const xd = xp + Math.max(1.5, r.decodeS * kx);
      const xe = x0 + cw - 2;
      const tps = r.decodeS > 0 ? r.generatedTokens / r.decodeS : 0;
      return {
        id: r.id,
        sep: x0,
        base: `M${(x0 + 2).toFixed(1)},${yb}H${xa.toFixed(1)}`,
        pre: `M${xa.toFixed(1)},${yb}V${yp.toFixed(1)}H${xp.toFixed(1)}`,
        dec: `M${xp.toFixed(1)},${yp.toFixed(1)}V${yd}H${xd.toFixed(1)}V${yb}`,
        tail: `M${xd.toFixed(1)},${yb}H${xe.toFixed(1)}`,
        title:
          `#${r.id}: prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached), prefill ${r.prefillS.toFixed(1)} s; ` +
          `${fmtInt(r.generatedTokens)} tok in ${r.decodeS.toFixed(1)} s (${tps.toFixed(1)} tok/s)`,
      };
    });
  });

  /** Fault marker: just after the last request. */
  const xf = $derived.by(() => {
    const cw = w / cells;
    return requests.length * cw + (mini ? cw * 0.28 : Math.min(18, cw * 0.14)) * 0.6;
  });
</script>

<div class="tl {variant}" bind:clientWidth={w} bind:clientHeight={h}>
  {#if w > 10}
    <svg width={w} height={h} viewBox="0 0 {w} {h}" role="img" aria-label="{requests.length} recent requests">
      {#if !mini}
        {#each Array.from({ length: cells - 1 }, (_, i) => ((i + 1) * w) / cells) as x}
          <line class="sep" x1={x} x2={x} y1="0" y2={h} />
        {/each}
      {:else}
        <line class="base0" x1="0" x2={w} y1={h - 3} y2={h - 3} />
      {/if}
      {#if !mini && !fault}
        {#each Array.from({ length: cells - requests.length }, (_, i) => i) as i (i)}
          <line class="base0" x1={(i * w) / cells + 6} x2={((i + 1) * w) / cells - 6} y1={h - 4} y2={h - 4} />
        {/each}
      {/if}
      {#each glyphs as g (g.id)}
        <g>
          <title>{g.title}</title>
          <path d={g.pre} class="glow amber" />
          <path d={g.dec} class="glow cyan" />
          <path d={g.base} class="ln base" />
          <path d={g.tail} class="ln base" />
          <path d={g.pre} class="ln amber" />
          <path d={g.dec} class="ln cyan" />
        </g>
      {/each}
      {#if fault}
        <line class="fline" x1={xf} x2={w - 2} y1={h - (mini ? 3 : 4)} y2={h - (mini ? 3 : 4)} />
        <circle class="fdot halo" cx={xf} cy={h - (mini ? 3 : 4)} r={mini ? 9 : 7} />
        <circle class="fdot" cx={xf} cy={h - (mini ? 3 : 4)} r={mini ? 4.5 : 3.5} />
      {/if}
    </svg>
  {/if}
</div>

<style>
  .tl {
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
    vector-effect: non-scaling-stroke;
  }
  .sep {
    stroke: var(--ph-grat);
    stroke-width: 1;
  }
  .base0 {
    stroke: var(--ph-rule);
    stroke-width: 1.5;
    stroke-dasharray: 2 6;
  }
  .ln {
    stroke-width: 1.6;
  }
  .mini .ln {
    stroke-width: 3;
  }
  .ln.base {
    stroke: #2c7a8c;
    stroke-width: 1.2;
  }
  .mini .ln.base {
    stroke-width: 2;
  }
  .ln.amber {
    stroke: var(--ph-amber);
  }
  .ln.cyan {
    stroke: var(--ph-cyan);
  }
  .glow {
    stroke-width: 6;
    opacity: 0.14;
  }
  .mini .glow {
    stroke-width: 9;
  }
  .glow.amber {
    stroke: var(--ph-amber);
  }
  .glow.cyan {
    stroke: var(--ph-cyan);
  }
  .fline {
    stroke: var(--ph-danger);
    stroke-width: 1.4;
    stroke-dasharray: 6 5;
  }
  .fdot {
    fill: var(--ph-danger);
  }
  .fdot.halo {
    opacity: 0.28;
  }
</style>
