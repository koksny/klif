<script lang="ts">
  // Loom: the transformer in 3D on an amber phosphor screen (layers, experts, KV cache, logits).
  // Branches on size; each size branches on session / phase / slot kind internally.
  import type { SkinProps } from '../contract';
  import Full from './Full.svelte';
  import Mini from './Mini.svelte';

  let { vm, actions, size }: SkinProps = $props();
</script>

<div class="loom">
  {#if size === 'mini'}
    <Mini {vm} {actions} />
  {:else}
    <Full {vm} {actions} />
  {/if}
</div>

<style>
  .loom {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #050302;
    color: #e8a83c;
  }
</style>
