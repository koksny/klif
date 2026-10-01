<script lang="ts">
  // Session bodies other than a live LLM: starting/loading (LLM or image), a live image (Krea) session,
  // and a fault. Same hardware as the live panel: hero row, strip, VRAM dial + right column, bottom
  // panel; each panel reads what the current state actually measures.
  import type { Actions, Session, ViewModel } from '../../../lib/model/types';
  import { fmtCtx, fmtInt, fmtSeconds } from '../../../lib/model/format';
  import { clamp, fmtAgo, fmtJobS, frac, median, sessionSpanS, sessionVram, slotById } from '../theme';
  import Drum from '../parts/Drum.svelte';
  import StepDrum from '../parts/StepDrum.svelte';
  import LedBar from '../parts/LedBar.svelte';
  import Led from '../parts/Led.svelte';
  import VramDial from '../parts/VramDial.svelte';
  import ContextDial from '../parts/ContextDial.svelte';
  import StepList from '../parts/StepList.svelte';
  import Timeline from '../parts/Timeline.svelte';
  import RecentJobs from '../parts/RecentJobs.svelte';
  import Icon from '../parts/Icon.svelte';
  import SysPanel from './SysPanel.svelte';

  let { vm, s }: { vm: ViewModel; s: Session; actions?: Actions } = $props();

  const slot = $derived(slotById(vm, s.slot));
  const label = $derived(slot?.label ?? s.slot.toUpperCase());
  const isImage = $derived(slot ? slot.kind === 'image' : !!s.image);
  const view = $derived(s.phase === 'fault' ? 'fault' : (s.phase === 'live' || s.phase === 'stopping') && s.image ? 'image' : 'loading');
  const ld = $derived(s.loading);
  const f = $derived(s.fault);
  const img = $derived(s.image);
  const llm = $derived(s.llm);

  // Load progress of the weights step: resident weights GiB over the weights layer the slot expects to
  // hold in VRAM (same quantity, so it ends at 100%; an image model keeps part of its file off the GPU,
  // so the size on disk is only the fallback).
  const weightsTotal = $derived(
    slot?.expectedVram?.find((l) => l.id === 'weights')?.gib ?? s.model.weightsGiB ?? 0,
  );
  const weightsNow = $derived(vm.vram.layers.find((l) => l.id === 'weights')?.gib ?? 0);
  const weightsFrac = $derived(weightsTotal > 0 ? clamp(weightsNow / weightsTotal) : null);
  // Where the load is headed: the slot's expected layers on top of what others hold.
  const target = $derived.by(() => {
    if (view !== 'loading' || !slot?.expectedVram?.length) return null;
    const base = vm.vram.baselineGiB ?? vm.vram.layers.find((l) => l.id === 'other')?.gib ?? 0;
    return base + slot.expectedVram.reduce((a, l) => a + l.gib, 0);
  });

  // A young session's cliff spans the session (the load builds it, a crash leaves its whole ghost)
  // instead of a sliver at "now" after minutes of idle floor.
  const dialVram = $derived(sessionVram(vm.vram, sessionSpanS(s)));

  const generating = $derived(!!img && img.activity === 'generating');
  const stepFrac = $derived(img && generating ? frac(img.step, img.steps) : 0);
  const lastJob = $derived(img?.recent.length ? img.recent[img.recent.length - 1] : null);
  const med = $derived(img ? median(img.recent.slice(-12).map((j) => j.seconds)) : 0);

  // Fault details
  const exitText = $derived.by(() => {
    if (!f) return '';
    if (f.exitCode === undefined) return 'no exit code';
    return f.exitCodeHex ? `exit code ${f.exitCodeHex} · ${f.exitCode}` : `exit code ${f.exitCode}`;
  });
  const failedStep = $derived(f?.steps?.find((x) => x.state === 'failed') ?? null);
  const faultSteps = $derived(f?.steps ?? null);
  const faultDoneFrac = $derived(
    faultSteps ? faultSteps.filter((x) => x.state === 'done').length / Math.max(1, faultSteps.length) : 0,
  );
  const logLines = $derived(f ? f.logTail.slice(-4) : []);

  // Context panel (LLM): live/last reading, or the configured preset with the needle at rest.
  const ctxTotal = $derived(llm?.context.totalTokens ?? s.model.ctxTokens ?? 0);
  const ctxUsed = $derived(llm?.context.usedTokens ?? 0);
  const lastReq = $derived(llm?.requests.length ? llm.requests[llm.requests.length - 1] : null);

  const PHASE_WORD: Record<string, string> = { starting: 'STARTING', loading: 'LOADING', stopping: 'STOPPING' };
  let ctxW = $state(0);
  let ctxH = $state(0);
  const dialPx = $derived(Math.max(60, Math.min(ctxH - 24, ctxW * 0.42)));
</script>

<!-- hero row -->
<section class="panel hero" class:fault={view === 'fault'}>
  {#if isImage}
    {#if view === 'fault'}
      <span class="lbl big two">LAST<br />IMAGE</span>
      <div class="drum" style="--cells:{2.62}"><Drum value={lastJob?.seconds ?? 0} intDigits={2} narrowLead={(lastJob?.seconds ?? 0) < 9.95} blank={!lastJob} /></div>
      <span class="unit">s</span>
    {:else}
      <span class="lbl big">STEP</span>
      <div class="sdrum" style="--cells:{2 * String(img?.steps ?? 8).length}">
        <StepDrum step={img?.step ?? 0} steps={img?.steps ?? 8} blank={!generating} />
      </div>
      <span class="vsep"></span>
      <div class="speed">
        <span class="sv">{generating && img && img.sPerIt > 0 ? `${img.sPerIt.toFixed(1)} s/it` : '— s/it'}</span>
        <span class="se">{generating && img ? `elapsed ${fmtSeconds(img.elapsedS)}` : view === 'loading' ? 'server loading' : 'waiting for a job'}</span>
      </div>
    {/if}
  {:else}
    <span class="lbl big" class:two={view === 'fault'}>{#if view === 'fault'}LAST<br />DECODE{:else}DECODE{/if}</span>
    <div class="drum" style="--cells:{3.62}"><Drum value={llm?.decodeTps ?? 0} intDigits={3} narrowLead={(llm?.decodeTps ?? 0) < 99.95} blank={!llm} /></div>
    <span class="unit">tok/s</span>
  {/if}
  <span class="vsep"></span>
  {#if view === 'fault' && f}
    <div class="plate" role="alert">
      <span class="warn"><Icon name="warn" size="calc(58 * var(--u))" /></span>
      <div class="ptext">
        <span class="ptitle" title={f.title}>{f.title}</span>
        <span class="pmeta">{exitText}</span>
        <span class="pmeta">{label} stopped {fmtAgo(f.sinceS)}</span>
      </div>
    </div>
  {:else if isImage}
    <div class="prog">
      <div class="pbar"><LedBar fraction={stepFrac} segments={img && img.steps > 0 && img.steps <= 24 ? img.steps * 2 : 16} label="Sampling progress" /></div>
      <div class="pnum">
        <span class="pp">{generating ? `${Math.round(stepFrac * 100)}%` : '—'}</span>
        <span class="pw">{generating ? 'generating' : view === 'loading' ? 'loading' : 'idle'}</span>
      </div>
      <Led on={generating} size="calc(16 * var(--u))" title="generating now" />
    </div>
  {:else}
    <div class="hist">
      <span class="htitle">decode history</span>
      <div class="empty"><span>no requests yet · the server is {PHASE_WORD[s.phase]?.toLowerCase() ?? 'loading'}</span></div>
    </div>
  {/if}
</section>

<!-- strip -->
<section class="panel strip">
  {#if view === 'loading'}
    <span class="lbl big">{PHASE_WORD[s.phase] ?? 'LOADING'}</span>
    <div class="lbar"><LedBar fraction={ld?.fraction ?? 0} segments={30} label="Load progress" /></div>
    <span class="pct">{Math.round(clamp(ld?.fraction ?? 0) * 100)}%</span>
  {:else if isImage && img}
    {#if view === 'fault'}
      <!-- the hero already reads the last image's seconds -->
      <span class="lbl big">LAST JOB</span>
      <span class="txt">{#if lastJob}<b>{lastJob.width}×{lastJob.height}</b>{lastJob.edit ? ' · edit' : ' · new image'}{:else}none finished{/if}</span>
    {:else}
      <span class="lbl big">LAST IMAGE</span>
      <span class="txt"><b>{lastJob ? fmtJobS(lastJob.seconds) : '—'}</b>{#if lastJob}&nbsp;· {lastJob.width}×{lastJob.height}{lastJob.edit ? ' · edit' : ''}{/if}</span>
    {/if}
    <span class="vsep"></span>
    <span class="lbl big">THIS SESSION</span>
    <span class="txt"><b>{fmtInt(img.imagesThisSession)}</b> images</span>
    <span class="vsep"></span>
    <span class="lbl big">MEDIAN</span>
    <span class="txt"><b>{img.recent.length ? fmtJobS(med) : '—'}</b>{#if img.recent.length}&nbsp;· last {Math.min(12, img.recent.length)}{/if}</span>
  {:else if faultSteps}
    <span class="lbl big">LOAD</span>
    <div class="lbar"><LedBar fraction={faultDoneFrac} segments={30} label="Load steps done before the fault" /></div>
    <span class="txt hot">failed at <b>{failedStep?.label ?? 'start'}</b></span>
  {:else if llm}
    <span class="lbl big">LAST REQUEST</span>
    <span class="txt">{#if lastReq}<b>#{lastReq.id}</b> · {fmtInt(lastReq.promptTokens)} tok prompt · {fmtInt(lastReq.generatedTokens)} tok generated{:else}none{/if}</span>
    <span class="vsep"></span>
    <span class="lbl big">SESSION</span>
    <span class="txt"><b>{fmtInt(llm.totals.requests)}</b> requests · {fmtInt(llm.totals.generatedTokens)} tok</span>
  {:else}
    <span class="lbl big">LOAD</span>
    <span class="txt hot">died <b>{fmtSeconds(s.uptimeS)}</b> after launch, before the server came up</span>
  {/if}
</section>

<!-- dials -->
<div class="dials" class:img={isImage}>
  <section class="panel vram">
    <VramDial vram={dialVram} mode={view === 'fault' ? 'fault' : 'live'} {target} />
  </section>
  <div class="rcol" class:loading={view === 'loading'}>
    <section class="panel ctx" bind:clientWidth={ctxW} bind:clientHeight={ctxH}>
      <div class="cdial" style="width:{dialPx}px; height:{dialPx}px">
        {#if isImage}
          {@const st = img?.steps ?? 8}
          <ContextDial used={generating ? (img?.step ?? 0) : 0} total={st} hiLabel={String(st)} majors={st <= 24 ? st : 4} minorPer={st <= 12 ? 4 : st <= 24 ? 2 : 10} dim={!generating} label="Generation" />
        {:else}
          <!-- at a fault the reading is the last one the dead server reported: the needle is unlit -->
          <ContextDial used={ctxUsed} total={ctxTotal} dim={!llm || view === 'fault'} />
        {/if}
      </div>
      <div class="ctext">
        {#if isImage}
          <span class="h">GENERATION</span>
          <span class="v">{generating && img ? `step ${img.step} / ${img.steps}` : view === 'loading' ? 'no job yet' : 'no job running'}</span>
        {:else}
          <span class="h">CONTEXT</span>
          {#if llm}
            <span class="v"><b>{fmtInt(ctxUsed)} / {fmtInt(ctxTotal)} tokens</b> <b>· {Math.round(frac(ctxUsed, ctxTotal) * 100)}%</b>{#if view === 'fault'}<br /><span class="at">last reading before the fault</span>{/if}</span>
          {:else}
            <span class="v">{ctxTotal ? `${fmtCtx(ctxTotal)} preset` : 'preset unknown'}{s.model.kvType ? ` · KV ${s.model.kvType}` : ''}</span>
          {/if}
        {/if}
      </div>
    </section>
    {#if view === 'loading'}
      <section class="panel steps">
        <StepList steps={ld?.steps ?? []} {weightsFrac} />
      </section>
    {:else if view === 'fault' && f}
      <section class="panel logp">
        <div class="lhead">
          <Led on tone="orange" size="calc(20 * var(--u))" />
          <span class="h2">LAST LOG LINES</span>
        </div>
        <pre class="log mono">{#each logLines as line, i (i)}<span class="ll">{line}</span>{/each}</pre>
      </section>
    {:else if img}
      <section class="panel job">
        <div class="lhead">
          <Led on={generating} size="calc(20 * var(--u))" title="generating now" />
          <span class="h2">{label} · {generating ? 'GENERATING' : 'IDLE'}</span>
        </div>
        <span class="jl">{img.width} × {img.height}{img.edit ? ' · edit job' : ' · new image'}</span>
        <span class="jl dim">{img.steps} sampling steps{generating && img.sPerIt > 0 ? ` · ${img.sPerIt.toFixed(2)} s/it` : ''}</span>
      </section>
    {/if}
    <SysPanel system={vm.system} />
  </div>
</div>

<!-- bottom panel -->
{#if isImage}
  <section class="panel jobs">
    <div class="thead">
      <span class="lbl big">RECENT JOBS</span>
      <span class="tot">seconds per image · {img ? `${fmtInt(img.imagesThisSession)} this session` : 'none yet'}</span>
      <span class="legend">
        <span><i class="sw dc"></i>new</span>
        <span><i class="sw pf"></i>edit</span>
      </span>
    </div>
    <div class="rjbox"><RecentJobs jobs={img?.recent ?? []} /></div>
  </section>
{:else}
  <section class="panel tline">
    <div class="tmain">
      <div class="thead">
        <span class="lbl big">REQUEST TIMELINE</span>
        <span class="tot">
          {#if llm}session: {fmtInt(llm.totals.requests)} requests · {fmtInt(llm.totals.generatedTokens)} tok generated{:else}no requests yet{/if}
        </span>
      </div>
      <Timeline requests={llm?.requests ?? []} />
    </div>
    <div class="legend col">
      <span><i class="sw pf"></i>prefill</span>
      <span><i class="sw dc"></i>decode</span>
    </div>
  </section>
{/if}

<style>
  .big {
    font-size: calc(19 * var(--u));
    letter-spacing: 0.06em;
    color: var(--cream);
  }
  /* hero row: same plate as the live DECODE row */
  .hero {
    height: calc(126 * var(--u));
    display: flex;
    align-items: center;
    padding: 0 calc(26 * var(--u));
    gap: calc(18 * var(--u));
  }
  .hero > .lbl {
    align-self: flex-start;
    margin-top: calc(30 * var(--u));
    width: calc(64 * var(--u));
    line-height: 1.15;
  }
  .hero > .lbl.two {
    margin-top: calc(26 * var(--u));
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
  .sdrum {
    height: calc(108 * var(--u));
    width: calc((var(--cells) * 62 + 40 + 10) * var(--u));
    flex: 0 0 auto;
    font-size: calc(92 * var(--u));
    --drum-gap: calc(5 * var(--u));
    --drum-pad: calc(5 * var(--u));
    --slash-w: calc(40 * var(--u));
    --digit-dy: 0.015em;
  }
  .unit {
    align-self: flex-end;
    margin-bottom: calc(26 * var(--u));
    font-size: calc(33 * var(--u));
    font-weight: 500;
    margin-left: calc(-4 * var(--u));
  }
  .hero .vsep {
    height: calc(96 * var(--u));
    align-self: center;
    margin: 0 calc(8 * var(--u));
  }
  .speed {
    display: flex;
    flex-direction: column;
    gap: calc(12 * var(--u));
    white-space: nowrap;
  }
  .sv {
    font-weight: 500;
    font-size: calc(38 * var(--u));
    line-height: 1;
    letter-spacing: 0.02em;
  }
  .se {
    font-size: calc(21 * var(--u));
    color: var(--cream-2);
    line-height: 1;
  }
  .prog {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(18 * var(--u));
  }
  .pbar {
    flex: 1 1 auto;
    min-width: 0;
    height: calc(44 * var(--u));
    --seg-gap: calc(4 * var(--u));
    --seg-glow: calc(6 * var(--u));
  }
  .pnum {
    display: flex;
    flex-direction: column;
    gap: calc(6 * var(--u));
    min-width: calc(92 * var(--u));
  }
  .pp {
    font-weight: 600;
    font-size: calc(34 * var(--u));
    line-height: 1;
  }
  .pw {
    font-size: calc(19 * var(--u));
    color: var(--cream-2);
    line-height: 1;
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
  .empty {
    flex: 1 1 auto;
    border: 1px solid rgba(237, 230, 214, 0.14);
    border-radius: 2px;
    background: rgba(0, 0, 0, 0.12);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: calc(16 * var(--u));
    color: var(--muted);
    letter-spacing: 0.03em;
  }
  /* the fault plate */
  .plate {
    flex: 1 1 auto;
    min-width: 0;
    align-self: stretch;
    margin: calc(10 * var(--u)) 0;
    display: flex;
    align-items: center;
    gap: calc(18 * var(--u));
    padding: 0 calc(20 * var(--u));
    border: calc(1.5 * var(--u)) solid var(--orange);
    border-radius: calc(5 * var(--u));
    background: linear-gradient(180deg, rgba(255, 107, 44, 0.16), rgba(255, 107, 44, 0.07));
    box-shadow:
      0 0 calc(14 * var(--u)) rgba(255, 107, 44, 0.18),
      inset 0 0 calc(18 * var(--u)) rgba(255, 107, 44, 0.08);
  }
  .warn {
    display: flex;
    color: var(--orange);
  }
  .warn :global(.ic) {
    stroke-width: 1.7;
  }
  .ptext {
    display: flex;
    flex-direction: column;
    gap: calc(5 * var(--u));
    min-width: 0;
  }
  .ptitle {
    font-weight: 600;
    font-size: calc(23 * var(--u));
    line-height: 1.08;
    color: #ff7a40;
    letter-spacing: 0.01em;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .pmeta {
    font-size: calc(18 * var(--u));
    color: var(--cream);
    line-height: 1.05;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* strip */
  .strip {
    height: calc(56 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(14 * var(--u));
    padding: 0 calc(22 * var(--u)) 0 calc(26 * var(--u));
  }
  .strip .big {
    font-size: calc(17 * var(--u));
  }
  .lbar {
    flex: 1 1 auto;
    min-width: 0;
    height: calc(20 * var(--u));
    --seg-gap: calc(4 * var(--u));
    --seg-glow: calc(5 * var(--u));
  }
  .pct {
    font-size: calc(21 * var(--u));
    font-weight: 500;
    min-width: calc(48 * var(--u));
    text-align: right;
  }
  .strip .vsep {
    height: calc(34 * var(--u));
    align-self: center;
    margin: 0 calc(4 * var(--u));
  }
  .txt {
    font-size: calc(18 * var(--u));
    letter-spacing: 0.025em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    color: var(--cream-2);
  }
  .txt b {
    font-weight: 500;
    color: var(--cream);
  }
  .txt.hot,
  .txt.hot b {
    color: var(--orange);
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
    overflow: hidden;
  }
  .cdial {
    flex: 0 0 auto;
  }
  .ctext {
    display: flex;
    flex-direction: column;
    gap: calc(14 * var(--u));
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
    color: var(--cream);
  }
  .ctext .v b {
    font-weight: 400;
    white-space: nowrap;
  }
  .ctext .v .at {
    font-size: calc(15 * var(--u));
    color: var(--muted);
  }
  .steps {
    flex: 0 0 calc(222 * var(--u));
    padding: calc(4 * var(--u)) calc(22 * var(--u)) calc(4 * var(--u)) calc(14 * var(--u));
  }
  .lhead {
    display: flex;
    align-items: center;
    gap: calc(16 * var(--u));
  }
  .h2 {
    font-size: calc(21 * var(--u));
    font-weight: 500;
    letter-spacing: 0.08em;
    line-height: 1;
    white-space: nowrap;
  }
  .logp {
    flex: 0 0 calc(176 * var(--u));
    padding: calc(18 * var(--u)) calc(20 * var(--u)) calc(14 * var(--u));
    display: flex;
    flex-direction: column;
    gap: calc(12 * var(--u));
  }
  .log {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0;
    padding: calc(8 * var(--u)) calc(12 * var(--u));
    background: #121315;
    border-radius: calc(4 * var(--u));
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.7);
    font-size: calc(14.5 * var(--u));
    line-height: 1.4;
    color: var(--cream);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .ll {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .job {
    flex: 0 0 calc(128 * var(--u));
    padding: calc(22 * var(--u)) calc(26 * var(--u)) 0 calc(20 * var(--u));
    display: flex;
    flex-direction: column;
    gap: calc(12 * var(--u));
  }
  .jl {
    margin-left: calc(36 * var(--u));
    font-size: calc(20 * var(--u));
    letter-spacing: 0.03em;
    line-height: 1;
    white-space: nowrap;
  }
  .jl.dim {
    color: var(--cream-2);
    font-size: calc(18 * var(--u));
  }

  /* bottom: request timeline (LLM) */
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
    gap: calc(18 * var(--u));
    font-size: calc(16 * var(--u));
    color: var(--cream-2);
    margin-left: auto;
  }
  .legend.col {
    flex-direction: column;
    justify-content: center;
    gap: calc(12 * var(--u));
    padding-bottom: calc(14 * var(--u));
    margin-left: 0;
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
  /* bottom: recent jobs (image) */
  .jobs {
    height: calc(172 * var(--u));
    padding: calc(14 * var(--u)) calc(26 * var(--u)) calc(14 * var(--u));
    display: flex;
    flex-direction: column;
    gap: calc(8 * var(--u));
  }
  .rjbox {
    flex: 1 1 auto;
    min-height: 0;
    padding-top: calc(20 * var(--u));
  }
</style>
