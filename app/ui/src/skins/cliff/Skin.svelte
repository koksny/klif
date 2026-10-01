<script lang="ts">
  import type { SkinProps } from '../contract';
  import Contours from './Contours.svelte';
  import Full from './full/Full.svelte';
  import Mini from './mini/Mini.svelte';

  let { vm, actions, size }: SkinProps = $props();
</script>

<div class="cliff" class:is-mini={size === 'mini'}>
  <Contours />
  {#if size === 'mini'}
    <Mini {vm} />
  {:else}
    <Full {vm} {actions} />
  {/if}
</div>

<style>
  .cliff {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    container-type: size;
    background: var(--basalt);
    color: var(--foam);
    font-family: var(--f-ui);
    -webkit-font-smoothing: antialiased;
  }
</style>
