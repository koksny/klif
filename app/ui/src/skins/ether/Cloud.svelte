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

  interface Props {
    vm: ViewModel;
    /** The cloud's centre moved left by this many screen heights (0 = the original composition). */
    shift: number;
    /** The cloud's centre moved up by this many screen heights (0 = the original composition). */
    lift?: number;
  }
  let { vm, shift, lift = 0 }: Props = $props();

  let box: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let vis: EtherVis | null = null;
  let unsub: (() => void) | null = null;
  let error = $state('');
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
      error = e instanceof Error ? e.message : String(e);
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

<div class="cloud" bind:this={box} aria-hidden="true">
  <canvas bind:this={canvas}></canvas>
  {#if error}<div class="err">background unavailable: {error}</div>{/if}
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
  .err {
    position: absolute;
    left: 4%;
    top: 50%;
    font: 300 12px/1.4 'Iosevka', ui-monospace, monospace;
    color: rgba(237, 232, 245, 0.35);
  }
</style>
