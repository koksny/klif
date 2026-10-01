<script lang="ts">
  // Context radar. The filled arc is the context fill (clockwise from 12 o'clock).
  // The sweep turns only while the server works; its speed is proportional to decode tok/s
  // (2 degrees per second per tok/s; during prefill, before any decode speed exists, 1/16 of the
  // prefill rate). It runs as a compositor transform animation and is paused when idle, in calm or
  // off tiers, and under prefers-reduced-motion (then it rests on the fill edge).
  import { Tween } from 'svelte/motion';
  import { cubicOut } from 'svelte/easing';
  import { untrack } from 'svelte';
  import { getTier, onTier } from '../../lib/render/scheduler';
  import { nextUid, prefersReducedMotion } from './palette';

  let {
    fraction,
    turnsPerSec,
    size,
    tone = 'cyan',
    children,
  }: {
    /** Context fill 0..1. */
    fraction: number;
    /** 0 = static. */
    turnsPerSec: number;
    size: number;
    tone?: 'cyan' | 'amber';
    children?: import('svelte').Snippet;
  } = $props();

  const R = 96;
  const uid = nextUid();
  const reduced = prefersReducedMotion();
  const fill = new Tween(0, { duration: 400, easing: cubicOut });
  $effect(() => {
    fill.target = Math.max(0, Math.min(1, fraction || 0));
  });

  const arc = $derived.by(() => {
    const f = fill.current;
    if (f <= 0.0005) return '';
    if (f >= 0.9995) return `M0,${-R}A${R},${R} 0 1,1 0,${R}A${R},${R} 0 1,1 0,${-R}Z`;
    const a = f * Math.PI * 2;
    const x = R * Math.sin(a);
    const y = -R * Math.cos(a);
    return `M0,0L0,${-R}A${R},${R} 0 ${f > 0.5 ? 1 : 0},1 ${x.toFixed(2)},${y.toFixed(2)}Z`;
  });

  const edge = $derived.by(() => {
    const a = fill.current * Math.PI * 2;
    return [R * Math.sin(a), -R * Math.cos(a)];
  });

  let sweepEl: HTMLDivElement | undefined = $state();
  let anim: Animation | null = null;
  let tier = $state(getTier());

  $effect(() => onTier((t) => (tier = t)));

  // Created once per mount. The start angle (the fill edge) is read untracked: a context change
  // must never re-create the animation, or the sweep would snap back at every telemetry tick.
  $effect(() => {
    if (!sweepEl) return;
    const a = sweepEl.animate([{ transform: 'rotate(0turn)' }, { transform: 'rotate(1turn)' }], {
      duration: 1000,
      iterations: Infinity,
    });
    a.pause();
    a.currentTime = untrack(() => Math.max(0, Math.min(1, fraction || 0))) * 1000;
    anim = a;
    untrack(() => applyRate(turnsPerSec, tier));
    return () => {
      a.cancel();
      anim = null;
    };
  });

  function applyRate(rate: number, t: typeof tier) {
    if (!anim) return;
    const run = !reduced && rate > 0 && (t === 'ambient' || t === 'live');
    if (run) {
      anim.updatePlaybackRate(rate);
      if (anim.playState !== 'running') anim.play();
    } else if (anim.playState === 'running') {
      anim.pause();
    }
  }

  $effect(() => {
    const rate = turnsPerSec;
    const t = tier;
    untrack(() => applyRate(rate, t));
  });

  // Reduced motion: no sweep motion at all; the beam rests on the fill edge.
  $effect(() => {
    const f = fill.target;
    if (reduced && anim) anim.currentTime = f * 1000;
  });
</script>

<div class="radar {tone}" style="width:{size}px;height:{size}px">
  <svg viewBox="-100 -100 200 200" aria-hidden="true">
    <defs>
      <radialGradient id="ph-radar-fill-{uid}" cx="0" cy="0" r="96" gradientUnits="userSpaceOnUse">
        <stop offset="0" class="f0" />
        <stop offset="1" class="f1" />
      </radialGradient>
    </defs>
    <g class="grid">
      <circle r="72" />
      <circle r="48" />
      <circle r="24" />
      {#each [0, 30, 60, 90, 120, 150] as deg}
        <line x1="0" y1="-96" x2="0" y2="96" transform="rotate({deg})" />
      {/each}
    </g>
    {#if arc}
      <path d={arc} fill="url(#ph-radar-fill-{uid})" class="arc" />
      <line class="edge" x1="0" y1="0" x2={edge[0]} y2={edge[1]} />
      <line class="edge" x1="0" y1="0" x2="0" y2="-96" />
    {/if}
    <circle r="96" class="rim" />
    <circle r="3.2" class="hub" />
  </svg>
  <div class="sweep" bind:this={sweepEl}><div class="trail"></div><div class="beam"></div></div>
  {#if children}<div class="center">{@render children()}</div>{/if}
</div>

<style>
  .radar {
    position: relative;
    flex: none;
  }
  svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .grid circle,
  .grid line {
    fill: none;
    stroke: var(--ph-rule);
    stroke-width: 0.9;
    stroke-dasharray: 2 3;
    vector-effect: non-scaling-stroke;
  }
  .rim {
    fill: none;
    stroke: var(--ph-brand);
    stroke-width: 1.4;
    vector-effect: non-scaling-stroke;
    opacity: 0.85;
  }
  .hub {
    fill: var(--ph-hot);
  }
  .cyan .f0 {
    stop-color: var(--ph-cyan);
    stop-opacity: 0.08;
  }
  .cyan .f1 {
    stop-color: var(--ph-cyan);
    stop-opacity: 0.5;
  }
  .amber .f0 {
    stop-color: var(--ph-amber);
    stop-opacity: 0.12;
  }
  .amber .f1 {
    stop-color: var(--ph-amber);
    stop-opacity: 0.62;
  }
  .edge {
    stroke: var(--ph-cyan);
    stroke-width: 1.6;
    vector-effect: non-scaling-stroke;
  }
  .amber .edge {
    stroke: var(--ph-amber);
  }
  .sweep {
    position: absolute;
    inset: 0;
    will-change: transform;
    pointer-events: none;
  }
  .trail {
    position: absolute;
    inset: 2%;
    border-radius: 50%;
    background: conic-gradient(from -36deg, transparent 0deg, rgba(127, 227, 255, 0.16) 35deg, transparent 36deg);
  }
  .beam {
    position: absolute;
    left: calc(50% - 1px);
    top: 2%;
    width: 2px;
    height: 48%;
    background: linear-gradient(to top, rgba(127, 227, 255, 0.25), var(--ph-hot));
    box-shadow: 0 0 6px rgba(127, 227, 255, 0.6);
  }
  .center {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    text-align: center;
  }
</style>
