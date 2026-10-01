<script lang="ts">
  // Window chrome for the frameless host: three vector glyphs drawn like the rest of the scope.
  import type { Actions } from '../../lib/model/types';

  let { actions, maximized = false }: { actions: Actions; maximized?: boolean } = $props();
</script>

<div class="win" role="group" aria-label="Window">
  <button class="wb" title="Minimize" aria-label="Minimize" onclick={() => actions.minimize?.()}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2.5 8.5H13.5" /></svg>
  </button>
  <button
    class="wb"
    title={maximized ? 'Restore' : 'Maximize'}
    aria-label={maximized ? 'Restore' : 'Maximize'}
    onclick={() => actions.toggleMaximize?.()}
  >
    <svg viewBox="0 0 16 16" aria-hidden="true">
      {#if maximized}
        <path d="M5.5 3.5H12.5V10.5" />
        <rect x="3.5" y="5.5" width="7" height="7" />
      {:else}
        <rect x="3" y="3" width="10" height="10" />
      {/if}
    </svg>
  </button>
  <button class="wb close" title="Close" aria-label="Close" onclick={() => actions.closeWindow?.()}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 3.5L12.5 12.5M12.5 3.5L3.5 12.5" /></svg>
  </button>
</div>

<style>
  .win {
    display: flex;
    gap: calc(4px * var(--k));
  }
  .wb {
    width: calc(44px * var(--k));
    height: calc(28px * var(--k));
    display: grid;
    place-items: center;
    border-radius: 4px;
    color: var(--ph-brand);
  }
  .wb svg {
    width: calc(16px * var(--k));
    height: calc(16px * var(--k));
    overflow: visible;
  }
  .wb path,
  .wb rect {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    vector-effect: non-scaling-stroke;
  }
  .wb:hover {
    color: var(--ph-hot);
    background: rgba(18, 48, 58, 0.6);
    box-shadow: inset 0 0 0 1px var(--ph-rule);
  }
  .wb:hover svg {
    filter: drop-shadow(0 0 3px rgba(127, 227, 255, 0.7));
  }
  .wb.close:hover {
    color: #ffd9d6;
    background: rgba(229, 97, 92, 0.32);
    box-shadow: inset 0 0 0 1px var(--ph-danger);
  }
</style>
