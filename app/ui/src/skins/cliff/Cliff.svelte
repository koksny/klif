<script lang="ts">
  // The signature element: VRAM as a sea-cliff cross-section (see geometry.ts for the axis rule).
  // Canvas 1 (static): rock strata, face, sea, headlands, loupe. Repainted only when the layer
  // composition, the mode, the warning state or the size changes.
  // Canvas 2 (only while spilling): the spill stream, animated via the shared scheduler at <= 24 fps.
  // Canvas 3 (only after a fault): debris breaking off the emptied face, played once, then still.
  // SVG overlay: every label, leader, bracket and the loupe frame.
  import { untrack } from 'svelte';
  import type { VramLayer } from '../../lib/model/types';
  import { fmtGiB, fmtInt } from '../../lib/model/format';
  import { getTier, onFrame, onTier } from '../../lib/render/scheduler';
  import { bezierPathD, buildGeom, yOfGiB } from './geometry';
  import { gpuDetail, gpuStatus, type GpuView } from './power';
  import {
    DEBRIS_S,
    debrisRocks,
    getRockTexture,
    paintDebris,
    paintScene,
    paintSpill,
    spillBox,
    tallHeadroom,
    type SceneMode,
  } from './paint';
  import { fmtLayer, prefersReducedMotion, sumGiB } from './util';

  interface Props {
    variant: 'full' | 'mini';
    totalGiB: number;
    usedGiB: number;
    /** live/building/fault: vram.layers. fit: the slot's expected layers (without the baseline). */
    layers: VramLayer[];
    spillMiB: number;
    warnBelowGiB: number;
    device: string;
    mode?: SceneMode;
    /** fit: measured VRAM the expected layers stack on (vm.vram.baselineGiB). */
    baseGiB?: number;
    /** fault: seconds since the failure; the debris fall plays only if the fault is this fresh. */
    faultSinceS?: number;
    /** fault: VRAM the failure released (GiB); the amount of debris follows it, none if nothing was held. */
    releasedGiB?: number;
    kicker?: string;
    /** GPU dormant (vm.vram.dormant): `layers` are the session's ALLOCATIONS, drawn as ghosts above the resident part. */
    gpu?: GpuView | null;
  }

  let {
    variant,
    totalGiB,
    usedGiB,
    layers,
    spillMiB,
    warnBelowGiB,
    device,
    mode = 'live',
    baseGiB = 0,
    faultSinceS,
    releasedGiB = 0,
    kicker = 'VRAM cliff',
    gpu = null,
  }: Props = $props();

  let w = $state(0);
  let h = $state(0);
  let titleH = $state(0);
  let rock: HTMLCanvasElement | undefined = $state();
  let spillCanvas: HTMLCanvasElement | undefined = $state();
  let debrisCanvas: HTMLCanvasElement | undefined = $state();
  const dpr = Math.min(2, typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1);

  const full = $derived(variant === 'full');
  const fit = $derived(mode === 'fit');
  const fault = $derived(mode === 'fault');
  /** The GPU is asleep (or waking): the strata are ghosts and only the resident bottom is solid rock. */
  const dorm = $derived(!!gpu && mode === 'live');
  /** fit: the measured baseline sits under the expected strata as one solid band. */
  const nBase = $derived(fit && baseGiB > 0.004 ? 1 : 0);
  const stackLayers = $derived<VramLayer[]>(
    nBase ? [{ id: 'other', label: 'other', gib: baseGiB }, ...layers] : layers,
  );
  const sidePad = $derived(Math.max(16, w * 0.0235));
  const lipTop = $derived(full ? Math.round(titleH + Math.max(8, h * 0.02)) : Math.round(h * 0.17));
  const stacked = $derived(sumGiB(stackLayers));
  /** Headroom: measured free VRAM, or the spare a fit preview predicts (dormant: what the allocations leave). */
  const free = $derived(fit ? totalGiB - stacked : dorm ? Math.max(0, totalGiB - stacked) : Math.max(0, totalGiB - usedGiB));
  const warn = $derived(fit ? free < 0 : free < warnBelowGiB);
  const spilling = $derived(!fit && spillMiB > 0);

  const g = $derived(
    w > 0 && h > 0 ? buildGeom(w, h, totalGiB, stackLayers, variant, lipTop, sidePad, mode === 'fault') : null,
  );
  const tall = $derived(!!g && (tallHeadroom(g) || fault));

  /** Dormant: GiB resident (the solid bottom of the rock) and how much of the session the sea holds. */
  const solidGiB = $derived(dorm && gpu ? Math.min(stacked, gpu.residentGiB) : 0);
  const ramK = $derived(dorm && gpu && stacked > 0 ? Math.min(1, Math.max(0, gpu.pagedOutGiB / stacked)) : 0);
  /** The restore front: the top of the resident rock. */
  const frontY = $derived(g ? g.baseY - solidGiB * g.ppg : 0);

  const paintKey = $derived(
    `${w}x${h}@${dpr}|${variant}|${mode}|${nBase}|${warn}|${lipTop}|${totalGiB.toFixed(2)}|` +
      (dorm ? `dorm:${Math.round(solidGiB * (g?.ppg ?? 0))}:${ramK.toFixed(2)}|` : '') +
      stackLayers.map((l) => `${l.id}:${l.gib.toFixed(2)}`).join(','),
  );

  $effect(() => {
    void paintKey;
    const c = rock;
    if (!c) return;
    untrack(() => {
      if (!g) return;
      const W = Math.round(g.W * dpr);
      const H = Math.round(g.H * dpr);
      if (c.width !== W) c.width = W;
      if (c.height !== H) c.height = H;
      const ctx = c.getContext('2d');
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      paintScene(
        ctx,
        g,
        getRockTexture(g.W, g.H, dpr),
        { mode, warn, nBase, overGiB: fit ? Math.max(0, stacked - totalGiB) : 0, dormant: dorm ? { solidGiB, ramK } : null },
        dpr,
      );
    });
  });

  // The spill canvas covers only the stream's box (it is the one canvas that animates continuously).
  const sbox = $derived(spilling && g ? spillBox(g, spillMiB) : null);

  $effect(() => {
    const c = spillCanvas;
    const geo = g;
    const box = sbox;
    const mib = spillMiB;
    if (!spilling || !c || !geo || !box) return;
    c.width = Math.round(box.w * dpr);
    c.height = Math.round(box.h * dpr);
    const ctx = c.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, -box.x * dpr, -box.y * dpr);
    if (prefersReducedMotion()) {
      paintSpill(ctx, geo, mib, 0);
      return;
    }
    return onFrame((t) => paintSpill(ctx, geo, mib, t), { maxFps: 24 });
  });

  // Debris: geometry only changes with the size, so the rocks are keyed on it, not on every snapshot.
  /** Pieces of rock: about three per GiB released (scenery scale, capped), none if nothing was held. */
  const debrisN = $derived(Math.min(full ? 34 : 20, Math.round(Math.max(0, releasedGiB) * (full ? 3 : 2))));
  const debrisKey = $derived(fault && g && debrisN > 0 ? `${g.W}x${g.H}|${g.lipY}|${g.baseY}|${variant}|${debrisN}` : '');
  $effect(() => {
    void debrisKey;
    const c = debrisCanvas;
    if (!debrisKey || !c) return;
    return untrack(() => {
      const geo = g;
      if (!geo) return;
      c.width = Math.round(geo.W * dpr);
      c.height = Math.round(geo.H * dpr);
      const ctx = c.getContext('2d');
      if (!ctx) return;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const rocks = debrisRocks(geo, debrisN);
      const since = faultSinceS ?? DEBRIS_S;
      const k0 = Math.min(1, Math.max(0, since / DEBRIS_S));
      paintDebris(ctx, geo, rocks, k0);
      // No frames to play it in (reduced motion, or the window is calm/hidden): show where it ends.
      const still = () => prefersReducedMotion() || getTier() === 'calm' || getTier() === 'off';
      if (k0 >= 1 || still()) {
        if (k0 < 1) paintDebris(ctx, geo, rocks, 1);
        return;
      }
      // Fresh fault: play the fall once from where it is now, then leave the last frame standing.
      const t0 = performance.now() - since * 1000;
      let off: (() => void) | null = null;
      let offTier: (() => void) | null = null;
      const finish = () => {
        off?.();
        offTier?.();
        off = offTier = null;
      };
      off = onFrame(
        (now) => {
          const k = Math.min(1, (now - t0) / 1000 / DEBRIS_S);
          paintDebris(ctx, geo, rocks, k);
          if (k >= 1) finish();
        },
        { maxFps: 30 },
      );
      // Frames stop if the window goes calm mid-fall: never leave rocks hanging in the air.
      offTier = onTier(() => {
        if (!still()) return;
        paintDebris(ctx, geo, rocks, 1);
        finish();
      });
      return finish;
    });
  });

  // ---- overlay layout (full)
  const fsL = $derived(Math.max(11.5, Math.min(14, w * 0.0109)));
  const fsS = $derived(Math.max(10.5, Math.min(12.5, w * 0.0097)));
  const bx = $derived(Math.max(10, w * 0.016));
  const lx = $derived(bx + Math.max(14, w * 0.02));

  const strata = $derived.by(() => {
    if (!g || !full) return [];
    const gap = fsL * 1.5;
    const items = [...g.bands].reverse().map((b) => ({ b, yc: (b.yTop + b.yBot) / 2, y: 0 }));
    let prev = -Infinity;
    // Labels stay below the lip, and clear of the surface of the rock once it sits visibly below it.
    const minY = g.rockY - g.lipY > fsL * 0.6 ? g.rockY + fsL * 0.8 : g.lipY + fsL * 0.95;
    for (const it of items) {
      it.y = Math.max(it.yc, prev + gap, minY);
      prev = it.y;
    }
    let next = g.baseY - fsL * 0.85;
    for (let i = items.length - 1; i >= 0; i--) {
      if (items[i].y > next) items[i].y = next;
      next = items[i].y - gap;
    }
    return items;
  });

  function leader(yc: number, y: number): string {
    if (Math.abs(y - yc) < 0.75) return `M${bx} ${yc.toFixed(1)} H${(lx - 6).toFixed(1)}`;
    return `M${bx} ${yc.toFixed(1)} H${(bx + 6).toFixed(1)} L${(lx - 10).toFixed(1)} ${y.toFixed(1)} H${(lx - 6).toFixed(1)}`;
  }

  const freeText = $derived(fmtGiB(Math.abs(free)));
  const freeUnit = $derived(fit || dorm ? (free >= 0 ? 'GiB spare' : 'GiB over') : 'GiB free');
  const freePrefix = $derived(fit ? (free >= 0 ? 'fits · ' : 'does not fit · ') : '');

  /** Dormant, full: the lip label says where the rock is right now (system RAM), one row above the headroom label. */
  const dlab = $derived.by(() => {
    if (!g || !full || !dorm || !gpu) return null;
    const prefix = gpu.phase === 'waking' ? 'restoring · ' : gpu.phase === 'sleeping' ? 'paging out · ' : 'paged out · ';
    const value = fmtGiB(gpu.pagedOutGiB);
    let unit = gpu.phase === 'waking' ? 'GiB still in RAM' : 'GiB in RAM';
    const room = w - sidePad - (g.lipX + 14);
    const est = (u: string) => (prefix.length + value.length + u.length + 1) * fsL * 0.52;
    // A narrow window shortens the label before it gives up the right-hand side of the lip.
    if (est(unit) > room && est('GiB') <= room) unit = 'GiB';
    const ly = Math.max(fsL * 0.8, g.lipY - fsL * (tall ? 2.6 : 2.35));
    const right = est(unit) <= room;
    return {
      prefix,
      value,
      unit,
      ly,
      right,
      tx: right ? g.lipX + 12 : g.lipX - 14,
      px: tall ? g.lipX : g.lipX - 4,
      py: tall ? g.lipY : (g.lipY + g.rockY) / 2,
    };
  });

  /** Dormant: the resident rock grows bottom-up while the GPU wakes; chevrons mark the restore front. */
  const front = $derived.by(() => {
    if (!g || !dorm || !gpu || gpu.phase === 'asleep') return null;
    const restoring = gpu.phase === 'waking';
    // Mini: out at the cliff edge, clear of the words on the rock.
    const x = full ? Math.max(lx + 190, g.lipX * 0.42) : g.lipX - 28;
    const s = full ? 7 : 11;
    const y = frontY - (full ? 10 : 14);
    // Only while the front is a visible way below the surface and above the foot.
    if (frontY < g.rockY + s * 2.2 || frontY > g.baseY - 4) return { chev: null, label: null as string | null };
    const pct = Math.round(gpu.frac * 100);
    const label = full ? `${fmtGiB(gpu.residentGiB)} GiB resident · ${pct}%` : null;
    // The chevrons stay clear of the label that ends at the lip.
    const cx = label ? Math.max(lx + 70, Math.min(x, g.lipX - 16 - label.length * fsS * 0.6 - 26)) : x;
    return { chev: restoring ? { x: cx, y, s } : null, label };
  });

  /** Thin headroom: a pin in the free band with a leader up to the label above the lip. */
  const callout = $derived.by(() => {
    if (!g || !full || tall || dorm) return null;
    const px = g.lipX - 4;
    const py = (g.lipY + g.rockY) / 2;
    const ly = g.lipY - fsL * 1.05;
    const est = (freePrefix.length + freeText.length + freeUnit.length + 1) * fsL * 0.56;
    // The loupe only blocks the right side where it is drawn (not in the fit preview or after a fault).
    const limit = g.loupe && !fit && !fault ? g.loupe.x - 14 : w - sidePad;
    const right = g.lipX + 14 + est < limit;
    return { px, py, ly, right, tx: right ? g.lipX + 12 : g.lipX - 14 };
  });

  /** Tall headroom: the lip is a capacity mark, the free amount is a dimension arrow under it. */
  const dim = $derived.by(() => {
    if (!g || !full || !tall) return null;
    const ax = g.lipX - Math.max(12, w * 0.014);
    const y0 = g.lipY + 4;
    const y1 = g.rockY - 4;
    return {
      ax,
      y0,
      y1,
      my: (g.lipY + g.rockY) / 2,
      ly: g.lipY - fsL * 1.05,
      cap: `${totalGiB.toFixed(2)} GiB${fault ? ' capacity' : ''}`,
    };
  });

  /** building: chevrons over the surface of the growing rock. */
  const grow = $derived.by(() => {
    if (!g || mode !== 'building' || !tall) return null;
    const x = full ? Math.max(lx + 190, g.lipX * 0.42) : g.lipX * 0.5;
    const s = full ? 7 : 11;
    return { x, y: g.rockY - (full ? 10 : 14), s };
  });

  const spillLabel = $derived.by(() => {
    if (!g) return null;
    const [, , , p3] = g.spill;
    const value = `${fmtInt(spillMiB)} MiB`;
    const x = p3[0] + 13;
    const est = (24 + value.length) * fsS * 0.5;
    return { x, y: p3[1], value, twoLine: x + est > w - 6, d: bezierPathD(g.spill), p3 };
  });

  const loupeInfo = $derived.by(() => {
    if (!g || !g.loupe || fit || fault) return null;
    const L = g.loupe;
    const yL = (gib: number) => L.y + (L.hi - gib) * L.k;
    const ticks: number[] = [];
    for (let v = Math.ceil(L.lo * 4) / 4; v <= L.hi + 1e-6; v += 0.25) ticks.push(yL(v));
    const fs = Math.max(10.5, Math.min(12, w * 0.0094));
    const slices: { y: number; text: string; value: string; free?: boolean }[] = [];
    const top = Math.max(g.stackTop, L.lo);
    if (L.hi - top > 0 && yL(top) - yL(L.hi) >= fs * 1.25)
      slices.push({ y: (yL(top) + yL(L.hi)) / 2, text: 'free', value: fmtGiB(L.hi - g.stackTop), free: true });
    for (const b of g.bands) {
      const hi = Math.min(b.hi, L.hi);
      const lo = Math.max(b.lo, L.lo);
      if (hi <= lo) continue;
      const y0 = yL(hi);
      const y1 = yL(lo);
      if (y1 - y0 < fs * 1.25) continue;
      slices.push({ y: (y0 + y1) / 2, text: b.label, value: fmtLayer(b.gib) });
    }
    const win = L.hi - L.lo;
    return {
      L,
      ticks,
      fs,
      slices,
      head: `last ${win >= 0.995 ? '1' : win.toFixed(2)} GiB · ×${L.scale}`,
      top: L.hi.toFixed(2),
      bottom: L.lo.toFixed(2),
      winX0: g.lipX - Math.max(36, w * 0.06),
      winY1: yOfGiB(g, L.lo),
    };
  });

  const seaLabel = $derived.by(() => {
    if (!g) return null;
    return { x: w - Math.max(sidePad, w * 0.06), y: g.waterY + (g.baseY - g.waterY) * 0.52 };
  });

  const aria = $derived(
    `${fit ? 'Expected VRAM' : 'VRAM'} on ${device}: ${(fit ? stacked : usedGiB).toFixed(2)} of ${totalGiB.toFixed(2)} GiB${dorm ? ' resident' : ''}. ` +
      stackLayers.map((l) => `${l.label} ${fmtLayer(l.gib)}`).join(', ') +
      `. ${freePrefix}${freeText} ${freeUnit}. Spill to shared memory ${fmtInt(spillMiB)} MiB.` +
      (dorm && gpu ? ` ${gpuStatus(gpu)}: ${gpuDetail(gpu)}; ${fmtGiB(gpu.pagedOutGiB)} GiB of the session is in system RAM.` : ''),
  );
</script>

<div class="scene {variant}" class:warn bind:clientWidth={w} bind:clientHeight={h} role="img" aria-label={aria}>
  <canvas class="layer" bind:this={rock}></canvas>
  {#if fault && debrisN > 0}<canvas class="layer" bind:this={debrisCanvas}></canvas>{/if}
  {#if spilling && sbox}<canvas
      class="spillc"
      bind:this={spillCanvas}
      style:left="{sbox.x}px"
      style:top="{sbox.y}px"
      style:width="{sbox.w}px"
      style:height="{sbox.h}px"
    ></canvas>{/if}

  {#if g}
    <svg class="ov" width={w} height={h} viewBox="0 0 {w} {h}" aria-hidden="true">
      {#if grow}
        <path
          class="grow"
          d="M{grow.x - grow.s} {grow.y} l{grow.s} {-grow.s} l{grow.s} {grow.s} M{grow.x - grow.s} {grow.y + grow.s * 0.95} l{grow.s} {-grow.s} l{grow.s} {grow.s}"
        />
      {/if}
      {#if front?.chev}
        {@const c = front.chev}
        <path
          class="grow rise"
          d="M{c.x - c.s} {c.y} l{c.s} {-c.s} l{c.s} {c.s} M{c.x - c.s} {c.y + c.s * 0.95} l{c.s} {-c.s} l{c.s} {c.s}"
        />
      {/if}
      {#if full}
        <!-- strata column + labels -->
        {#each strata as s (s.b.id + s.b.lo)}
          {#if s.b.yBot - s.b.yTop >= 5}
            <path
              class="col"
              d="M{bx - 3} {(s.b.yTop + 1.5).toFixed(1)} H{bx} V{(s.b.yBot - 1.5).toFixed(1)} H{bx - 3}"
            />
          {/if}
          <path class="lead" d={leader(s.yc, s.y)} />
          <text class="sl" x={lx} y={s.y} dy="0.36em" style:font-size="{fsL}px"
            ><tspan class="n">{s.b.label}</tspan><tspan class="v" dx="0.5em">{fmtLayer(s.b.gib)}</tspan></text
          >
        {/each}

        <!-- headroom callout at the lip -->
        {#if callout}
          <path
            class="callout"
            d="M{callout.px} {callout.py.toFixed(1)} V{callout.ly.toFixed(1)} H{callout.right
              ? g.lipX + 8
              : g.lipX - 10}"
          />
          <circle class="pin" cx={callout.px} cy={callout.py} r="3.4" />
          <text
            class="free"
            x={callout.tx}
            y={callout.ly}
            dy="0.36em"
            text-anchor={callout.right ? 'start' : 'end'}
            style:font-size="{fsL}px"
            ><tspan class="fp">{freePrefix}</tspan><tspan class="fv">{freeText}</tspan><tspan
              class="fu"
              dx="0.3em">{freeUnit}</tspan
            ></text
          >
        {/if}

        <!-- dormant: where the rock is right now (system RAM), at the lip -->
        {#if dlab}
          {#if !tall}
            <path
              class="callout"
              d="M{dlab.px} {dlab.py.toFixed(1)} V{dlab.ly.toFixed(1)} H{dlab.right ? g.lipX + 8 : g.lipX - 10}"
            />
            <circle class="pin" cx={dlab.px} cy={dlab.py} r="3.4" />
          {/if}
          <text
            class="free dlab"
            x={dlab.tx}
            y={dlab.ly}
            dy="0.36em"
            text-anchor={dlab.right ? 'start' : 'end'}
            style:font-size="{fsL}px"
            ><tspan class="fp">{dlab.prefix}</tspan><tspan class="fv">{dlab.value}</tspan><tspan class="fu" dx="0.3em"
              >{dlab.unit}</tspan
            ></text
          >
        {/if}

        <!-- tall headroom: capacity pin at the lip + dimension arrow down to the rock -->
        {#if dim}
          <path
            class="callout cap"
            d="M{g.lipX} {g.lipY - 4} V{(dlab ? dlab.ly : dim.ly).toFixed(1)} H{g.lipX + 8}"
          />
          <circle class="pin cap" cx={g.lipX} cy={g.lipY} r="4" />
          <text class="capt" x={g.lipX + 12} y={dim.ly} dy="0.36em" style:font-size="{fsL}px">{dim.cap}</text>
          <path class="dim" d="M{dim.ax} {dim.y0} V{dim.y1}" />
          <path class="dimh" d="M{dim.ax - 4.5} {dim.y0 + 6} L{dim.ax} {dim.y0} L{dim.ax + 4.5} {dim.y0 + 6}" />
          <path class="dimh" d="M{dim.ax - 4.5} {dim.y1 - 6} L{dim.ax} {dim.y1} L{dim.ax + 4.5} {dim.y1 - 6}" />
          {#if dim.y1 - dim.y0 > fsL * 1.6}
            <text class="free" x={dim.ax - 12} y={dim.my} dy="0.36em" text-anchor="end" style:font-size="{fsL}px"
              ><tspan class="fp">{freePrefix}</tspan><tspan class="fv">{freeText}</tspan><tspan class="fu" dx="0.3em"
                >{freeUnit}</tspan
              ></text
            >
          {/if}
        {/if}

        <!-- dormant: the restore front -->
        {#if front?.label}
          <text class="flab" x={g.lipX - 16} y={frontY - fsL * 0.7} text-anchor="end" style:font-size="{fsS}px">{front.label}</text>
        {/if}

        <!-- loupe: the last GiB under the lip, magnified at a stated scale -->
        {#if loupeInfo}
          {@const L = loupeInfo.L}
          <rect class="win" x={loupeInfo.winX0} y={g.lipY} width={g.lipX + 2 - loupeInfo.winX0} height={loupeInfo.winY1 - g.lipY} />
          <path class="conn" d="M{g.lipX + 2} {g.lipY} H{L.x}" />
          <path class="conn" d="M{g.lipX + 2} {loupeInfo.winY1.toFixed(1)} L{L.x} {(L.y + L.h).toFixed(1)}" />
          <rect class="lframe" x={L.x + 0.5} y={L.y + 0.5} width={L.w - 1} height={L.h - 1} rx="5" />
          <text class="lhead" x={L.x + L.w} y={L.y - 8} text-anchor="end" style:font-size="{loupeInfo.fs}px"
            >{loupeInfo.head}</text
          >
          {#each loupeInfo.ticks as ty, i (i)}
            <line class="tick" x1={L.x + L.w - 7} x2={L.x + L.w} y1={ty} y2={ty} />
          {/each}
          <text class="ltick" x={L.x + L.w - 10} y={L.y + 4} dy="0.9em" text-anchor="end" style:font-size="{loupeInfo.fs - 1}px"
            >{loupeInfo.top}</text
          >
          <text class="ltick" x={L.x + L.w - 10} y={L.y + L.h - 5} text-anchor="end" style:font-size="{loupeInfo.fs - 1}px"
            >{loupeInfo.bottom}</text
          >
          {#each loupeInfo.slices as sl, i (i)}
            <text class="lsl" class:lfree={sl.free} x={L.x + 9} y={sl.y} dy="0.36em" style:font-size="{loupeInfo.fs}px"
              >{#if sl.free}<tspan class="v">{sl.value}</tspan><tspan dx="0.35em">GiB {dorm ? 'spare' : 'free'}</tspan>{:else}<tspan
                  >{sl.text}</tspan
                ><tspan class="v" dx="0.45em">{sl.value}</tspan>{/if}</text
            >
          {/each}
        {/if}

        <!-- the sea is system RAM; spill goes over the edge into it -->
        {#if seaLabel}
          <text class="sea" class:lit={dorm} x={seaLabel.x} y={seaLabel.y} text-anchor="end" style:font-size="{fsL * 1.05}px">System RAM</text>
        {/if}
        {#if spillLabel}
          {#if !spilling}<path class="chan" d={spillLabel.d} />{/if}
          <circle class="pin" class:hot={spilling} cx={spillLabel.p3[0]} cy={spillLabel.p3[1]} r="4.2" />
          {#if spillLabel.twoLine}
            <text class="spill" class:hot={spilling} x={spillLabel.x} y={spillLabel.y - fsS * 0.62} dy="0.36em" style:font-size="{fsS}px"
              >spill to shared memory:</text
            >
            <text class="spill" class:hot={spilling} x={spillLabel.x} y={spillLabel.y + fsS * 0.72} dy="0.36em" style:font-size="{fsS}px"
              ><tspan class="v">{spillLabel.value}</tspan></text
            >
          {:else}
            <text class="spill" class:hot={spilling} x={spillLabel.x} y={spillLabel.y} dy="0.36em" style:font-size="{fsS}px"
              >spill to shared memory: <tspan class="v">{spillLabel.value}</tspan></text
            >
          {/if}
        {/if}
      {:else}
        <!-- mini: the lip marker only; the numbers are in the panel's VRAM strip, not scattered on the drawing -->
        {#if !fault}
          <path class="mark" d="M{g.lipX - 9} {g.lipY - 22} H{g.lipX + 9} L{g.lipX} {g.lipY - 9} Z" />
        {/if}
      {/if}
    </svg>
  {/if}

  {#if full}
    <div class="title" bind:clientHeight={titleH} style:padding-left="{sidePad}px">
      <div class="kicker">{kicker}</div>
      <div class="ttl">
        <span>{device}</span><span class="sep">·</span><span class="num">{(fit ? stacked : usedGiB).toFixed(2)} / {totalGiB.toFixed(2)} GiB</span
        >{#if fit}<span class="exp">expected</span>{/if}{#if dorm}<span class="exp">resident</span>{/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .scene {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  .layer,
  .ov {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .ov {
    overflow: visible;
  }
  .spillc {
    position: absolute;
  }
  .title {
    position: absolute;
    left: 0;
    top: 0;
    padding-top: max(6px, calc(var(--u) * 8));
  }
  /* map title: a small caption over the sheet name */
  .kicker {
    font: 600 var(--fs-xs) / 1.2 var(--f-ui);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--label);
  }
  .ttl {
    margin-top: max(2px, calc(var(--u) * 3));
    font: 400 var(--fs-l) / 1.2 var(--f-ui);
    letter-spacing: 0.01em;
    color: var(--mist);
    white-space: nowrap;
  }
  .ttl .num {
    font-weight: 500;
    color: var(--foam);
  }
  .ttl .sep {
    margin: 0 0.45em;
    color: var(--dim);
  }
  .ttl .exp {
    margin-left: 0.7em;
    font-weight: 600;
    font-size: var(--fs-xs);
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--muted);
  }

  /* svg text */
  text {
    font-family: var(--f-ui);
    font-variant-numeric: tabular-nums;
    fill: var(--foam);
    paint-order: stroke;
    stroke: rgba(12, 16, 19, 0.6);
    stroke-width: 3px;
    stroke-linejoin: round;
  }
  .sl .n {
    font-weight: 500;
  }
  .sl .v,
  .free .fv,
  .lsl .v,
  .spill .v,
  .ltick,
  .capt {
    font-weight: 600;
  }
  .col {
    fill: none;
    stroke: rgba(220, 239, 248, 0.45);
    stroke-width: 1;
  }
  .lead {
    fill: none;
    stroke: rgba(220, 239, 248, 0.5);
    stroke-width: 1;
  }
  .callout {
    fill: none;
    stroke: var(--foam);
    stroke-opacity: 0.8;
    stroke-width: 1.2;
  }
  .callout.cap {
    stroke: var(--sky);
  }
  .pin {
    fill: var(--basalt);
    stroke: var(--foam);
    stroke-width: 1.4;
  }
  .pin.cap {
    stroke: var(--sky);
    stroke-width: 1.8;
  }
  .pin.hot {
    stroke: var(--amber);
  }
  .capt {
    fill: var(--sky);
    stroke: rgba(12, 16, 19, 0.85);
  }
  .dim,
  .dimh {
    fill: none;
    stroke: var(--sky);
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .grow {
    fill: none;
    stroke: var(--sky);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.85;
  }
  .mini .grow {
    stroke-width: 3;
  }
  .free {
    font-weight: 500;
    stroke: rgba(12, 16, 19, 0.85);
  }
  .free .fu,
  .free .fp {
    fill: var(--sky);
  }
  .warn .free .fu,
  .warn .free .fp,
  .warn .free .fv {
    fill: var(--amber);
  }
  .win {
    fill: none;
    stroke: var(--sky);
    stroke-opacity: 0.7;
    stroke-dasharray: 3 3;
    stroke-width: 1;
  }
  .conn {
    fill: none;
    stroke: var(--sky);
    stroke-opacity: 0.45;
    stroke-width: 1;
  }
  .lframe {
    fill: none;
    stroke: rgba(220, 239, 248, 0.42);
    stroke-width: 1;
  }
  .lhead {
    fill: var(--muted);
    letter-spacing: 0.02em;
    stroke: rgba(12, 16, 19, 0.9);
  }
  .tick {
    stroke: rgba(220, 239, 248, 0.55);
    stroke-width: 1;
  }
  .ltick {
    fill: var(--muted);
    stroke: rgba(12, 16, 19, 0.85);
  }
  .lsl {
    font-weight: 500;
  }
  .lsl.lfree {
    fill: var(--sky);
  }
  .warn .lsl.lfree {
    fill: var(--amber);
  }
  .lsl.lfree .v {
    fill: var(--foam);
  }
  /* hydrography: water is labelled in italic */
  .sea {
    font-style: italic;
    fill: #9db4c0;
    fill-opacity: 0.85;
    stroke: none;
    letter-spacing: 0.03em;
  }
  /* dormant: the sea holds the model */
  .sea.lit {
    fill: #bfe4f6;
    fill-opacity: 1;
  }
  .flab {
    fill: var(--sky);
    font-weight: 600;
    letter-spacing: 0.01em;
    stroke: rgba(12, 16, 19, 0.85);
  }
  /* the restore front's chevrons rise, slowly (still under reduced motion: cliff.css switches animation off) */
  .rise {
    animation: rise 1.6s ease-in-out infinite;
  }
  @keyframes rise {
    0%,
    100% {
      opacity: 0.35;
      transform: translateY(2px);
    }
    50% {
      opacity: 1;
      transform: translateY(-4px);
    }
  }
  .chan {
    fill: none;
    stroke: rgba(220, 239, 248, 0.7);
    stroke-width: 1.2;
    stroke-dasharray: 3.5 3.5;
  }
  .spill {
    fill: var(--foam);
    stroke: rgba(12, 16, 19, 0.85);
  }
  .spill.hot {
    fill: var(--amber);
  }

  /* mini */
  .mark {
    fill: var(--foam);
  }
</style>
