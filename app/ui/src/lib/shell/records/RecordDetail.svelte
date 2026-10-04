<script lang="ts">
  // One record in full: the exact file and source, the backend build, the conditions of every metric it holds,
  // how it climbed, and the card export (clipboard or a PNG in Pictures\KLIF). Forget removes a junk entry.
  import type { HardwareInfo, RecordEntry, RecordEvent, RecordMetric } from '../../model/types';
  import { paintCard, cardBlob, cardFileName, copyCard, type CardData } from './card';
  import Climb from './Climb.svelte';
  import { backendLabel, conditions, deltaText, fmtDate, fmtValue, metricMeta, METRICS } from './metrics';

  let {
    entry,
    metric,
    now,
    hw,
    fresh,
    version,
    loadClimb,
    save,
    forget,
    toast,
    onclose,
  }: {
    entry: RecordEntry;
    metric: RecordMetric;
    now: number;
    hw: HardwareInfo | undefined;
    fresh?: RecordEvent;
    version: string;
    loadClimb: (key: string, metric: RecordMetric) => Promise<{ at: number; value: number }[]>;
    save: (name: string, blob: Blob) => Promise<string | null>;
    forget: (() => Promise<void>) | null;
    toast: (text: string) => void;
    onclose: () => void;
  } = $props();

  const m = $derived(metricMeta(metric));
  const v = $derived(entry.best[metric]!);
  const held = $derived(METRICS.filter((x) => entry.best[x.id]));
  let climb = $state<{ at: number; value: number }[]>([]);
  let armed = $state(false);
  let busy = $state(false);

  // The view model is replaced at 2 Hz: depend on primitives, so the history is fetched again only when this
  // record (key, metric) or its best value changes, and Forget stays armed between two clicks.
  const key = $derived(entry.key);
  const bestAt = $derived(entry.best[metric]?.at);
  const bestVal = $derived(entry.best[metric]?.value);
  $effect(() => {
    const k = key;
    const mt = metric;
    climb = bestAt !== undefined && bestVal !== undefined ? [{ at: bestAt, value: bestVal }] : [];
    let live = true;
    loadClimb(k, mt)
      .then((pts) => {
        if (live && pts.length) climb = pts;
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  });
  $effect(() => {
    void key;
    void metric;
    armed = false;
  });

  function data(): CardData {
    return { entry, metric, hw, machine: entry.machine, now, version, fresh, climb };
  }

  async function copy() {
    busy = true;
    try {
      const ok = await copyCard(await cardBlob(await paintCard(data())));
      toast(ok ? 'Card copied: paste it anywhere' : 'The clipboard is not available here: save the PNG instead');
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    } finally {
      busy = false;
    }
  }

  async function savePng() {
    busy = true;
    try {
      const d = data();
      const path = await save(cardFileName(d), await cardBlob(await paintCard(d)));
      toast(path ? `Card saved: ${path}` : 'Card downloaded');
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    } finally {
      busy = false;
    }
  }

  async function doForget() {
    if (!forget) return;
    if (!armed) {
      armed = true;
      return;
    }
    busy = true;
    try {
      await forget();
      onclose();
    } catch {
      // The player has already said why (a toast); stay open and disarmed.
      armed = false;
    } finally {
      busy = false;
    }
  }

  const short = (sha?: string) => (sha ? `${sha.slice(0, 12)}…` : 'not hashed yet');
</script>

<aside class="detail" aria-label="Record details">
  <header>
    <span class="lbl">{m.long} record</span>
    <button type="button" class="x" onclick={onclose} aria-label="Close the details">✕</button>
  </header>
  <div class="hv"><b>{fmtValue(metric, v.value)}</b><span>{m.unit}</span></div>
  {#if fresh}<div class="delta">{deltaText(metric, fresh)}</div>{/if}
  <div class="mn">{entry.model.name}</div>
  <div class="mq">{entry.model.quant ?? ''} <span class="be">{backendLabel(entry.backend)}</span></div>

  <dl>
    <dt>File</dt><dd class="mono">{entry.model.file}</dd>
    <dt>SHA-256</dt><dd class="mono" title={entry.model.sha256 ?? ''}>{short(entry.model.sha256)}</dd>
    {#if entry.model.source}<dt>Source</dt><dd class="mono">{entry.model.source}</dd>{/if}
    <dt>Backend</dt><dd>{backendLabel(entry.backend)}{v.backendBuild ? ` · ${v.backendBuild}` : ''}</dd>
    <dt>GPU</dt><dd>{v.gpus.join(' + ') || '—'}</dd>
    {#if v.ctx}<dt>Context</dt><dd>{v.ctx.toLocaleString('en-US')}{v.kv ? ` · KV ${v.kv}` : ''}</dd>{/if}
    {#if v.promptTokens}<dt>Prompt</dt><dd>{v.promptTokens.toLocaleString('en-US')} tokens{v.cachedTokens ? `, ${v.cachedTokens.toLocaleString('en-US')} from cache` : ''}</dd>{/if}
    {#if v.genTokens}<dt>Generated</dt><dd>{v.genTokens.toLocaleString('en-US')} tokens</dd>{/if}
    {#if v.width && v.height}<dt>Output</dt><dd>{v.width}x{v.height}{v.frames ? ` · ${v.frames} frames` : ''}{v.steps ? ` · ${v.steps} steps` : ''}</dd>{/if}
    <dt>Set</dt><dd>{fmtDate(v.at, now)} · {v.source === 'bench' ? 'klif-cli bench' : 'everyday use'}</dd>
    <dt>Machine</dt><dd>{entry.machine}</dd>
    {#if v.preset}<dt>Preset</dt><dd class="mono">{v.preset}</dd>{/if}
  </dl>

  {#if held.length > 1}
    <div class="all">
      <span class="lbl">Every record of this file</span>
      {#each held as h}
        {@const hv = entry.best[h.id]!}
        <div class="rec" class:on={h.id === metric}>
          <span>{h.label}</span><b>{fmtValue(h.id, hv.value)} <small>{h.unit}</small></b>
          <em>{conditions(h.id, hv) || ' '} · {fmtDate(hv.at, now)}</em>
        </div>
      {/each}
    </div>
  {/if}

  <div class="climb">
    <span class="lbl">Climb</span>
    <Climb points={climb} {metric} {now} />
  </div>

  <div class="acts">
    <button type="button" class="primary" onclick={copy} disabled={busy}>Copy card</button>
    <button type="button" onclick={savePng} disabled={busy}>Save PNG</button>
    {#if forget}
      <button type="button" class="forget" class:armed onclick={doForget} disabled={busy} title="Remove this entry and its history">
        {armed ? 'Forget it' : 'Forget'}
      </button>
    {/if}
  </div>
</aside>

<style>
  .detail {
    display: grid;
    align-content: start;
    gap: 10px;
    padding: 16px 18px 18px;
    border-left: 1px solid var(--k-line, #2c363c);
    background: color-mix(in srgb, var(--k-surface, #1d252a) 92%, transparent);
    overflow-y: auto;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .lbl {
    font: 600 10px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .x {
    all: unset;
    cursor: pointer;
    padding: 2px 6px;
    color: var(--k-muted, #8a8a8a);
    font-size: 14px;
  }
  .x:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
  }
  .hv b {
    font: 500 48px/1 var(--k-font-data, monospace);
    color: var(--k-ink, #eee);
  }
  .hv span {
    margin-left: 6px;
    font: 13px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .delta {
    font: 600 12px var(--k-font-data, monospace);
    color: var(--k-record, #f2a33a);
  }
  .mn {
    font: 600 16px/1.3 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  .mq {
    font: 13px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .be {
    margin-left: 4px;
    padding: 0 6px;
    border: 1px solid color-mix(in srgb, var(--k-accent, #5ab6eb) 55%, transparent);
    border-radius: 4px;
    font: 600 10.5px/16px var(--k-font-data, monospace);
    color: var(--k-accent, #5ab6eb);
  }
  dl {
    display: grid;
    grid-template-columns: 76px minmax(0, 1fr);
    gap: 5px 10px;
    margin: 4px 0 0;
    font: 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  dt {
    color: var(--k-muted, #8a8a8a);
  }
  dd {
    margin: 0;
    color: var(--k-ink, #eee);
    overflow-wrap: anywhere;
  }
  .mono {
    font: 11px/1.4 var(--k-font-data, monospace);
  }
  .all {
    display: grid;
    gap: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--k-line, #2c363c);
  }
  .rec {
    display: grid;
    grid-template-columns: 1fr auto;
    font: 12px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .rec b {
    font: 500 13px var(--k-font-data, monospace);
    color: var(--k-ink, #eee);
  }
  .rec small {
    color: var(--k-muted, #8a8a8a);
  }
  .rec em {
    grid-column: 1 / 3;
    font: 10.5px var(--k-font-data, monospace);
    font-style: normal;
  }
  .rec.on span {
    color: var(--k-accent, #5ab6eb);
  }
  .climb {
    display: grid;
    gap: 6px;
    padding-top: 8px;
    border-top: 1px solid var(--k-line, #2c363c);
  }
  .acts {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-top: 6px;
  }
  .acts button {
    padding: 7px 12px;
    border: 1px solid var(--k-line, #2c363c);
    border-radius: var(--k-radius, 6px);
    background: transparent;
    color: var(--k-ink, #eee);
    font: 600 12px var(--k-font-ui, system-ui, sans-serif);
    cursor: pointer;
  }
  .acts button:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
    outline-offset: 1px;
  }
  .acts .primary {
    background: var(--k-accent, #5ab6eb);
    border-color: var(--k-accent, #5ab6eb);
    color: var(--k-accent-ink, #0f1316);
  }
  .acts .forget {
    margin-left: auto;
    color: var(--k-muted, #8a8a8a);
  }
  .acts .forget.armed {
    border-color: var(--k-danger, #e05a5a);
    color: var(--k-danger, #e05a5a);
  }
  .acts button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
