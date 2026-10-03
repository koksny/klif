<script lang="ts">
  // Ether: luminous plasma in a black void (KLIF fx Ether, src/lib/fx/ether, driven by the view model), the UI engraved on
  // glass floating over it. Branches on size; each size branches on session / phase / slot kind internally.
  import type { SkinProps } from '../contract';
  import Full from './Full.svelte';
  import Mini from './Mini.svelte';

  let { vm, actions, size }: SkinProps = $props();
</script>

<div class="ether">
  {#if size === 'mini'}
    <Mini {vm} {actions} />
  {:else}
    <Full {vm} {actions} />
  {/if}
</div>

<style>
  .ether {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #000;
    color: #ede8f5;
  }
</style>
