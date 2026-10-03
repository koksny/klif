<script lang="ts">
  // Spirit: Edan Kwan's The Spirit (1M-particle curl-noise smoke) seen through a camera viewfinder. The smoke
  // is the subject being filmed, KLIF's data is the camera's OSD. Branches on size; each size branches on
  // session / phase / slot kind internally.
  import type { SkinProps } from '../contract';
  import Full from './Full.svelte';
  import Mini from './Mini.svelte';

  let { vm, actions, size }: SkinProps = $props();
</script>

<div class="spirit">
  {#if size === 'mini'}
    <Mini {vm} {actions} />
  {:else}
    <Full {vm} {actions} />
  {/if}
</div>

<style>
  .spirit {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #080808;
    color: #d9f3ff;
  }
</style>
