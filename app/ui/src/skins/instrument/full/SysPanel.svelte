<script lang="ts">
  import type { SystemStats } from '../../../lib/model/types';
  import Icon from '../parts/Icon.svelte';

  let { system }: { system: SystemStats } = $props();
</script>

<section class="panel sys">
  <div class="cell">
    <span class="lbl">RAM</span>
    <span class="val"><Icon name="ram" size="calc(38 * var(--u))" />{system.ramUsedGiB.toFixed(1)} / {system.ramTotalGiB.toFixed(1)} GiB</span>
  </div>
  <span class="vsep"></span>
  <div class="cell">
    <span class="lbl">CPU</span>
    <span class="val"><Icon name="cpu" size="calc(38 * var(--u))" />{system.cpuName} · {Math.round(system.cpuPct)}%</span>
  </div>
</section>

<style>
  .sys {
    flex: 0 0 calc(104 * var(--u));
    display: flex;
    align-items: stretch;
    padding: calc(20 * var(--u)) calc(26 * var(--u)) calc(16 * var(--u));
    gap: calc(30 * var(--u));
  }
  .cell {
    flex: 1 1 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: calc(14 * var(--u));
  }
  .sys .vsep {
    margin: calc(2 * var(--u)) 0;
  }
  .val {
    display: flex;
    align-items: center;
    gap: calc(18 * var(--u));
    font-size: calc(21 * var(--u));
    letter-spacing: 0.035em;
    white-space: nowrap;
    color: var(--cream);
  }
  .val :global(.ic) {
    color: rgba(237, 230, 214, 0.85);
  }
</style>
