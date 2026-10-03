<script lang="ts">
  // Window controls for the frameless host: three small glass circles at the end of the header bar.
  import type { Actions } from '../../lib/model/types';

  let { actions, maximized = false }: { actions: Actions; maximized?: boolean } = $props();
</script>

<div class="wctl" role="group" aria-label="Window">
  <button class="wb" onclick={() => actions.minimize?.()} title="Minimize" aria-label="Minimize">
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 6h7" /></svg>
  </button>
  <button class="wb" onclick={() => actions.toggleMaximize?.()} title={maximized ? 'Restore' : 'Maximize'} aria-label={maximized ? 'Restore' : 'Maximize'}>
    <svg viewBox="0 0 12 12" aria-hidden="true">
      {#if maximized}<path d="M4 2.5h5.5V8M2.5 4H8v5.5H2.5z" />{:else}<rect x="2.5" y="2.5" width="7" height="7" rx="1.5" />{/if}
    </svg>
  </button>
  <button class="wb close" onclick={() => actions.closeWindow?.()} title="Close" aria-label="Close">
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 3l6 6M9 3L3 9" /></svg>
  </button>
</div>

<style>
  .wctl {
    display: flex;
    gap: calc(var(--u) * 6);
    flex: none;
  }
  .wb {
    width: calc(var(--u) * 28);
    height: calc(var(--u) * 28);
    min-width: 24px;
    min-height: 24px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: 1px solid rgba(120, 220, 255, 0.16);
    background: rgba(255, 255, 255, 0.04);
    color: #8fa9b8;
    padding: 0;
    cursor: pointer;
  }
  .wb svg {
    width: calc(var(--u) * 11);
    height: calc(var(--u) * 11);
    min-width: 10px;
    min-height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linecap: round;
    vector-effect: non-scaling-stroke;
  }
  .wb:hover {
    border-color: rgba(57, 225, 255, 0.5);
    color: #e3f7ff;
    background: rgba(57, 225, 255, 0.1);
  }
  .wb.close:hover {
    border-color: #ff5c7a;
    background: rgba(255, 92, 122, 0.22);
    color: #fff;
  }
  .wb:focus-visible {
    outline: 1px solid #39e1ff;
    outline-offset: 2px;
  }
</style>
