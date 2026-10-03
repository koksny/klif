<script lang="ts">
  // Rings: an optical laboratory. The sphere is a specimen under glass (KLIF fx Rings, src/lib/fx/rings), the UI is
  // laboratory glassware: frosted panels over the shader and concentric ring gauges around the sphere that measure
  // the session. Branches on size; each size branches on phase and slot kind internally.
  import type { SkinProps } from '../contract';
  import Full from './Full.svelte';
  import Mini from './Mini.svelte';

  let { vm, actions, size }: SkinProps = $props();
</script>

<div class="rings">
  {#if size === 'mini'}
    <Mini {vm} {actions} />
  {:else}
    <Full {vm} {actions} />
  {/if}
</div>

<style>
  .rings {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #191a26;
    color: #e3f7ff;
  }
</style>
