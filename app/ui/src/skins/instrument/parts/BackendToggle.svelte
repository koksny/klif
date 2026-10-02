<script lang="ts">
  // Two-position HIP | VULKAN toggle showing the backend of the running (or selected) model.
  // Full size: clicking opens the tune drawer (the backend is part of the recipe). Mini: indicator only.
  import type { Backend } from '../../../lib/model/types';

  let {
    backend,
    variant = 'full',
    onclick,
  }: { backend: Backend | null; variant?: 'full' | 'mini'; onclick?: () => void } = $props();

  const pos = $derived(backend === 'HIP' ? 'left' : backend === 'Vulkan' ? 'right' : 'mid');
</script>

{#snippet track()}
  <span class="track" class:off={pos === 'mid'}>
    <span class="thumb {pos}"></span>
  </span>
{/snippet}

{#if variant === 'full'}
  <div class="tog full">
    {#if onclick}
      <button class="hit" {onclick} aria-label="Backend {backend ?? 'none'}: open tune">{@render track()}</button>
    {:else}
      <span class="hit">{@render track()}</span>
    {/if}
    <div class="names">
      <span class:on={pos === 'left'}>HIP</span>
      <span class="bar">|</span>
      <span class:on={pos === 'right'}>VULKAN</span>
      {#if backend === 'CPU'}<span class="on cpu">CPU</span>{/if}
    </div>
  </div>
{:else}
  <div class="tog mini" role="img" aria-label="Backend {backend ?? 'none'}">
    <span class="nm" class:on={pos === 'left'}>{backend === 'CPU' ? 'CPU' : 'HIP'}</span>
    {@render track()}
    <span class="bar"></span>
    <span class="nm" class:on={pos === 'right'}>VULKAN</span>
  </div>
{/if}

<style>
  .tog {
    display: flex;
    align-items: center;
  }
  .full {
    --tw: 62;
    --th: 32;
    flex-direction: column;
    gap: calc(9 * var(--u));
  }
  .full .thumb {
    top: calc(4 * var(--u));
    left: calc(4 * var(--u));
    width: calc((var(--th) - 8) * var(--u));
    height: calc((var(--th) - 8) * var(--u));
  }
  .full .track {
    box-shadow:
      inset 0 2px 5px rgba(0, 0, 0, 0.85),
      0 0 0 calc(2.5 * var(--u)) #2a2b2e,
      0 0 0 calc(3.5 * var(--u)) #0b0c0d;
  }
  .hit {
    display: block;
    background: none;
    border: 0;
    padding: 0;
    margin: 0;
    cursor: pointer;
    border-radius: 999px;
  }
  span.hit {
    cursor: default;
  }
  .track {
    position: relative;
    display: block;
    width: calc(var(--tw, 86) * var(--u));
    height: calc(var(--th, 44) * var(--u));
    border-radius: 999px;
    background: linear-gradient(180deg, #0b0c0d, #1a1b1d);
    box-shadow:
      inset 0 2px 5px rgba(0, 0, 0, 0.85),
      0 0 0 calc(3 * var(--u)) #2a2b2e,
      0 0 0 calc(4 * var(--u)) #0b0c0d;
  }
  .thumb {
    position: absolute;
    top: calc(5 * var(--u));
    left: calc(5 * var(--u));
    width: calc((var(--th, 44) - 10) * var(--u));
    height: calc((var(--th, 44) - 10) * var(--u));
    border-radius: 50%;
    background: radial-gradient(circle at 45% 40%, #bfe6fb 0%, #5ab6eb 48%, #3d9ed4 100%);
    box-shadow: 0 0 calc(12 * var(--u)) rgba(90, 182, 235, 0.55);
    transition: transform 300ms cubic-bezier(0.3, 0.8, 0.3, 1);
  }
  .thumb.right {
    transform: translateX(calc((var(--tw, 86) - var(--th, 44)) * var(--u)));
  }
  .thumb.mid {
    transform: translateX(calc((var(--tw, 86) - var(--th, 44)) * 0.5 * var(--u)));
    background: radial-gradient(circle at 45% 40%, #55575c 0%, #34363a 100%);
    box-shadow: none;
  }
  .names {
    display: flex;
    gap: calc(8 * var(--u));
    font-family: var(--font-label);
    font-weight: 600;
    font-size: var(--fs-lbl);
    letter-spacing: 0.1em;
    line-height: 1;
    color: rgba(237, 230, 214, 0.7);
  }
  .names .bar {
    color: rgba(237, 230, 214, 0.35);
    font-weight: 400;
  }
  .names .on {
    color: var(--cyan);
  }
  .mini {
    --tw: 84;
    --th: 44;
    gap: calc(16 * var(--u));
  }
  .mini .nm {
    font-weight: 600;
    font-size: calc(31 * var(--u));
    letter-spacing: 0.04em;
    color: rgba(237, 230, 214, 0.55);
    line-height: 1;
  }
  .mini .nm.on {
    color: var(--cream);
  }
  .mini .bar {
    width: 2px;
    height: calc(40 * var(--u));
    background: rgba(237, 230, 214, 0.28);
    margin: 0 calc(6 * var(--u));
  }
</style>
