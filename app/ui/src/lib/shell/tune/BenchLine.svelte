<script lang="ts">
  // The latest bench of the active preset, or how to run one (klif-cli bench).
  import { fmtTps } from '../../model/format';
  import { joinParts } from '../../model/systems';
  import type { System } from '../../model/types';
  import { fmtDate } from './util';

  interface Props {
    system: System;
  }
  let { system }: Props = $props();

  const b = $derived(system.bench);
  const line = $derived(
    b
      ? joinParts([
          b.decodeTps !== undefined ? `${fmtTps(b.decodeTps)} tok/s decode` : '',
          b.prefillTps !== undefined ? `${fmtTps(b.prefillTps)} tok/s prefill` : '',
          b.ttftS !== undefined ? `TTFT ${b.ttftS.toFixed(2)} s` : '',
          b.secondsPerImage !== undefined ? `${b.secondsPerImage.toFixed(1)} s/image` : '',
          b.loadS !== undefined ? `load ${b.loadS.toFixed(1)} s` : '',
          b.peakVramGiB !== undefined ? `peak ${b.peakVramGiB.toFixed(2)} GiB` : '',
          b.spillMiB ? `spill ${Math.round(b.spillMiB)} MiB` : '',
        ])
      : '',
  );
</script>

<div class="field bench">
  <span>Bench</span>
  {#if b}
    <span class="ro">
      {line || 'no numbers'}
      <small>{fmtDate(b.at)} · {b.runs} run{b.runs === 1 ? '' : 's'}{b.stale ? ' · the command changed since' : ''}</small>
    </span>
  {:else if system.node}
    <span class="ro"><small>None yet. Run klif-cli bench on that machine.</small></span>
  {:else if system.preset}
    <span class="ro"><small>None yet: </small><code>klif-cli bench {system.id}</code></span>
  {:else}
    <span class="ro"><small>Set a preset first.</small></span>
  {/if}
</div>

<style>
  .bench .ro {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 8px;
    line-height: 1.4;
  }
  code {
    font: 500 12px/1.2 var(--k-font-data, ui-monospace, monospace);
    color: var(--k-ink, #e6e6e6);
    background: var(--k-surface, #141414);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: 4px;
    padding: 2px 6px;
  }
</style>
