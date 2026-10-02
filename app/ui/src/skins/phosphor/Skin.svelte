<script lang="ts">
  // Phosphor: a vector oscilloscope behind dark glass. Root component: picks the size class and
  // scales the whole layout with one factor `--k` so it reflows from 760x900 up to 1230x1380.
  // The mini panel lays itself out on a fixed 960x640 sheet and scales that sheet on its own.
  import type { SkinProps } from '../contract';
  import Full from './Full.svelte';
  import Mini from './Mini.svelte';
  import { clamp } from './geom';

  let { vm, actions, size }: SkinProps = $props();

  let w = $state(0);
  let h = $state(0);
  const k = $derived(
    size === 'mini' ? clamp(Math.min(w / 960, h / 640), 0.5, 2) : clamp(Math.min(w / 1024, h / 1152), 0.7, 1.2),
  );
</script>

<div class="ph {size}" style="--k:{k.toFixed(4)}" bind:clientWidth={w} bind:clientHeight={h}>
  {#if w > 0 && h > 0}
    {#if size === 'mini'}
      <Mini {vm} {actions} />
    {:else}
      <Full {vm} {actions} {k} />
    {/if}
  {/if}
</div>

<style>
  .ph {
    --ph-glass: #05090b;
    --ph-grat: #12303a;
    --ph-rule: #174f5c;
    --ph-cyan: #7fe3ff;
    --ph-brand: #5ab6eb;
    --ph-amber: #e8b04a;
    --ph-hot: #f0fbff;
    --ph-ink: #cdf3fc;
    --ph-muted: #4f98b4;
    --ph-danger: #e5615c;
    /* Saira (with its width axis) for everything readable; the mono face only for console text,
       ports, API keys and fault log lines. */
    --ph-ui: 'Saira Variable', 'Saira', system-ui, sans-serif;
    --ph-display: 'Saira Variable', 'Saira', system-ui, sans-serif;
    --ph-mono: 'Share Tech Mono', ui-monospace, monospace;
    --ph-glow: 0 0 6px rgba(127, 227, 255, 0.45), 0 0 16px rgba(90, 182, 235, 0.22);
    --ph-glow-soft: 0 0 5px rgba(127, 227, 255, 0.28);
    /* type scale: small caption, label, body, tier label */
    --ph-fs-xs: max(10px, calc(9.5px * var(--k)));
    --ph-fs-s: max(10px, calc(10px * var(--k)));
    --ph-fs-m: max(11.5px, calc(11.75px * var(--k)));
    --ph-fs-l: max(12.5px, calc(13px * var(--k)));

    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    color: var(--ph-ink);
    font-family: var(--ph-ui);
    font-size: var(--ph-fs-m);
    font-variant-numeric: tabular-nums;
    background:
      radial-gradient(120% 80% at 50% 0%, rgba(23, 79, 92, 0.16), transparent 60%),
      radial-gradient(140% 100% at 50% 100%, rgba(10, 40, 50, 0.22), transparent 70%),
      var(--ph-glass);
  }

  /* Shared building blocks for every Phosphor widget. */
  :global(:where(.ph) .panel) {
    position: relative;
    border: 1px solid var(--ph-rule);
    border-radius: 6px;
    background-color: rgba(3, 9, 12, 0.72);
    box-shadow:
      inset 0 0 24px rgba(23, 79, 92, 0.12),
      0 0 0 1px rgba(5, 9, 11, 0.6);
    min-height: 0;
    min-width: 0;
  }
  :global(:where(.ph) .grat) {
    background-image:
      linear-gradient(to right, rgba(18, 48, 58, 0.5) 1px, transparent 1px),
      linear-gradient(to bottom, rgba(18, 48, 58, 0.5) 1px, transparent 1px);
    background-size: calc(40px * var(--k)) calc(40px * var(--k));
    background-position: -1px -1px;
  }
  /* Instrument labels: Saira semi-condensed caps, tracked out, with the phosphor glow. */
  :global(:where(.ph) .lbl) {
    font-family: var(--ph-ui);
    font-size: var(--ph-fs-s);
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
    white-space: nowrap;
  }
  :global(:where(.ph) .val) {
    color: var(--ph-ink);
    text-shadow: var(--ph-glow-soft);
    white-space: nowrap;
  }
  :global(:where(.ph) .mono) {
    font-family: var(--ph-mono);
    letter-spacing: 0.02em;
  }
  :global(:where(.ph) .mut) {
    color: var(--ph-muted);
  }
  :global(:where(.ph) .amber) {
    color: var(--ph-amber);
  }
  :global(:where(.ph) .danger) {
    color: var(--ph-danger);
  }
  :global(:where(.ph) button) {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
  }
  :global(:where(.ph) button:focus-visible),
  :global(:where(.ph) [tabindex]:focus-visible) {
    outline: 2px solid var(--ph-hot);
    outline-offset: 2px;
    box-shadow: 0 0 0 4px rgba(127, 227, 255, 0.25);
  }
  :global(:where(.ph) button:disabled) {
    cursor: default;
    opacity: 0.45;
  }
</style>
