<script lang="ts">
  // The 3D scene's canvas: owns one LoomScene for its lifetime (created on mount, disposed with its WebGL
  // context on unmount, since skins switch at runtime), feeds it every snapshot and its box size.
  import { onMount, untrack } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import type { ViewModel } from '../../lib/model/types';
  import { LoomScene, type Labels, type Variant } from './scene';
  import type { Shape } from './shape';

  interface Props {
    vm: ViewModel;
    shape: Shape;
    variant: Variant;
    /** Screen labels the scene places over itself (full only). */
    labels?: Labels;
    /** CSS scale of an ancestor (the mini sheet), for a crisp backing store. */
    pxScale?: number;
  }
  let { vm, shape, variant, labels, pxScale = 1 }: Props = $props();

  let box: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let scene: LoomScene | null = null;
  let error = $state('');

  onMount(() => {
    let sc: LoomScene;
    try {
      sc = new LoomScene(canvas, variant);
    } catch (e) {
      error = `3D view unavailable: ${e instanceof Error ? e.message : String(e)}`;
      return;
    }
    scene = sc;
    sc.setLabels(labels ?? {});
    sc.setStill(prefersReducedMotion.current);
    sc.setVm(vm, shape);
    const fit = () => sc.resize(box.clientWidth, box.clientHeight, pxScale);
    const ro = new ResizeObserver(fit);
    ro.observe(box);
    fit();
    return () => {
      ro.disconnect();
      scene = null;
      sc.dispose();
    };
  });

  $effect(() => {
    const v = vm;
    const sh = shape;
    untrack(() => scene?.setVm(v, sh));
  });

  $effect(() => {
    const still = prefersReducedMotion.current;
    untrack(() => scene?.setStill(still));
  });

  $effect(() => {
    const k = pxScale;
    untrack(() => scene?.resize(box.clientWidth, box.clientHeight, k));
  });
</script>

<div class="stage" bind:this={box}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
  {#if error}<div class="err">{error}</div>{/if}
</div>

<style>
  .stage {
    position: absolute;
    inset: 0;
    overflow: hidden;
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
    inset: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    text-align: center;
    color: #ff3b30;
  }
</style>
