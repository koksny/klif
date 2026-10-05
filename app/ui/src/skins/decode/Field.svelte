<script lang="ts">
  // The Decode terminal: mounts the WebGL field in its box, feeds it the view model and drives it from the
  // shared frame scheduler. In the calm/off tiers no frames come: one still frame per snapshot keeps the
  // picture following the data. Unmounting releases the GL context (skins are switched at runtime).
  import { onMount, untrack } from 'svelte';
  import { getTier, onFrame, onTier } from '../../lib/render/scheduler';
  import type { ViewModel } from '../../lib/model/types';
  import { DecodeField } from './field';
  import NoGl from '../../lib/fx/NoGl.svelte';
  import { glDetail } from '../../lib/fx/nogl';

  interface Props {
    vm: ViewModel;
    /** Glyph size in layout px. */
    fontPx: number;
    /** Share of the width given to the VRAM margin (0 = none). */
    margin?: number;
    /** CSS transform scale applied by an ancestor (the mini sheet), for a crisp backing store. */
    pxScale?: number;
    /** Overlay the terminal dims and keeps its banners out of (a sibling inside the same positioned box). */
    hero?: HTMLElement | null;
    compact?: boolean;
    maxFps?: number;
    label?: string;
  }
  let { vm, fontPx, margin = 0, pxScale = 1, hero = null, compact = false, maxFps = 60, label = '' }: Props = $props();

  let canvas: HTMLCanvasElement;
  let w = $state(0);
  let h = $state(0);
  let field = $state.raw<DecodeField | null>(null);
  /** Set when the terminal could not be built (no WebGL 2): the browser's sentence worth showing, or ''. */
  let failed = $state<string | null>(null);
  let last = 0;

  const flowing = () => {
    const t = getTier();
    return t === 'ambient' || t === 'live';
  };

  /** One frame outside the scheduler (no frames are flowing). */
  function still() {
    if (!field) return;
    const now = performance.now();
    field.tick(last ? (now - last) / 1000 : 0.016, 1);
    last = now;
  }

  function placeHero() {
    field?.setHero(hero ? { x: hero.offsetLeft, y: hero.offsetTop, w: hero.offsetWidth, h: hero.offsetHeight } : null);
  }

  onMount(() => {
    let dead = false;
    let unsub = () => {};
    let offTier = () => {};
    document.fonts
      .load('500 56px "Iosevka"')
      .catch(() => undefined)
      .then(() => {
        if (dead) return;
        try {
          field = new DecodeField(canvas, { margin: untrack(() => margin), fontPx: untrack(() => fontPx), compact: untrack(() => compact) });
        } catch (e) {
          failed = glDetail(e);
          return;
        }
        unsub = onFrame(
          (now, dt) => {
            last = now;
            field?.tick(dt / 1000);
          },
          { maxFps: untrack(() => maxFps) },
        );
        offTier = onTier(() => {
          if (!flowing()) still();
        });
      });
    return () => {
      dead = true;
      unsub();
      offTier();
      field?.destroy();
      field = null;
    };
  });

  // Size / scale / glyph size -> a new grid.
  $effect(() => {
    const f = field;
    if (!f || w <= 0 || h <= 0) return;
    f.resize(w, h, pxScale, fontPx);
    untrack(() => {
      placeHero();
      if (!flowing()) still();
    });
  });

  // Telemetry snapshot.
  $effect(() => {
    const f = field;
    const v = vm;
    if (!f) return;
    f.setVm(v);
    untrack(() => {
      placeHero();
      if (!flowing()) still();
    });
  });

  // The hero overlay changes size with its text.
  $effect(() => {
    const el = hero;
    if (!field) return;
    untrack(placeHero);
    if (!el) return;
    const ro = new ResizeObserver(() => placeHero());
    ro.observe(el);
    return () => ro.disconnect();
  });
</script>

<div class="term" bind:clientWidth={w} bind:clientHeight={h} role="img" aria-label={label}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
  {#if failed !== null}<NoGl skin="Decode" needs="WebGL 2" detail={failed} panel={compact} />{/if}
</div>

<style>
  .term {
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
