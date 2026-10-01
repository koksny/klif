<script lang="ts">
  import type { LlmLive } from '../../../lib/model/types';
  import { fmtInt, fmtPct, fmtSeconds, fmtTps } from '../../../lib/model/format';
  import Swell from '../Swell.svelte';

  let { llm }: { llm: LlmLive } = $props();

  const SEGS = 16;

  const prefillText = $derived.by(() => {
    const p = llm.prefill;
    if (!p) return '—';
    // While prefilling, speed and time left are in the hero above; this line keeps the token counts.
    if (llm.activity === 'prefill')
      return `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok${p.cachedTokens ? ` · ${fmtInt(p.cachedTokens)} cached` : ''} · in progress`;
    return `${fmtInt(p.tokens)} tok · ${fmtTps(p.tps)} tok/s · done in ${fmtSeconds(p.elapsedS)}`;
  });
  const ctxFrac = $derived(
    llm.context.totalTokens > 0 ? Math.min(1, llm.context.usedTokens / llm.context.totalTokens) : 0,
  );
  const prefillFrac = $derived(
    llm.prefill && llm.prefill.tokens > 0 ? Math.min(1, llm.prefill.doneTokens / llm.prefill.tokens) : 0,
  );
  /** The prefill hero: only while the prompt is being processed. */
  const pf = $derived(llm.activity === 'prefill' && llm.prefill && llm.prefill.tokens > 0 ? llm.prefill : null);
  const specOn = $derived(llm.spec ? Math.round((llm.spec.acceptancePct / 100) * SEGS) : 0);
</script>

<section class="hero">
  {#if pf}
    <!-- prompt processing is the news while it runs: progress, speed and time left, never "0.0 tok/s" -->
    <div class="c-lbl">Prefill<span class="q">{' · current request'}</span></div>
    <div class="row">
      <div class="num">
        <span class="big">{Math.round(prefillFrac * 100)}</span><span class="unit">%</span>
      </div>
      <div class="pfx">
        <div class="pft c-data">
          <span><span class="hv">{fmtTps(pf.tps)}</span> tok/s</span><span class="sep">·</span><span
            ><span class="hv">{fmtSeconds(pf.etaS)}</span> left</span
          >
        </div>
        <div class="pfbar" role="meter" aria-label="Prefill progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(prefillFrac * 100)}>
          <i style:transform="scaleX({prefillFrac})"></i>
        </div>
        <div class="pfc">
          {fmtInt(pf.doneTokens)} / {fmtInt(pf.tokens)} tok{#if pf.cachedTokens}<span class="q">{` · ${fmtInt(pf.cachedTokens)} cached`}</span>{/if}<span
            class="q">{` · ${fmtSeconds(pf.elapsedS)} elapsed`}</span
          >
        </div>
      </div>
    </div>
  {:else}
    <div class="c-lbl">Decode speed{#if llm.activity !== 'decode'}<span class="q">{' · last request'}</span>{/if}</div>
    <div class="row">
      <div class="num">
        <span class="big">{fmtTps(llm.decodeTps)}</span><span class="unit">tok/s</span>
      </div>
      <div class="sw"><Swell history={llm.decodeHistory} /></div>
    </div>
  {/if}
</section>

<div class="c-rule"></div>

<section class="req">
  <div class="c-lbl">{llm.activity === 'idle' ? 'Last request' : 'Current request'}</div>
  <div class="line">
    <div class="pf">
      <span class="k">Prefill</span>
      <span class="c-data">{prefillText}</span>
    </div>
    <div class="dc">
      <i class="dot" class:on={llm.activity === 'decode'}></i>
      <span class="k">Decode</span>
      <span class="c-data">{fmtInt(llm.generatedTokens)} tok generated</span>
    </div>
  </div>

  <div class="two">
    <div class="cell">
      <div class="c-lbl">Context fill</div>
      <div class="val c-data">
        {fmtInt(llm.context.usedTokens)} / {fmtInt(llm.context.totalTokens)} tokens · {fmtPct(ctxFrac)}
      </div>
      <div class="bar" role="meter" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(ctxFrac * 100)} aria-label="Context fill">
        <i style:transform="scaleX({ctxFrac})"></i>
      </div>
    </div>
    <div class="vr" aria-hidden="true"></div>
    <div class="cell">
      <div class="c-lbl">Speculative decoding</div>
      {#if llm.spec}
        <div class="val c-data">{Math.round(llm.spec.acceptancePct)}% accepted · {llm.spec.mode}</div>
        <div class="segs" role="meter" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(llm.spec.acceptancePct)} aria-label="Speculative acceptance">
          {#each { length: SEGS } as _, i (i)}<i class:on={i < specOn}></i>{/each}
        </div>
      {:else}
        <div class="val c-data off">off</div>
        <div class="segs" aria-hidden="true">{#each { length: SEGS } as _, i (i)}<i></i>{/each}</div>
      {/if}
    </div>
  </div>
</section>

<style>
  .hero {
    padding-top: max(7px, calc(var(--u) * 13));
    padding-bottom: max(5px, calc(var(--u) * 8));
  }
  .q {
    color: var(--muted);
  }
  .row {
    display: flex;
    align-items: stretch;
    gap: max(14px, calc(var(--u) * 24));
    margin-top: max(2px, calc(var(--u) * 4));
  }
  .num {
    display: flex;
    align-items: baseline;
    gap: max(10px, calc(var(--u) * 16));
    flex: none;
  }
  .big {
    font-family: var(--f-disp);
    font-stretch: 125%;
    font-weight: 800;
    font-size: max(58px, calc(var(--u) * 85));
    line-height: 0.9;
    letter-spacing: -0.01em;
    color: #f2f9fc;
    font-variant-numeric: tabular-nums;
  }
  .unit {
    font-family: var(--f-disp);
    font-stretch: 100%;
    font-weight: 600;
    font-size: max(28px, calc(var(--u) * 41));
    color: var(--sky);
    letter-spacing: 0.005em;
  }
  .pfx {
    flex: 1 1 auto;
    min-width: 0;
    height: max(64px, calc(var(--u) * 82));
    align-self: flex-end;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: max(5px, calc(var(--u) * 8));
  }
  .pft {
    font-size: max(15px, calc(var(--u) * 22));
    color: #c3d3db;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pft .hv {
    color: var(--foam);
    font-weight: 500;
  }
  .pft .sep {
    margin: 0 0.6em;
    color: var(--muted);
  }
  .pfbar {
    position: relative;
    height: max(9px, calc(var(--u) * 12));
    border-radius: 99px;
    background: var(--s1);
    overflow: hidden;
  }
  .pfbar i {
    position: absolute;
    inset: 0;
    background: var(--sky);
    border-radius: 99px;
    transform-origin: left;
    transition: transform 400ms ease-out;
  }
  .pfc {
    font-family: var(--f-data);
    font-size: max(11.5px, calc(var(--u) * 13.5));
    color: var(--foam);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sw {
    flex: 1 1 auto;
    min-width: 0;
    height: max(64px, calc(var(--u) * 82));
    align-self: flex-end;
  }

  .req {
    padding-top: max(7px, calc(var(--u) * 12));
    padding-bottom: max(9px, calc(var(--u) * 17));
  }
  .line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
    margin-top: max(4px, calc(var(--u) * 7));
    font-size: max(12.5px, calc(var(--u) * 16.5));
  }
  .k {
    font-family: var(--f-ui);
    font-weight: 450;
    font-size: 1.07em;
    color: var(--foam);
    margin-right: max(10px, calc(var(--u) * 26));
  }
  .pf {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    min-width: 0;
  }
  .dc {
    display: flex;
    align-items: baseline;
    flex: none;
    margin-right: max(0px, calc(var(--u) * 120));
  }
  .dc .k {
    margin-right: max(14px, calc(var(--u) * 30));
  }
  .dot {
    width: max(10px, calc(var(--u) * 13));
    height: max(10px, calc(var(--u) * 13));
    border-radius: 50%;
    align-self: center;
    margin-right: max(10px, calc(var(--u) * 18));
    box-shadow: inset 0 0 0 2px var(--muted);
  }
  .dot.on {
    background: var(--sky);
    box-shadow: 0 0 10px rgba(90, 182, 235, 0.55);
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1px 1fr;
    column-gap: max(20px, calc(var(--u) * 42));
    margin-top: max(9px, calc(var(--u) * 15));
  }
  .vr {
    background: var(--rule);
  }
  .val {
    margin-top: max(3px, calc(var(--u) * 6));
    font-size: max(12.5px, calc(var(--u) * 17));
    color: var(--foam);
    white-space: nowrap;
  }
  .val.off {
    color: var(--muted);
  }
  .bar {
    position: relative;
    height: max(8px, calc(var(--u) * 11));
    margin-top: max(7px, calc(var(--u) * 12));
    border-radius: 99px;
    background: var(--s1);
    overflow: hidden;
  }
  .bar i {
    position: absolute;
    inset: 0;
    background: var(--sky);
    border-radius: 99px;
    transform-origin: left;
    transition: transform 400ms ease-out;
  }
  .segs {
    display: grid;
    grid-template-columns: repeat(16, 1fr);
    gap: max(2px, calc(var(--u) * 3.5));
    height: max(8px, calc(var(--u) * 11));
    margin-top: max(7px, calc(var(--u) * 12));
  }
  .segs i {
    border-radius: 2px;
    background: var(--s1);
  }
  .segs i.on {
    background: var(--sky);
  }
</style>
