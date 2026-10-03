<script lang="ts">
  // Decode: the terminal is the model's context window. Churning noise = free context, dim stable text = used
  // context, decoding finds tokens in the noise. Branches on size; each size branches on phase internally.
  import type { SkinProps } from '../contract';
  import Full from './Full.svelte';
  import Mini from './Mini.svelte';

  let { vm, actions, size }: SkinProps = $props();
</script>

<div class="decode">
  {#if size === 'mini'}
    <Mini {vm} {actions} />
  {:else}
    <Full {vm} {actions} />
  {/if}
</div>

<style>
  .decode {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #010409;
    color: #b9d3ea;
  }
</style>
