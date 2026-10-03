<script lang="ts">
  // The exposure meter (−2 … 0 … +2, a tick every third of a stop) repurposed as a fill gauge: context fill,
  // sampling steps or the load. The caret and the fill ease to each new value (a CSS transition, so it also
  // settles with no frames flowing); nothing jumps. Sized by font-size; the width comes from the parent.
  import type { Meter } from './osd.svelte';

  let { m, label = true }: { m: Meter; label?: boolean } = $props();

  const TICKS = Array.from({ length: 13 }, (_, i) => i);
  const NUMS = ['−2', '1', '0', '1', '+2'];
  const f = $derived(Math.max(0, Math.min(1, m.frac)));
</script>

<div class="meter {m.tone}" role="meter" aria-label={m.label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(f * 100)}>
  {#if label}<span class="ml">{m.label}</span>{/if}
  <div class="scale" style="--f:{f}">
    <div class="nums">{#each NUMS as n, i (i)}<span style="left:{i * 25}%">{n}</span>{/each}</div>
    <div class="track">
      {#each TICKS as i (i)}<span class="tk" class:maj={i % 3 === 0} style="left:{(i / 12) * 100}%"></span>{/each}
      <span class="fill"></span>
      <span class="caret"></span>
    </div>
  </div>
</div>

<style>
  .meter {
    display: flex;
    align-items: flex-end;
    gap: 0.9em;
    color: var(--ink);
  }
  .ml {
    flex: none;
    color: var(--muted);
    letter-spacing: 0.1em;
    line-height: 1;
    padding-bottom: 0.15em;
  }
  .scale {
    position: relative;
    flex: 1 1 auto;
    height: 2.15em;
  }
  .nums {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 1em;
    font-size: 0.82em;
    line-height: 1;
    color: var(--osd);
  }
  .nums span {
    position: absolute;
    transform: translateX(-50%);
  }
  .track {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0.55em;
    height: 1px;
    background: var(--osd);
  }
  .tk {
    position: absolute;
    bottom: 0;
    width: 1px;
    height: 0.32em;
    background: var(--osd);
    transform: translateX(-0.5px);
  }
  .tk.maj {
    height: 0.62em;
  }
  .fill {
    position: absolute;
    left: 0;
    top: -1px;
    height: 3px;
    width: calc(var(--f) * 100%);
    background: var(--cyan);
    box-shadow: 0 0 6px rgba(43, 200, 255, 0.55);
    transition: width 0.9s cubic-bezier(0.25, 0.8, 0.3, 1);
  }
  .caret {
    position: absolute;
    left: calc(var(--f) * 100%);
    top: 3px;
    width: 0;
    height: 0;
    border-left: 0.32em solid transparent;
    border-right: 0.32em solid transparent;
    border-bottom: 0.48em solid var(--ink);
    transform: translateX(-50%);
    transition: left 0.9s cubic-bezier(0.25, 0.8, 0.3, 1);
  }
  .meter.dim .fill {
    background: var(--muted);
    box-shadow: none;
  }
  .meter.dim .caret {
    border-bottom-color: var(--muted);
  }
  .meter.warn .fill {
    background: var(--warn);
    box-shadow: none;
  }
  .meter.red .fill {
    background: var(--red);
    box-shadow: none;
  }
  .meter.red .caret {
    border-bottom-color: var(--red);
  }
</style>
