<script lang="ts">
  // VRAM cliff in FIT PREVIEW (idle launcher). Same frame as the live cliff: y = GiB from 0 (floor)
  // to the dashed ceiling at totalGiB, the edge at the right drops to the SYSTEM RAM floor.
  // - The bright line is what is in use right now (baselineGiB: driver and other processes).
  // - The ghost stack on top of it is the selected System's expectedVram, strictly to scale, labelled
  //   on the left. It is a projection, not a history: the traces are flat.
  // - "fits · N GiB spare" = total - baseline - expected (amber); a stack over the edge says by how much.
  import type { GpuMemory, System } from '../../lib/model/types';
  import { fmtGiB } from '../../lib/model/format';
  import { P, nextUid } from './palette';

  let { vram, system, variant = 'full', k = 1 }: { vram: GpuMemory; system: System | undefined; variant?: 'full' | 'mini'; k?: number } =
    $props();

  let w = $state(0);
  let h = $state(0);
  const uid = nextUid();
  const mini = $derived(variant === 'mini');
  const fs = $derived(Math.max(11.5, 11.75 * k));

  /** VRAM held by others. With no session running everything in use is the baseline. */
  const base = $derived(Math.max(0, vram.baselineGiB ?? vram.usedGiB));

  const geo = $derived.by(() => {
    const padL = mini ? 2 : 6;
    const padR = mini ? 2 : 6;
    const mt = mini ? 10 : Math.round(fs * 2.4);
    const mb = mini ? 4 : Math.round(fs * 1.2);
    const xNow = mini ? w - padR - 10 : padL + (w - padL - padR) * 0.77;
    const yRam = h - mb;
    const yCeil = mt;
    const total = Math.max(0.5, vram.totalGiB);
    const yOf = (g: number) => yRam - Math.max(0, Math.min(1.06, g / total)) * (yRam - yCeil);
    return { padL, padR, xNow, yRam, yCeil, total, yOf };
  });

  const stack = $derived.by(() => {
    let acc = base;
    return (system?.expectedVram ?? []).map((l, j) => {
      const lo = acc;
      acc += l.gib;
      return { ...l, j, lo, hi: acc };
    });
  });
  const need = $derived(stack.reduce((a, l) => a + l.gib, 0));
  const top = $derived(base + need);
  const spare = $derived(vram.totalGiB - top);
  const fits = $derived(spare >= 0);
  const tight = $derived(spare < vram.warnBelowGiB);

  const gib1 = (g: number) => (g < 1 ? g.toFixed(2) : g.toFixed(1));

  /** Left labels: band middle (top line for thin bands), nudged apart top-down then bottom-up. */
  const labels = $derived.by(() => {
    if (mini || stack.length === 0) return [];
    const { yOf, yCeil, yRam } = geo;
    const gap = fs * 1.45;
    const items = [
      ...stack.map((l) => {
        const yt = yOf(l.hi);
        const yb = yOf(l.lo);
        return { key: l.id + l.j, text: l.label, num: gib1(l.gib), want: yb - yt > gap * 1.4 ? (yt + yb) / 2 : yt + fs * 0.8, tone: 'lay' };
      }),
      { key: 'base', text: 'in use now', num: gib1(base), want: yOf(base), tone: 'base' },
    ].sort((a, b) => a.want - b.want);
    const lo = yCeil + fs * 0.9;
    const hi = yRam - fs * 1.0;
    const ys = items.map((it) => it.want);
    for (let i = 0; i < ys.length; i++) ys[i] = Math.max(ys[i], i === 0 ? lo : ys[i - 1] + gap);
    for (let i = ys.length - 1; i >= 0; i--) ys[i] = Math.min(ys[i], i === ys.length - 1 ? hi : ys[i + 1] - gap);
    return items.map((it, i) => ({ ...it, y: ys[i] }));
  });

  const lineTone = (j: number, n: number) => {
    if (!mini) return P.cyan;
    const t = n <= 1 ? 0 : 1 - j / (n - 1);
    return t > 0.66 ? P.amber : t > 0.33 ? '#B9D98A' : P.cyan;
  };
</script>

<div class="fit {variant}" bind:clientWidth={w} bind:clientHeight={h}>
  {#if w > 20 && h > 20}
    {@const yTop = geo.yOf(top)}
    {@const yBase = geo.yOf(base)}
    <svg
      width={w}
      height={h}
      viewBox="0 0 {w} {h}"
      role="img"
      aria-label="Fit preview: {system?.label ?? ''} needs {fmtGiB(need)} GiB on top of {fmtGiB(base)} GiB in use; {fits
        ? `${fmtGiB(spare)} GiB spare`
        : `${fmtGiB(-spare)} GiB over`} of {fmtGiB(vram.totalGiB)} GiB"
    >
      <defs>
        <linearGradient id="fit-fill-{uid}" x1="0" y1={geo.yCeil} x2="0" y2={geo.yRam} gradientUnits="userSpaceOnUse">
          <stop offset="0" stop-color={mini ? '#3D7A48' : '#16889B'} stop-opacity={mini ? 0.4 : 0.34} />
          <stop offset="0.4" stop-color={mini ? '#21503A' : '#0D5866'} stop-opacity="0.22" />
          <stop offset="1" stop-color={mini ? '#0E2A20' : '#08303A'} stop-opacity="0.06" />
        </linearGradient>
      </defs>

      {#if stack.length}
        <!-- the projected stack: faint fill under its top, one band per expected layer -->
        <path d="M{geo.padL},{yTop}H{geo.xNow - 2}V{yBase}H{geo.padL}Z" style="fill:url(#fit-fill-{uid})" />
        {#each stack as l (l.id + l.j)}
          {@const yt = geo.yOf(l.hi)}
          {@const yb = geo.yOf(l.lo)}
          {#if l.j % 2 === 1 && yb - yt > 0.5}
            <rect x={geo.padL} y={yt} width={Math.max(0, geo.xNow - 2 - geo.padL)} height={yb - yt} class="band" />
          {/if}
        {/each}
        {#each stack as l (l.id + l.j)}
          {@const y = geo.yOf(l.hi)}
          {@const xd = geo.xNow - 2 * (stack.length - l.j)}
          {@const d = `M${geo.padL},${y.toFixed(1)}H${xd.toFixed(1)}V${yBase.toFixed(1)}`}
          <path {d} class="glow" stroke={lineTone(l.j, stack.length)} />
          <path {d} class="ghost" stroke={lineTone(l.j, stack.length)} />
        {/each}
        <path class="ghost edge" d="M{geo.padL},{yBase}V{yTop}" />
      {/if}

      <!-- ceiling: usable dedicated memory -->
      <line class="ceil" class:amber={mini} class:over={!fits} x1={geo.padL} x2={w - geo.padR} y1={geo.yCeil} y2={geo.yCeil} />

      <!-- what is in use now (driver, other processes), and the edge -->
      <path class="glow top" d="M{geo.padL},{yBase}H{geo.xNow}V{geo.yRam}" />
      <path class="now" d="M{geo.padL},{yBase}H{geo.xNow}V{geo.yRam}" />
      <line class="floor glow" x1={geo.xNow} x2={w - geo.padR} y1={geo.yRam} y2={geo.yRam} />
      <line class="floor" x1={geo.xNow} x2={w - geo.padR} y1={geo.yRam} y2={geo.yRam} />

      {#if !mini}
        <text class="t val" x={geo.padL + 2} y={geo.yCeil - fs * 0.6} style="font-size:{fs}px">{fmtGiB(vram.totalGiB)} GiB</text>

        {#each labels as l (l.key)}
          <path class="tick" d="M{geo.padL},{l.y.toFixed(1)}H{(geo.padL + fs * 0.75).toFixed(1)}" />
          <text class="t {l.tone}" x={geo.padL + fs * 1.05} y={l.y + fs * 0.36} style="font-size:{fs}px"
            >{l.text}<tspan class="num" dx={fs * 0.5}>{l.num}</tspan></text
          >
        {/each}

        {#if stack.length}
          <!-- spare (or overshoot) between the stack top and the ceiling -->
          {@const ax = geo.xNow - fs * 6}
          {@const yEnd = fits ? yTop - 3 : geo.yCeil + 3}
          <path class="ann" class:bad={!fits || tight} d="M{ax + fs * 0.45},{geo.yCeil - fs * 1.2}H{ax}V{yEnd}" />
          {#if fits}
            <path class="ann" class:bad={tight} d="M{ax - 3},{yEnd - 5}L{ax},{yEnd}L{ax + 3},{yEnd - 5}" />
          {/if}
          <text class="t ann-t" class:bad={!fits || tight} x={ax + fs * 0.75} y={geo.yCeil - fs * 0.9} style="font-size:{fs * 1.05}px"
            >{fits ? `fits · ${fmtGiB(spare)} GiB spare` : `over the edge · ${fmtGiB(-spare)} GiB`}</text
          >
        {:else}
          <text class="t mut" x={geo.padL + fs} y={(geo.yCeil + geo.yRam) / 2} style="font-size:{fs}px">no fit preview for this preset</text>
        {/if}

        <text class="t ram" x={w - geo.padR} y={geo.yRam - fs * 0.6} text-anchor="end" style="font-size:{fs}px">SYSTEM RAM</text>
      {/if}
    </svg>
  {/if}
</div>

<style>
  .fit {
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
    vector-effect: non-scaling-stroke;
  }
  .band {
    fill: var(--ph-cyan);
    fill-opacity: 0.045;
  }
  .mini .band {
    fill: #7fd89a;
  }
  .ghost {
    stroke-width: 1.4;
    opacity: 0.8;
    stroke-linejoin: round;
  }
  .ghost.edge {
    stroke: var(--ph-cyan);
    opacity: 0.55;
  }
  .mini .ghost {
    stroke-width: 2.4;
    opacity: 0.85;
  }
  .glow {
    stroke-width: 5;
    opacity: 0.1;
    stroke-linejoin: round;
  }
  .glow.top {
    stroke: var(--ph-cyan);
    stroke-width: 7;
    opacity: 0.18;
  }
  .now {
    stroke: var(--ph-hot);
    stroke-width: 1.8;
    stroke-linejoin: round;
  }
  .mini .now {
    stroke-width: 2.6;
  }
  .ceil {
    stroke: var(--ph-cyan);
    stroke-width: 1.2;
    stroke-dasharray: 7 6;
    opacity: 0.85;
  }
  .ceil.amber {
    stroke: var(--ph-amber);
    stroke-width: 2.2;
    stroke-dasharray: 10 7;
    opacity: 1;
  }
  .ceil.over {
    stroke: var(--ph-danger);
  }
  .floor {
    stroke: var(--ph-amber);
    stroke-width: 2;
  }
  .floor.glow {
    stroke-width: 7;
    opacity: 0.22;
  }
  .tick {
    stroke: var(--ph-cyan);
    stroke-width: 1.2;
    opacity: 0.8;
  }
  .ann {
    stroke: var(--ph-amber);
    stroke-width: 1.3;
  }
  .ann.bad {
    stroke: var(--ph-danger);
  }
  .t {
    font-family: var(--ph-ui);
    fill: var(--ph-ink);
  }
  .t.lay,
  .t.base {
    paint-order: stroke;
    stroke: var(--ph-glass);
    stroke-width: 5px;
    stroke-linejoin: round;
  }
  .t.val {
    fill: var(--ph-ink);
  }
  .t.mut {
    fill: var(--ph-muted);
  }
  .t.lay {
    fill: var(--ph-ink);
  }
  .t.base {
    fill: var(--ph-muted);
  }
  .t .num {
    fill: var(--ph-cyan);
  }
  .t.ram {
    fill: var(--ph-cyan);
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
  }
  .ann-t {
    fill: var(--ph-amber);
  }
  .ann-t.bad {
    fill: var(--ph-danger);
  }
</style>
