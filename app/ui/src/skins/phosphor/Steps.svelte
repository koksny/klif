<script lang="ts">
  // Load steps as a checklist: done (filled check), active (amber ring, spinning only while the
  // scheduler tier allows CSS motion), pending (empty ring), failed (red cross). The weights step can
  // carry a progress bar; its fraction is supplied by the caller from real bytes.
  // layout: 'compact' = three rows flowing into columns (the fault panel), 'grid' = two rows of three
  // cells with rules between them (the status block's detail rows while loading).
  import type { LoadStep } from '../../lib/model/types';

  let {
    steps,
    weightsFrac = null,
    layout = 'compact',
  }: { steps: LoadStep[]; weightsFrac?: number | null; layout?: 'compact' | 'grid' } = $props();
</script>

<ol class="steps {layout}">
  {#each steps as st (st.id)}
    <li class={st.state}>
      <svg class="ic" class:spin={st.state === 'active'} viewBox="0 0 24 24" aria-label={st.state}>
        {#if st.state === 'done'}
          <circle cx="12" cy="12" r="10" class="c-done" />
          <path d="M7 12.4L10.4 15.6L17 8.6" class="ck" />
        {:else if st.state === 'active'}
          <circle cx="12" cy="12" r="9" class="c-ring" />
          <path d="M12 3A9 9 0 0 1 20.6 14.6" class="arc" />
        {:else if st.state === 'failed'}
          <circle cx="12" cy="12" r="10" class="c-fail" />
          <path d="M8 8L16 16M16 8L8 16" class="ck" />
        {:else}
          <circle cx="12" cy="12" r="9" class="c-pend" />
        {/if}
      </svg>
      <span class="txt">
        <span class="nm">{st.label}</span>{#if st.detail}<span class="dt">{st.detail}</span>{/if}
      </span>
      {#if st.id === 'weights' && st.state === 'active' && weightsFrac !== null}
        <span class="bar" role="img" aria-label="Weights loaded {Math.round(weightsFrac * 100)}%">
          <span class="fill" style="transform:scaleX({weightsFrac.toFixed(4)})"></span>
          <span class="hw" style="transform:translateX({(weightsFrac * 100).toFixed(2)}%)"><span class="head"></span></span>
        </span>
      {/if}
    </li>
  {/each}
</ol>

<style>
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    min-width: 0;
    font-size: var(--ph-fs-m);
  }
  .compact {
    align-content: start;
    grid-template-rows: repeat(3, auto);
    grid-auto-flow: column;
    grid-auto-columns: minmax(0, 1fr);
    gap: calc(7px * var(--k)) calc(22px * var(--k));
  }
  .grid {
    height: 100%;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    grid-template-rows: repeat(2, minmax(0, 1fr));
  }
  li {
    position: relative;
    display: grid;
    grid-template-columns: calc(16px * var(--k)) minmax(0, 1fr);
    align-items: center;
    column-gap: calc(10px * var(--k));
    color: var(--ph-muted);
    min-width: 0;
  }
  .grid li {
    padding: 0 calc(12px * var(--k));
    border-right: 1px solid var(--ph-grat);
  }
  .grid li:nth-child(3n) {
    border-right: 0;
  }
  .grid li:nth-child(n + 4) {
    border-top: 1px solid var(--ph-grat);
  }
  li.done {
    color: var(--ph-cyan);
  }
  li.active {
    color: #f3d9a4;
  }
  li.failed {
    color: #ff8f88;
  }
  .ic {
    width: calc(16px * var(--k));
    height: calc(16px * var(--k));
    min-width: 13px;
    min-height: 13px;
    overflow: visible;
  }
  .ic circle,
  .ic path {
    vector-effect: non-scaling-stroke;
  }
  .c-done {
    fill: var(--ph-cyan);
    filter: drop-shadow(0 0 3px rgba(127, 227, 255, 0.6));
  }
  .c-fail {
    fill: var(--ph-danger);
    filter: drop-shadow(0 0 3px rgba(229, 97, 92, 0.7));
  }
  .ck {
    fill: none;
    stroke: var(--ph-glass);
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .c-ring {
    fill: none;
    stroke: rgba(232, 176, 74, 0.3);
    stroke-width: 2;
  }
  .arc {
    fill: none;
    stroke: var(--ph-amber);
    stroke-width: 2.2;
    stroke-linecap: round;
  }
  /* the whole icon box turns (a compositor transform); the ring under the arc is symmetric */
  .spin {
    transform-origin: 50% 50%;
    animation: ph-spin 1.1s linear infinite;
  }
  @keyframes ph-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
  .c-pend {
    fill: none;
    stroke: var(--ph-muted);
    stroke-width: 1.5;
    opacity: 0.8;
  }
  .txt {
    display: flex;
    align-items: baseline;
    gap: calc(8px * var(--k));
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
  }
  .nm {
    flex: none;
  }
  .done .nm,
  .active .nm {
    text-shadow: var(--ph-glow-soft);
  }
  .dt {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--ph-muted);
  }
  .active .dt {
    color: #d9c49a;
  }
  .bar {
    grid-column: 2;
    position: relative;
    height: calc(5px * var(--k));
    margin-top: calc(4px * var(--k));
    border: 1px solid #2a7f93;
    border-radius: 2px;
    overflow: hidden;
    background: rgba(3, 9, 12, 0.7);
  }
  /* grid cell: the weights bar runs along the bottom edge of its cell */
  .grid .bar {
    position: absolute;
    left: calc(38px * var(--k));
    right: calc(12px * var(--k));
    bottom: calc(5px * var(--k));
    margin: 0;
  }
  .fill {
    position: absolute;
    inset: 0;
    transform-origin: left center;
    background: linear-gradient(90deg, rgba(127, 227, 255, 0.55), var(--ph-cyan));
    box-shadow: 0 0 10px rgba(127, 227, 255, 0.5);
    transition: transform 0.5s ease-out;
  }
  .hw {
    position: absolute;
    inset: 0;
    transition: transform 0.5s ease-out;
  }
  .head {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -2px;
    width: 3px;
    background: var(--ph-hot);
    box-shadow: 0 0 8px var(--ph-hot);
  }
</style>
