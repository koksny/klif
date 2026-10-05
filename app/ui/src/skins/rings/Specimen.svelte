<script lang="ts">
  // The specimen under glass: KLIF fx Rings (RingsVis, src/lib/fx/rings) painted full-bleed behind the whole skin,
  // with the sphere placed where the layout wants it (cx, cy, r), and the ring gauges drawn around it in SVG.
  // One subscription to the shared frame scheduler drives both: Drive smooths the view model for the shader, and
  // the gauges ease toward their targets on the same clock (never a jump). Without frames (calm / off tier, or
  // reduced motion) one still frame is drawn per snapshot and the gauges sit at their targets. Unmounting releases
  // the GL context (skins switch at runtime).
  import { onMount, untrack } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import { getTier, onFrame, onTier } from '../../lib/render/scheduler';
  import type { System, SystemKind, ViewModel } from '../../lib/model/types';
  import { Drive } from '../../lib/fx/drive';
  import { RingsVis, SPHERE_R } from '../../lib/fx/rings/rings';
  import NoGl from '../../lib/fx/NoGl.svelte';
  import { glDetail } from '../../lib/fx/nogl';
  import type { Sleep } from './sleep.svelte';
  import { arc, CAPTION_Y, CYAN, DANGER, mixRgb, rgba, styleOf, targetsOf, ticks, WARN } from './gauges';

  interface Props {
    vm: ViewModel;
    kind: SystemKind;
    sel: System | undefined;
    dz: Sleep | null;
    waking: boolean;
    /** Sphere centre and visible radius in this box's CSS px (the box fills the skin). */
    cx: number;
    cy: number;
    r: number;
    /** full: thin rings with ticks and a legend with values; mini: thicker rings, names only. */
    variant: 'full' | 'mini';
    /** The layout unit: gauge strokes and type scale with it. */
    k: number;
    /** Canvas backing store per CSS px, on top of a device pixel ratio capped at 1. */
    res?: number;
  }
  let { vm, kind, sel, dz, waking, cx, cy, r, variant, k, res = 1 }: Props = $props();

  const G = $derived(styleOf(variant, k));
  const R = $derived(G.radii.map((f) => f * r));

  let canvas: HTMLCanvasElement;
  let w = $state(0);
  let h = $state(0);
  /** Set when the sphere could not be built (no WebGL 2): the browser's sentence worth showing, or ''. */
  let failed = $state<string | null>(null);
  let vis: RingsVis | null = null;
  const drive = new Drive();
  let off: (() => void) | null = null;
  let last = 0;
  /** Nothing is drawn until the layout has placed the sphere. */
  let placed = false;

  const targets = $derived(targetsOf(vm, { kind, sel, dz, waking }));

  // eased gauge state (frame clock)
  let gv = $state(0);
  let gA = $state(0);
  let gB = $state(0);
  let gm = $state(0);
  let gi = $state(0);
  let warnK = $state(0);
  let dangerK = $state(0);
  let overK = $state(0);
  let sleepK = $state(0);
  let faultK = $state(0);

  const approach = (v: number, to: number, rate: number, dt: number) => {
    const n = v + (to - v) * (1 - Math.exp(-rate * dt));
    return Math.abs(to - n) < 1e-4 ? to : n;
  };

  function ease(dt: number) {
    const t = targets;
    gv = approach(gv, t.vram, 3, dt);
    gA = approach(gA, t.ghostA, 3, dt);
    gB = approach(gB, t.ghostB, 3, dt);
    gm = approach(gm, t.mid, 3, dt);
    gi = approach(gi, t.inner, 4, dt);
    warnK = approach(warnK, t.tone === 'warn' ? 1 : 0, 3, dt);
    dangerK = approach(dangerK, t.tone === 'danger' ? 1 : 0, 3, dt);
    overK = approach(overK, t.ghostOver ? 1 : 0, 3, dt);
    // the same smoothing the shader sees
    sleepK = drive.sleep < 1e-3 ? 0 : drive.sleep;
    faultK = drive.fault < 1e-3 ? 0 : drive.fault;
  }

  const reduced = () => prefersReducedMotion.current;
  const flowing = () => {
    const t = getTier();
    return !reduced() && (t === 'ambient' || t === 'live');
  };

  function tick(_now: number, dtMs: number) {
    if (!vis || !placed) return;
    const dt = dtMs / 1000;
    last = performance.now();
    drive.update(vm, dt);
    vis.frame(drive, dt);
    ease(dt);
  }

  /** One frame outside the scheduler: the picture follows the snapshot, the gauges sit at their targets. */
  function still() {
    if (!vis || !placed) return;
    const now = performance.now();
    const dt = last ? Math.min(1, (now - last) / 1000) : 1 / 60;
    last = now;
    const rm = reduced();
    drive.update(vm, rm ? 10 : dt);
    vis.frame(drive, rm ? 0 : dt);
    ease(10);
  }

  /** Placement in the shader's terms (screen heights of this canvas); the canvas backing store is capped at DPR 1. */
  function place() {
    if (!vis || w < 2 || h < 2 || r <= 0) return;
    const pr = Math.min(1, window.devicePixelRatio || 1) * res;
    const bw = Math.max(1, Math.round(w * pr));
    const bh = Math.max(1, Math.round(h * pr));
    if (canvas.width !== bw || canvas.height !== bh) {
      canvas.width = bw;
      canvas.height = bh;
    }
    vis.shift = (w / 2 - cx) / h;
    vis.lift = (h / 2 - cy) / h;
    vis.zoom = r / ((SPHERE_R * h) / 2);
    placed = true;
    // a resize clears the canvas: redraw now (no time passes), whatever the tier
    vis.frame(drive, 0);
  }

  onMount(() => {
    let v: RingsVis;
    try {
      v = new RingsVis(canvas);
    } catch (e) {
      failed = glDetail(e);
      return;
    }
    vis = v;
    drive.update(untrack(() => vm), 10);
    ease(10);
    place();
    const offTier = onTier(() => {
      if (!flowing()) still();
    });
    return () => {
      off?.();
      off = null;
      offTier();
      vis = null;
      v.dispose();
    };
  });

  // Frames: one subscription at up to 60 fps (the decode motion needs it), none with reduced motion.
  $effect(() => {
    const rm = prefersReducedMotion.current;
    untrack(() => {
      off?.();
      off = null;
      if (!rm) off = onFrame(tick, { maxFps: 60 });
      if (!flowing()) still();
    });
  });

  // Size and placement.
  $effect(() => {
    void [w, h, cx, cy, r, res];
    untrack(place);
  });

  // Each snapshot: a still frame when no frames flow.
  $effect(() => {
    void vm;
    untrack(() => {
      if (!flowing()) still();
    });
  });

  // ---- drawing ------------------------------------------------------------------------------------------------
  const valueCol = $derived(mixRgb(mixRgb(CYAN, WARN, warnK), DANGER, Math.max(dangerK, faultK)));
  const ringCol = $derived(mixRgb(CYAN, DANGER, faultK));
  const trackCol = $derived(rgba(mixRgb(CYAN, DANGER, faultK), 0.16 + 0.08 * faultK));
  const glow = (c: [number, number, number]) => `drop-shadow(0 0 ${G.glow.toFixed(1)}px ${rgba(c, 0.5)})`;
  const tickPath = $derived(G.tick > 0 ? ticks(cx, cy, R[0] + G.stroke * 0.5 + 2 * k, G.tick) : '');
  const ghostCol = $derived(rgba(mixRgb(sleepK > 0.5 ? WARN : CYAN, DANGER, Math.max(overK, faultK)), 0.55));
  // the caption: one line centred under the sphere, between the arc ends, outer ring first
  const capY = $derived(cy + CAPTION_Y * r + G.gap + G.font * 0.8);
  const L = $derived(targets.legend);
  const label = $derived(`Ring gauges: ${L.map((l) => `${l.pre} ${l.val} ${l.post}`.trim()).join(', ')}`);
</script>

<div class="specimen" bind:clientWidth={w} bind:clientHeight={h}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
  {#if failed !== null}
    <NoGl skin="Rings" needs="WebGL 2" detail={failed} at={{ x: cx, y: cy }} panel={variant === 'mini'} />
  {/if}
  {#if r > 4}
    <svg class="gauges" width={w} height={h} role="img" aria-label={label} style="opacity:{1 - 0.65 * sleepK}">
      <g fill="none" stroke-linecap="round">
        {#each R as rad, i (i)}
          <path d={arc(cx, cy, rad, 0, 1)} stroke={trackCol} stroke-width={G.track} />
        {/each}
        {#if tickPath}<path d={tickPath} stroke={rgba(ringCol, 0.38)} stroke-width={Math.max(1, k)} stroke-linecap="butt" />{/if}
        {#if gB - gA > 0.002}
          <path d={arc(cx, cy, R[0], gA, gB)} stroke={ghostCol} stroke-width={G.stroke} stroke-linecap="butt" stroke-dasharray="{(2 * k).toFixed(1)} {(4 * k).toFixed(1)}" />
        {/if}
        <path d={arc(cx, cy, R[0], 0, gv)} stroke={rgba(valueCol)} stroke-width={G.stroke} style="filter:{glow(valueCol)}" />
        <path d={arc(cx, cy, R[1], 0, gm)} stroke={rgba(ringCol)} stroke-width={G.stroke} style="filter:{glow(ringCol)}" />
        <path d={arc(cx, cy, R[2], 0, gi)} stroke={rgba(ringCol)} stroke-width={G.stroke} style="filter:{glow(ringCol)}" />
      </g>
      <text class="legend" x={cx} y={capY} font-size={G.font} text-anchor="middle">
        {#each L as l, i (i)}
          {@const tone = i === 0 ? valueCol : ringCol}
          {@const hot = i === 0 ? Math.max(warnK, dangerK, faultK) : faultK}
          {#if G.values}
            {#if l.pre}<tspan class="n" dx={i ? `${G.sep}` : undefined}>{l.pre}</tspan>{' '}{/if}<tspan class="v" dx={i && !l.pre ? `${G.sep}` : undefined} fill={rgba(mixRgb([227, 247, 255], tone, hot))}>{l.val}</tspan>{#if l.post}{' '}<tspan class="n">{l.post}</tspan>{/if}
          {:else}
            <tspan class="n" dx={i ? `${G.sep}` : undefined} fill={rgba(mixRgb([143, 169, 184], tone, Math.max(0.35, hot)))}>{l.pre || l.post}</tspan>
          {/if}
        {/each}
      </text>
    </svg>
  {/if}
</div>

<style>
  .specimen {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
  .gauges {
    position: absolute;
    left: 0;
    top: 0;
    overflow: visible;
  }
  .legend {
    font-family: 'Barlow Condensed', 'Barlow', system-ui, sans-serif;
    font-weight: 600;
    letter-spacing: 0.14em;
    font-variant-numeric: tabular-nums;
  }
  .legend .n {
    fill: #8fa9b8;
  }
</style>
