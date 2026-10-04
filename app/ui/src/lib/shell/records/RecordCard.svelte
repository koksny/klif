<script lang="ts">
  // One model file on one backend: the active metric's best as the hero number with its own conditions and date,
  // a bar to scale against the leader, the entry's other records small. The leader and a fresh record stand out.
  import type { RecordEntry, RecordEvent, RecordMetric } from '../../model/types';
  import { backendLabel, conditions, deltaText, fmtDate, fmtValue, metricMeta, METRICS, share } from './metrics';

  let {
    entry,
    metric,
    rank,
    leader,
    event,
    now,
    selected = false,
    onopen,
  }: {
    entry: RecordEntry;
    metric: RecordMetric;
    rank: number;
    leader: number;
    event?: RecordEvent;
    now: number;
    selected?: boolean;
    onopen: () => void;
  } = $props();

  const m = $derived(metricMeta(metric));
  const v = $derived(entry.best[metric]!);
  const others = $derived(METRICS.filter((x) => x.id !== metric && entry.best[x.id]));
</script>

<button type="button" class="card" class:lead={rank === 1} class:fresh={!!event} class:sel={selected} onclick={onopen} title="Details of this record">
  <header>
    <span class="rk" aria-label="Rank {rank}">{rank}</span>
    <span class="be">{backendLabel(entry.backend)}</span>
    {#if entry.node}<span class="mch">{entry.machine}</span>{/if}
    {#if event}<span class="new">New record</span>{/if}
  </header>
  <div class="body">
    <div class="mn" title={entry.model.file}>{entry.model.name}</div>
    <div class="mq">{entry.model.quant ?? entry.model.file}</div>
    <div class="hv"><b>{fmtValue(metric, v.value)}</b><span>{m.unit}</span></div>
    <div class="bar" aria-hidden="true"><i style="width:{share(metric, v.value, leader) * 100}%"></i></div>
    <div class="cond">{conditions(metric, v) || ' '}</div>
    {#if event}<div class="delta">{deltaText(metric, event)}</div>{/if}
    {#if others.length}
      <div class="oth">
        {#each others as o}
          <span><em>{o.label}</em> {fmtValue(o.id, entry.best[o.id]!.value)} <small>{o.unit}</small></span>
        {/each}
      </div>
    {/if}
    <footer><span>{fmtDate(v.at, now)}{v.source === 'bench' ? ' · bench' : ''}</span><span>{v.gpus.join(' + ')}</span></footer>
  </div>
</button>

<style>
  .card {
    all: unset;
    box-sizing: border-box;
    position: relative;
    display: grid;
    grid-template-rows: auto 1fr;
    min-width: 0;
    background: linear-gradient(180deg, var(--k-surface, #1d252a), color-mix(in srgb, var(--k-surface, #1d252a) 70%, var(--k-bg, #0f1316)));
    border: 1px solid var(--k-line, #2c363c);
    border-radius: var(--k-radius, 8px);
    overflow: hidden;
    cursor: pointer;
    text-align: left;
    transition:
      border-color 0.15s ease,
      transform 0.15s ease,
      box-shadow 0.15s ease;
  }
  .card:hover {
    border-color: color-mix(in srgb, var(--k-accent, #5ab6eb) 55%, var(--k-line, #2c363c));
  }
  .card:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
    outline-offset: 2px;
  }
  .card.sel {
    border-color: var(--k-accent, #5ab6eb);
  }
  .card.lead {
    border-color: color-mix(in srgb, var(--k-record, #f2a33a) 65%, var(--k-line, #2c363c));
    box-shadow: 0 8px 34px color-mix(in srgb, var(--k-record, #f2a33a) 12%, transparent);
  }
  .card.fresh {
    border-color: var(--k-record, #f2a33a);
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--k-record, #f2a33a) 35%, transparent),
      0 10px 40px color-mix(in srgb, var(--k-record, #f2a33a) 16%, transparent);
    transform: translateY(-2px);
  }
  header {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 32px;
    padding: 0 10px;
    border-bottom: 1px solid var(--k-line, #2c363c);
    background: linear-gradient(90deg, color-mix(in srgb, var(--k-accent, #5ab6eb) 16%, transparent), transparent 70%);
  }
  .lead header {
    background: linear-gradient(90deg, color-mix(in srgb, var(--k-record, #f2a33a) 20%, transparent), transparent 70%);
  }
  .rk {
    display: inline-grid;
    place-items: center;
    min-width: 20px;
    height: 20px;
    padding: 0 3px;
    box-sizing: border-box;
    border: 1px solid var(--k-line, #2c363c);
    border-radius: 999px;
    font: 600 11px/1 var(--k-font-data, monospace);
    color: var(--k-muted, #8a8a8a);
  }
  .lead .rk {
    border-color: var(--k-record, #f2a33a);
    color: var(--k-record, #f2a33a);
    box-shadow: 0 0 10px color-mix(in srgb, var(--k-record, #f2a33a) 40%, transparent);
  }
  .be {
    padding: 0 6px;
    border: 1px solid color-mix(in srgb, var(--k-accent, #5ab6eb) 55%, transparent);
    border-radius: 4px;
    font: 600 10.5px/16px var(--k-font-data, monospace);
    color: var(--k-accent, #5ab6eb);
  }
  .mch {
    font: 500 11px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .new {
    margin-left: auto;
    padding: 2px 8px;
    border-radius: 3px;
    background: var(--k-record, #f2a33a);
    color: var(--k-bg, #0f1316);
    font: 700 9.5px/1.4 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.14em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  .body {
    display: grid;
    align-content: start;
    gap: 2px;
    padding: 10px 14px 10px;
    min-width: 0;
  }
  .mn {
    font: 600 14.5px/1.3 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mq {
    font: 400 12px/1.3 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hv {
    display: flex;
    align-items: baseline;
    gap: 6px;
    margin-top: 6px;
  }
  .hv b {
    font: 500 38px/1 var(--k-font-data, monospace);
    letter-spacing: -0.02em;
    color: var(--k-ink, #eee);
  }
  .lead .hv b,
  .fresh .hv b {
    color: color-mix(in srgb, var(--k-record, #f2a33a) 40%, var(--k-ink, #eee));
  }
  .hv span {
    font: 400 12px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .bar {
    height: 3px;
    margin: 6px 0 4px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--k-ink, #eee) 6%, transparent);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--k-accent, #5ab6eb);
  }
  .lead .bar i {
    background: var(--k-record, #f2a33a);
  }
  .cond {
    font: 11.5px/1.35 var(--k-font-data, monospace);
    color: var(--k-muted, #8a8a8a);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .delta {
    font: 600 11.5px/1.35 var(--k-font-data, monospace);
    color: var(--k-record, #f2a33a);
  }
  .oth {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 14px;
    margin-top: 6px;
    font: 12px var(--k-font-data, monospace);
    color: var(--k-ink, #eee);
    opacity: 0.85;
  }
  .oth em {
    font: 600 9.5px var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.12em;
    text-transform: uppercase;
    font-style: normal;
    color: var(--k-muted, #8a8a8a);
  }
  .oth small {
    color: var(--k-muted, #8a8a8a);
  }
  footer {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    margin-top: 8px;
    font: 11px var(--k-font-data, monospace);
    color: var(--k-muted, #8a8a8a);
  }
  footer span:last-child {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  @media (prefers-reduced-motion: reduce) {
    .card {
      transition: none;
    }
    .card.fresh {
      transform: none;
    }
  }
</style>
