<script lang="ts">
  // Window controls for the frameless host, drawn as thin OSD strokes.
  import type { Actions } from '../../lib/model/types';

  let { actions, maximized = false }: { actions: Actions; maximized?: boolean } = $props();
</script>

<div class="wctl" role="group" aria-label="Window">
  <button class="wb" onclick={() => actions.minimize?.()} title="Minimize" aria-label="Minimize">
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M1.5 9.5h9" /></svg>
  </button>
  <button class="wb" onclick={() => actions.toggleMaximize?.()} title={maximized ? 'Restore' : 'Maximize'} aria-label={maximized ? 'Restore' : 'Maximize'}>
    <svg viewBox="0 0 12 12" aria-hidden="true">
      {#if maximized}<path d="M3.5 1.5h7v7M1.5 3.5h7v7h-7z" />{:else}<path d="M1.5 1.5h9v9h-9z" />{/if}
    </svg>
  </button>
  <button class="wb close" onclick={() => actions.closeWindow?.()} title="Close" aria-label="Close">
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M1.8 1.8l8.4 8.4M10.2 1.8l-8.4 8.4" /></svg>
  </button>
</div>

<style>
  .wctl {
    display: flex;
    align-self: flex-start;
    margin-right: calc(var(--u) * -16px);
    flex: none;
  }
  .wb {
    width: calc(var(--u) * 44px);
    height: calc(var(--u) * 32px);
    display: grid;
    place-items: center;
    color: var(--muted);
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
  }
  .wb svg {
    width: calc(var(--u) * 11px);
    height: calc(var(--u) * 11px);
    fill: none;
    stroke: currentColor;
    stroke-width: 1.2;
    vector-effect: non-scaling-stroke;
  }
  .wb:hover {
    background: #151a1f;
    color: var(--ink);
  }
  .wb.close:hover {
    background: #c42b2b;
    color: #fff;
  }
  .wb:focus-visible {
    outline: 1px solid var(--cyan);
    outline-offset: -2px;
  }
</style>
