<script lang="ts">
  // Load steps as a column of indicator lamps: done = lit cyan, active = lit cyan with a cyan label,
  // pending = dark ring, failed = lit orange. The active weights step carries its own LED bar, whose
  // fraction the caller derives from resident weights GiB over the weights layer the slot expects.
  import type { LoadStep } from '../../../lib/model/types';
  import LedBar from './LedBar.svelte';

  let {
    steps,
    weightsFrac = null,
  }: { steps: LoadStep[]; weightsFrac?: number | null } = $props();

  const WORD: Record<LoadStep['state'], string> = { done: 'done', active: 'running', pending: 'pending', failed: 'failed' };
</script>

<ul class="steps">
  {#each steps as st (st.id)}
    {@const bar = st.id === 'weights' && st.state === 'active' && weightsFrac !== null}
    <li class="st {st.state}" class:withbar={bar}>
      <span class="lamp"></span>
      <span class="lab">{st.label}</span>
      {#if st.detail}<span class="det">{st.detail}</span>{/if}
      <span class="word">{bar ? `${Math.round((weightsFrac ?? 0) * 100)}%` : WORD[st.state]}</span>
      {#if bar}
        <span class="bar"><LedBar fraction={weightsFrac ?? 0} segments={17} label="Weights loaded" /></span>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .st {
    flex: 1 1 0;
    min-height: 0;
    display: grid;
    grid-template-columns: calc(30 * var(--u)) auto minmax(0, 1fr) auto;
    grid-template-rows: 1fr;
    align-items: center;
    column-gap: calc(12 * var(--u));
    border-bottom: 1px solid rgba(237, 230, 214, 0.08);
    font-size: calc(18.5 * var(--u));
    letter-spacing: 0.03em;
    color: rgba(237, 230, 214, 0.55);
    white-space: nowrap;
  }
  .st:last-child {
    border-bottom: 0;
  }
  .st.withbar {
    flex-grow: 1.9;
    grid-template-rows: 1fr 1fr;
  }
  .lamp {
    justify-self: center;
    width: calc(15 * var(--u));
    height: calc(15 * var(--u));
    border-radius: 50%;
    background: #141517;
    box-shadow:
      inset 0 0 0 calc(2 * var(--u)) rgba(237, 230, 214, 0.5),
      0 0 0 calc(1.5 * var(--u)) #0b0c0d;
  }
  .done .lamp,
  .active .lamp {
    background: radial-gradient(circle at 45% 40%, #bfe6fb 0%, #5ab6eb 50%, #3d9ed4 100%);
    box-shadow:
      0 0 0 calc(1.5 * var(--u)) #0b0c0d,
      0 0 calc(8 * var(--u)) rgba(90, 182, 235, 0.6);
  }
  .failed .lamp {
    background: radial-gradient(circle at 45% 40%, #ffd2bd 0%, #ff6b2c 50%, #d84e14 100%);
    box-shadow:
      0 0 0 calc(1.5 * var(--u)) #0b0c0d,
      0 0 calc(8 * var(--u)) rgba(255, 107, 44, 0.6);
  }
  .lab {
    color: inherit;
  }
  .done {
    color: var(--cream);
  }
  .active {
    color: var(--cyan);
  }
  .failed {
    color: var(--orange);
  }
  .det {
    color: rgba(237, 230, 214, 0.78);
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .pending .det {
    color: rgba(237, 230, 214, 0.45);
  }
  .word {
    font-size: calc(15.5 * var(--u));
    color: rgba(237, 230, 214, 0.6);
    text-align: right;
  }
  .active .word {
    color: var(--cream);
    font-size: calc(18.5 * var(--u));
  }
  .failed .word {
    color: var(--orange);
  }
  .withbar .lamp,
  .withbar .lab,
  .withbar .det {
    grid-row: 1;
  }
  .withbar .word {
    grid-row: 2;
    grid-column: 4;
  }
  .bar {
    grid-row: 2;
    grid-column: 2 / 4;
    height: calc(15 * var(--u));
    --seg-gap: calc(3 * var(--u));
    --seg-glow: calc(4 * var(--u));
  }
</style>
