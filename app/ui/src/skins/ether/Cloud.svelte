<script lang="ts">
  // The Ether background: one EtherVis (KLIF fx Ether, src/lib/fx/ether) on a full-bleed
  // canvas, fed the view model through Drive and clocked by the shared frame scheduler. The backing store is
  // kept at a device-pixel ratio of 1 (a soft glow needs no more). In the calm/off tiers and under reduced
  // motion no frames flow: one still frame per snapshot (time held, every signal at its target) keeps it
  // following the data. Unmounting releases the WebGL context (skins switch at runtime).
  import { onMount, untrack } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import { getTier, onFrame, onTier } from '../../lib/render/scheduler';
  import type { ViewModel } from '../../lib/model/types';
  import { Drive } from '../../lib/fx/drive';
  import { EtherVis } from '../../lib/fx/ether/ether';
  import NoGl from '../../lib/fx/NoGl.svelte';
  import { glDetail } from '../../lib/fx/nogl';

  interface Props {
    vm: ViewModel;
    /** The cloud's centre moved left by this many screen heights (0 = the original composition). */
    shift: number;
    /** The cloud's centre moved up by this many screen heights (0 = the original composition). */
    lift?: number;
    /** The 960x640 panel. */
    panel?: boolean;
  }
  let { vm, shift, lift = 0, panel = false }: Props = $props();

  let box: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let vis: EtherVis | null = null;
  let unsub: (() => void) | null = null;
  /** Set when the cloud could not be built (no WebGL 2): the browser's sentence worth showing, or ''. */
  let failed = $state<string | null>(null);
  let bw = $state(0);
  let bh = $state(0);
  const drive = new Drive();

  /** True while the scheduler delivers frames to the cloud. */
  const flowing = () => {
    const t = getTier();
    return !!unsub && (t === 'ambient' || t === 'live');
  };

  /** One frame outside the scheduler: every smoothed signal settles on its target, time does not move. */
  function still() {
    if (!vis || getTier() === 'off') return;
    const wave = drive.wave;
    drive.update(vm, 8);
    drive.wave = wave;
    vis.shift = shift;
    vis.lift = lift;
    vis.frame(drive, 0);
  }

  /** Canvas backing store at CSS pixels (DPR 1); a size change clears it. */
  function fit(): boolean {
    const w = Math.max(1, Math.round(box.clientWidth));
    const h = Math.max(1, Math.round(box.clientHeight));
    if (canvas.width === w && canvas.height === h) return false;
    canvas.width = w;
    canvas.height = h;
    return true;
  }

  function subscribe() {
    unsub?.();
    unsub = null;
    if (!vis || prefersReducedMotion.current) return;
    unsub = onFrame(
      (_now, dtMs) => {
        if (!vis) return;
        const dt = dtMs / 1000;
        drive.update(vm, dt);
        vis.shift = shift;
        vis.lift = lift;
        vis.frame(drive, dt);
      },
      { maxFps: 60 },
    );
  }

  onMount(() => {
    let v: EtherVis;
    try {
      v = new EtherVis(canvas);
    } catch (e) {
      failed = glDetail(e);
      return;
    }
    vis = v;
    fit();
    const ro = new ResizeObserver(() => {
      if (fit() && !flowing()) still();
    });
    ro.observe(box);
    subscribe();
    const offTier = onTier(() => {
      if (!flowing()) still();
    });
    still();
    return () => {
      ro.disconnect();
      offTier();
      unsub?.();
      unsub = null;
      vis = null;
      v.dispose();
    };
  });

  // Reduced motion: no frames, a still per snapshot instead.
  $effect(() => {
    void prefersReducedMotion.current;
    untrack(() => {
      if (!vis) return;
      subscribe();
      if (!flowing()) still();
    });
  });

  // A new snapshot (or a new layout offset) while no frames flow.
  $effect(() => {
    void vm;
    void shift;
    void lift;
    untrack(() => {
      if (!flowing()) still();
    });
  });
</script>

<div class="cloud" bind:this={box} bind:clientWidth={bw} bind:clientHeight={bh}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
  {#if failed !== null}
    <NoGl skin="Ether" needs="WebGL 2" detail={failed} at={{ x: bw / 2 - shift * bh, y: bh / 2 - lift * bh }} {panel} />
  {/if}
</div>

<style>
  .cloud {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
    background: #000;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
</style>
