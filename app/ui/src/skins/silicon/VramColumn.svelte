<script lang="ts">
  // The VRAM cliff as a dimensioned section drawing. Strata heights are exactly proportional to GiB
  // (one scale for the whole column); the dimension line spans totalGiB; the free gap is dimensioned
  // at true scale. Below the ground line: spill into shared system memory.
  //
  // Modes (all at the same scale, edge fixed at totalGiB):
  //   live    - the measured composition (vram.layers).
  //   preview - idle fit preview: the measured baseline, plus the selected tier's expected layers
  //             stacked on top as hatched dashed ghosts, dimensioned as "fits · N GiB spare".
  //   fault   - the measured (emptied) composition, the level held just before the fault as a red
  //             dashed ghost, and the collapsed allocation drawn below the floor.
  import type { GpuMemory, VramLayer, VramLayerId } from '../../lib/model/types';
  import { fmtGiB } from '../../lib/model/format';

  interface Props {
    vram: GpuMemory;
    u: number;
    /** System RAM size and type, printed on the (schematic) RAM band. */
    ramTotalGiB?: number;
    ramType?: string;
    /** Idle fit preview: expected layers of the selected tier. */
    preview?: VramLayer[] | null;
    /** Where the preview stacks from (baseline GiB). */
    previewBaseGiB?: number;
    /** Fault: GiB in use just before the fault (null = unknown or nothing was loaded). */
    faultGiB?: number | null;
  }
  let { vram, u, ramTotalGiB, ramType, preview = null, previewBaseGiB = 0, faultGiB = null }: Props = $props();

  let w = $state(0);
  let h = $state(0);
  const dw = $derived(w > 0 ? w / u : 300);
  const dh = $derived(h > 0 ? h / u : 500);

  const FILL: Record<VramLayerId, { fill: string; hatch: boolean }> = {
    weights: { fill: '#175F8F', hatch: true },
    kv: { fill: '#2275A8', hatch: true },
    buffers: { fill: '#4CAFE6', hatch: false },
    draft: { fill: '#2C88BF', hatch: true },
    projector: { fill: '#3B9BD2', hatch: true },
    other: { fill: '#2A6E98', hatch: true },
  };

  const colX = 26;
  const colW = 108;
  const colR = colX + colW;
  const topY = 40;
  /** Highest point a preview may reach above the edge (over-limit material). */
  const overTop = 8;

  interface Stratum {
    id: VramLayerId;
    label: string;
    gib: number;
    y0: number;
    y1: number;
    mid: number;
    fill: string;
    hatch: boolean;
    ghost: boolean;
  }

  const g = $derived.by(() => {
    const total = Math.max(0.01, vram.totalGiB);
    const ground = Math.max(topY + 120, dh - 112);
    const scale = (ground - topY) / total;
    const yAt = (gib: number) => ground - gib * scale;
    const strata: Stratum[] = [];
    // Measured layers: clipped at the edge (spill shows what went over).
    let cum = 0;
    for (const l of vram.layers) {
      const gib = Math.max(0, l.gib);
      const y1 = yAt(Math.min(total, cum));
      cum += gib;
      const y0 = yAt(Math.min(total, cum));
      strata.push({ id: l.id, label: l.label, gib, y0, y1, mid: (y0 + y1) / 2, ...FILL[l.id], ghost: false });
    }
    const used = vram.usedGiB;
    // Preview ghosts: stacked from the baseline, allowed to rise above the edge (drawn as over-limit).
    const base = Math.max(previewBaseGiB, 0);
    let pTop = base;
    let clipped = false;
    if (preview) {
      for (const l of preview) {
        const gib = Math.max(0, l.gib);
        const y1 = Math.max(overTop, yAt(pTop));
        pTop += gib;
        let y0 = yAt(pTop);
        if (y0 < overTop) {
          y0 = overTop;
          clipped = true;
        }
        strata.push({ id: l.id, label: l.label, gib, y0, y1, mid: (y0 + y1) / 2, ...FILL[l.id], ghost: true });
      }
    }
    // Labels: top layer first, at least 21 apart, first below the gap callout.
    const vis = strata.map((s, i) => i).filter((i) => strata[i].y1 - strata[i].y0 > 0.05);
    const order = vis.slice().sort((a, b) => strata[a].mid - strata[b].mid);
    const ly = new Array<number>(strata.length);
    let prev = topY + 22;
    for (const i of order) {
      const want = strata[i].mid + 4;
      ly[i] = Math.max(want, prev + 21);
      prev = ly[i];
    }
    let limit = ground - 8;
    for (let k = order.length - 1; k >= 0; k--) {
      const i = order[k];
      if (ly[i] > limit) ly[i] = limit;
      limit = ly[i] - 21;
    }
    const usedTop = yAt(Math.min(total, used));
    // The gap that is dimensioned: free above the measured fill, or spare above the preview.
    const gapTopGiB = preview ? pTop : used;
    const gapBottom = yAt(Math.min(total, gapTopGiB));
    return { total, used, ground, scale, strata, ly, usedTop, pTop, clipped, gapTopGiB, gapBottom, gapPx: gapBottom - topY, yAt };
  });

  const free = $derived(vram.totalGiB - g.gapTopGiB);
  const gapText = $derived(
    preview ? (free >= 0 ? `fits · ${fmGiB(free)} GiB spare` : `over by ${fmGiB(-free)} GiB`) : `${fmGiB(Math.max(0, free))} GiB free`,
  );
  /** Preview callout in two short lines (the column's side is narrow). */
  const gapHead = $derived(preview ? (free >= 0 ? 'FITS' : 'OVER LIMIT') : '');
  const gapVal = $derived(preview ? (free >= 0 ? `${fmGiB(free)} GiB spare` : `by ${fmGiB(-free)} GiB`) : gapText);
  const gapWarn = $derived(free <= 0 || (!preview && free < vram.warnBelowGiB));
  const gapAmber = $derived(!preview && free > 0 && free < vram.warnBelowGiB);

  /** Expected values: 0.12 -> "0.12", 0.9 -> "0.9", 11.6 -> "11.6". */
  function ghostNum(v: number): string {
    return v >= 1 ? v.toFixed(1) : v.toFixed(2).replace(/0$/, '');
  }

  function fmGiB(v: number): string {
    return fmtGiB(v);
  }

  const dimX = $derived(Math.max(colR + 124, dw - 22));

  /**
   * The gap callout when it sits inside the gap (beside the free dimension): one line if it clears the
   * total dimension line, otherwise the value and its word on two lines ("15.75 GiB" / "free").
   * 13 px JetBrains Mono advances 0.6 em per glyph.
   */
  const gapLines = $derived.by(() => {
    const room = dimX - 5 - (colR + 36);
    const fit = (t: string) => t.length * 7.8 <= room;
    const split = (t: string) => {
      if (fit(t)) return [t];
      const k = t.lastIndexOf(' ');
      return k > 0 ? [t.slice(0, k), t.slice(k + 1)] : [t];
    };
    return gapHead ? split(gapVal) : split(gapText);
  });
  const spillGiB = $derived(vram.spillMiB / 1024);
  const spillH = $derived(spillGiB > 0 ? Math.max(3, Math.min(46, spillGiB * g.scale)) : 0);

  // Fault: the level held just before the fault, and whether anything collapsed.
  const fault = $derived.by(() => {
    if (faultGiB === null || !(faultGiB > g.used + 0.05)) return null;
    const y = g.yAt(Math.min(g.total, faultGiB));
    return { y, label: g.usedTop - y > 44 };
  });

  // Collapsed rubble below the floor, right of the column (illustrates the fault; the ghost carries the number).
  const rubble = $derived.by(() => {
    if (!fault) return null;
    const x0 = colR + 6;
    const y0 = g.ground;
    const span = Math.max(80, Math.min(150, dimX - x0 - 14));
    const k = span / 150;
    const P = (pts: [number, number][]) => pts.map(([x, y]) => `${(x0 + x * k).toFixed(1)},${(y0 + y).toFixed(1)}`).join(' ');
    return {
      crack: P([
        [-6, 0],
        [4, 10],
        [14, 7],
        [22, 18],
        [36, 14],
        [46, 26],
        [62, 22],
      ]),
      shards: [
        { pts: P([[30, 30], [58, 24], [62, 36], [36, 42]]), fill: '#175F8F' },
        { pts: P([[66, 16], [92, 12], [98, 26], [72, 30]]), fill: '#2275A8' },
        { pts: P([[16, 38], [32, 40], [28, 50], [12, 48]]), fill: '#4CAFE6' },
        { pts: P([[100, 32], [126, 28], [134, 44], [108, 48]]), fill: '#175F8F' },
        { pts: P([[70, 36], [86, 34], [82, 48]]), fill: '#2C88BF' },
        { pts: P([[118, 10], [136, 8], [140, 18]]), fill: '#2275A8' },
      ],
    };
  });

  function arrow(x: number, y: number, dir: 'up' | 'down' | 'left' | 'right', s = 1): string {
    const a = 4.2 * s;
    const l = 9 * s;
    if (dir === 'up') return `M${x},${y} L${x - a},${y + l} L${x + a},${y + l} Z`;
    if (dir === 'down') return `M${x},${y} L${x - a},${y - l} L${x + a},${y - l} Z`;
    if (dir === 'left') return `M${x},${y} L${x + l},${y - a} L${x + l},${y + a} Z`;
    return `M${x},${y} L${x - l},${y - a} L${x - l},${y + a} Z`;
  }
</script>

<div class="col" bind:clientWidth={w} bind:clientHeight={h}>
  {#if w > 0}
    <svg
      viewBox="0 0 {dw} {dh}"
      width="100%"
      height="100%"
      aria-label={preview
        ? `VRAM fit preview: ${fmtGiB(g.pTop)} of ${fmtGiB(vram.totalGiB)} GiB expected, ${gapText}`
        : `VRAM section: ${fmtGiB(g.used)} of ${fmtGiB(vram.totalGiB)} GiB used`}
    >
      <defs>
        <pattern id="si-hatch" width="7" height="7" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="7" stroke="rgba(150,210,245,0.20)" stroke-width="1.3" />
        </pattern>
        <pattern id="si-hatch-ghost" width="7" height="7" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="7" stroke="rgba(110,190,240,0.30)" stroke-width="1.1" />
        </pattern>
        <pattern id="si-hatch-dim" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="8" stroke="#22303A" stroke-width="1.2" />
        </pattern>
        <pattern id="si-hatch-red" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="6" stroke="#FF5A36" stroke-width="1.4" />
        </pattern>
        <pattern id="si-hatch-redfaint" width="7" height="7" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="7" stroke="rgba(255,90,54,0.22)" stroke-width="1.1" />
        </pattern>
      </defs>

      <!-- fault: the level held just before the process died (drawn behind the remaining fill) -->
      {#if fault}
        <rect x={colX + 3} y={fault.y} width={colW - 6} height={Math.max(0, g.ground - fault.y)} fill="url(#si-hatch-redfaint)" class="hair fghost" />
        {#if fault.label}
          <text x={colX + colW / 2} y={fault.y + 17} class="gl red" text-anchor="middle">AT FAULT</text>
          <text x={colX + colW / 2} y={fault.y + 32} class="gl v red" text-anchor="middle">{fmtGiB(faultGiB ?? 0)} GiB</text>
        {/if}
      {/if}

      <!-- strata (measured solid; preview as hatched ghosts) -->
      {#each g.strata as s, i (s.id + i + (s.ghost ? 'g' : ''))}
        {#if s.y1 - s.y0 > 0.05}
          {#if s.ghost}
            <rect x={colX + 2} y={s.y0} width={colW - 4} height={s.y1 - s.y0} fill={s.fill} opacity="0.22" />
            <rect x={colX + 2} y={s.y0} width={colW - 4} height={s.y1 - s.y0} fill="url(#si-hatch-ghost)" />
            <line x1={colX + 2} x2={colR - 2} y1={s.y0} y2={s.y0} class="hair" stroke="#5AB6EB" stroke-dasharray="4 3" />
          {:else}
            <rect x={colX} y={s.y0} width={colW} height={s.y1 - s.y0} fill={s.fill} />
            {#if s.hatch}<rect x={colX} y={s.y0} width={colW} height={s.y1 - s.y0} fill="url(#si-hatch)" />{/if}
            <line x1={colX} x2={colR} y1={s.y0} y2={s.y0} class="hair" stroke="rgba(200,232,250,0.55)" />
          {/if}
        {/if}
      {/each}

      <!-- preview outline (dashed), and the over-limit part in red -->
      {#if preview && g.pTop > previewBaseGiB}
        {@const pTopY = Math.max(overTop, g.yAt(g.pTop))}
        {@const pBaseY = g.yAt(Math.min(g.total, previewBaseGiB))}
        <rect x={colX + 2} y={pTopY} width={colW - 4} height={Math.max(0, pBaseY - pTopY)} class="hair pv" />
        {#if g.pTop > g.total}
          <rect x={colX + 2} y={pTopY} width={colW - 4} height={Math.max(0, topY - pTopY)} fill="url(#si-hatch-red)" class="hair" stroke="#FF5A36" />
          {#if g.clipped}
            <path d="M{colX - 2},{overTop + 6} l12,-6 l8,10 l12,-8 l10,9 l12,-7 l12,8 l12,-8 l10,8 l12,-7 l12,6" class="hair" stroke="#FF5A36" fill="none" />
          {/if}
        {/if}
      {/if}

      <!-- column frame: sides up to the cliff edge, double line on the right -->
      <path d="M{colX},{g.ground} V{topY} M{colR},{g.ground} V{topY}" class="hair" stroke="#9CC6E0" fill="none" />
      <path d="M{colR - 4},{g.ground} V{g.usedTop}" class="hair" stroke="rgba(200,232,250,0.45)" fill="none" />
      <!-- the cliff edge: total capacity, the limit -->
      <line x1={colX - 6} x2={colR + 6} y1={topY} y2={topY} class="hair" stroke="#FF5A36" stroke-dasharray="5 3" />

      <!-- leaders -->
      {#each g.strata as s, i (s.id + 'l' + i + (s.ghost ? 'g' : ''))}
        {#if s.y1 - s.y0 > 0.05}
          {@const y = g.ly[i]}
          {#if s.ghost}
            <circle cx={colR - 8} cy={s.mid} r="2.6" fill="#0C1318" stroke="#9CD3F2" class="hair" />
          {:else}
            <circle cx={colR - 8} cy={s.mid} r="2.6" fill="#F4FAFF" />
          {/if}
          <path d="M{colR - 8},{s.mid} L{colR + 16},{y - 4} H{colR + 26}" class="hair" stroke="#C9DDEA" fill="none" />
          <text x={colR + 30} y={y} class="lbl" class:sm={s.label.length > 10} class:gh={s.ghost}>{s.label} {s.ghost ? ghostNum(s.gib) : s.gib.toFixed(1)}</text>
        {/if}
      {/each}

      <!-- the dimensioned gap: free (live) or spare (preview), at true scale -->
      {#if free > 0}
        <line x1={colR + 4} x2={colR + 28} y1={topY} y2={topY} class="hair" stroke="#C9DDEA" />
        <line x1={colR + 4} x2={colR + 28} y1={g.gapBottom} y2={g.gapBottom} class="hair" stroke="#C9DDEA" />
        {#if g.gapPx < 30}
          <line x1={colR + 24} x2={colR + 24} y1={topY - 22} y2={topY} class="hair" stroke="#C9DDEA" />
          <path d={arrow(colR + 24, topY, 'down', 0.8)} fill="#C9DDEA" />
          <line x1={colR + 24} x2={colR + 24} y1={g.gapBottom} y2={g.gapBottom + 22} class="hair" stroke="#C9DDEA" />
          <path d={arrow(colR + 24, g.gapBottom, 'up', 0.8)} fill="#C9DDEA" />
          {#if gapHead}
            <text x={colR + 36} y={topY - 24} class="gl">{gapHead}</text>
            <text x={colR + 36} y={topY - 9} class="val">{gapVal}</text>
          {:else}
            <text x={colR + 36} y={topY - 10} class="val" class:amber={gapAmber}>{gapText}</text>
          {/if}
        {:else}
          <line x1={colR + 24} x2={colR + 24} y1={topY} y2={g.gapBottom} class="hair" stroke="#C9DDEA" />
          <path d={arrow(colR + 24, topY, 'up', 0.8)} fill="#C9DDEA" />
          <path d={arrow(colR + 24, g.gapBottom, 'down', 0.8)} fill="#C9DDEA" />
          {@const mid = (topY + g.gapBottom) / 2}
          {@const y0 = mid + 4 - ((gapLines.length - 1) * 16) / 2 + (gapHead ? 8 : 0)}
          {#if gapHead}
            <text x={colR + 36} y={y0 - 16} class="gl">{gapHead}</text>
          {/if}
          {#each gapLines as ln, k (k)}
            <text x={colR + 36} y={y0 + k * 16} class="val" class:amber={gapAmber}>{ln}</text>
          {/each}
        {/if}
      {:else if preview}
        <text x={colR + 36} y={topY - 24} class="gl red">{gapHead}</text>
        <text x={colR + 36} y={topY - 9} class="val warn">{gapVal}</text>
      {:else}
        <text x={colR + 36} y={topY - 10} class="val" class:warn={gapWarn}>0.00 GiB free</text>
      {/if}

      <!-- total dimension -->
      <line x1={colR + 40} x2={dimX + 10} y1={topY} y2={topY} class="hair" stroke="#C9DDEA" />
      <line x1={dimX} x2={dimX} y1={topY} y2={g.ground} class="hair" stroke="#C9DDEA" />
      <path d={arrow(dimX, topY, 'up')} fill="#C9DDEA" />
      <path d={arrow(dimX, g.ground, 'down')} fill="#C9DDEA" />
      <text class="val dim" transform="translate({dimX + 9},{(topY + g.ground) / 2}) rotate(90)" text-anchor="middle">{fmtGiB(vram.totalGiB)} GiB</text>

      <!-- ground: the foot of the cliff -->
      <line x1="4" x2={dw - 4} y1={g.ground} y2={g.ground} class="hair" stroke="#9CC6E0" />

      <!-- spill into shared memory -->
      <rect x={colX} y={g.ground + 4} width="56" height="46" fill="url(#si-hatch-dim)" stroke="#4E6779" stroke-dasharray="4 3" class="hair" />
      {#if spillH > 0}
        <rect x={colX} y={g.ground + 4} width="56" height={spillH} fill="url(#si-hatch-red)" stroke="#FF5A36" class="hair" />
      {/if}
      <line x1={colX + 60} x2={colX + 66} y1={g.ground + 27} y2={g.ground + 27} class="hair" stroke="#C9DDEA" />
      {#if rubble}
        <text x={colX + 70} y={g.ground + 25} class="lbl sp" class:warn={vram.spillMiB > 0}>spill</text>
        <text x={colX + 70} y={g.ground + 39} class="lbl sp" class:warn={vram.spillMiB > 0}>{Math.round(vram.spillMiB)} MiB</text>
      {:else}
        <text x={colX + 70} y={g.ground + 31} class="lbl sp" class:warn={vram.spillMiB > 0}>spill to shared memory: {Math.round(vram.spillMiB)} MiB</text>
      {/if}

      <!-- system RAM band (schematic, not to scale) -->
      <line x1="4" x2={dw - 4} y1={g.ground + 56} y2={g.ground + 56} class="hair" stroke="#4E6779" />
      <rect x={colX} y={g.ground + 60} width={dw - colX - 14} height="44" fill="url(#si-hatch-dim)" stroke="#4E6779" stroke-dasharray="4 3" class="hair" />
      <text x={colX + 8} y={g.ground + 77} class="cap">SYSTEM RAM{ramTotalGiB ? ` ${ramTotalGiB.toFixed(1)} GiB` : ''}{ramType ? ` ${ramType}` : ''}</text>
      <text x={colX + 8} y={g.ground + 93} class="cap dimc">NOT TO SCALE</text>

      <!-- fault: the allocation collapsed through the floor -->
      {#if rubble}
        <polyline points={rubble.crack} class="hair crack" fill="none" />
        {#each rubble.shards as sh, i (i)}
          <polygon points={sh.pts} fill={sh.fill} />
          <polygon points={sh.pts} fill="url(#si-hatch)" stroke="#FF5A36" class="hair" />
        {/each}
      {/if}
    </svg>
  {/if}
</div>

<style>
  .col {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .hair {
    vector-effect: non-scaling-stroke;
    stroke-width: 1;
  }
  text {
    font-family: 'JetBrains Mono Variable', 'JetBrains Mono', monospace;
  }
  .lbl {
    font-size: 12.5px;
    font-weight: 500;
    fill: #dfe9f0;
  }
  .lbl.sm {
    font-size: 11.5px;
  }
  .lbl.gh {
    fill: #b9dcf1;
  }
  .lbl.sp {
    font-size: 11px;
  }
  .val {
    font-size: 13px;
    font-weight: 500;
    fill: #f4faff;
  }
  .dim {
    font-size: 12.5px;
  }
  .cap {
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.06em;
    fill: #8a9dab;
  }
  .warn {
    fill: #ff5a36;
  }
  .dimc {
    fill: #5f7280;
  }
  .amber {
    fill: #f2b33a;
  }
  .pv {
    fill: none;
    stroke: #5ab6eb;
    stroke-dasharray: 5 4;
  }
  .fghost {
    stroke: #ff5a36;
    stroke-dasharray: 5 4;
  }
  .crack {
    stroke: #ff5a36;
    stroke-width: 1.6;
  }
  .gl {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    fill: #86c3e6;
  }
  .gl.v {
    font-size: 12.5px;
    letter-spacing: 0;
    fill: #f4faff;
  }
  .gl.red {
    fill: #ff7a5c;
  }
  .gl.v.red {
    fill: #ffd6cc;
  }
</style>
