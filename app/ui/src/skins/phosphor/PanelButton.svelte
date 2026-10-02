<script lang="ts">
  // Panel-mode button for the full header: a small monitor glyph drawn in the scope's line style plus
  // a PANEL label, styled like a window control with a resting outline so it reads as a labelled chip.
  // Shown whenever the host reports a small screen to move to (not only when frameless).
  import type { Actions, HostInfo } from '../../lib/model/types';

  let { actions, panel }: { actions: Actions; panel: HostInfo['panel'] } = $props();

  const tip = $derived(`Panel mode: show on the small screen${panel.target ? ` (${panel.target})` : ''}`);
</script>

<button class="pb" class:on={panel.active} title={tip} aria-label={tip} aria-pressed={panel.active} onclick={() => actions.togglePanel?.()}>
  <svg viewBox="0 0 16 16" aria-hidden="true">
    <rect x="1.75" y="2.5" width="12.5" height="8.5" />
    <path d="M8 11V13.5M5.25 13.5H10.75" />
  </svg>
  <span class="t">Panel</span>
</button>

<style>
  .pb {
    flex: none;
    height: calc(28px * var(--k));
    display: inline-flex;
    align-items: center;
    gap: calc(7px * var(--k));
    padding: 0 calc(10px * var(--k)) 0 calc(8px * var(--k));
    border-radius: 4px;
    color: var(--ph-brand);
    box-shadow: inset 0 0 0 1px var(--ph-rule);
    white-space: nowrap;
  }
  .pb svg {
    width: calc(15px * var(--k));
    height: calc(15px * var(--k));
    min-width: 13px;
    min-height: 13px;
    overflow: visible;
  }
  .pb path,
  .pb rect {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    vector-effect: non-scaling-stroke;
  }
  .t {
    font-size: var(--ph-fs-s);
    font-weight: 500;
    font-stretch: 87.5%;
    line-height: 1;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }
  .pb:hover {
    color: var(--ph-hot);
    background: rgba(18, 48, 58, 0.6);
    box-shadow: inset 0 0 0 1px var(--ph-brand);
  }
  .pb.on {
    color: var(--ph-cyan);
    box-shadow: inset 0 0 0 1px var(--ph-cyan);
  }
  .pb:hover svg {
    filter: drop-shadow(0 0 3px rgba(127, 227, 255, 0.7));
  }
</style>
