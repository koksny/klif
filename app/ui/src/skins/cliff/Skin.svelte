<script lang="ts">
  import type { SkinProps } from '../contract';
  import Contours from './Contours.svelte';
  import Full from './full/Full.svelte';
  import Mini from './mini/Mini.svelte';
  import { gpuView } from './power';

  let { vm, actions, size }: SkinProps = $props();

  // Dormant GPU: which way the resident amount is moving tells waking (restore in flight) from falling
  // asleep. Tracked here once so every part of the skin tells the same story.
  let rising = $state<boolean | null>(null);
  let lastUsed: number | null = null;
  $effect(() => {
    const used = vm.vram.dormant ? vm.vram.usedGiB : null;
    if (used === null) {
      lastUsed = null;
      rising = null;
      return;
    }
    if (lastUsed !== null) {
      if (used > lastUsed + 0.004) rising = true;
      else if (used < lastUsed - 0.004) rising = false;
    }
    lastUsed = used;
  });
  const gpu = $derived(gpuView(vm, rising));
</script>

<div class="cliff" class:is-mini={size === 'mini'}>
  <Contours />
  {#if size === 'mini'}
    <Mini {vm} {actions} {gpu} />
  {:else}
    <Full {vm} {actions} {gpu} />
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
