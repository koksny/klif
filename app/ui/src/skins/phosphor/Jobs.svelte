<script lang="ts">
  // Recent image jobs as pulses, oldest at the left. Pulse width = seconds on one shared scale
  // (only widths carry data; the height is fixed glyph form). Plain jobs are cyan, edit jobs amber.
  // The longest job is labelled with its seconds. The running job (if any) is an open pulse that
  // grows with its elapsed time. In a fault the row ends in a red marker and a dashed line.
  import type { ImageJob } from '../../lib/model/types';

  let {
    jobs,
    current = null,
    variant = 'full',
    fault = false,
    maxShown = 12,
  }: {
    jobs: ImageJob[];
    current?: { elapsedS: number; edit: boolean } | null;
    variant?: 'full' | 'mini';
    fault?: boolean;
    maxShown?: number;
  } = $props();

  let w = $state(0);
  let h = $state(0);
  const mini = $derived(variant === 'mini');

  const shown = $derived(jobs.slice(-maxShown));
  const items = $derived([
    ...shown.map((j) => ({ s: j.seconds, edit: j.edit, done: true, title: `${j.width}x${j.height}${j.edit ? ' edit' : ''} · ${j.seconds.toFixed(1)} s` })),
    ...(current ? [{ s: current.elapsedS, edit: current.edit, done: false, title: `running${current.edit ? ' edit' : ''} · ${current.elapsedS.toFixed(1)} s so far` }] : []),
  ]);
  const cells = $derived(fault ? Math.max(maxShown, items.length + 2) : Math.max(mini ? Math.max(1, items.length) : maxShown, items.length));
  const maxS = $derived(Math.max(1, ...items.map((i) => i.s)));
  const longest = $derived.by(() => {
    let best = -1;
    for (let i = 0; i < items.length; i++) if (items[i].done && (best < 0 || items[i].s > items[best].s)) best = i;
    return best;
  });

  const geo = $derived.by(() => {
    if (w < 10 || h < 6) return null;
    const cw = w / cells;
    const lead = mini ? cw * 0.22 : Math.min(16, cw * 0.14);
    const tail = mini ? cw * 0.16 : Math.min(10, cw * 0.1);
    const usable = Math.max(4, cw - lead - tail);
    const kx = usable / maxS;
    const yb = h - (mini ? 3 : 4);
    const yt = mini ? h * 0.2 : h * 0.42;
    const offset = fault ? 0 : cells - items.length;
    const glyphs = items.map((it, i) => {
      const x0 = (offset + i) * cw;
      const xa = x0 + lead;
      const xe = xa + Math.max(2, it.s * kx);
      const pulse = it.done
        ? `M${xa.toFixed(1)},${yb}V${yt.toFixed(1)}H${xe.toFixed(1)}V${yb}`
        : `M${xa.toFixed(1)},${yb}V${yt.toFixed(1)}H${xe.toFixed(1)}`;
      return {
        i,
        sep: x0,
        base: `M${(x0 + 2).toFixed(1)},${yb}H${xa.toFixed(1)}${it.done ? `M${xe.toFixed(1)},${yb}H${(x0 + cw - 2).toFixed(1)}` : ''}`,
        pulse,
        cx: (xa + xe) / 2,
        xe,
        ...it,
      };
    });
    const xf = (offset + items.length) * cw + lead * 0.6;
    return { cw, yb, yt, glyphs, xf };
  });
</script>

<div class="jobs {variant}" bind:clientWidth={w} bind:clientHeight={h}>
  {#if geo}
    <svg width={w} height={h} viewBox="0 0 {w} {h}" role="img" aria-label="{jobs.length} recent image jobs, longest {maxS.toFixed(1)} s">
      {#if !mini}
        {#each Array.from({ length: cells - 1 }, (_, i) => ((i + 1) * w) / cells) as x}
          <line class="sep" x1={x} x2={x} y1={geo.yt - 6} y2={h} />
        {/each}
      {:else}
        <line class="base0" x1="0" x2={w} y1={geo.yb} y2={geo.yb} />
      {/if}
      {#if !mini && !fault}
        {#each Array.from({ length: cells - items.length }, (_, i) => i) as i (i)}
          <line class="base0" x1={(i * w) / cells + 6} x2={((i + 1) * w) / cells - 6} y1={geo.yb} y2={geo.yb} />
        {/each}
      {/if}
      {#each geo.glyphs as g (g.i)}
        <g class:edit={g.edit} class:run={!g.done}>
          <title>{g.title}</title>
          <path d={g.pulse} class="glow" />
          <path d={g.base} class="ln base" />
          <path d={g.pulse} class="ln pulse" />
          {#if !g.done}<circle cx={g.xe} cy={geo.yt} r={mini ? 4 : 3} class="dot" />{/if}
          {#if !mini && g.i === longest}
            <text class="lab" x={g.cx} y={geo.yt - 6} text-anchor="middle">{g.s.toFixed(0)} s{g.edit ? ' · edit' : ''}</text>
          {/if}
        </g>
      {/each}
      {#if fault}
        <line class="fline" x1={geo.xf} x2={w - 2} y1={geo.yb} y2={geo.yb} />
        <circle class="fdot glowc" cx={geo.xf} cy={geo.yb} r={mini ? 9 : 7} />
        <circle class="fdot" cx={geo.xf} cy={geo.yb} r={mini ? 4.5 : 3.5} />
      {/if}
    </svg>
  {/if}
</div>

<style>
  .jobs {
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
  .pulse {
    stroke: var(--ph-cyan);
  }
  .edit .pulse {
    stroke: var(--ph-amber);
  }
  .glow {
    stroke: var(--ph-cyan);
    stroke-width: 6;
    opacity: 0.14;
  }
  .mini .glow {
    stroke-width: 9;
  }
  .edit .glow {
    stroke: var(--ph-amber);
    opacity: 0.18;
  }
  .run .pulse {
    stroke-dasharray: 5 3;
  }
  .dot {
    fill: var(--ph-hot);
  }
  .lab {
    font: var(--ph-fs-xs, 10px) var(--ph-ui);
    fill: var(--ph-cyan);
    letter-spacing: 0.02em;
  }
  .edit .lab {
    fill: var(--ph-amber);
  }
  .fline {
    stroke: var(--ph-danger);
    stroke-width: 1.4;
    stroke-dasharray: 6 5;
  }
  .fdot {
    fill: var(--ph-danger);
  }
  .fdot.glowc {
    opacity: 0.28;
  }
</style>
