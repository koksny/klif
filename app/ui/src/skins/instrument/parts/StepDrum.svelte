<script lang="ts">
  // "5 / 8" on drum windows: the current sampling step rolls on its own drums, the step count sits on
  // the right of a printed slash. blank = no job (every window empty).
  import DrumDigit from './DrumDigit.svelte';

  let {
    step,
    steps,
    variant = 'full',
    blank = false,
  }: { step: number; steps: number; variant?: 'full' | 'mini'; blank?: boolean } = $props();

  const n = $derived(Math.max(1, String(Math.max(0, Math.round(steps))).length));
  const digits = (v: number) => {
    const s = String(Math.max(0, Math.round(v))).padStart(n, ' ');
    return s.split('').map((c) => ({ d: c === ' ' ? 0 : Number(c), lead: c === ' ' }));
  };
  const a = $derived(digits(step));
  const b = $derived(digits(steps));
</script>

<div class="odo {variant}" role="img" aria-label={blank ? 'no job' : `step ${step} of ${steps}`}>
  {#each a as d, i (i)}
    <div class="win"><DrumDigit digit={d.d} up={true} dim={blank || d.lead} /></div>
  {/each}
  <div class="win sl"><span class:off={blank}>/</span></div>
  {#each b as d, i (i)}
    <div class="win"><DrumDigit digit={d.d} up={true} dim={blank || d.lead} /></div>
  {/each}
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
  .win.sl {
    flex: 0 0 auto;
    width: var(--slash-w, 0.42em);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.62em;
    color: rgba(237, 230, 214, 0.78);
  }
  .off {
    opacity: 0;
  }
  .full {
    font-family: 'Oswald Variable', 'Barlow Condensed', sans-serif;
    font-weight: 500;
  }
  .mini {
    font-family: 'Archivo Variable', sans-serif;
    font-weight: 800;
  }
  .mini .win:not(.sl)::before {
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
