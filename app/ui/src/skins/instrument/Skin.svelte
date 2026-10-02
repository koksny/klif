<script lang="ts">
  // Instrument (Zegar) root: measures its box, derives the panel scale and branches on size class.
  // State branching (idle / loading / live / fault, llm / image) happens inside Full and Mini so the
  // header, selector and bottom bar stay one piece of hardware across states.
  import { onMount } from 'svelte';
  import type { SkinProps } from '../contract';
  import './instrument.css';
  import { machinedTexture } from './theme';
  import Full from './full/Full.svelte';
  import Mini from './mini/Mini.svelte';

  let { vm, actions, size }: SkinProps = $props();

  let w = $state(0);
  let h = $state(0);
  let tex = $state('');

  // Panel scale: the full window is designed at 1024 x 1152, the mini panel at 960 x 640.
  const k = $derived.by(() => {
    if (!w || !h) return 1;
    if (size === 'mini') return Math.min(w / 960, h / 640);
    return Math.max(0.6, Math.min(1.3, Math.min(w / 1000, h / 1152)));
  });

  onMount(() => {
    tex = machinedTexture();
  });
</script>

<div
  class="ins {size}"
  bind:clientWidth={w}
  bind:clientHeight={h}
  style="--k:{k}; --tex:{tex ? `url(${tex})` : 'none'}"
>
  {#if size === 'mini'}
    <Mini {vm} {actions} />
  {:else}
    <Full {vm} {actions} />
  {/if}
</div>
