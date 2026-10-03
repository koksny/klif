<script lang="ts">
  // The subject being filmed: Edan Kwan's The Spirit (1M particles) in an iframe, stepped one frame per KLIF
  // frame from the shared scheduler (the page schedules nothing itself, #external=1). No frames (calm / off)
  // = the smoke holds still. `shift` moves the smoke's centre left by that many px: the iframe is made 2x
  // that much wider and pulled left by 2x. Unmounting removes the iframe (and with it its WebGL context).
  import { onMount, untrack } from 'svelte';
  import { prefersReducedMotion } from 'svelte/motion';
  import { getTier, onFrame } from '../../lib/render/scheduler';
  import type { ViewModel } from '../../lib/model/types';
  import { Drive } from '../../lib/fx/drive';
  import { SPIRIT_SRC, SpiritVis } from '../../lib/fx/spirit/spirit';

  interface Props {
    vm: ViewModel;
    /** Smoke centre offset to the left, in CSS px of this box. */
    shift?: number;
    /** True once the particles exist and the smoke has settled in (the frame stays black before). */
    ready?: boolean;
  }
  let { vm, shift = 0, ready = $bindable(false) }: Props = $props();

  type SpiritWin = Window & { __spirit?: object; __spiritFrame?: () => void; __spiritJNow?: number };

  let iframe: HTMLIFrameElement;
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
    if (!ready && ++stepped >= SETTLE) ready = true;
  };

  onMount(() => {
    const v = new SpiritVis(iframe);
    vis = v;
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

<div class="smoke" class:ready>
  <iframe
    bind:this={iframe}
    src={SPIRIT_SRC}
    title="The Spirit (Edan Kwan)"
    aria-hidden="true"
    tabindex="-1"
    style="left:{-2 * shift}px; width:calc(100% + {2 * shift}px)"
  ></iframe>
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
