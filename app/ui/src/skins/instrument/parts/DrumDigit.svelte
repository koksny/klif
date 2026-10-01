<script lang="ts">
  // One drum of the odometer: a strip 0-9 repeated three times. A change rolls in the direction the
  // value moved, then silently re-centres on the middle copy so the strip never runs out.
  import { untrack } from 'svelte';

  let { digit, up, dim = false }: { digit: number; up: boolean; dim?: boolean } = $props();

  const STRIP = Array.from({ length: 30 }, (_, i) => i % 10);
  let pos = $state(untrack(() => 10 + digit));
  let snap = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const d = digit;
    untrack(() => {
      const cur = ((pos % 10) + 10) % 10;
      if (d === cur) return;
      const delta = up ? (d - cur + 10) % 10 : -((cur - d + 10) % 10);
      snap = false;
      pos = pos + delta;
      clearTimeout(timer);
      timer = setTimeout(() => {
        snap = true;
        pos = 10 + d;
      }, 420);
    });
  });

  $effect(() => () => clearTimeout(timer));
</script>

<span class="drum" class:dim>
  <span class="strip" class:snap style="transform: translate3d(0, {(-pos / 30) * 100}%, 0)">
    {#each STRIP as n, i (i)}<span class="cell"><span class="g">{n}</span></span>{/each}
  </span>
</span>

<style>
  .drum {
    position: relative;
    display: block;
    height: 100%;
    overflow: hidden;
  }
  .strip {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 3000%;
    display: flex;
    flex-direction: column;
    transition: transform 380ms cubic-bezier(0.3, 0.7, 0.25, 1);
    will-change: transform;
  }
  .strip.snap {
    transition: none;
  }
  .cell {
    flex: 1 1 0;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }
  .g {
    display: block;
    line-height: 1;
    transform: translateY(var(--digit-dy, 0));
  }
  /* A leading zero is a blank window, as on the mockup's drum. */
  .dim .strip {
    opacity: 0;
  }
</style>
