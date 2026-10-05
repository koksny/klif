<script lang="ts">
  // The subject being filmed: Edan Kwan's The Spirit (1M particles) in an iframe, stepped one frame per KLIF
  // frame from the shared scheduler (the page schedules nothing itself, #external=1). No frames (calm / off)
  // = the smoke holds still. `shift` moves the smoke's centre left by that many px: the iframe is made 2x
  // that much wider and pulled left by 2x. Unmounting removes the iframe (and with it its WebGL context).
  // A window without what the build needs (spiritMissing) gets no iframe and a note instead. While the smoke has
  // not appeared, a note says why when the page steps at under SLOW_FPS (WebGL drawn in software, as under WSLg)
  // or has not started after LATE_MS; only seconds with flowing frames count. It goes away when the smoke appears.
  import { onMount, untrack } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import { getTier, onFrame } from '../../lib/render/scheduler';
  import type { ViewModel } from '../../lib/model/types';
  import { Drive } from '../../lib/fx/drive';
  import { SPIRIT_SRC, SpiritVis, spiritMissing } from '../../lib/fx/spirit/spirit';
  import NoGl from '../../lib/fx/NoGl.svelte';

  interface Props {
    vm: ViewModel;
    /** Smoke centre offset to the left, in CSS px of this box. */
    shift?: number;
    /** True once the particles exist and the smoke has settled in (the frame stays black before). */
    ready?: boolean;
    /** The 960x640 panel. */
    panel?: boolean;
  }
  let { vm, shift = 0, ready = $bindable(false), panel = false }: Props = $props();

  type SpiritWin = Window & { __spirit?: object; __spiritFrame?: () => void; __spiritJNow?: number };

  const LATE_MS = 20_000;
  /** Steps per second below which the page is too slow, averaged over SLOW_S seconds of flowing frames. */
  const SLOW_FPS = 10;
  const SLOW_S = 5;
  /** What this window lacks for the build; the iframe is not made when it lacks something. */
  const missing = spiritMissing();
  /** Why there is no smoke yet: the page has not started, or it steps this many times per second. */
  let late = $state<{ kind: 'start' } | { kind: 'slow'; fps: number } | null>(null);
  let bw = $state(0);
  let bh = $state(0);
  let iframe: HTMLIFrameElement | undefined = $state();
  let vis: SpiritVis | null = null;
  const drive = new Drive();
  const flowing = () => {
    const t = getTier();
    return t === 'ambient' || t === 'live';
  };
  const win = () => (iframe?.contentWindow as SpiritWin | null) ?? null;
  /**
   * The page eases its background from its own grey to the configured #080808 over its first frames, so the
   * smoke is shown only after this many steps: until then the frame stays black (the OSD is already there).
   */
  const SETTLE = 45;
  /** Steps taken before the smoke appears: also the frame rate the page reaches in this window. */
  let stepped = 0;
  /** One step of the page. A throw inside it (a page still building) must never reach the shared frame loop. */
  const step = (v: SpiritVis) => {
    const w = win();
    if (!w?.__spirit || !w.__spiritFrame) return;
    try {
      v.frame(drive);
    } catch {
      return; // the next frame tries again
    }
    if (!ready && ++stepped >= SETTLE) {
      ready = true;
      late = null;
    }
  };

  onMount(() => {
    if (missing) {
      console.error(`Spirit cannot run in this window: ${missing}`);
      return;
    }
    if (!iframe) return;
    const v = new SpiritVis(iframe);
    vis = v;
    // Once a second until the smoke appears.
    let flowingS = 0;
    let lastSteps = 0;
    let wasFlowing = false;
    const perSecond: number[] = [];
    const watch = setInterval(() => {
      if (ready) return clearInterval(watch);
      const now = flowing();
      if (now && wasFlowing && stepped > 0) perSecond.push(stepped - lastSteps);
      lastSteps = stepped;
      wasFlowing = now;
      if (!now) return;
      flowingS++;
      if (perSecond.length >= SLOW_S) {
        const fps = perSecond.slice(-SLOW_S).reduce((a, b) => a + b, 0) / SLOW_S;
        if (fps < SLOW_FPS && late?.kind !== 'slow') {
          late = { kind: 'slow', fps };
          console.error(`Spirit: ${fps.toFixed(1)} frames per second in this window, the smoke is slow to appear`);
        }
      } else if (!stepped && !late && flowingS * 1000 >= LATE_MS) {
        late = { kind: 'start' };
        console.error(`Spirit: the page has not started after ${LATE_MS / 1000} s`);
      }
    }, 1000);
    // The bundle needs a few seconds to build 1M particles: wait for its frame hook (a timer, not frames).
    let poll: ReturnType<typeof setInterval> | 0 = setInterval(() => {
      const w = win();
      if (!w?.__spirit || !w.__spiritFrame) return;
      if (poll) clearInterval(poll);
      poll = 0;
      // One step on mount when no frames flow (calm / off): the smoke then holds still where it is.
      if (!flowing()) {
        drive.update(vm, 1 / 60);
        step(v);
      }
    }, 200);
    const off = onFrame(
      (_t, dtMs) => {
        const dt = dtMs / 1000;
        drive.update(vm, dt);
        // Reduced motion: let the smoke gather once, then hold it still (state changes re-render one frame).
        if (prefersReducedMotion.current && ready && (win()?.__spiritJNow ?? 1) >= 0.999 && drive.form >= 0.995) return;
        step(v);
      },
      { maxFps: 60 },
    );
    return () => {
      if (poll) clearInterval(poll);
      clearInterval(watch);
      off();
      v.dispose();
      vis = null;
    };
  });

  // Reduced motion: the smoke holds still, so a change of state (fault red, sleep grey) draws one settled frame.
  const look = $derived(`${vm.session?.phase ?? 'idle'}:${!!vm.vram.dormant}`);
  $effect(() => {
    void look;
    if (!ready || !prefersReducedMotion.current) return;
    const t = setTimeout(() => {
      if (!vis || !flowing()) return;
      untrack(() => drive.update(vm, 10));
      step(vis);
    }, 400);
    return () => clearTimeout(t);
  });
</script>

<div class="smoke" class:ready bind:clientWidth={bw} bind:clientHeight={bh}>
  {#if !missing}
    <iframe
      bind:this={iframe}
      src={SPIRIT_SRC}
      title="The Spirit (Edan Kwan)"
      aria-hidden="true"
      tabindex="-1"
      style="left:{-2 * shift}px; width:calc(100% + {2 * shift}px)"
    ></iframe>
  {/if}
  {#if missing}
    <NoGl skin="Spirit" needs="WebGL with float textures" detail={missing} at={{ x: bw / 2 - shift, y: bh / 2 }} {panel} />
  {:else if late?.kind === 'start'}
    <NoGl
      skin="Spirit"
      needs="WebGL with float textures"
      reason="The smoke has not started after {LATE_MS / 1000} seconds; this window's WebGL may lack what it needs. The note goes away if it starts."
      at={{ x: bw / 2 - shift, y: bh / 2 }}
      {panel}
    />
  {:else if late?.kind === 'slow'}
    <NoGl
      skin="Spirit"
      needs="WebGL on the GPU"
      reason="This window draws it at {late.fps < 1 ? 'under 1' : Math.round(late.fps)} frames per second (WebGL in software?), so the smoke is slow to appear. The note goes away when it does."
      at={{ x: bw / 2 - shift, y: bh / 2 }}
      {panel}
    />
  {/if}
</div>

<style>
  .smoke {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: #080808;
    pointer-events: none;
  }
  iframe {
    position: absolute;
    top: 0;
    height: 100%;
    display: block;
    border: 0;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.9s ease;
  }
  .ready iframe {
    opacity: 1;
  }
</style>
