<script lang="ts">
  // The 3D scene's canvas: owns one LoomScene for its lifetime (created on mount, disposed with its WebGL
  // context on unmount, since skins switch at runtime), feeds it every snapshot and its box size.
  import { onMount, untrack } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import type { ViewModel } from '../../lib/model/types';
  import { LoomScene, type Labels, type Variant } from './scene';
  import type { Shape } from './shape';
  import NoGl from '../../lib/fx/NoGl.svelte';
  import { glDetail } from '../../lib/fx/nogl';

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
  /** Set when the scene could not be built (no WebGL 2): the browser's sentence worth showing, or ''. */
  let failed = $state<string | null>(null);

  onMount(() => {
    let sc: LoomScene;
    try {
      sc = new LoomScene(canvas, variant);
    } catch (e) {
      failed = glDetail(e);
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
  {#if failed !== null}<NoGl skin="Loom" needs="WebGL 2" detail={failed} panel={variant === 'mini'} />{/if}
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
</style>
