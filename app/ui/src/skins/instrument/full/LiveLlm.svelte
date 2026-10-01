<script lang="ts">
  // LIVE body for an LLM session: drum odometer + history, LED meters, VRAM cliff dial, context dial,
  // speculative decoding, system, request timeline. While a prompt is being processed the hero row
  // reads the prefill instead (percent on the drum, tok/s and time left beside it), never "0.0 tok/s".
  import type { LlmLive, ViewModel } from '../../../lib/model/types';
  import { fmtInt, fmtTps } from '../../../lib/model/format';
  import { fmtLeft, frac, sessionSpanS, sessionVram } from '../theme';
  import Drum from '../parts/Drum.svelte';
  import HistoryTrace from '../parts/HistoryTrace.svelte';
  import LedBar from '../parts/LedBar.svelte';
  import Led from '../parts/Led.svelte';
  import VramDial from '../parts/VramDial.svelte';
  import ContextDial from '../parts/ContextDial.svelte';
  import Timeline from '../parts/Timeline.svelte';
  import SysPanel from './SysPanel.svelte';

  let { vm, llm }: { vm: ViewModel; llm: LlmLive } = $props();

  const peak = $derived(Math.max(llm.decodeTps, ...llm.decodeHistory, 0));
  const pf = $derived(llm.prefill);
  // Compact forms keep the active-prefill line as short as the finished one ("done in" is the mockup's).
  const kTok = (n: number) => (n >= 10000 ? `${(n / 1000).toFixed(1)}k` : fmtInt(n));
  const left = fmtLeft;
  const prefillText = $derived.by(() => {
    if (!pf) return 'no request yet';
    if (pf.etaS > 0 || pf.doneTokens < pf.tokens)
      return `${kTok(pf.doneTokens)} / ${kTok(pf.tokens)} tok · ${Math.round(pf.tps)} tok/s · ${left(pf.etaS)} left`;
    return `${pf.tokens < 100000 ? fmtInt(pf.tokens) : kTok(pf.tokens)} tok · ${Math.round(pf.tps)} tok/s · done in ${left(pf.elapsedS)}`;
  });
  const histLabel = $derived.by(() => {
    const n = llm.decodeHistory.length;
    if (n >= 285) return '5-minute history';
    if (n >= 60) return `${Math.round(n / 60)}-minute history`;
    return `${n}-second history`;
  });
  const ctx = $derived(llm.context);
  const ctxPct = $derived(Math.round(frac(ctx.usedTokens, ctx.totalTokens) * 100));
  // One line as in the mockup: long six-digit counts get a slightly smaller face instead of wrapping.
  const ctxFont = $derived.by(() => {
    const n = `${fmtInt(ctx.usedTokens)} / ${fmtInt(ctx.totalTokens)} tokens · ${ctxPct}%`.length;
    return Math.min(18.5, (18.5 * 27) / n);
  });
  // Three integer drums as in the mockup: the hundreds window is narrow and blank until the
  // 5-minute peak needs it (it can then roll to a digit).
  const intDigits = $derived(peak >= 999.95 ? 4 : 3);
  const narrowLead = $derived(peak < 99.95);

  const prefilling = $derived(llm.activity === 'prefill' && !!pf && pf.tokens > 0);
  const pfFrac = $derived(pf ? frac(pf.doneTokens, pf.tokens) : 0);

  let ctxW = $state(0);
  let ctxH = $state(0);
  const dialPx = $derived(Math.max(60, Math.min(ctxH - 24, ctxW * 0.42)));
</script>

<section class="panel decode">
  {#if prefilling && pf}
    <span class="lbl big">PREFILL</span>
    <div class="drum" style="--cells:{3 - 0.38}"><Drum value={pfFrac * 100} intDigits={3} narrowLead={pfFrac < 0.9995} /></div>
    <span class="unit">%</span>
    <span class="vsep"></span>
    <div class="pfh">
      <div class="pfr">
        <span class="pfv">{Math.round(pf.tps)} <small>tok/s</small></span>
        <span class="pfl">{left(pf.etaS)} <small>left</small></span>
      </div>
      <div class="pfbar"><LedBar fraction={pfFrac} segments={28} label="Prompt processed" /></div>
      <span class="pft">{kTok(pf.doneTokens)} / {kTok(pf.tokens)} tok processed{pf.cachedTokens ? ` · ${kTok(pf.cachedTokens)} from cache` : ''} · {left(pf.elapsedS)} so far</span>
    </div>
  {:else}
    <span class="lbl big">DECODE</span>
    <div class="drum" style="--cells:{intDigits + 1 - (narrowLead ? 0.38 : 0)}"><Drum value={llm.decodeTps} {intDigits} {narrowLead} /></div>
    <span class="unit">tok/s</span>
    <span class="vsep"></span>
    <div class="hist">
      <span class="htitle">{histLabel}</span>
      <div class="trace"><HistoryTrace data={llm.decodeHistory} /></div>
    </div>
  {/if}
</section>

<section class="panel leds">
  <span class="lbl big">PREFILL</span>
  <div class="segs"><LedBar fraction={pf ? frac(pf.doneTokens, pf.tokens) : 0} segments={10} label="Prefill progress" /></div>
  <span class="txt">{prefillText}</span>
  <span class="vsep"></span>
  <span class="lbl big">DECODE</span>
  <div class="segs d"><LedBar fraction={prefilling ? 0 : frac(llm.decodeTps, peak)} segments={11} label="Decode speed relative to the 5-minute peak" /></div>
  {#if prefilling}
    <!-- nothing decodes while the prompt is processed: no "0.0" reading, the last speed only if there is one -->
    <span class="txt quiet">waits for prefill{llm.decodeTps > 0 ? ` · last ${fmtTps(llm.decodeTps)} tok/s` : ''}</span>
  {:else}
    <span class="txt" title="{fmtInt(llm.generatedTokens)} tokens generated in this request">{fmtTps(llm.decodeTps)} / peak {fmtTps(peak)} · {fmtInt(llm.generatedTokens)} tok</span>
  {/if}
  <span class="dled"><Led on={llm.activity === 'decode'} size="calc(14 * var(--u))" title="decoding now" /></span>
</section>

<div class="dials">
  <section class="panel vram">
    <!-- a session younger than the 5-minute history: the cliff spans the session (unchanged after that) -->
    <VramDial vram={sessionVram(vm.vram, sessionSpanS(vm.session))} />
  </section>
  <div class="rcol">
    <section class="panel ctx" bind:clientWidth={ctxW} bind:clientHeight={ctxH}>
      <div class="cdial" style="width:{dialPx}px; height:{dialPx}px">
        <ContextDial used={ctx.usedTokens} total={ctx.totalTokens} />
      </div>
      <div class="ctext">
        <span class="h">CONTEXT</span>
        <span class="v" style="font-size: calc({ctxFont} * var(--u))"><b>{fmtInt(ctx.usedTokens)} / {fmtInt(ctx.totalTokens)} tokens</b> <b>· {ctxPct}%</b></span>
      </div>
    </section>
    <section class="panel spec">
      <div class="shead">
        <Led on={!!llm.spec && llm.activity === 'decode'} size="calc(22 * var(--u))" title="drafting now" />
        <span class="h2">SPECULATIVE DECODING</span>
      </div>
      <div class="ssegs"><LedBar fraction={llm.spec ? llm.spec.acceptancePct / 100 : 0} segments={16} label="Draft acceptance" /></div>
      <span class="stxt">
        {#if llm.spec}{Math.round(llm.spec.acceptancePct)}% accepted · {llm.spec.mode}{:else}off for this model{/if}
      </span>
    </section>
    <SysPanel system={vm.system} />
  </div>
</div>

<section class="panel tline">
  <div class="tmain">
    <div class="thead">
      <span class="lbl big">REQUEST TIMELINE</span>
      <span class="tot">session: {fmtInt(llm.totals.requests)} requests · {fmtInt(llm.totals.generatedTokens)} tok generated</span>
    </div>
    <Timeline requests={llm.requests} />
  </div>
  <div class="legend">
    <span><i class="sw pf"></i>prefill</span>
    <span><i class="sw dc"></i>decode</span>
  </div>
</section>

<style>
  .big {
    font-size: calc(19 * var(--u));
    letter-spacing: 0.06em;
    color: var(--cream);
  }
  /* decode row */
  .decode {
    height: calc(126 * var(--u));
    display: flex;
    align-items: center;
    padding: 0 calc(26 * var(--u));
    gap: calc(18 * var(--u));
  }
  .decode > .lbl {
    align-self: flex-start;
    margin-top: calc(30 * var(--u));
    width: calc(64 * var(--u));
  }
  .drum {
    height: calc(108 * var(--u));
    width: calc((var(--cells) * 54 + 26 + 30) * var(--u));
    flex: 0 0 auto;
    font-size: calc(90 * var(--u));
    --drum-gap: calc(5 * var(--u));
    --drum-pad: calc(5 * var(--u));
    --dot-w: calc(26 * var(--u));
    --digit-dy: 0.015em;
  }
  .unit {
    align-self: flex-end;
    margin-bottom: calc(26 * var(--u));
    font-size: calc(33 * var(--u));
    font-weight: 500;
    margin-left: calc(-4 * var(--u));
  }
  .decode .vsep {
    height: calc(96 * var(--u));
    align-self: center;
    margin: 0 calc(8 * var(--u));
  }
  .hist {
    flex: 1 1 auto;
    min-width: 0;
    height: calc(106 * var(--u));
    display: flex;
    flex-direction: column;
    gap: calc(8 * var(--u));
  }
  .htitle {
    font-size: calc(17 * var(--u));
    letter-spacing: 0.03em;
    color: var(--cream);
    line-height: 1;
    margin-top: calc(4 * var(--u));
  }
  .trace {
    flex: 1 1 auto;
    min-height: 0;
  }
  /* prefill hero */
  .pfh {
    flex: 1 1 auto;
    min-width: 0;
    height: calc(106 * var(--u));
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: calc(2 * var(--u)) 0 calc(4 * var(--u));
  }
  .pfr {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: calc(20 * var(--u));
    line-height: 1;
    white-space: nowrap;
  }
  .pfv,
  .pfl {
    font-weight: 500;
    font-size: calc(38 * var(--u));
    letter-spacing: 0.01em;
  }
  .pfl {
    color: var(--cyan);
  }
  .pfh small {
    font-size: calc(19 * var(--u));
    font-weight: 400;
    color: var(--cream-2);
    letter-spacing: 0.04em;
  }
  .pfbar {
    height: calc(18 * var(--u));
    --seg-gap: calc(3 * var(--u));
    --seg-glow: calc(5 * var(--u));
  }
  .pft {
    font-size: calc(16 * var(--u));
    color: var(--cream-2);
    letter-spacing: 0.03em;
    line-height: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .txt.quiet {
    color: var(--muted);
  }

  /* LED meter row */
  .leds {
    height: calc(56 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(11 * var(--u));
    padding: 0 calc(22 * var(--u)) 0 calc(26 * var(--u));
  }
  .leds .big {
    font-size: calc(17 * var(--u));
  }
  /* Square LED blocks as in the mockup: 13 px cells on a 16 px pitch. */
  .segs {
    flex: 0 0 auto;
    width: calc(152 * var(--u));
    height: calc(15 * var(--u));
    --seg-gap: calc(3 * var(--u));
    --seg-glow: calc(4 * var(--u));
  }
  .segs.d {
    width: calc(167 * var(--u));
  }
  .txt {
    font-size: calc(17 * var(--u));
    letter-spacing: 0.025em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 0 1 auto;
    min-width: 0;
  }
  .dled {
    display: flex;
    margin-left: auto;
    padding-left: calc(4 * var(--u));
  }
  .leds .vsep {
    height: calc(34 * var(--u));
    align-self: center;
    margin: 0 calc(4 * var(--u));
  }

  /* dial row */
  .dials {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1.16fr) minmax(0, 1fr);
    gap: calc(4 * var(--u));
  }
  .vram {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(10 * var(--u));
    overflow: hidden;
  }
  .rcol {
    display: flex;
    flex-direction: column;
    gap: calc(4 * var(--u));
    min-height: 0;
  }
  .ctx {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: calc(20 * var(--u));
    padding: 0 calc(18 * var(--u)) 0 calc(22 * var(--u));
  }
  .cdial {
    flex: 0 0 auto;
  }
  .ctext {
    display: flex;
    flex-direction: column;
    gap: calc(16 * var(--u));
    align-self: flex-start;
    margin-top: calc(66 * var(--u));
    min-width: 0;
  }
  .ctext .h {
    font-weight: 600;
    font-size: calc(31 * var(--u));
    letter-spacing: 0.04em;
    line-height: 1;
  }
  .ctext .v {
    font-size: calc(18.5 * var(--u));
    letter-spacing: 0.02em;
    line-height: 1.35;
  }
  .ctext .v b {
    font-weight: 400;
    white-space: nowrap;
  }
  .spec {
    flex: 0 0 calc(128 * var(--u));
    padding: calc(22 * var(--u)) calc(26 * var(--u)) 0 calc(20 * var(--u));
    display: flex;
    flex-direction: column;
    gap: calc(12 * var(--u));
  }
  .shead {
    display: flex;
    align-items: center;
    gap: calc(16 * var(--u));
  }
  .h2 {
    font-size: calc(21 * var(--u));
    font-weight: 500;
    letter-spacing: 0.08em;
    line-height: 1;
  }
  .ssegs {
    height: calc(20 * var(--u));
    margin-left: calc(38 * var(--u));
    --seg-gap: calc(4 * var(--u));
  }
  .stxt {
    margin-left: calc(38 * var(--u));
    font-size: calc(19.5 * var(--u));
    letter-spacing: 0.035em;
    line-height: 1;
    white-space: nowrap;
  }

  /* timeline */
  .tline {
    height: calc(94 * var(--u));
    padding: calc(13 * var(--u)) calc(26 * var(--u)) 0;
    display: flex;
    gap: calc(40 * var(--u));
  }
  .tmain {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: calc(9 * var(--u));
  }
  .thead {
    display: flex;
    align-items: baseline;
    gap: calc(22 * var(--u));
  }
  .thead .big {
    font-size: calc(17 * var(--u));
  }
  .tot {
    font-size: max(11px, calc(14 * var(--u)));
    color: var(--muted);
    letter-spacing: 0.03em;
    white-space: nowrap;
  }
  .legend {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(12 * var(--u));
    font-size: calc(16 * var(--u));
    color: var(--cream-2);
    padding-bottom: calc(14 * var(--u));
  }
  .legend span {
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
  }
  .sw {
    display: inline-block;
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    border-radius: 2px;
  }
  .sw.pf {
    background: #e9e1d0;
  }
  .sw.dc {
    background: #4fb2ea;
  }
</style>
