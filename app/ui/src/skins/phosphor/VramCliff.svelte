<script lang="ts">
  // VRAM cliff: x = time (the last 5 minutes of vram.history, 1 Hz), y = GiB from 0 (the floor)
  // to the dashed ceiling at totalGiB. The axis is NOT truncated, so every layer band is drawn to
  // scale and older samples are never clamped to a fake floor.
  // - The bright top trace is the measured usedGiB history.
  // - The stacked lines under it are the CURRENT composition (vram.layers), labelled in the
  //   "composition · now" column. They fade toward the past and are clipped under the measured
  //   trace, so they never claim more than was used at any moment.
  // - At "now" every trace drops to the SYSTEM RAM floor; spillMiB sits on that floor in amber,
  //   on the same GiB scale.
  // - GPU dormant (gpu != null, vm.vram.dormant): the allocations are paged out to system RAM, so the
  //   composition traces turn into dim dashed ghosts and a dashed amber line marks the committed level
  //   (resident + paged out, to scale). Its span down to the trace is what is paged out; it goes over the
  //   edge into SYSTEM RAM at "now". While the GPU wakes, the span closes as the trace climbs back up.
  // - spanS narrows the time window (loading: the climb since launch fills the width); the left axis
  //   label always names the real span. markAgoS draws the fault moment as a red dashed line.
  import type { GpuMemory } from '../../lib/model/types';
  import { fmtGiB } from '../../lib/model/format';
  import { P, nextUid } from './palette';
  import { fmtSpan, type GpuSleep } from './geom';

  let {
    vram,
    variant = 'full',
    k = 1,
    spanS = 300,
    markAgoS = null,
    gpu = null,
  }: {
    vram: GpuMemory;
    variant?: 'full' | 'mini';
    k?: number;
    spanS?: number;
    markAgoS?: number | null;
    /** Set while the GPU is dormant (see gpuSleep in geom.ts). */
    gpu?: GpuSleep | null;
  } = $props();

  const SLOTS = $derived(Math.max(2, Math.round(spanS)));
  let w = $state(0);
  let h = $state(0);
  const uid = nextUid();

  const mini = $derived(variant === 'mini');
  const fs = $derived(Math.max(11, 13.5 * k));

  const geo = $derived.by(() => {
    const padL = mini ? 2 : 6;
    const padR = mini ? 2 : 6;
    const mt = mini ? 10 : Math.round(fs * 2.1);
    const mb = mini ? 4 : Math.round(fs * 1.55);
    const xNow = mini ? w - padR - 10 : padL + (w - padL - padR) * 0.77;
    const yRam = h - mb;
    const yCeil = mt;
    const total = Math.max(0.5, vram.totalGiB);
    const pxPerGiB = (yRam - yCeil) / total;
    const yOf = (g: number) => yRam - Math.max(0, Math.min(1.04, g / total)) * (yRam - yCeil);
    return { padL, padR, mt, mb, xNow, yRam, yCeil, total, yOf, pxPerGiB };
  });

  /** Stepped path of the measured history, newest sample at xNow. */
  const hist = $derived.by(() => {
    const { padL, xNow, yOf } = geo;
    const all = vram.history.length ? vram.history : [vram.usedGiB];
    const v = all.length > SLOTS ? all.slice(all.length - SLOTS) : all;
    const n = v.length;
    const dx = (xNow - padL) / (SLOTS - 1);
    let d = '';
    let firstX = xNow;
    let lastY = 0;
    // start of the newest run at the current level (within 0.05 GiB): the "free" arrow lands on it
    let j = n - 1;
    while (j > 0 && Math.abs(v[j - 1] - v[n - 1]) <= 0.05) j--;
    const flatX = j > 0 ? xNow - (n - j) * dx : xNow - (n - 1) * dx;
    for (let i = 0; i < n; i++) {
      const x = xNow - (n - 1 - i) * dx;
      const y = yOf(v[i]);
      if (i === 0) {
        // the oldest sample is only the left end of the next one's step (no stub at the edge)
        firstX = x;
        if (n === 1) d = `M${x.toFixed(1)},${y.toFixed(1)}`;
      } else if (i === 1) {
        d = `M${firstX.toFixed(1)},${y.toFixed(1)}H${x.toFixed(1)}`;
      } else {
        // each sample's level spans the second that ended at its x, so the newest one is visible too
        d += `V${y.toFixed(1)}H${x.toFixed(1)}`;
      }
      lastY = y;
    }
    return { d, firstX, lastY, flatX };
  });

  const area = $derived(`${hist.d}V${geo.yRam}H${hist.firstX.toFixed(1)}Z`);

  /** x of the fault moment (null when there is none or it left the window). */
  const xMark = $derived.by(() => {
    if (markAgoS === null || markAgoS === undefined) return null;
    const a = Math.round(markAgoS);
    if (a >= SLOTS) return null;
    return geo.xNow - a * ((geo.xNow - geo.padL) / (SLOTS - 1));
  });

  const layers = $derived.by(() => {
    let acc = 0;
    return vram.layers.map((l, j) => {
      const lo = acc;
      acc += l.gib;
      return { ...l, j, lo, hi: acc };
    });
  });

  /**
   * x of the "free" arrow: on the newest stretch of the trace that is at the current level, so its tip
   * touches the line it measures from (a flat live trace keeps the usual spot, 5.5 em left of now).
   */
  /** x where the GPU went dormant (the left edge of the paged-out stretch). */
  const xDorm = $derived.by(() => {
    if (!gpu) return geo.xNow;
    const dx = (geo.xNow - geo.padL) / (SLOTS - 1);
    return Math.max(geo.padL, geo.xNow - Math.max(0, gpu.sinceS) * dx);
  });

  const ax = $derived.by(() => {
    const { xNow } = geo;
    if (gpu) {
      // dormant: the middle of the paged-out stretch, never closer to "now" than the arrow head is wide
      return xNow - Math.min(fs * 5.5, Math.max(fs * 0.7, (xNow - xDorm) / 2));
    }
    let a = Math.min(xNow - fs * 1.2, Math.max(xNow - fs * 5.5, hist.flatX + fs * 0.6));
    // keep clear of the fault marker and its label (left of it, on the floor)
    if (xMark !== null && a > xMark - fs * 4.5 && a < xMark + fs * 0.6) {
      a = xNow - xMark >= fs * 1.4 ? xMark + fs * 0.6 : xMark - fs * 4.5;
    }
    return a;
  });

  /** Dormant GPU: the committed level and the resident level, in pixels. */
  const yCom = $derived(gpu ? geo.yOf(gpu.committedGiB) : 0);
  const yRes = $derived(geo.yOf(vram.usedGiB));
  const free = $derived(Math.max(0, vram.totalGiB - vram.usedGiB));
  const tight = $derived(free < vram.warnBelowGiB);
  const spillGiB = $derived(Math.max(0, vram.spillMiB) / 1024);
  const spillPx = $derived(Math.min(geo.yRam - geo.yCeil, spillGiB * geo.pxPerGiB));
  /** Baseline of the "SYSTEM RAM" label: on the floor, above any spill block. */
  const ramTextY = $derived(geo.yRam - Math.max(spillPx, 0) - fs * 0.55);

  /** Composition labels in the "now" column, nudged apart so none overlap. */
  const labels = $derived.by(() => {
    if (mini) return [];
    const { yOf, yCeil } = geo;
    const gap = fs * 1.35;
    const top = yCeil + fs * 2.6;
    const bottom = Math.max(top, ramTextY - fs * 1.45);
    const items = layers
      .map((l) => ({ ...l, want: (yOf(l.lo) + yOf(l.hi)) / 2 }))
      .sort((a, b) => a.want - b.want);
    const ys = items.map((it) => it.want);
    for (let i = 0; i < ys.length; i++) ys[i] = Math.max(ys[i], i === 0 ? top : ys[i - 1] + gap);
    for (let i = ys.length - 1; i >= 0; i--) ys[i] = Math.min(ys[i], i === ys.length - 1 ? bottom : ys[i + 1] - gap);
    return items.map((it, i) => ({ ...it, y: Math.max(top, ys[i]) }));
  });

  const gib1 = (g: number) => (g < 1 ? g.toFixed(2) : g.toFixed(1));
  const lineTone = (j: number, n: number) => {
    if (!mini) return P.cyan;
    // Mini mockup: lower traces lean toward the afterglow amber.
    const t = n <= 1 ? 0 : 1 - j / (n - 1);
    return t > 0.66 ? P.amber : t > 0.33 ? '#B9D98A' : P.cyan;
  };
</script>

<div class="cliff {variant}" bind:clientWidth={w} bind:clientHeight={h}>
  {#if w > 20 && h > 20}
    <svg width={w} height={h} viewBox="0 0 {w} {h}" role="img" aria-label="VRAM {fmtGiB(vram.usedGiB)} of {fmtGiB(vram.totalGiB)} GiB used, spill {Math.round(vram.spillMiB)} MiB{gpu ? `, GPU ${gpu.state === 'waking' ? 'waking' : 'asleep'}, ${fmtGiB(gpu.pagedOutGiB)} GiB paged out` : ''}">
      <defs>
        <linearGradient id="cf-fill-{uid}" x1="0" y1={geo.yCeil} x2="0" y2={geo.yRam} gradientUnits="userSpaceOnUse">
          <stop offset="0" stop-color={mini ? '#3D7A48' : '#16889B'} stop-opacity={mini ? 0.6 : 0.7} />
          <stop offset="0.3" stop-color={mini ? '#21503A' : '#0D5866'} stop-opacity="0.45" />
          <stop offset="1" stop-color={mini ? '#0E2A20' : '#08303A'} stop-opacity="0.14" />
        </linearGradient>
        <!-- the composition is "now": it fades toward the past -->
        <linearGradient id="cf-fade-{uid}" x1={geo.padL} y1="0" x2={geo.xNow} y2="0" gradientUnits="userSpaceOnUse">
          <stop offset="0" stop-color="#fff" stop-opacity="0.3" />
          <stop offset="0.6" stop-color="#fff" stop-opacity="0.75" />
          <stop offset="1" stop-color="#fff" stop-opacity="1" />
        </linearGradient>
        <mask id="cf-mask-{uid}" maskUnits="userSpaceOnUse" x="0" y="0" width={w} height={h}>
          <rect x="0" y="0" width={w} height={h} fill="url(#cf-fade-{uid})" />
        </mask>
        <clipPath id="cf-clip-{uid}">
          <path d={area} />
        </clipPath>
        {#if gpu}
          <!-- the paged-out stretch: hatched between the trace and the committed level -->
          <pattern id="cf-hatch-{uid}" width={mini ? 11 : 8} height={mini ? 11 : 8} patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <rect x="0" y="0" width={mini ? 1.8 : 1.2} height={mini ? 11 : 8} fill={P.amber} fill-opacity={mini ? 0.5 : 0.42} />
          </pattern>
          <mask id="cf-dm-{uid}" maskUnits="userSpaceOnUse" x="0" y="0" width={w} height={h}>
            <rect x="0" y="0" width={w} height={h} fill="#fff" />
            <path d={area} fill="#000" />
          </mask>
        {/if}
      </defs>

      <!-- measured area under the history trace -->
      <path d={area} style="fill:url(#cf-fill-{uid})" />

      {#if gpu && xDorm < geo.xNow - 0.5 && geo.yRam - yCom > 1}
        <rect x={xDorm} y={yCom} width={geo.xNow - xDorm} height={geo.yRam - yCom} fill="url(#cf-hatch-{uid})" mask="url(#cf-dm-{uid})" />
      {/if}

      <!-- current composition, clipped under the measured trace, fading toward the past -->
      <g clip-path="url(#cf-clip-{uid})">
        <g mask="url(#cf-mask-{uid})">
          {#each layers as l (l.id + l.j)}
            {@const yt = geo.yOf(l.hi)}
            {@const yb = geo.yOf(l.lo)}
            {#if yb - yt > 0.5}
              <rect
                x={geo.padL}
                y={yt}
                width={Math.max(0, geo.xNow - geo.padL)}
                height={yb - yt}
                class="band"
                style="fill:{mini ? '#7FD89A' : P.cyan};fill-opacity:{l.j % 2 ? 0.1 : 0.035}"
              />
            {/if}
          {/each}
          {#if !gpu}
            {#each layers as l (l.id + l.j)}
              {@const y = geo.yOf(l.hi)}
              {@const xd = geo.xNow - 2 * (layers.length - l.j)}
              {@const d = `M${geo.padL},${y.toFixed(1)}H${xd.toFixed(1)}V${geo.yRam}`}
              <path {d} class="glow" stroke={lineTone(l.j, layers.length)} />
              <path {d} class="layer" stroke={lineTone(l.j, layers.length)} />
            {/each}
          {/if}
        </g>
      </g>

      {#if gpu}
        <!-- dormant: the allocations as dim dashed ghosts, not clipped (they are not resident) -->
        <g mask="url(#cf-mask-{uid})">
          {#each layers as l (l.id + l.j)}
            {@const y = geo.yOf(l.hi)}
            {@const xd = geo.xNow - 2 * (layers.length - l.j)}
            <path d="M{geo.padL},{y.toFixed(1)}H{xd.toFixed(1)}" class="ghost" />
          {/each}
        </g>
      {/if}

      <!-- ceiling: usable dedicated memory -->
      <line
        class="ceil"
        class:amber={mini && !gpu}
        x1={geo.padL}
        x2={w - geo.padR}
        y1={geo.yCeil}
        y2={geo.yCeil}
      />

      <!-- measured usage trace and its drop at "now" -->
      <path d="{hist.d}V{geo.yRam}" class="glow top" />
      <path d="{hist.d}V{geo.yRam}" class="trace" />
      {#if gpu}
        <!-- the committed level, and its fall over the edge into system RAM -->
        <path class="com glow" d="M{geo.padL},{yCom.toFixed(1)}H{geo.xNow}" />
        <path class="com" d="M{geo.padL},{yCom.toFixed(1)}H{geo.xNow}" />
        <path class="com edge" d="M{geo.xNow},{yCom.toFixed(1)}V{geo.yRam}" />
      {:else}
        <line class="drop" x1={geo.xNow} x2={geo.xNow} y1={hist.lastY + (geo.yRam - hist.lastY) * 0.4} y2={geo.yRam} />
      {/if}

      {#if xMark !== null}
        <line class="mark glow" x1={xMark} x2={xMark} y1={geo.yCeil + (mini ? 4 : fs * 0.6)} y2={geo.yRam} />
        <line class="mark" x1={xMark} x2={xMark} y1={geo.yCeil + (mini ? 4 : fs * 0.6)} y2={geo.yRam} />
        <circle class="mark-dot" cx={xMark} cy={geo.yRam} r={mini ? 6 : 4} />
        {#if !mini}
          <!-- labelled right at its dot, on the floor side away from "now" -->
          <text class="t mark-t" x={xMark - fs * 0.6} y={geo.yRam - fs * 0.5} text-anchor="end" style="font-size:{fs * 0.9}px">fault</text>
        {/if}
      {/if}

      {#if !mini}
        <!-- SYSTEM RAM floor beyond the edge, spill in amber on the same GiB scale -->
        {#if spillPx > 0.5}
          <rect x={geo.xNow} y={geo.yRam - spillPx} width={w - geo.padR - geo.xNow} height={spillPx} class="spill" />
        {/if}
        <line class="floor glow" x1={geo.xNow} x2={w - geo.padR} y1={geo.yRam} y2={geo.yRam} />
        <line class="floor" x1={geo.xNow} x2={w - geo.padR} y1={geo.yRam} y2={geo.yRam} />

        <!-- axis: ceiling value and time span (the floor is 0 GiB) -->
        <text class="t val" x={geo.padL + 2} y={geo.yCeil - fs * 0.55} style="font-size:{fs}px">{fmtGiB(vram.totalGiB)} GiB</text>
        <text class="t mut" x={geo.padL + 2} y={geo.yRam + fs * 1.3} style="font-size:{fs * 0.88}px">{fmtSpan(SLOTS)}</text>
        <text class="t mut" x={geo.xNow - 4} y={geo.yRam + fs * 1.3} text-anchor="end" style="font-size:{fs * 0.88}px">now</text>

        {#if gpu}
          <!-- dormant: the span from what is resident up to the committed level is what is paged out -->
          <path class="pgo-lead" d="M{ax + fs * 0.45},{geo.yCeil - fs * 1.15}H{ax}V{yCom.toFixed(1)}" />
          {#if yRes - yCom > 12}
            <path class="pgo-span" class:waking={gpu.state === 'waking'} d="M{ax},{yRes.toFixed(1)}V{(yCom + 3).toFixed(1)}" />
            <path class="pgo-head" d="M{ax - 3.5},{(yCom + 8).toFixed(1)}L{ax},{(yCom + 2.5).toFixed(1)}L{ax + 3.5},{(yCom + 8).toFixed(1)}" />
            <path class="pgo-head" d="M{ax - 3.5},{yRes.toFixed(1)}H{ax + 3.5}" />
          {/if}
          <text class="t pgo-t" x={ax + fs * 0.75} y={geo.yCeil - fs * 0.85} style="font-size:{fs * 0.95}px">paged out {fmtGiB(gpu.pagedOutGiB)} GiB</text>
        {:else}
          <!-- headroom to the edge -->
          <path class="free" class:tight d="M{ax + fs * 0.45},{geo.yCeil - fs * 1.15}H{ax}V{hist.lastY - 3}" />
          <path class="free" class:tight d="M{ax - 3},{hist.lastY - 7}L{ax},{hist.lastY - 2}L{ax + 3},{hist.lastY - 7}" />
          <text class="t free-t" class:tight x={ax + fs * 0.75} y={geo.yCeil - fs * 0.85} style="font-size:{fs * 0.95}px">{fmtGiB(free)} GiB free</text>
        {/if}

        <!-- composition at "now" (dormant: the allocations, paged out) -->
        <text class="t mut" x={geo.xNow + fs * 1.6} y={geo.yCeil + fs * 1.5} style="font-size:{fs * 0.88}px">{gpu ? 'allocations · paged out' : 'composition · now'}</text>
        {#each labels as l (l.id + l.j)}
          {@const yb = (geo.yOf(l.lo) + geo.yOf(l.hi)) / 2}
          <path class="leader" class:ghost-l={!!gpu} d="M{geo.xNow + 3},{yb.toFixed(1)}L{(geo.xNow + fs * 1.2).toFixed(1)},{l.y.toFixed(1)}H{(geo.xNow + fs * 1.45).toFixed(1)}" />
          <text class="t lay" class:ghost-t={!!gpu} x={geo.xNow + fs * 1.6} y={l.y + fs * 0.35} style="font-size:{fs}px">{l.label}<tspan class="num" dx={fs * 0.5}>{gib1(l.gib)}</tspan></text>
        {/each}

        <!-- the floor's own labels: its name on it, the spill under it -->
        <text class="t ram" x={w - geo.padR} y={ramTextY} text-anchor="end" style="font-size:{fs}px">SYSTEM RAM</text>
        <text class="t mut" x={w - geo.padR} y={geo.yRam + fs * 1.3} text-anchor="end" style="font-size:{fs * 0.9}px"
          >spill to shared memory: <tspan class="spill-t">{Math.round(vram.spillMiB)} MiB</tspan></text
        >
      {/if}
    </svg>
  {/if}
</div>

<style>
  .cliff {
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
  .layer {
    stroke-width: 1.4;
  }
  .mini .layer {
    stroke-width: 2.2;
    opacity: 0.85;
    stroke-linejoin: round;
  }
  .glow {
    stroke-width: 5;
    opacity: 0.14;
    stroke-linejoin: round;
  }
  .glow.top {
    stroke: var(--ph-cyan);
    stroke-width: 7;
    opacity: 0.18;
  }
  .trace {
    stroke: var(--ph-hot);
    stroke-width: 1.8;
    stroke-linejoin: round;
  }
  .mini .trace {
    stroke-width: 2.6;
  }
  .mini .glow.top {
    stroke-width: 10;
    opacity: 0.22;
  }
  .drop {
    stroke: var(--ph-amber);
    stroke-width: 2;
    opacity: 0.9;
  }
  /* dormant GPU: allocations paged out. Dim ghosts for the composition, one amber line for the commitment. */
  .ghost {
    stroke: var(--ph-cyan);
    stroke-width: 1.2;
    stroke-dasharray: 3 6;
    opacity: 0.42;
  }
  .mini .ghost {
    stroke-width: 2;
    stroke-dasharray: 5 8;
    opacity: 0.5;
  }
  .com {
    stroke: var(--ph-amber);
    stroke-width: 1.8;
    stroke-dasharray: 10 6;
  }
  .mini .com {
    stroke-width: 2.8;
    stroke-dasharray: 14 8;
  }
  .com.glow {
    stroke-width: 7;
    stroke-dasharray: none;
    opacity: 0.18;
  }
  .mini .com.glow {
    stroke-width: 11;
  }
  .com.edge {
    stroke-dasharray: 5 5;
    opacity: 0.9;
  }
  .mini .com.edge {
    stroke-dasharray: 7 7;
  }
  .pgo-lead,
  .pgo-span,
  .pgo-head {
    stroke: var(--ph-amber);
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .pgo-span {
    stroke-dasharray: 4 4;
  }
  .pgo-span.waking {
    stroke-width: 2;
    animation: pgo-flow 0.6s linear infinite;
  }
  @keyframes pgo-flow {
    to {
      stroke-dashoffset: -8;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .pgo-span.waking {
      animation: none;
    }
  }
  .t.pgo-t {
    fill: var(--ph-amber);
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.35);
  }
  .leader.ghost-l {
    stroke-dasharray: 2 3;
    opacity: 0.55;
  }
  .t.lay.ghost-t {
    fill: var(--ph-muted);
  }
  .t.lay.ghost-t .num {
    fill: var(--ph-amber);
    opacity: 0.8;
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
  .floor {
    stroke: var(--ph-amber);
    stroke-width: 2;
  }
  .floor.glow {
    stroke-width: 7;
    opacity: 0.22;
  }
  .spill {
    fill: var(--ph-amber);
    opacity: 0.5;
  }
  .mark {
    stroke: var(--ph-danger);
    stroke-width: 1.4;
    stroke-dasharray: 5 4;
  }
  .mini .mark {
    stroke-width: 2.4;
    stroke-dasharray: 8 6;
  }
  .mark.glow {
    stroke-width: 6;
    stroke-dasharray: none;
    opacity: 0.14;
  }
  .mark-dot {
    fill: var(--ph-danger);
  }
  .t.mark-t {
    fill: var(--ph-danger);
    letter-spacing: 0.06em;
    paint-order: stroke;
    stroke: var(--ph-glass);
    stroke-width: 4px;
    stroke-linejoin: round;
  }
  .leader {
    stroke: var(--ph-muted);
    stroke-width: 1;
    opacity: 0.8;
  }
  .free {
    stroke: var(--ph-amber);
    stroke-width: 1.3;
  }
  .free.tight {
    stroke: var(--ph-danger);
  }
  .t {
    font-family: var(--ph-ui);
    fill: var(--ph-ink);
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
  .t.lay .num {
    fill: var(--ph-cyan);
  }
  .t.ram {
    fill: var(--ph-cyan);
    letter-spacing: 0.08em;
  }
  .spill-t {
    fill: var(--ph-amber);
  }
  .free-t {
    fill: var(--ph-amber);
  }
  .free-t.tight {
    fill: var(--ph-danger);
  }
</style>
