<script lang="ts">
  // The status block of the full window: one fixed box in every phase, a hero plate over two detail rows.
  // A fault plate covers both (same outer box). What each phase puts in it:
  //   idle      the selected tier's hero dimmed (blank drum + unit) with a word: NOT RUNNING, or why the tier
  //             cannot launch (amber); rows keep their live labels with dim values and configured facts
  //   loading   STARTUP % on the drum, the active step and the overall LED bar; the six startup steps fill
  //             the rows, three per row
  //   live LLM  decode speed on the drum with its history (the prefill progress while a prompt is processed);
  //             rows PREFILL / DECODE and CONTEXT / SPECULATIVE with their LED meters
  //   live img  the sampling step on the drums, s/it, elapsed, LED bar; rows LAST IMAGE / IMAGES, SIZE / MODE
  //   stopping  dim hero with STOPPING · RELEASING x GiB; no data yet: WAITING FOR DATA
  //   dormant   (live, vram.dormant set) the last readings dimmed, the meters dark; a request waiting on the
  //             restore turns the prefill bar into the restore progress (amber)
  import type { ViewModel } from '../../../lib/model/types';
  import { idleState, KIND_LABEL } from '../../../lib/model/systems';
  import { fmtClock, fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../../lib/model/format';
  import { fmtAgo, fmtJobS, fmtKTok, fmtLeft, frac, kindOf, median, pointerSlot, sleepOf, slotById, viewOf } from '../theme';
  import Drum from '../parts/Drum.svelte';
  import StepDrum from '../parts/StepDrum.svelte';
  import HistoryTrace from '../parts/HistoryTrace.svelte';
  import LedBar from '../parts/LedBar.svelte';
  import Led from '../parts/Led.svelte';
  import Icon from '../parts/Icon.svelte';

  let { vm }: { vm: ViewModel } = $props();

  const s = $derived(vm.session);
  const view = $derived(viewOf(vm));
  const kind = $derived(kindOf(vm));
  const slot = $derived(slotById(vm, pointerSlot(vm)));
  const model = $derived(s?.model ?? slot?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);
  const idle = $derived(idleState(slot));
  const genBusy = $derived(view === 'live-gen' && (gen?.requestsInFlight ?? 0) > 0);

  // ---- LLM ----------------------------------------------------------------------------------------------
  const pf = $derived(llm?.prefill ?? null);
  const prefilling = $derived(view === 'live-llm' && !!llm && llm.activity === 'prefill' && !!pf && pf.tokens > 0);
  const pfFrac = $derived(pf ? frac(pf.doneTokens, pf.tokens) : 0);
  const peak = $derived(llm ? Math.max(llm.decodeTps, ...llm.decodeHistory, 0) : 0);
  // Three integer drums: the hundreds window is narrow and blank until the 5-minute peak needs it.
  const intDigits = $derived(peak >= 999.95 ? 4 : 3);
  const narrowLead = $derived(peak < 99.95);
  const histLabel = $derived.by(() => {
    const n = llm?.decodeHistory.length ?? 0;
    if (n >= 285) return '5-minute history';
    if (n >= 60) return `${Math.round(n / 60)}-minute history`;
    return `${n}-second history`;
  });
  const prefillText = $derived.by(() => {
    if (!pf) return 'no request yet';
    if (pf.etaS > 0 || pf.doneTokens < pf.tokens) return `${fmtKTok(pf.doneTokens)} / ${fmtKTok(pf.tokens)} tok · ${Math.round(pf.tps)} tok/s · ${fmtLeft(pf.etaS)} left`;
    return `${pf.tokens < 100000 ? fmtInt(pf.tokens) : fmtKTok(pf.tokens)} tok · ${Math.round(pf.tps)} tok/s · done in ${fmtLeft(pf.elapsedS)}`;
  });
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || 0);
  const ctxFrac = $derived(llm ? frac(llm.context.usedTokens, llm.context.totalTokens) : 0);

  // ---- GPU dormant ----------------------------------------------------------------------------------------
  const sleep = $derived(view === 'live-llm' || view === 'live-img' ? sleepOf(vm.vram) : null);
  // Nothing can run: every reading on the plate is the last one.
  const quiet = $derived(!!sleep && (llm ? llm.activity === 'idle' : img ? img.activity === 'idle' : true));
  // A request is in and waits for the memory to come back.
  const waking = $derived(!!sleep && prefilling);

  // ---- image ----------------------------------------------------------------------------------------------
  const generating = $derived(view === 'live-img' && !!img && img.activity === 'generating' && img.steps > 0);
  const stepFrac = $derived(generating && img ? frac(img.step, img.steps) : 0);
  const lastJob = $derived(img?.recent.length ? img.recent[img.recent.length - 1] : null);
  const med = $derived(img?.recent.length ? median(img.recent.slice(-12).map((j) => j.seconds)) : 0);

  // ---- loading --------------------------------------------------------------------------------------------
  const steps = $derived(s?.loading?.steps ?? []);
  const activeStep = $derived(steps.find((x) => x.state === 'active') ?? null);
  const loadFrac = $derived(frac(s?.loading?.fraction ?? 0, 1));

  // ---- fault ----------------------------------------------------------------------------------------------
  const f = $derived(view === 'fault' ? (s?.fault ?? null) : null);
  const exitText = $derived.by(() => {
    if (!f) return '';
    if (f.exitCode === undefined) return 'no exit code reported';
    return f.exitCodeHex ? `exit code ${f.exitCodeHex} · ${f.exitCode}` : `exit code ${f.exitCode}`;
  });
  const logLines = $derived((f?.logTail ?? []).slice(-4));

  // ---- the word on a hero with nothing to measure ----------------------------------------------------------
  const word = $derived(
    view === 'idle'
      ? idle.warn
        ? `CANNOT LAUNCH · ${idle.text.toUpperCase()}`
        : idle.text.toUpperCase()
      : view === 'stopping'
        ? `STOPPING · RELEASING ${fmtGiB(vm.vram.usedGiB)} GiB`
        : 'WAITING FOR DATA',
  );
  const wordAmber = $derived(view === 'stopping' || (view === 'idle' && idle.warn));
</script>

<!-- A hero with nothing to measure (idle, stopping, no data yet): the kind's own plate, dimmed, with a word. -->
{#snippet waitHero()}
  <section class="panel hero dim">
    {#if kind === 'image'}
      <span class="lbl hl">STEP</span>
      <div class="fig">
        <div class="sdrum" style="--cells:2"><StepDrum step={0} steps={img?.steps ?? 8} blank /></div>
      </div>
      <span class="vsep"></span>
      <div class="hr prog">
        <div class="pstats"><span class="word" class:amb={wordAmber}>{word}</span></div>
        <div class="pbar"><LedBar fraction={0} segments={32} label="Sampling progress" dark /></div>
        <span class="pcap">{img ? `${img.steps} sampling steps` : model?.imageSize ? `default size ${model.imageSize}` : ''}</span>
      </div>
    {:else if kind !== 'llm'}
      <span class="lbl hl">{KIND_LABEL[kind].toUpperCase()}</span>
      <div class="fig">
        <div class="drum" style="--cells:3.62"><Drum value={0} intDigits={3} narrowLead blank /></div>
        <span class="unit">req</span>
      </div>
      <span class="vsep"></span>
      <div class="hr prog">
        <div class="pstats"><span class="word" class:amb={wordAmber}>{word}</span></div>
        <div class="pbar"><LedBar fraction={0} segments={32} label="Request in flight" dark /></div>
        <span class="pcap">{model?.name ?? ''}</span>
      </div>
    {:else}
      <span class="lbl hl">DECODE</span>
      <div class="fig">
        <div class="drum" style="--cells:3.62"><Drum value={0} intDigits={3} narrowLead blank /></div>
        <span class="unit">tok/s</span>
      </div>
      <span class="vsep"></span>
      <div class="hr hist">
        <span class="htitle">5-minute history</span>
        <div class="empty"><span class="word" class:amb={wordAmber}>{word}</span></div>
      </div>
    {/if}
  </section>
{/snippet}

<div class="block">
  {#if view === 'fault' && f}
    <section class="panel fplate" role="alert">
      <div class="fhead">
        <span class="warn"><Icon name="warn" size="calc(30 * var(--u))" /></span>
        <div class="ft">
          <span class="ftitle" title={f.title}>{f.title}</span>
          <span class="fsub">{slot?.label ?? ''} · {s?.model.name ?? ''} · {exitText}</span>
        </div>
        <span class="fago">stopped {fmtAgo(f.sinceS)}</span>
      </div>
      <div class="fbody" class:withsteps={!!f.steps?.length}>
        <div class="flog mono">
          {#each logLines as line, i (i)}<span class="ll" title={line}>{line}</span>{/each}
          {#if !logLines.length}<span class="ll none">no log output</span>{/if}
        </div>
        {#if f.steps?.length}
          <ol class="fsteps">
            {#each f.steps as st (st.id)}
              <li class={st.state}><span class="lamp"></span>{st.label}</li>
            {/each}
          </ol>
        {/if}
      </div>
    </section>
  {:else}
    <!-- hero -->
    {#if view === 'live-llm' && llm}
      {#if prefilling && pf}
        <section class="panel hero">
          <span class="lbl hl">PREFILL</span>
          <div class="fig">
            <div class="drum" style="--cells:2.62"><Drum value={pfFrac * 100} intDigits={3} narrowLead={pfFrac < 0.9995} /></div>
            <span class="unit">%</span>
          </div>
          <span class="vsep"></span>
          <div class="hr prog">
            <div class="pstats">
              <span><b>{Math.round(pf.tps)}</b> tok/s</span>
              <span class="right cy"><b>{fmtLeft(pf.etaS)}</b> left</span>
            </div>
            {#if waking && sleep}
              <div class="pbar"><LedBar fraction={sleep.restored} segments={32} tone="amber" label="GPU memory restored" /></div>
              <span class="pcap"><span class="amb">waiting for the GPU</span> · {fmtGiB(sleep.usedGiB)} GiB restored, {fmtGiB(sleep.pagedGiB)} to go · {fmtLeft(pf.elapsedS)} so far</span>
            {:else}
              <div class="pbar"><LedBar fraction={pfFrac} segments={32} label="Prompt processed" /></div>
              <span class="pcap">{fmtKTok(pf.doneTokens)} / {fmtKTok(pf.tokens)} tok processed{pf.cachedTokens ? ` · ${fmtKTok(pf.cachedTokens)} from cache` : ''} · {fmtLeft(pf.elapsedS)} so far</span>
            {/if}
          </div>
        </section>
      {:else}
        <section class="panel hero" class:quiet>
          <span class="lbl hl">{quiet ? 'LAST DECODE' : 'DECODE'}</span>
          <div class="fig">
            <div class="drum" style="--cells:{intDigits + 1 - (narrowLead ? 0.38 : 0)}"><Drum value={llm.decodeTps} {intDigits} {narrowLead} /></div>
            <span class="unit">tok/s</span>
          </div>
          <span class="vsep"></span>
          <div class="hr hist">
            <span class="htitle">{histLabel}{#if quiet}<span class="amb">&nbsp;· GPU asleep, last reading</span>{/if}</span>
            <div class="trace"><HistoryTrace data={llm.decodeHistory} /></div>
          </div>
        </section>
      {/if}
    {:else if view === 'live-img' && img}
      <section class="panel hero" class:quiet>
        <span class="lbl hl">{img.edit && generating ? 'STEP · EDIT' : 'STEP'}</span>
        <div class="fig">
          <div class="sdrum" style="--cells:{2 * String(img.steps).length}"><StepDrum step={img.step} steps={img.steps} blank={!generating} /></div>
        </div>
        <span class="vsep"></span>
        <div class="hr prog">
          <div class="pstats">
            <span><b>{generating && img.sPerIt > 0 ? img.sPerIt.toFixed(2) : '—'}</b> s/it</span>
            <span>elapsed <b>{generating ? fmtSeconds(img.elapsedS) : '—'}</b></span>
            <span class="right"><b>{generating ? `${Math.round(stepFrac * 100)}%` : '—'}</b> {generating ? 'generating' : quiet ? 'GPU asleep' : 'idle'}</span>
            <Led on={generating} size="calc(11 * var(--u))" title="generating now" />
          </div>
          <div class="pbar"><LedBar fraction={stepFrac} segments={img.steps > 0 && img.steps <= 16 ? img.steps * 4 : 32} label="Sampling progress" dark={quiet} /></div>
          <span class="pcap">{generating ? `${img.width} × ${img.height} · ${img.edit ? 'edit job' : 'new image'} · ${img.steps} sampling steps` : `waiting for the next job · ${img.steps} sampling steps`}</span>
        </div>
      </section>
    {:else if view === 'live-gen' && gen}
      <section class="panel hero" class:quiet={!genBusy}>
        <span class="lbl hl">{KIND_LABEL[kind].toUpperCase()}{genBusy ? ' · WORKING' : ''}</span>
        <div class="fig">
          <div class="drum" style="--cells:{String(gen.requestsTotal ?? 0).length + 1}"><Drum value={gen.requestsTotal ?? 0} intDigits={Math.max(3, String(gen.requestsTotal ?? 0).length)} narrowLead={(gen.requestsTotal ?? 0) < 100} /></div>
          <span class="unit">req</span>
        </div>
        <span class="vsep"></span>
        <div class="hr prog">
          <div class="pstats">
            <span>{#if genBusy}<b>{gen.requestsInFlight}</b> in flight{:else}waiting for the next request{/if}</span>
            <span class="right">{gen.lastActivityS !== undefined ? `last activity ${fmtSeconds(gen.lastActivityS)} ago` : ''}</span>
            <Led on={genBusy} size="calc(11 * var(--u))" title="a request is running" />
          </div>
          <div class="pbar"><LedBar fraction={genBusy ? 1 : 0} segments={32} label="Request in flight" dark={!genBusy} /></div>
          <span class="pcap">{model?.name ?? ''}{model?.quant ? ` · ${model.quant}` : ''}{gen.modelId ? ` · reports ${gen.modelId}` : ''}</span>
        </div>
      </section>
    {:else if view === 'loading' && s}
      <section class="panel hero">
        <span class="lbl hl">STARTUP</span>
        <div class="fig">
          <div class="drum" style="--cells:2.62"><Drum value={loadFrac * 100} intDigits={3} narrowLead={loadFrac < 0.9995} /></div>
          <span class="unit">%</span>
        </div>
        <span class="vsep"></span>
        <div class="hr prog">
          <div class="pstats">
            <span class="cur">{activeStep?.label ?? 'starting the server'}{#if activeStep?.detail}<span class="mut">{` · ${activeStep.detail}`}</span>{/if}</span>
            <span class="right">elapsed <b>{fmtClock(s.loading?.elapsedS ?? s.uptimeS)}</b></span>
          </div>
          <div class="pbar"><LedBar fraction={loadFrac} segments={32} label="Startup progress" /></div>
          <span class="pcap">VRAM {fmtGiB(vm.vram.usedGiB)} / {fmtGiB(vm.vram.totalGiB)} GiB · {steps.filter((x) => x.state === 'done').length} of {steps.length} steps done</span>
        </div>
      </section>
    {:else}
      {@render waitHero()}
    {/if}

    <!-- detail rows -->
    {#if view === 'loading'}
      <section class="panel rows steps">
        {#each [steps.slice(0, 3), steps.slice(3, 6)] as line, li (li)}
          <div class="srow">
            {#each line as st (st.id)}
              <div class="sc {st.state}">
                <span class="lamp"></span>
                <span class="sl">{st.label}{#if st.detail}<em>{st.detail}</em>{/if}</span>
                <span class="sw">{st.state === 'done' ? 'done' : st.state === 'active' ? 'running' : ''}</span>
              </div>
            {/each}
          </div>
        {/each}
      </section>
    {:else if kind === 'image'}
      <section class="panel rows img" class:dim={!img} class:quiet>
        <div class="row">
          <span class="lbl k">LAST IMAGE</span>
          <span class="v">
            {#if lastJob}<span class="t"><b>{fmtJobS(lastJob.seconds)}</b> · {lastJob.width}×{lastJob.height}{lastJob.edit ? ' · edit' : ''}</span>{:else}<span class="t">{img ? 'none yet' : '—'}</span>{/if}
          </span>
          <span class="lbl k">IMAGES</span>
          <span class="v">
            {#if img}<span class="t"><b>{fmtInt(img.imagesThisSession)}</b> this session{img.recent.length ? ` · median ${fmtJobS(med)} (last ${Math.min(12, img.recent.length)})` : ''}</span>{:else}<span class="t">—</span>{/if}
          </span>
        </div>
        <div class="row">
          <span class="lbl k">SIZE</span>
          <span class="v"><span class="t cfg">{model?.imageSize ?? '—'}</span></span>
          <span class="lbl k">MODE</span>
          <span class="v"><span class="t cfg">{model?.mode ?? '—'}</span></span>
        </div>
      </section>
    {:else if kind !== 'llm'}
      <section class="panel rows img" class:dim={!gen}>
        <div class="row">
          <span class="lbl k">IN FLIGHT</span>
          <span class="v"><span class="t">{#if gen}<b>{fmtInt(gen.requestsInFlight ?? 0)}</b>{:else}—{/if}</span></span>
          <span class="lbl k">LAST ACTIVITY</span>
          <span class="v"><span class="t">{#if gen?.lastActivityS !== undefined}<b>{fmtSeconds(gen.lastActivityS)}</b> ago{:else}—{/if}</span></span>
        </div>
        <div class="row">
          <span class="lbl k">MODEL</span>
          <span class="v"><span class="t cfg">{model?.name ?? '—'}{model?.quant ? ` · ${model.quant}` : ''}</span></span>
          <span class="lbl k">BACKEND</span>
          <span class="v"><span class="t cfg">{model?.backend || model?.engine || '—'}</span></span>
        </div>
      </section>
    {:else}
      <section class="panel rows llm" class:dim={!llm} class:quiet>
        <div class="row">
          <span class="lbl k">PREFILL</span>
          <span class="v">
            <span class="seg"><LedBar fraction={pf ? pfFrac : 0} segments={10} label="Prefill progress" dark={!llm || quiet} /></span>
            <span class="t">{llm ? prefillText : '—'}</span>
          </span>
          <span class="lbl k">DECODE</span>
          <span class="v">
            <span class="seg"><LedBar fraction={prefilling ? 0 : frac(llm?.decodeTps ?? 0, peak)} segments={10} label="Decode speed relative to the 5-minute peak" dark={!llm || quiet} /></span>
            {#if !llm}
              <span class="t">—</span>
            {:else if prefilling}
              <!-- nothing decodes while the prompt is processed: no "0.0" reading, the last speed only if there is one -->
              <span class="t mut">waits for prefill{llm.decodeTps > 0 ? ` · last ${fmtTps(llm.decodeTps)} tok/s` : ''}</span>
            {:else}
              <span class="t">{fmtTps(llm.decodeTps)} / peak {fmtTps(peak)} · {fmtInt(llm.generatedTokens)} tok</span>
            {/if}
            <span class="led"><Led on={llm?.activity === 'decode'} size="calc(10 * var(--u))" title="decoding now" /></span>
          </span>
        </div>
        <div class="row">
          <span class="lbl k">CONTEXT</span>
          <span class="v">
            <span class="seg"><LedBar fraction={ctxFrac} segments={10} label="Context used" dark={!llm} /></span>
            {#if llm}
              <span class="t"><b>{fmtInt(llm.context.usedTokens)}</b> / {fmtInt(llm.context.totalTokens)} tokens · {Math.round(ctxFrac * 100)}%</span>
            {:else}
              <span class="t">— / <span class="cfg">{ctxTotal ? fmtInt(ctxTotal) : '—'}</span> tokens</span>
            {/if}
          </span>
          <span class="lbl k">SPECULATIVE</span>
          <span class="v">
            <span class="seg"><LedBar fraction={llm?.spec ? llm.spec.acceptancePct / 100 : 0} segments={10} label="Draft acceptance" dark={!llm || quiet} /></span>
            {#if llm}
              <span class="t">{#if llm.spec}{Math.round(llm.spec.acceptancePct)}% accepted · {llm.spec.mode}{:else}off for this model{/if}</span>
            {:else}
              <span class="t cfg">{model?.specMode ?? 'off'}</span>
            {/if}
            <span class="led"><Led on={!!llm?.spec && llm.activity === 'decode'} size="calc(10 * var(--u))" title="drafting now" /></span>
          </span>
        </div>
      </section>
    {/if}
  {/if}
</div>

<style>
  /* One fixed box: hero plate over the two detail rows (a fault plate covers both). */
  .block {
    height: calc(150 * var(--u));
    display: grid;
    grid-template-rows: calc(82 * var(--u)) minmax(0, 1fr);
    gap: calc(4 * var(--u));
    font-family: var(--font-text);
  }
  .amb {
    color: var(--amber);
  }
  .cy {
    color: var(--cyan);
  }
  .mut {
    color: rgba(237, 230, 214, 0.58);
  }

  /* hero plate: label | drum + unit | divider | the reading's detail. The first two columns are fixed so the
     divider stands in the same place in every phase. */
  .hero {
    display: grid;
    grid-template-columns: calc(74 * var(--u)) calc(196 * var(--u)) auto minmax(0, 1fr);
    align-items: center;
    column-gap: calc(14 * var(--u));
    padding: 0 calc(20 * var(--u)) 0 calc(20 * var(--u));
  }
  .hl {
    align-self: start;
    margin-top: calc(14 * var(--u));
    color: var(--cream);
    white-space: normal;
    line-height: 1.2;
  }
  .fig {
    display: flex;
    align-items: flex-end;
    gap: calc(8 * var(--u));
    min-width: 0;
  }
  .drum {
    height: calc(62 * var(--u));
    width: calc((var(--cells) * 31 + 40) * var(--u));
    flex: 0 0 auto;
    font-size: calc(52 * var(--u));
    --drum-gap: calc(3 * var(--u));
    --drum-pad: calc(3.5 * var(--u));
    --drum-radius: calc(4 * var(--u));
    --dot-w: calc(14 * var(--u));
    --digit-dy: 0.015em;
  }
  .sdrum {
    height: calc(62 * var(--u));
    width: calc((var(--cells) * 34 + 22 + 24) * var(--u));
    flex: 0 0 auto;
    font-size: calc(53 * var(--u));
    --drum-gap: calc(3 * var(--u));
    --drum-pad: calc(3.5 * var(--u));
    --drum-radius: calc(4 * var(--u));
    --slash-w: calc(22 * var(--u));
    --digit-dy: 0.015em;
  }
  .unit {
    margin-bottom: calc(6 * var(--u));
    font-family: var(--font-text);
    font-size: var(--fs-read);
    font-weight: 500;
    line-height: 1;
    white-space: nowrap;
  }
  .hero .vsep {
    height: calc(62 * var(--u));
    align-self: center;
  }
  .hr {
    min-width: 0;
    height: calc(62 * var(--u));
    display: flex;
    flex-direction: column;
  }
  /* LLM: the decode history */
  .hist {
    gap: calc(6 * var(--u));
  }
  .htitle {
    font-size: var(--fs-small);
    color: var(--cream-2);
    line-height: 1;
    white-space: nowrap;
    overflow: hidden;
  }
  .trace {
    flex: 1 1 auto;
    min-height: 0;
  }
  .empty {
    flex: 1 1 auto;
    min-height: 0;
    border-left: 1px solid rgba(237, 230, 214, 0.18);
    border-bottom: 1px solid rgba(237, 230, 214, 0.18);
    background: rgba(0, 0, 0, 0.1);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .word {
    font-family: var(--font-label);
    font-weight: 600;
    font-size: var(--fs-lbl);
    letter-spacing: 0.16em;
    color: rgba(237, 230, 214, 0.55);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .word.amb {
    color: var(--amber);
  }
  /* progress (prefill, startup, sampling): stats, LED bar, caption */
  .prog {
    justify-content: space-between;
  }
  .pstats {
    display: flex;
    align-items: center;
    gap: calc(22 * var(--u));
    font-size: var(--fs-body);
    color: var(--cream-2);
    line-height: 1;
    white-space: nowrap;
    min-width: 0;
  }
  .pstats b {
    font-weight: 500;
    font-size: var(--fs-read);
    color: var(--cream);
  }
  .pstats .cy b {
    color: var(--cyan);
  }
  .pstats .right {
    margin-left: auto;
  }
  .pstats .cur {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    color: var(--cream);
    font-weight: 500;
  }
  .pbar {
    height: calc(12 * var(--u));
    --seg-gap: calc(3 * var(--u));
    --seg-glow: calc(4 * var(--u));
  }
  .pcap {
    font-size: var(--fs-small);
    color: rgba(237, 230, 214, 0.62);
    line-height: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* nothing to measure: the plate dimmed */
  .hero.dim .drum,
  .hero.dim .sdrum,
  .hero.dim .unit {
    opacity: 0.45;
  }
  .hero.dim .htitle {
    color: rgba(237, 230, 214, 0.4);
  }
  /* GPU asleep: the last readings stay, dimmed */
  .hero.quiet .drum,
  .hero.quiet .sdrum,
  .hero.quiet .unit,
  .hero.quiet .trace {
    opacity: 0.38;
  }

  /* detail rows: two rows, label | value | label | value */
  .rows {
    display: grid;
    grid-template-rows: repeat(2, minmax(0, 1fr));
  }
  .row {
    display: grid;
    grid-template-columns: calc(98 * var(--u)) minmax(0, 1fr) calc(108 * var(--u)) minmax(0, 1fr);
    align-items: stretch;
    min-height: 0;
  }
  .row + .row,
  .srow + .srow {
    border-top: 1px solid rgba(0, 0, 0, 0.55);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
  }
  .k {
    display: flex;
    align-items: center;
    padding-left: calc(20 * var(--u));
    border-right: 1px solid rgba(237, 230, 214, 0.1);
    overflow: hidden;
  }
  .v {
    display: flex;
    align-items: center;
    gap: calc(11 * var(--u));
    padding: 0 calc(14 * var(--u));
    min-width: 0;
    font-size: var(--fs-body);
    color: var(--cream);
    white-space: nowrap;
    border-right: 1px solid rgba(237, 230, 214, 0.1);
  }
  .v:last-child {
    border-right: 0;
  }
  .t {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t b {
    font-weight: 600;
  }
  .seg {
    flex: 0 0 calc(76 * var(--u));
    height: calc(10 * var(--u));
    --seg-gap: calc(2 * var(--u));
    --seg-glow: calc(3 * var(--u));
  }
  .led {
    display: flex;
    margin-left: auto;
    padding-left: calc(4 * var(--u));
  }
  .rows.dim .t {
    color: rgba(237, 230, 214, 0.42);
  }
  .rows.dim .t.cfg,
  .rows.dim .cfg {
    color: rgba(237, 230, 214, 0.78);
  }
  .rows.quiet .t {
    opacity: 0.55;
  }

  /* loading: the six startup steps in the rows */
  .srow {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    min-height: 0;
  }
  .sc {
    display: flex;
    align-items: center;
    gap: calc(9 * var(--u));
    padding: 0 calc(14 * var(--u)) 0 calc(20 * var(--u));
    min-width: 0;
    font-size: var(--fs-body);
    color: rgba(237, 230, 214, 0.5);
    border-right: 1px solid rgba(237, 230, 214, 0.1);
    white-space: nowrap;
  }
  .sc:last-child {
    border-right: 0;
  }
  .sc.done {
    color: var(--cream);
  }
  .sc.active {
    color: var(--cyan);
  }
  .sl {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sl em {
    font-style: normal;
    margin-left: calc(8 * var(--u));
    color: rgba(237, 230, 214, 0.62);
  }
  .sc.active .sl em {
    color: var(--cream);
  }
  .sw {
    margin-left: auto;
    font-size: var(--fs-small);
    color: rgba(237, 230, 214, 0.5);
  }
  .sc.active .sw {
    color: var(--cream-2);
  }
  .lamp {
    flex: 0 0 auto;
    width: calc(11 * var(--u));
    height: calc(11 * var(--u));
    border-radius: 50%;
    background: #141517;
    box-shadow:
      inset 0 0 0 calc(1.5 * var(--u)) rgba(237, 230, 214, 0.45),
      0 0 0 calc(1.2 * var(--u)) #0b0c0d;
  }
  .done .lamp,
  .active .lamp {
    background: radial-gradient(circle at 45% 40%, #bfe6fb 0%, #5ab6eb 50%, #3d9ed4 100%);
    box-shadow:
      0 0 0 calc(1.2 * var(--u)) #0b0c0d,
      0 0 calc(7 * var(--u)) rgba(90, 182, 235, 0.6);
  }
  .active .lamp {
    animation: breathe 1.4s ease-in-out infinite;
  }
  .failed .lamp {
    background: radial-gradient(circle at 45% 40%, #ffd2bd 0%, #ff6b2c 50%, #d84e14 100%);
    box-shadow:
      0 0 0 calc(1.2 * var(--u)) #0b0c0d,
      0 0 calc(7 * var(--u)) rgba(255, 107, 44, 0.6);
  }
  @keyframes breathe {
    50% {
      opacity: 0.45;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .active .lamp {
      animation: none;
    }
  }

  /* fault plate: covers the whole block */
  .fplate {
    grid-row: 1 / -1;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(12 * var(--u)) calc(20 * var(--u)) calc(10 * var(--u));
    border: calc(1.5 * var(--u)) solid var(--orange);
    background-color: #2a221f;
    background-image: var(--tex, none), linear-gradient(180deg, rgba(255, 107, 44, 0.15), rgba(255, 107, 44, 0.05));
    box-shadow:
      0 0 calc(14 * var(--u)) rgba(255, 107, 44, 0.18),
      inset 0 0 calc(18 * var(--u)) rgba(255, 107, 44, 0.08);
    overflow: hidden;
  }
  .fhead {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    column-gap: calc(14 * var(--u));
    align-items: center;
    padding-bottom: calc(9 * var(--u));
    border-bottom: 1px solid rgba(255, 107, 44, 0.45);
  }
  .warn {
    display: flex;
    color: var(--orange);
  }
  .warn :global(.ic) {
    stroke-width: 1.7;
  }
  .ft {
    display: flex;
    flex-direction: column;
    gap: calc(4 * var(--u));
    min-width: 0;
  }
  .ftitle {
    font-weight: 600;
    font-size: var(--fs-read);
    line-height: 1.1;
    color: #ff7a40;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fsub {
    font-size: var(--fs-body);
    color: var(--cream);
    line-height: 1.1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fago {
    align-self: end;
    font-size: var(--fs-body);
    color: var(--orange);
    white-space: nowrap;
  }
  .fbody {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    column-gap: calc(16 * var(--u));
    min-height: 0;
    padding-top: calc(8 * var(--u));
  }
  .fbody.withsteps {
    grid-template-columns: minmax(0, 1fr) calc(300 * var(--u));
  }
  .flog {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: calc(3 * var(--u));
    font-size: var(--fs-small);
    line-height: 1.25;
    color: var(--cream);
  }
  .ll {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ll.none {
    color: var(--muted);
  }
  .fsteps {
    list-style: none;
    margin: 0;
    padding: 0 0 0 calc(14 * var(--u));
    border-left: 1px solid rgba(255, 107, 44, 0.35);
    display: grid;
    grid-template-rows: repeat(3, auto);
    grid-auto-flow: column;
    grid-auto-columns: minmax(0, 1fr);
    align-content: start;
    row-gap: calc(6 * var(--u));
    column-gap: calc(12 * var(--u));
    font-size: var(--fs-small);
    color: rgba(237, 230, 214, 0.5);
  }
  .fsteps li {
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
    white-space: nowrap;
    overflow: hidden;
  }
  .fsteps li.done {
    color: var(--cream);
  }
  .fsteps li.failed {
    color: var(--orange);
  }
</style>
