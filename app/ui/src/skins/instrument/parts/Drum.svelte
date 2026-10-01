<script lang="ts">
  // Mechanical drum odometer. The shown value is a damped copy of the real one: it moves towards the
  // target at most twice per second (so digits never re-roll faster than that) and lands exactly on it.
  import { untrack } from 'svelte';
  import DrumDigit from './DrumDigit.svelte';

  // Leading zeros are blank windows (as in the mockup). narrowLead draws the first window narrower
  // while it can only ever be blank (the caller knows the value range).
  // blank: no reading at all (nothing measured yet): every window is empty.
  let {
    value,
    intDigits = 2,
    variant = 'full',
    narrowLead = false,
    blank = false,
  }: { value: number; intDigits?: number; variant?: 'full' | 'mini'; narrowLead?: boolean; blank?: boolean } = $props();

  const reduce =
    typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const r1 = (v: number) => Math.round(Math.max(0, v) * 10) / 10;

  let shown = $state(untrack(() => r1(value)));
  let up = $state(true);
  let last = -1e9;
  let timer: ReturnType<typeof setTimeout> | undefined;

  // One damped step towards the real value, never sooner than 500 ms after the previous roll. While
  // the shown value has not landed, a single timer schedules the next step; nothing runs at rest.
  function step() {
    timer = undefined;
    const target = r1(value);
    if (target === shown) return;
    const wait = 500 - (performance.now() - last);
    if (wait > 0) {
      timer = setTimeout(step, wait);
      return;
    }
    last = performance.now();
    const d = target - shown;
    up = d > 0;
    shown = reduce || Math.abs(d) <= 0.5 ? target : r1(shown + d * 0.75);
    if (shown !== target) timer = setTimeout(step, 500);
  }

  $effect(() => {
    value;
    untrack(() => {
      if (!timer) step();
    });
  });
  $effect(() => () => clearTimeout(timer));

  const tenths = $derived(Math.round(shown * 10));
  const ints = $derived(
    Array.from({ length: intDigits }, (_, i) => {
      const p = intDigits - 1 - i; // place value exponent for this drum (0 = units)
      const dgt = Math.floor(tenths / 10 / 10 ** p) % 10;
      const leadingZero = p > 0 && Math.floor(tenths / 10 / 10 ** p) === 0;
      return { dgt, leadingZero };
    }),
  );
</script>

<div class="odo {variant}" role="img" aria-label={blank ? 'no reading' : shown.toFixed(1)}>
  {#each ints as d, i (i)}
    <div class="win" class:lead={i === 0 && narrowLead && intDigits > 1}><DrumDigit digit={d.dgt} {up} dim={blank || d.leadingZero} /></div>
  {/each}
  <div class="win dot"><span class:off={blank}>.</span></div>
  <div class="win"><DrumDigit digit={tenths % 10} {up} dim={blank} /></div>
</div>

<style>
  .odo {
    display: flex;
    height: 100%;
    gap: var(--drum-gap, 3px);
    padding: var(--drum-pad, 4px);
    background: #0c0d0e;
    border-radius: var(--drum-radius, 6px);
    box-shadow:
      inset 0 2px 6px rgba(0, 0, 0, 0.8),
      0 1px 0 rgba(255, 255, 255, 0.05);
    color: var(--cream);
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }
  .win {
    position: relative;
    flex: 1 1 0;
    min-width: 0;
    background: linear-gradient(180deg, #121314 0%, #222326 30%, #26272a 50%, #222326 70%, #121314 100%);
    border-radius: 3px;
    overflow: hidden;
  }
  /* Cylinder shading over the digits: plainly the drum's curvature. */
  .win::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: linear-gradient(
      180deg,
      rgba(0, 0, 0, 0.72) 0%,
      rgba(0, 0, 0, 0.12) 24%,
      rgba(0, 0, 0, 0) 40%,
      rgba(0, 0, 0, 0) 60%,
      rgba(0, 0, 0, 0.12) 76%,
      rgba(0, 0, 0, 0.72) 100%
    );
  }
  .win.dot {
    flex: 0 0 auto;
    width: var(--dot-w, 0.32em);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  /* Tall industrial numerals for the desk panel (mockup), heavy grotesk for the tiny panel. */
  .full {
    font-family: 'Oswald Variable', 'Barlow Condensed', sans-serif;
    font-weight: 500;
  }
  .full .win:not(.dot) {
    flex-basis: 0;
  }
  .win.lead {
    flex-grow: 0.62;
  }
  .off {
    opacity: 0;
  }
  .mini {
    font-family: 'Archivo Variable', sans-serif;
    font-weight: 800;
    font-stretch: 100%;
  }
  /* The split seam of the mini drums (mockup), a plain mechanical line. */
  .mini .win::before {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 2px;
    margin-top: -1px;
    background: rgba(0, 0, 0, 0.55);
    z-index: 1;
    pointer-events: none;
  }
</style>
