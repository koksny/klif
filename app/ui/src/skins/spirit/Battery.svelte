<script lang="ts">
  // VRAM as the camera's battery icon: 10 segments, filled = the used share. Paged-out allocations (GPU asleep)
  // are dim segments, the idle fit preview is outlined. Warn below warn_below_gib free; a spill is a small note after
  // the figure, in the warn tone. Sized by font-size.
  import type { Battery } from './osd.svelte';

  let { b }: { b: Battery } = $props();

  const SX = 3.2;
  const SW = 3.3;
</script>

<span class="bat {b.tone}" class:over={b.over} title={b.title}>
  <svg viewBox="0 0 52 20" aria-hidden="true">
    <rect class="body" x="0.75" y="0.75" width="46.5" height="18.5" rx="1.5" />
    <rect class="nub" x="48.4" y="6" width="3" height="8" rx="0.8" />
    {#each b.segs as sg, i (i)}
      <rect class="sg {sg}" x={SX + i * (SW + 0.88)} y="3.4" width={SW} height="13.2" />
    {/each}
  </svg>
  <span class="t">{b.text}</span>
  {#if b.spill}<span class="sp">{b.spill}</span>{/if}
</span>

<style>
  .bat {
    display: inline-flex;
    align-items: center;
    gap: 0.6em;
    white-space: nowrap;
    color: var(--ink);
  }
  svg {
    width: 2.75em;
    height: 1.06em;
    flex: none;
    overflow: visible;
  }
  .body {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
  }
  .nub {
    fill: currentColor;
  }
  .sg {
    fill: transparent;
  }
  .sg.fill {
    fill: currentColor;
  }
  .sg.paged {
    fill: var(--muted);
    opacity: 0.55;
  }
  .sg.ghost {
    fill: none;
    stroke: var(--cyan);
    stroke-width: 0.9;
    stroke-dasharray: 2 1.4;
  }
  .over .sg.ghost {
    stroke: var(--red);
  }
  .over .t {
    color: var(--red);
  }
  .bat.dim {
    color: var(--muted);
  }
  .bat.warn {
    color: var(--warn);
  }
  .bat.red {
    color: var(--red);
  }
  .t {
    font-variant-numeric: tabular-nums;
  }
  .sp {
    margin-left: -0.2em;
    font-size: 0.7em;
    color: var(--warn);
    opacity: 0.85;
  }
</style>
