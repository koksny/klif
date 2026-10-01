<script lang="ts">
  import { fly } from 'svelte/transition';
  import { ui } from '../state/ui.svelte';
</script>

<div class="toasts" aria-live="polite" aria-atomic="false">
  {#each ui.toasts as t (t.id)}
    <div class="toast" in:fly|global={{ y: 12, duration: 160 }} out:fly|global={{ y: 8, duration: 140 }}>{t.text}</div>
  {/each}
</div>

<style>
  .toasts {
    position: absolute;
    left: 50%;
    bottom: 20px;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    z-index: 80;
    pointer-events: none;
    max-width: calc(100% - 32px);
  }
  .toast {
    background: var(--k-surface-raised, #1a1a1a);
    color: var(--k-ink, #e6e6e6);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    font: 500 13px/1.3 var(--k-font-ui, system-ui, sans-serif);
    padding: 8px 14px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
