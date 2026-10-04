<script lang="ts">
  // The ranked list: one row per model file and backend, a bar to scale against the leader, the value and its
  // conditions. Dense, for many records; the cards are the showcase.
  import type { RecordEntry, RecordEvent, RecordMetric } from '../../model/types';
  import { backendLabel, conditions, deltaText, fmtDate, fmtValue, freshEvent, metricMeta, share } from './metrics';

  let {
    rows,
    metric,
    leader,
    rankOf,
    events,
    now,
    selectedKey,
    onopen,
  }: {
    rows: RecordEntry[];
    metric: RecordMetric;
    /** The best value of the metric (bars are to scale against it, whatever the sort). */
    leader: number;
    /** The place of an entry by value (the list may be sorted by date). */
    rankOf: (e: RecordEntry) => number;
    events: RecordEvent[];
    now: number;
    selectedKey: string | null;
    onopen: (e: RecordEntry) => void;
  } = $props();

  const m = $derived(metricMeta(metric));
</script>

<div class="list">
  <div class="head" aria-hidden="true">
    <span>#</span><span>Model · quant · backend</span><span></span><span class="r">{m.label} ({m.unit})</span><span>Conditions</span><span>Set</span>
  </div>
  {#each rows as e (e.key)}
    {@const v = e.best[metric]!}
    {@const ev = freshEvent(events, e, metric, now)}
    <button type="button" class="row" class:lead={rankOf(e) === 1} class:sel={e.key === selectedKey} class:fresh={!!ev} onclick={() => onopen(e)}>
      <span class="rk">{rankOf(e)}</span>
      <span class="nm">
        <b>{e.model.name}</b> <em>{e.model.quant ?? e.model.file}</em>
        <span class="be">{backendLabel(e.backend)}</span>
        {#if e.node}<span class="mch">{e.machine}</span>{/if}
      </span>
      <span class="track" aria-hidden="true"><i style="width:{share(metric, v.value, leader) * 100}%"></i></span>
      <span class="val">{fmtValue(metric, v.value)}</span>
      <span class="cx">{#if ev}<span class="delta">{deltaText(metric, ev)}</span>{:else}{conditions(metric, v)}{/if}</span>
      <span class="dt">{fmtDate(v.at, now)}{v.source === 'bench' ? ' · bench' : ''}</span>
    </button>
  {/each}
</div>

<style>
  .list {
    display: grid;
    gap: 2px;
  }
  .head,
  .row {
    display: grid;
    grid-template-columns: 28px minmax(220px, 1.6fr) minmax(80px, 1fr) 92px minmax(120px, 1fr) 96px;
    align-items: center;
    gap: 12px;
    padding: 7px 10px;
  }
  .head {
    font: 600 10px var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
    border-bottom: 1px solid var(--k-line, #2c363c);
  }
  .r {
    text-align: right;
  }
  .row {
    all: unset;
    box-sizing: border-box;
    display: grid;
    grid-template-columns: 28px minmax(220px, 1.6fr) minmax(80px, 1fr) 92px minmax(120px, 1fr) 96px;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 6px;
    cursor: pointer;
  }
  .row:hover {
    background: color-mix(in srgb, var(--k-ink, #eee) 4%, transparent);
  }
  .row:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
  }
  .row.sel {
    background: color-mix(in srgb, var(--k-accent, #5ab6eb) 10%, transparent);
    box-shadow: inset 2px 0 0 var(--k-accent, #5ab6eb);
  }
  .rk {
    font: 500 12px var(--k-font-data, monospace);
    color: var(--k-muted, #8a8a8a);
  }
  .lead .rk {
    color: var(--k-record, #f2a33a);
  }
  .nm {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font: 400 13px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  .nm b {
    font-weight: 600;
  }
  .nm em {
    font-style: normal;
    color: var(--k-muted, #8a8a8a);
  }
  .be {
    margin-left: 6px;
    padding: 0 5px;
    border: 1px solid color-mix(in srgb, var(--k-accent, #5ab6eb) 55%, transparent);
    border-radius: 4px;
    font: 600 10px/15px var(--k-font-data, monospace);
    color: var(--k-accent, #5ab6eb);
  }
  .mch {
    margin-left: 6px;
    font-size: 11px;
    color: var(--k-muted, #8a8a8a);
  }
  .track {
    height: 8px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--k-ink, #eee) 5%, transparent);
    overflow: hidden;
  }
  .track i {
    display: block;
    height: 100%;
    background: repeating-linear-gradient(90deg, var(--k-accent, #5ab6eb) 0 3px, color-mix(in srgb, var(--k-accent, #5ab6eb) 60%, transparent) 3px 4px);
  }
  .lead .track i {
    background: repeating-linear-gradient(90deg, var(--k-record, #f2a33a) 0 3px, color-mix(in srgb, var(--k-record, #f2a33a) 60%, transparent) 3px 4px);
  }
  .val {
    text-align: right;
    font: 500 16px var(--k-font-data, monospace);
    color: var(--k-ink, #eee);
  }
  .fresh .val {
    color: var(--k-record, #f2a33a);
  }
  .cx,
  .dt {
    font: 11.5px var(--k-font-data, monospace);
    color: var(--k-muted, #8a8a8a);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .delta {
    color: var(--k-record, #f2a33a);
    font-weight: 600;
  }
  @container records (max-width: 760px) {
    .head,
    .row {
      grid-template-columns: 24px minmax(160px, 1fr) 84px 84px;
    }
    .head span:nth-child(3),
    .head span:nth-child(6),
    .track,
    .dt {
      display: none;
    }
  }
</style>
