<script lang="ts">
  // LLM status block: the hero (decode speed with its 5-minute swell; the prefill progress while the prompt
  // is processed, never "0.0 tok/s") over two fixed rows: PREFILL / DECODE and CONTEXT / SPECULATIVE.
  // With nothing to measure (idle, stopping, no data yet) the same boxes stay, dimmed: the hero carries a
  // word and the rows show the configured facts of the tier (context window, speculative mode).
  import type { LlmLive, ModelRef } from '../../../lib/model/types';
  import { fmtGiB, fmtInt, fmtPct, fmtSeconds, fmtTps } from '../../../lib/model/format';
  import type { GpuView } from '../power';
  import Swell from '../Swell.svelte';
  import { fmtEta } from '../util';

  let {
    llm,
    model,
    ctxTotal,
    word = null,
    wordAmber = false,
    gpu = null,
  }: {
    llm: LlmLive | null;
    model: ModelRef | null;
    /** Context window in tokens: the live one, or the configured one. */
    ctxTotal: number;
    /** Set when there is nothing to measure: the hero is dimmed with this word over it. */
    word?: string | null;
    wordAmber?: boolean;
    /** The GPU is dormant (asleep, waking, or paging out). */
    gpu?: GpuView | null;
  } = $props();

  const SEGS = 16;
  const live = $derived(!!llm && word === null);
  /** The prefill hero: only while the prompt is being processed. */
  const pf = $derived(live && llm && llm.activity === 'prefill' && llm.prefill && llm.prefill.tokens > 0 ? llm.prefill : null);
  const pfFrac = $derived(pf ? Math.min(1, pf.doneTokens / pf.tokens) : 0);
  /** A request waiting for the restore: nothing has been prefilled yet. */
  const waitGpu = $derived(!!gpu && !!pf && pf.doneTokens === 0);
  /** GPU asleep with nothing in flight: the figure is the last request's, not a reading. */
  const stale = $derived(!!gpu && !!llm && llm.activity === 'idle');
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? Math.min(1, llm.context.usedTokens / llm.context.totalTokens) : 0);
  const specOn = $derived(llm?.spec ? Math.round((llm.spec.acceptancePct / 100) * SEGS) : 0);
</script>

<!-- Hero -->
{#if !live || !llm}
  <section class="c-hero c-panel">
    <div class="c-hl">
      <span class="c-lbl">Decode speed</span>
      <div class="c-fig dim"><b>—</b><span class="u">tok/s</span></div>
    </div>
    <div class="c-hr one"><div class="c-wait" class:amb={wordAmber}><span>{word ?? 'waiting for data'}</span></div></div>
  </section>
{:else if pf}
  <section class="c-hero c-panel">
    <div class="c-hl" class:amb={waitGpu}>
      <span class="c-lbl">Prefill progress</span>
      <div class="c-fig" class:faded={waitGpu}><b>{Math.floor(pfFrac * 100)}</b><span class="u">%</span></div>
    </div>
    <div class="c-hr">
      {#if waitGpu && gpu}
        <div class="c-hline"><span class="amb">waiting for the GPU to wake</span><span class="end">{fmtGiB(gpu.pagedOutGiB)} GiB still in system RAM</span></div>
      {:else}
        <div class="c-hline">
          <span><b>{fmtTps(pf.tps)}</b> tok/s</span><span><b>{fmtEta(pf.etaS)}</b> left</span><span class="end">{fmtSeconds(pf.elapsedS)} elapsed</span>
        </div>
      {/if}
      <div class="c-bar tall" role="meter" aria-label="Prefill progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(pfFrac * 100)}>
        <i style:transform="scaleX({pfFrac})"></i>
      </div>
      <div class="c-hline sm">
        <span>prompt processing · {fmtInt(pf.doneTokens)} / {fmtInt(pf.tokens)} tok{pf.cachedTokens ? ` · ${fmtInt(pf.cachedTokens)} from the prompt cache` : ''}</span>
      </div>
    </div>
  </section>
{:else}
  <section class="c-hero c-panel">
    <div class="c-hl">
      <span class="c-lbl">{llm.activity === 'decode' ? 'Decode speed' : 'Last decode speed'}</span>
      <div class="c-fig" class:faded={stale}><b>{fmtTps(llm.decodeTps)}</b><span class="u">tok/s</span></div>
    </div>
    <div class="c-hr one sw" class:faded={stale}><Swell history={llm.decodeHistory} /></div>
  </section>
{/if}

<!-- Rows -->
<section class="c-rows c-panel" class:dim={!live}>
  <div class="c-row">
    <span class="c-lbl">Prefill</span>
    <span class="c-v">
      {#if !llm?.prefill}
        <span class="t q">{llm ? 'no request yet' : '—'}</span>
      {:else if llm.activity === 'prefill'}
        <span class="t"><b>{fmtInt(llm.prefill.doneTokens)}</b> / {fmtInt(llm.prefill.tokens)} tok · in progress</span>
      {:else}
        <span class="t"><b>{fmtInt(llm.prefill.tokens)}</b> tok · <b>{fmtTps(llm.prefill.tps)}</b> tok/s · done in {fmtSeconds(llm.prefill.elapsedS)}</span>
      {/if}
    </span>
    <span class="c-lbl">Decode</span>
    <span class="c-v">
      {#if !llm}
        <span class="t q">—</span>
      {:else if llm.activity === 'prefill'}
        <span class="t q">waiting for prefill</span>
      {:else}
        <span class="t"><b>{fmtInt(llm.generatedTokens)}</b> tok generated{llm.activity === 'idle' ? ' · idle' : ''}</span>
      {/if}
    </span>
  </div>
  <div class="c-row">
    <span class="c-lbl">Context</span>
    <span class="c-v">
      <span class="t">
        {#if llm}<b class:red={ctxFrac >= 0.95}>{fmtInt(llm.context.usedTokens)}</b>{:else}—{/if} / {ctxTotal ? fmtInt(ctxTotal) : '—'}
        tokens{#if llm}{` · ${fmtPct(ctxFrac)}`}{/if}
      </span>
      <span class="c-bar" class:warn={ctxFrac >= 0.95} role="meter" aria-label="Context fill" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(ctxFrac * 100)}>
        <i style:transform="scaleX({ctxFrac})"></i>
      </span>
    </span>
    <span class="c-lbl">Speculative</span>
    <span class="c-v">
      {#if llm?.spec}
        <span class="t"><b>{Math.round(llm.spec.acceptancePct)}%</b> accepted · {llm.spec.mode}</span>
      {:else}
        <span class="t q">{llm ? 'off' : (model?.specMode ?? 'off')}</span>
      {/if}
      <span class="segs" class:off={!llm?.spec} role="meter" aria-label="Speculative acceptance" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(llm?.spec?.acceptancePct ?? 0)}>
        {#each { length: SEGS } as _, i (i)}<i class:on={i < specOn}></i>{/each}
      </span>
    </span>
  </div>
</section>

<style>
  .c-hr.one {
    grid-template-rows: minmax(0, 1fr);
    align-content: stretch;
  }
  .sw {
    padding-top: max(6px, calc(var(--u) * 8));
    padding-bottom: max(4px, calc(var(--u) * 6));
  }
  .sw.faded {
    opacity: 0.5;
  }
  .segs {
    flex: 1 1 auto;
    min-width: max(40px, calc(var(--u) * 60));
    max-width: max(120px, calc(var(--u) * 150));
    display: grid;
    grid-template-columns: repeat(16, minmax(0, 1fr));
    gap: max(2px, calc(var(--u) * 2.5));
    height: max(6px, calc(var(--u) * 7));
  }
  .segs i {
    border-radius: 1.5px;
    background: var(--s1);
  }
  .segs i.on {
    background: var(--sky);
  }
  .segs.off {
    opacity: 0.6;
  }
</style>
