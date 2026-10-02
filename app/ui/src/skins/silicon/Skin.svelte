<script lang="ts">
  // Silicon (Krzem): engineering blueprint of the GPU with a dimensioned VRAM section.
  // Branches on size; each size branches on session / phase / slot kind internally.
  import type { SkinProps } from '../contract';
  import Full from './Full.svelte';
  import Mini from './Mini.svelte';

  let { vm, actions, size }: SkinProps = $props();
</script>

<div class="silicon">
  {#if size === 'mini'}
    <Mini {vm} {actions} />
  {:else}
    <Full {vm} {actions} />
  {/if}
</div>

<style>
  .silicon {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #0d1115;
    color: #d8e4ec;
  }
</style>
