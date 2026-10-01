<script lang="ts">
  // Window controls for the frameless host: minimize, maximize/restore, close. Printed line icons on
  // the header plate; only drawn when vm.host.frameless.
  import Icon from './Icon.svelte';

  let {
    maximized = false,
    onmin,
    onmax,
    onclose,
  }: { maximized?: boolean; onmin: () => void; onmax: () => void; onclose: () => void } = $props();
</script>

<div class="wc">
  <button class="b" onclick={onmin} aria-label="Minimize" title="Minimize"><Icon name="min" size="calc(26 * var(--u))" /></button>
  <button class="b" onclick={onmax} aria-label={maximized ? 'Restore' : 'Maximize'} title={maximized ? 'Restore' : 'Maximize'}>
    <Icon name={maximized ? 'restore' : 'max'} size="calc(24 * var(--u))" />
  </button>
  <button class="b x" onclick={onclose} aria-label="Close" title="Close"><Icon name="close" size="calc(24 * var(--u))" /></button>
</div>

<style>
  .wc {
    display: flex;
    align-items: center;
    gap: calc(4 * var(--u));
  }
  .b {
    width: calc(44 * var(--u));
    height: calc(40 * var(--u));
    display: flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: calc(4 * var(--u));
    background: transparent;
    color: rgba(237, 230, 214, 0.86);
    cursor: pointer;
    padding: 0;
  }
  .b :global(.ic) {
    stroke-width: 1.5;
  }
  .b:hover {
    background: rgba(237, 230, 214, 0.08);
    color: var(--cream);
  }
  .b.x:hover {
    background: rgba(255, 107, 44, 0.85);
    color: #fff;
  }
</style>
