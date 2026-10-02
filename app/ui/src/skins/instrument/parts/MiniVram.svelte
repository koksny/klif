<script lang="ts">
  // Mini VRAM gauge: half dial (0 .. totalGiB, linear) with the cliff printed on its face as one bold
  // outline. Plateau = usedGiB history, height on the same 0..total axis; strata faintly filled,
  // proportional to GiB. Red zone = last warnBelowGiB, bright only while free < warnBelowGiB.
  // Modes as on the full dial: 'fit' = the idle preview (expected layers on the baseline, dashed,
  // needle at the expected total); 'fault' = the history above what is still resident drawn dashed,
  // with the fracture where it fell.
  // Live while vram.dormant is set (the GPU asleep, the allocations paged out to system RAM): the needle
  // drops to what is resident, a dashed ghost needle stays at the allocated total, the strata are
  // outlines inside a dashed envelope and the history silhouette is dimmed.
  import type { GpuMemory, VramLayer } from '../../../lib/model/types';
  import { VRAM_A0, VRAM_A1, clamp, polar, sectorPath, sleepOf } from '../theme';

  let {
    vram,
    mode = 'live',
    fit = null,
  }: { vram: GpuMemory; mode?: 'live' | 'fit' | 'fault'; fit?: { baseline: number; layers: VramLayer[] } | null } = $props();

  const isFit = $derived(mode === 'fit' && !!fit);
  const sleep = $derived(mode === 'live' ? sleepOf(vram) : null);
  const layers = $derived.by<VramLayer[]>(() => {
    if (!isFit || !fit) return vram.layers;
    const base = Math.max(0, fit.baseline);
    const out: VramLayer[] = base >= 0.005 ? [{ id: 'other', label: 'other', gib: base }] : [];
    return out.concat(fit.layers);
  });

  const uid = $props.id();
  const P = { x: 270, y: 262 };
  // The printed cliff sits in the upper face below the tick band, its floor (0 GiB) level with the
  // 0 tick and above the pivot, as in the mockup.
  const BOX = { x0: 96, xc: 388, top: 112, base: 204 };
  const H = BOX.base - BOX.top;

  const total = $derived(Math.max(0.01, vram.totalGiB));
  const used = $derived(isFit ? layers.reduce((a, l) => a + Math.max(0, l.gib), 0) : Math.max(0, vram.usedGiB));
  const histSrc = $derived(isFit ? [used, used] : vram.history.length ? vram.history : [used]);
  const free = $derived(Math.max(0, total - used));
  const warn = $derived(Math.max(0, Math.min(vram.warnBelowGiB, total)));
  const hot = $derived(free < warn);
  const ang = (g: number) => VRAM_A0 - (VRAM_A0 - VRAM_A1) * clamp(g / total);
  const yOf = (g: number) => BOX.base - clamp(g / total, 0, 1.02) * H;

  const ticks = $derived.by(() => {
    const major = total <= 20 ? 4 : total <= 40 ? 8 : 16;
    const out: { x1: number; y1: number; x2: number; y2: number }[] = [];
    for (let g = 0; g < total - major * 0.3; g += major) {
      const [x1, y1] = polar(P.x, P.y, 236, ang(g));
      const [x2, y2] = polar(P.x, P.y, 206, ang(g));
      out.push({ x1, y1, x2, y2 });
    }
    const [x1, y1] = polar(P.x, P.y, 236, VRAM_A1);
    const [x2, y2] = polar(P.x, P.y, 200, VRAM_A1);
    out.push({ x1, y1, x2, y2 });
    return out;
  });
  const red = $derived(warn > 0 ? sectorPath(P.x, P.y, 198, 240, ang(total - warn), VRAM_A1) : '');

  const land = $derived.by(() => {
    const hist = histSrc;
    const N = hist.length;
    const stride = Math.max(1, Math.floor(N / 80));
    let top = '';
    for (let i = 0; i < N; i += stride) {
      const x = BOX.x0 + (N > 1 ? i / (N - 1) : 0) * (BOX.xc - BOX.x0);
      top += `${top ? ' L' : 'M'}${x.toFixed(1)},${yOf(hist[i]).toFixed(1)}`;
    }
    const yT = yOf(used);
    const h = BOX.base - yT;
    top += ` L${BOX.xc},${yT.toFixed(1)}`;
    const face = ` C${BOX.xc + 16},${(yT + h * 0.15).toFixed(1)} ${BOX.xc + 24},${(BOX.base - h * 0.35).toFixed(1)} ${BOX.xc + 34},${(BOX.base - h * 0.1).toFixed(1)} C${BOX.xc + 40},${BOX.base - 2} ${BOX.xc + 48},${BOX.base} ${BOX.xc + 56},${BOX.base}`;
    const y0 = yOf(hist[0]).toFixed(1);
    return {
      outline: `M${BOX.x0},${BOX.base} L${BOX.x0},${y0} ${top.replace(/^M[^ ]+/, '')}${face}`,
      fill: `M${BOX.x0},${BOX.base} L${BOX.x0},${y0} ${top.replace(/^M[^ ]+/, '')}${face} Z`,
    };
  });
  // Asleep: the envelope of the allocations, a flat top at the allocated total with the same cliff face.
  const envelope = $derived.by(() => {
    if (!sleep) return null;
    const yT = yOf(sleep.allocGiB);
    const h = BOX.base - yT;
    const face = ` C${BOX.xc + 16},${(yT + h * 0.15).toFixed(1)} ${BOX.xc + 24},${(BOX.base - h * 0.35).toFixed(1)} ${BOX.xc + 34},${(BOX.base - h * 0.1).toFixed(1)} C${BOX.xc + 40},${BOX.base - 2} ${BOX.xc + 48},${BOX.base} ${BOX.xc + 56},${BOX.base}`;
    const top = `M${BOX.x0},${yT.toFixed(1)} L${BOX.xc},${yT.toFixed(1)}`;
    return { fill: `M${BOX.x0},${BOX.base} L${BOX.x0},${yT.toFixed(1)} L${BOX.xc},${yT.toFixed(1)}${face} Z`, outline: `${top}${face}` };
  });
  const bands = $derived.by(() => {
    let acc = 0;
    return layers.map((l, i) => {
      const lo = acc;
      acc += Math.max(0, l.gib);
      return { i, y: yOf(acc), h: yOf(lo) - yOf(acc) };
    });
  });
  // Faint strata (proportional to GiB) under a bold outline: the mockup reads as a line drawing.
  const TONES = ['rgba(237,230,214,0.07)', 'rgba(237,230,214,0.13)', 'rgba(237,230,214,0.05)', 'rgba(237,230,214,0.1)'];
  // The needle is drawn pointing at 9 o'clock (180 deg); turn it clockwise to A0, then by used/total.
  const needleDeg = $derived(180 - VRAM_A0 + (VRAM_A0 - VRAM_A1) * clamp(used / total));
  const ghostDeg = $derived(sleep ? 180 - VRAM_A0 + (VRAM_A0 - VRAM_A1) * clamp(sleep.allocGiB / total) : 0);
  const NEEDLE = `M${P.x + 30},${P.y} L${P.x + 20},${P.y - 9} L${P.x - 200},${P.y - 5.5} L${P.x - 214},${P.y} L${P.x - 200},${P.y + 5.5} L${P.x + 20},${P.y + 9} Z`;
</script>

<div class="dialbox">
<svg class="mv" viewBox="0 0 540 330" role="img" aria-label="VRAM {used.toFixed(2)} of {total.toFixed(2)} GiB">
  <defs>
    <linearGradient id="{uid}-bz" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#3a3b3f" />
      <stop offset="1" stop-color="#151618" />
    </linearGradient>
    <radialGradient id="{uid}-fc" cx="50%" cy="80%" r="80%">
      <stop offset="0" stop-color="#1e1f22" />
      <stop offset="1" stop-color="#121315" />
    </radialGradient>
    <clipPath id="{uid}-l"><path d={land.fill} /></clipPath>
    {#if envelope}<clipPath id="{uid}-e"><path d={envelope.fill} /></clipPath>{/if}
    <pattern id="{uid}-hz" width="12" height="12" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
      <line x1="0" y1="0" x2="0" y2="12" stroke="rgba(237,230,214,0.16)" stroke-width="3" />
    </pattern>
  </defs>
  <!-- D-shaped housing: upper half dial on a short flat foot -->
  <path d="M{P.x - 266},{P.y} A266,266 0 0 1 {P.x + 266},{P.y} L{P.x + 266},318 Q{P.x + 266},328 {P.x + 256},328 L{P.x - 256},328 Q{P.x - 266},328 {P.x - 266},318 Z" fill="#0b0c0d" />
  <path d="M{P.x - 259},{P.y} A259,259 0 0 1 {P.x + 259},{P.y} L{P.x + 259},314 Q{P.x + 259},321 {P.x + 252},321 L{P.x - 252},321 Q{P.x - 259},321 {P.x - 259},314 Z" fill="url(#{uid}-bz)" />
  <path d="M{P.x - 244},{P.y} A244,244 0 0 1 {P.x + 244},{P.y} L{P.x + 244},306 L{P.x - 244},306 Z" fill="url(#{uid}-fc)" />

  {#if red}<path d={red} class="red" class:hot />{/if}
  {#each ticks as t, i (i)}
    <line x1={t.x1} y1={t.y1} x2={t.x2} y2={t.y2} class="tk" />
  {/each}

  {#if envelope}
    <!-- asleep: allocations hatched where they are not resident, strata as dashed lines, dim resident history -->
    <path d={envelope.fill} fill="url(#{uid}-hz)" />
    <path d={land.fill} fill="rgba(30,31,34,0.7)" />
    <g clip-path="url(#{uid}-e)">
      {#each bands as b (b.i)}
        {#if b.h >= 4}<line x1={BOX.x0} x2={BOX.xc + 80} y1={b.y} y2={b.y} class="stratum" />{/if}
      {/each}
    </g>
    <path d={envelope.outline} class="envline" />
  {:else}
    <path d={land.fill} fill="rgba(0,0,0,0.18)" />
    <g clip-path="url(#{uid}-l)">
      {#each bands as b (b.i)}
        <rect x={BOX.x0} y={b.y} width={BOX.xc - BOX.x0 + 80} height={Math.max(0, b.h)} fill={TONES[b.i % TONES.length]} />
      {/each}
    </g>
  {/if}
  <path d={land.outline} class="outline" class:dash={mode !== 'live'} class:resident={!!sleep} />
  {#if mode !== 'fit' && vram.spillMiB > 0}
    <!-- material that went over the edge: demoted to shared memory, same GiB axis -->
    {@const sh = Math.max(5, (vram.spillMiB / 1024 / total) * H)}
    <rect x={BOX.xc + 62} y={BOX.base - sh} width="26" height={sh} class="spill" />
  {/if}
  {#if mode === 'fault'}
    <!-- what is still resident, solid under the dashed ghost of what fell -->
    <line x1={BOX.x0} x2={BOX.xc + 56} y1={yOf(used)} y2={yOf(used)} class="floorline" />
  {/if}

</svg>
{#if sleep}
  <div class="nlayer" style="transform: rotate({ghostDeg}deg); transform-origin: {(P.x / 540) * 100}% {(P.y / 330) * 100}%" aria-hidden="true">
    <svg class="mv" viewBox="0 0 540 330">
      <path d={NEEDLE} class="gneedle" />
      <circle cx={P.x} cy={P.y} r="20" class="ghub" />
    </svg>
  </div>
{/if}
<div class="nlayer" style="transform: rotate({needleDeg}deg); transform-origin: {(P.x / 540) * 100}% {(P.y / 330) * 100}%" aria-hidden="true">
  <svg class="mv" viewBox="0 0 540 330">
    <path d="M{P.x + 30},{P.y} L{P.x + 20},{P.y - 9} L{P.x - 200},{P.y - 5.5} L{P.x - 214},{P.y} L{P.x - 200},{P.y + 5.5} L{P.x + 20},{P.y + 9} Z" fill="#0c0d0e" />
    <path d="M{P.x - 150},{P.y - 6} L{P.x - 202},{P.y - 5.5} L{P.x - 212},{P.y} L{P.x - 202},{P.y + 5.5} L{P.x - 150},{P.y + 6} Z" fill="#5AB6EB" />
    <path d="M{P.x - 156},{P.y - 2} L{P.x - 204},{P.y - 1.6} L{P.x - 204},{P.y + 0.4} L{P.x - 156},{P.y} Z" fill="rgba(255,255,255,0.5)" />
    <circle cx={P.x} cy={P.y} r="26" fill="#0b0c0d" />
    <circle cx={P.x} cy={P.y} r="20" fill="#2a2b2e" />
  </svg>
</div>
</div>

<style>
  .mv {
    display: block;
    width: 100%;
    height: 100%;
  }
  .tk {
    stroke: #ede6d6;
    stroke-width: 5;
  }
  .red {
    fill: rgba(255, 107, 44, 0.35);
  }
  .red.hot {
    fill: #ff6b2c;
  }
  .outline {
    fill: none;
    stroke: #ede6d6;
    stroke-width: 6;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .outline.dash {
    stroke: rgba(237, 230, 214, 0.75);
    stroke-width: 4.5;
    stroke-dasharray: 14 10;
  }
  .spill {
    fill: #ff6b2c;
  }
  .outline.resident {
    stroke: rgba(237, 230, 214, 0.5);
    stroke-width: 4.5;
  }
  .envline {
    fill: none;
    stroke: #ede6d6;
    stroke-width: 5;
    stroke-dasharray: 14 10;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .stratum {
    stroke: rgba(237, 230, 214, 0.5);
    stroke-width: 3;
    stroke-dasharray: 10 9;
  }
  .gneedle {
    fill: rgba(11, 12, 13, 0.6);
    stroke: #ede6d6;
    stroke-width: 4.5;
    stroke-dasharray: 11 8;
    stroke-linejoin: round;
  }
  .ghub {
    fill: none;
    stroke: #ede6d6;
    stroke-width: 4.5;
    stroke-dasharray: 8 7;
  }
  .floorline {
    stroke: #ede6d6;
    stroke-width: 6;
    stroke-linecap: round;
  }
  .dialbox {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .nlayer {
    position: absolute;
    inset: 0;
    transition: transform 400ms cubic-bezier(0.3, 0.8, 0.3, 1);
    will-change: transform;
    pointer-events: none;
  }
</style>
