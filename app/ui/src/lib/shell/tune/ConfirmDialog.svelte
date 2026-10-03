<script lang="ts">
  // The drawer's own confirm prompt (a native confirm() does not exist in the desktop webview). Focus starts on
  // the safe choice; Escape answers "no".
  import { fade } from 'svelte/transition';
  import { getTune } from './state.svelte';

  const t = getTune();
  const req = $derived(t.confirmReq);
  let cancelBtn = $state<HTMLButtonElement | undefined>();

  $effect(() => {
    if (req && cancelBtn) cancelBtn.focus();
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      t.answer(false);
    }
  }
</script>

{#if req}
  <div class="veil" role="presentation" transition:fade={{ duration: 120 }} onclick={() => t.answer(false)}></div>
  <div
    class="confirm"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="tune-confirm-title"
    aria-describedby={req.detail ? 'tune-confirm-detail' : undefined}
    tabindex="-1"
    onkeydown={onKey}
    transition:fade={{ duration: 120 }}
  >
    <p id="tune-confirm-title" class="title">{req.title}</p>
    {#if req.detail}<p id="tune-confirm-detail" class="detail">{req.detail}</p>{/if}
    <div class="row">
      <button type="button" bind:this={cancelBtn} onclick={() => t.answer(false)}>{req.cancel ?? 'Cancel'}</button>
      <button type="button" class="primary" class:danger={req.danger} onclick={() => t.answer(true)}>{req.confirm}</button>
    </div>
  </div>
{/if}

<style>
  .veil {
    position: absolute;
    inset: 0;
    z-index: 20;
    background: color-mix(in srgb, var(--k-bg, #0d1117) 45%, transparent);
  }
  .confirm {
    position: absolute;
    left: 18px;
    right: 18px;
    bottom: 18px;
    z-index: 21;
    display: grid;
    gap: 8px;
    padding: 16px;
    background: var(--k-surface-raised, #1a1a1a);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    box-shadow: 0 -10px 40px rgba(0, 0, 0, 0.45);
  }
  .title {
    margin: 0;
    font: 600 15px/1.3 var(--k-font-ui, system-ui, sans-serif);
  }
  .detail {
    margin: 0;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.4 var(--k-font-ui, system-ui, sans-serif);
  }
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 6px;
  }
  .primary.danger {
    background: var(--k-danger, #e05a5a);
  }
</style>
