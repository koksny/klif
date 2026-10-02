<script lang="ts">
  // Silicon, full window. One fluid layout scaled by --u (1 design px), with the drawing panel
  // absorbing the remaining height. Branches on the session state:
  //   idle      - 2x2 tier cards, dark die, fit preview in the column, Launch + Tune, last session
  //   loading   - locked tabs, startup % + steps, VRAM building in the die gauge and the column
  //   live-llm  - decode speed (or prefill progress while prefilling), rows, request timeline
  //   live-img  - diffusion progress, last image / images, recent jobs
  //   fault     - fault panel, emptied column with the collapsed allocation, failed request marked
  //   dormant   - (live, vram.dormant set) the GPU is asleep or waking: GDDR6 blocks and the section show
  //               the paged-out allocations as hatched dashed outlines, tiles dark, amber status
  import type { Actions, Slot, ViewModel } from '../../lib/model/types';
  import { fmtClock, fmtFixed, fmtGiB, fmtInt, fmtPct, fmtSeconds, fmtTps } from '../../lib/model/format';
  import DieCanvas from './DieCanvas.svelte';
  import VramColumn from './VramColumn.svelte';
  import Sparkline from './Sparkline.svelte';
  import Meter from './Meter.svelte';
  import JobsChart from './JobsChart.svelte';
  import WinCtl from './WinCtl.svelte';
  import { held } from './held.svelte';
  import { useSleep } from './sleep.svelte';
  import {
    availabilityText,
    baselineOf,
    detailFraction,
    fmtAgo,
    fmtDur,
    fmtEta,
    lastSessionParts,
    modelLine,
    modelShort,
    vramAtFault,
  } from './text';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  let w = $state(1024);
  let h = $state(1152);
  const u = $derived(Math.max(0.72, Math.min(1.2, Math.min(w / 1024, h / 1152))));

  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const slotOf = (id: string): Slot | undefined => vm.slots.find((x) => x.id === id);
  const sessionSlot = $derived(s ? slotOf(s.slot) : undefined);
  const selectedSlot = $derived(slotOf(vm.selected) ?? vm.slots[0]);
  // The running slot's kind, or (idle) the selected slot's: the dark die still says what a tile means.
  const kind = $derived(sessionSlot?.kind ?? (s?.image ? 'image' : s?.llm ? 'llm' : (selectedSlot?.kind ?? 'llm')));
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const liveLlm = $derived(!!s && phase === 'live' && kind === 'llm' && !!llm);
  const liveImg = $derived(!!s && phase === 'live' && kind === 'image' && !!img);

  type View = 'idle' | 'loading' | 'live-llm' | 'live-img' | 'stopping' | 'fault' | 'other';
  const view = $derived<View>(
    !s
      ? 'idle'
      : phase === 'fault'
        ? 'fault'
        : phase === 'starting' || phase === 'loading'
          ? 'loading'
          : phase === 'stopping'
            ? 'stopping'
            : liveLlm
              ? 'live-llm'
              : liveImg
                ? 'live-img'
                : 'other',
  );
  // GPU dormant (live session, vm.vram.dormant set): asleep between requests, or waking while the VRAM is restored.
  const sleep = useSleep(() => vm);
  const dz = $derived(phase === 'live' ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);
  const tone = $derived(
    dz || view === 'loading' ? 'amber' : view === 'fault' ? 'fault' : view === 'live-llm' || view === 'live-img' || view === 'other' ? 'live' : 'off',
  );
  const frameless = $derived(!!vm.host?.frameless);
  // Panel mode (the read-only mini layout on the small screen): the button is always offered when a target exists.
  const panel = $derived(vm.host?.panel?.available ? vm.host.panel : null);
  const panelTip = $derived(`Panel mode: show on the small screen${panel?.target ? ` (${panel.target})` : ''}`);
  // Narrow window: the label gives way to the glyph so the header never wraps.
  const compactHdr = $derived(w < 700);

  // LLM: during prefill the hero is the prefill progress, never a bright 0.0 tok/s.
  const prefillActive = $derived(!!llm?.prefill && llm.activity === 'prefill');
  const tps = held(() => llm?.decodeTps ?? 0);
  const lastReq = $derived(llm && llm.requests.length ? llm.requests[llm.requests.length - 1] : null);
  const cacheFrac = $derived(lastReq && lastReq.promptTokens > 0 ? lastReq.cachedTokens / lastReq.promptTokens : null);
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0);
  const preFrac = $derived(prefillActive && llm?.prefill && llm.prefill.tokens > 0 ? llm.prefill.doneTokens / llm.prefill.tokens : 0);
  const preSegs = $derived(Math.round(Math.min(1, Math.max(0, preFrac)) * 64));

  // Image jobs light the CU tiles by sampling progress; the die caption says what a tile means.
  const imgGen = $derived(liveImg && !!img && img.activity === 'generating' && img.steps > 0);
  const imgFrac = $derived(imgGen && img ? img.step / img.steps : 0);
  const imgFill = $derived(imgGen ? imgFrac : null);
  const lastJob = $derived(img && img.recent.length ? img.recent[img.recent.length - 1] : null);
  const dieCaption = $derived.by(() => {
    if (kind === 'image') {
      const per = img && img.steps > 0 ? 64 / img.steps : 0;
      return Number.isInteger(per) && per > 0 ? [`${per} tiles`, '= 1 step'] : ['tiles =', 'sampling', 'progress'];
    }
    if (prefillActive) return ['1 tile =', '1/64 of', 'prompt'];
    return ['1 tile =', '1 token'];
  });

  // Idle fit preview: the selected tier's expected layers stacked on the baseline.
  const baseline = $derived(baselineOf(vm.vram));
  const previewBase = $derived(Math.max(baseline, vm.vram.usedGiB));
  const preview = $derived(view === 'idle' && selectedSlot?.expectedVram?.length ? selectedSlot.expectedVram : null);
  const previewTop = $derived(preview ? previewBase + preview.reduce((a, l) => a + l.gib, 0) : 0);

  // Loading.
  const steps = $derived(s?.loading?.steps ?? []);
  const stepsA = $derived(steps.slice(0, 3));
  const stepsB = $derived(steps.slice(3));

  // Fault.
  const fault = $derived(view === 'fault' ? (s?.fault ?? null) : null);
  const faultGiB = $derived(fault ? vramAtFault(vm.vram, fault.sinceS) : null);
  const faultTail = $derived((fault?.logTail ?? []).slice(-4));
  const exitText = $derived.by(() => {
    if (!fault) return '';
    if (fault.exitCode === undefined) return 'no exit code reported';
    return fault.exitCodeHex ? `exit code ${fault.exitCodeHex} (${fault.exitCode})` : `exit code ${fault.exitCode}`;
  });
  const faultHadWork = $derived(view === 'fault' && (!!llm || !!img));

  // LLM request timeline: last 8 (with a fault, the last slot marks the request that died).
  const reqN = $derived(view === 'fault' && llm ? 7 : 8);
  const requests = $derived(llm ? llm.requests.slice(-reqN) : []);
  const reqSlots = $derived(Array.from({ length: reqN }, (_, i) => requests[i - (reqN - requests.length)] ?? null));
  // Image slots show recent jobs in every session state (empty while loading), never a request timeline.
  const jobsView = $derived(kind === 'image' && view !== 'idle');
  // Was a request in flight when the LLM server died? (activity as last reported)
  const diedInRequest = $derived(view === 'fault' && !!llm && llm.activity !== 'idle');
  const tlCaption = $derived.by(() => {
    const n = requests.length;
    if (view === 'loading') return ' · no requests yet';
    if (view === 'fault') {
      if (!faultHadWork) return ' · no requests: failed during startup';
      const head = n === 0 ? 'no finished requests' : n < reqN ? `${n} so far` : `last ${n}`;
      return ` · ${head}, then ${diedInRequest ? 'the request that died' : 'the fault'}`;
    }
    if (n === 0) return ' · no finished requests yet';
    return n < reqN ? ` · ${n} so far, each split by time` : ` · last ${n}, each split by time`;
  });
  const jobsCaption = $derived.by(() => {
    if (!img) return view === 'fault' ? ' · no jobs: failed during startup' : ' · no jobs yet';
    const shown = Math.min(img.recent.length, view === 'fault' ? 11 : 12);
    if (shown === 0) return view === 'fault' ? ' · no finished jobs, then the fault' : ' · no finished jobs yet';
    return ` · bar height = time, last ${shown} of ${fmtInt(img.imagesThisSession)}`;
  });

  const phaseLabel = $derived(
    phase === 'idle' ? 'IDLE' : phase === 'live' ? 'LIVE' : phase === 'fault' ? 'FAULT' : phase === 'stopping' ? 'STOPPING' : phase === 'loading' ? 'LOADING' : 'STARTING',
  );
  // Header chip and drawing title block while the GPU is powered down.
  const statusLabel = $derived(dz ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : phaseLabel);
  const titleWord = $derived(dz ? (waking ? 'WAKING' : 'DORMANT') : phaseLabel);
  // Die title suffix: power state and how long, or how far the restore has got.
  const dieState = $derived.by(() => {
    if (!dz) return '';
    const ps = dz.powerState ? `${dz.powerState} · ` : '';
    return waking ? `WAKING${dz.powerState ? ` FROM ${dz.powerState}` : ''} · ${Math.round(dz.restoredFrac * 100)}% RESTORED` : `${ps}ASLEEP ${fmtDur(dz.sinceS)}`;
  });
  // A request waiting for the restore: nothing has been prefilled yet.
  const waitingForGpu = $derived(!!dz && prefillActive && (llm?.prefill?.doneTokens ?? 0) === 0);
  const dwg = $derived(vm.vram.device.replace(/^RX\s*/i, '').trim().replace(/\s+/g, '-'));
  // Drawing revision = the app's major.minor (host.appVersion "0.2.0" -> "0.2").
  const rev = $derived((vm.host?.appVersion ?? '').split('.').slice(0, 2).join('.') || '—');
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const freeNow = $derived(vm.vram.totalGiB - vm.vram.usedGiB);
  const lowFree = $derived(!!s && freeNow < vm.vram.warnBelowGiB);
  const last = $derived(vm.lastSession ? lastSessionParts(vm.lastSession, vm.slots) : null);
  const selReady = $derived(selectedSlot?.availability === 'ready');

  /** Lets a "a · b · c" line wrap only after a separator (spaces inside each part do not break). */
  function wrapAtDots(t: string): string {
    return t
      .split(' · ')
      .map((p) => p.replace(/ /g, ' '))
      .join(' · ');
  }

  function reqTitle(r: NonNullable<(typeof requests)[number]>): string {
    return `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${fmtSeconds(r.prefillS)} · ${fmtInt(r.generatedTokens)} tok in ${fmtSeconds(r.decodeS)}`;
  }
</script>

<div class="full" data-view={view} bind:clientWidth={w} bind:clientHeight={h} style="--u:{u}px">
  <!-- Header (window drag region; draws its own window controls when the host is frameless) -->
  <header class="hdr" class:frameless data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <div class="mark"><img src="/koksny-mark.png" alt="" draggable="false" /></div>
      <div class="words" data-tauri-drag-region>
        <div class="wordmark">KLIF</div>
        <div class="tagline">Koksny.com LOCAL INFERENCE FORNICATOR</div>
      </div>
    </div>
    <div class="status" data-tauri-drag-region>
      <span class="dot {tone}" class:pulse={waking}></span>
      <span class="phase {tone}">{statusLabel}</span>
      {#if s}
        <span class="vsep"></span>
        {#if view === 'loading'}
          <span class="uptime">elapsed <b>{fmtClock(s.loading?.elapsedS ?? s.uptimeS)}</b></span>
        {:else}
          <span class="uptime">uptime <b>{fmtClock(s.uptimeS)}</b></span>
        {/if}
      {:else if vm.host?.appVersion}
        <span class="vsep"></span>
        <span class="uptime ver">v{vm.host.appVersion}</span>
      {/if}
      {#if panel}
        <span class="vsep"></span>
        <button class="pbtn" class:on={panel.active} class:compact={compactHdr} onclick={() => actions.togglePanel?.()} title={panelTip} aria-label={panelTip} aria-pressed={panel.active}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="2.5" y="4" width="19" height="12.5" rx="1" /><path d="M8.5 20.5h7M12 16.5v4" /><path d="M14 9.5h5v4h-5z" /></svg>
          <span class="pl">PANEL</span>
        </button>
      {/if}
    </div>
    {#if frameless}<WinCtl {actions} maximized={!!vm.host?.maximized} />{/if}
  </header>

  {#if view === 'idle'}
    <!-- Launcher: one card per tier -->
    <div class="cards" role="radiogroup" aria-label="Tier">
      {#each vm.slots as slot (slot.id)}
        {@const sel = slot.id === vm.selected}
        {@const ok = slot.availability === 'ready'}
        <button
          class="card"
          class:sel
          class:na={!ok}
          role="radio"
          aria-checked={sel}
          title={ok ? `${slot.label}: ${slot.model.name}` : `${slot.label} cannot launch: ${availabilityText(slot.availability)}`}
          onclick={() => actions.select(slot.id)}
          ondblclick={() => ok && actions.launch(slot.id)}
        >
          <span class="c-top">
            <span class="c-l">{slot.label}</span>
            <span class="c-a" class:bad={!ok}><i></i>{availabilityText(slot.availability)}</span>
          </span>
          <span class="c-m">{modelShort(slot.model)}</span>
        </button>
      {/each}
    </div>
  {:else}
    <!-- Job selector: tiers, the model behind each is secondary -->
    <div class="tabs" class:tall={view === 'loading' || view === 'stopping'} role="tablist" aria-label="Job">
      {#each vm.slots as slot (slot.id)}
        {@const sel = slot.id === vm.selected}
        {@const locked = (view === 'loading' || view === 'stopping') && slot.id !== s?.slot}
        {@const na = slot.availability !== 'ready'}
        <button
          class="tab"
          class:sel
          class:fault={sel && view === 'fault'}
          class:locked
          class:na
          role="tab"
          aria-selected={sel}
          disabled={locked}
          title={locked ? `${slot.label}: locked while ${s?.model.name ?? 'the session'} is ${phaseLabel.toLowerCase()}` : na ? `${slot.label}: ${availabilityText(slot.availability)}` : `${slot.label}: ${slot.model.name}`}
          onclick={() => actions.select(slot.id)}
          ondblclick={() => {
            if ((!s || view === 'fault') && !na) actions.launch(slot.id);
          }}
        >
          {#if locked}
            <svg class="lock" viewBox="0 0 16 18" aria-hidden="true"><rect x="2" y="8" width="12" height="9" rx="1.5" /><path d="M5 8V5.5a3 3 0 0 1 6 0V8" /></svg>
          {/if}
          <span class="tt">
            <span class="tl">{slot.label}</span>
            <span class="tm" class:bad={na && !sel}>
              {#if s && s.slot === slot.id && !sel}<i class="run"></i>{/if}{wrapAtDots(na ? `${slot.model.name} · ${availabilityText(slot.availability)}` : modelShort(slot.model))}
            </span>
          </span>
        </button>
      {/each}
    </div>
  {/if}

  {#if view === 'fault' && fault}
    <!-- Fault panel -->
    <section class="fpanel panel" role="alert">
      <div class="f-head">
        <svg class="f-ico" viewBox="0 0 48 44" aria-hidden="true"><path d="M24 3.5 L45 40.5 H3 Z" /><path d="M24 16v12.5" /><circle cx="24" cy="34.2" r="1.6" class="fdot" /></svg>
        <div class="f-t">
          <div class="f-title" title={fault.title}>{fault.title}</div>
          <div class="f-sub">{sessionSlot?.label ?? ''} · {s?.model.name ?? ''} · {exitText}</div>
        </div>
        <div class="f-ago">{fmtAgo(fault.sinceS)}</div>
      </div>
      <div class="f-body" class:withsteps={!!fault.steps?.length}>
        <div class="f-log">
          {#each faultTail as line, i (i)}<div class="f-line" title={line}>{line}</div>{/each}
        </div>
        {#if fault.steps?.length}
          <ol class="f-steps">
            {#each fault.steps as st (st.id)}
              <li class={st.state}><span class="si"></span>{st.label}</li>
            {/each}
          </ol>
        {/if}
      </div>
    </section>
  {:else if view !== 'idle'}
    <div class="mline panel"><span>{modelLine(s?.model, kind, img?.steps)}</span></div>
  {/if}

  <!-- State body -->
  {#if view === 'live-llm' && llm}
    {#if prefillActive && llm.prefill}
      {@const p = llm.prefill}
      <section class="speed panel pf">
        <div class="sp-l">
          <div class="lbl ink">PREFILL PROGRESS</div>
          <div class="hero pct" class:faded={waitingForGpu}><b>{Math.floor(preFrac * 100)}</b><span class="unit">%</span></div>
        </div>
        <div class="sp-r pfr">
          {#if waitingForGpu && dz}
            <div class="cap"><span class="amb">waiting for the GPU to wake</span> <span class="dimcap">· {fmtGiB(dz.pagedOutGiB)} GiB still in system RAM</span></div>
          {:else}
            <div class="cap">prompt processing <span class="dimcap">· 1 segment = 1/64 of the prompt, as on the die</span></div>
          {/if}
          <div class="segs" role="img" aria-label="Prefill {Math.floor(preFrac * 100)} percent">
            {#each Array.from({ length: 64 }, (_, k) => k) as k (k)}<i class:on={k < preSegs}></i>{/each}
          </div>
          <div class="pf-stats">
            <span><b>{fmtInt(p.doneTokens)}</b> / {fmtInt(p.tokens)} tok</span>
            <span><b>{fmtInt(p.tps)}</b> tok/s</span>
            <span><b>{fmtEta(p.etaS)}</b> left</span>
          </div>
        </div>
      </section>
    {:else}
      <section class="speed panel">
        <div class="sp-l">
          <div class="lbl ink">{dz ? 'LAST DECODE SPEED' : 'DECODE SPEED'}</div>
          <div class="hero" class:faded={!!dz}><b>{fmtTps(tps.current)}</b><span class="unit">tok/s</span></div>
        </div>
        <div class="sp-r">
          <div class="cap">5-minute history <span class="dimcap">· grid 10 tok/s × 30 s</span></div>
          <div class="chart"><Sparkline values={llm.decodeHistory} /></div>
        </div>
      </section>
    {/if}

    <section class="row panel r-pre">
      <div class="k">PREFILL</div>
      <div class="v">
        {#if llm.prefill}
          {#if prefillActive}
            {#if llm.prefill.cachedTokens !== undefined}<b>{fmtInt(llm.prefill.cachedTokens)}</b>{` tok from prompt cache · `}{/if}{`running ${fmtSeconds(llm.prefill.elapsedS)}`}
          {:else}
            <b>{fmtInt(llm.prefill.tokens)}</b> tok · <b>{fmtInt(llm.prefill.tps)}</b> tok/s · done in {fmtSeconds(llm.prefill.elapsedS)}
          {/if}
        {:else}<span class="muted">no request yet</span>{/if}
      </div>
      <div class="k">DECODE</div>
      <div class="v">
        {#if prefillActive}<span class="muted">waiting for prefill</span>{:else}<b>{fmtInt(llm.generatedTokens)}</b> tok generated{llm.activity === 'idle' ? ' · idle' : ''}{/if}
      </div>
    </section>

    <section class="row panel r-ctx">
      <div class="k">CONTEXT</div>
      <div class="v ctx">
        <span class="ctxt"><b>{fmtInt(llm.context.usedTokens)}</b> <small>/ {fmtInt(llm.context.totalTokens)} tokens</small> · <b class:red={ctxFrac >= 0.95}>{fmtPct(ctxFrac)}</b></span>
        <span class="ctxbar"><Meter value={ctxFrac} warn={ctxFrac >= 0.95} /></span>
      </div>
      <div class="k">SPECULATIVE DECODING</div>
      <div class="v">
        {#if llm.spec}<b>{Math.round(llm.spec.acceptancePct)}%</b> accepted · {llm.spec.mode}{:else}<span class="muted">off</span>{/if}
      </div>
    </section>
  {:else if view === 'live-img' && img}
    <section class="speed panel diff">
      <div class="sp-l">
        <div class="lbl ink">DIFFUSION PROGRESS{imgGen && img.edit ? ' · EDIT' : ''}</div>
        <div class="hero dh" class:dimh={!imgGen}>
          <span class="unit pre">step</span><b>{imgGen ? img.step : '—'}</b><span class="unit">/ {img.steps}</span>
        </div>
      </div>
      <div class="sp-r dgr">
        {#if imgGen}
          <div class="d-stats">
            <span><b>{fmtFixed(img.sPerIt, 2)}</b> s/it</span>
            <span>elapsed <b>{fmtSeconds(img.elapsedS)}</b></span>
            <span class="muted sm">{img.width}×{img.height}{img.edit ? ' · edit' : ''}</span>
          </div>
        {:else}
          <div class="d-stats"><span class="muted">waiting for the next job</span></div>
        {/if}
        <div class="d-bar">
          <span class="d-meter"><Meter value={imgFrac} tall /></span>
          <b class="d-pct">{fmtPct(imgFrac)}</b>
        </div>
      </div>
    </section>
    <section class="krow">
      <div class="panel kc">
        <span class="k3">last image</span>
        {#if lastJob}<b>{fmtSeconds(lastJob.seconds)}</b><span class="muted sm">{lastJob.width}×{lastJob.height}{lastJob.edit ? ' · edit' : ''}</span>{:else}<span class="muted">none yet</span>{/if}
      </div>
      <div class="panel kc">
        <span class="k3">images this session</span><b>{fmtInt(img.imagesThisSession)}</b>
      </div>
    </section>
  {:else if view === 'loading' && s}
    <section class="speed panel wait">
      <div class="sp-l">
        <div class="lbl ink">{kind === 'image' ? 'DIFFUSION PROGRESS' : 'DECODE SPEED'}</div>
        <div class="hero dimh"><b>—</b><span class="unit">{kind === 'image' ? 'steps' : 'tok/s'}</span></div>
      </div>
      <div class="sp-r">
        <div class="cap">{kind === 'image' ? 'sampling progress' : '5-minute history'}</div>
        <div class="chart">
          {#if kind === 'image'}<span class="emptyframe"></span>{:else}<Sparkline values={[]} />{/if}
          <span class="waiting">WAITING FOR MODEL</span>
        </div>
      </div>
    </section>
    <section class="boot panel">
      <div class="b-l">
        <div class="lbl">STARTUP</div>
        <div class="b-pct"><b>{Math.floor((s.loading?.fraction ?? 0) * 100)}</b><span>%</span></div>
      </div>
      {#each [stepsA, stepsB] as col, ci (ci)}
        <ol class="b-col">
          {#each col as st (st.id)}
            {@const wf = st.id === 'weights' ? (st.state === 'done' ? 1 : detailFraction(st.detail)) : null}
            <li class={st.state}>
              <span class="si"></span>
              <span class="st-l">{st.label}{#if st.detail}<em>{st.detail}</em>{/if}</span>
              {#if st.id === 'weights'}<span class="st-bar"><Meter value={wf ?? 0} /></span>{/if}
            </li>
          {/each}
        </ol>
      {/each}
    </section>
  {:else if view === 'stopping' || view === 'other'}
    <section class="state panel">
      <div class="st-main">
        <div class="lbl">{phaseLabel}</div>
        <div class="st-big">{sessionSlot?.label ?? ''}</div>
        {#if view === 'stopping'}<div class="st-sub">releasing {fmtGiB(vm.vram.usedGiB)} GiB</div>{/if}
      </div>
    </section>
  {/if}

  <!-- The drawing: die + VRAM section -->
  <section class="draw panel">
    <div class="die-wrap">
      <div class="dtitle">{vm.vram.device} GPU DIE (TOP VIEW){#if dz}<span class="amb">&nbsp;· {dieState}</span>{/if}</div>
      <div class="die-canvas">
        <DieCanvas
          variant="full"
          llm={liveLlm ? llm : null}
          live={liveLlm}
          usedGiB={vm.vram.usedGiB}
          totalGiB={vm.vram.totalGiB}
          cacheFrac={liveLlm ? cacheFrac : null}
          jobFill={imgFill}
          caption={dieCaption}
          pagedOutGiB={dz ? dz.pagedOutGiB : null}
          label={dz
            ? `GPU die, GPU ${waking ? 'waking' : 'asleep'}: compute-unit tiles dark; 8 GDDR6 blocks show ${fmtGiB(dz.residentGiB)} GiB resident and ${fmtGiB(dz.pagedOutGiB)} GiB paged out to system RAM as hatched outlines`
            : `GPU die: compute-unit tiles show the token stream (${dieCaption.join(' ')}); 8 GDDR6 blocks show VRAM used; the cache block shows prompt-cache reuse`}
        />
      </div>
    </div>
    <div class="col-wrap">
      <div class="chdr">
        {#if preview}
          <span><span class="fp">FIT PREVIEW</span> · <b class:red={previewTop > vm.vram.totalGiB}>{fmtGiB(previewTop)}</b> / {fmtGiB(vm.vram.totalGiB)} GiB</span>
        {:else if dz}
          <span><span class="fp amb">RESIDENT</span> · <b>{fmtGiB(vm.vram.usedGiB)}</b> / {fmtGiB(vm.vram.totalGiB)} GiB</span>
        {:else}
          <span>{vm.vram.device} · <b class:amber={lowFree && vm.vram.spillMiB <= 0} class:red={vm.vram.spillMiB > 0}>{fmtGiB(vm.vram.usedGiB)}</b> / {fmtGiB(vm.vram.totalGiB)} GiB</span>
        {/if}
      </div>
      <div class="col-body">
        <VramColumn
          vram={vm.vram}
          {u}
          ramTotalGiB={vm.system.ramTotalGiB}
          ramType={vm.system.ramType}
          {preview}
          previewBaseGiB={previewBase}
          {faultGiB}
          sleep={dz}
          {waking}
        />
      </div>
    </div>
  </section>

  {#if view === 'idle'}
    <!-- Launch -->
    <section class="launch">
      <button class="btn primary big" onclick={() => actions.launch(vm.selected)} disabled={!selReady}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 4l14 8-14 8z" fill="currentColor" stroke="none" /></svg>
        {#if selReady}Launch <b>{selectedSlot?.label ?? ''}</b>{:else}<b>{selectedSlot?.label ?? ''}</b> · {availabilityText(selectedSlot?.availability ?? 'unsupported')}{/if}
      </button>
      <button class="btn big tune" onclick={() => actions.openTune(vm.selected)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h18M3 12h18M3 18h18" /><circle cx="15" cy="6" r="2.2" class="knob" /><circle cx="8" cy="12" r="2.2" class="knob" /><circle cx="14" cy="18" r="2.2" class="knob" /></svg>
        Tune
      </button>
    </section>
    <section class="row panel lastrow">
      <div class="k">LAST SESSION</div>
      <div class="v">
        {#if vm.lastSession && last}
          <span class="ls" title={vm.lastSession.model.name}
            ><b>{last.label}</b>{` · ${last.facts.join(' · ')} · `}{#if vm.lastSession.ended === 'fault'}<span class="red">{`ended in a fault ${fmtAgo(vm.lastSession.endedAgoS)}`}</span>{:else}{`stopped ${fmtAgo(vm.lastSession.endedAgoS)}`}{/if}</span
          >
        {:else}<span class="muted">none yet</span>{/if}
      </div>
    </section>
  {/if}

  <!-- System + title block -->
  <section class="sys panel">
    <div class="cell">
      <span class="k2">RAM</span>
      <span class="v2">{fmtFixed(vm.system.ramUsedGiB, 1)} / {fmtFixed(vm.system.ramTotalGiB, 1)} GiB</span>
      <span class="m2"><Meter value={vm.system.ramTotalGiB > 0 ? vm.system.ramUsedGiB / vm.system.ramTotalGiB : 0} /></span>
    </div>
    <div class="cell">
      <span class="k2">CPU</span>
      <span class="v2">{vm.system.cpuName} · {Math.round(vm.system.cpuPct)}%</span>
      <span class="m2"><Meter value={vm.system.cpuPct / 100} /></span>
    </div>
    <div class="cell tb-cell">
      <span class="titleblock" class:long={titleWord.length > 5}>KLIF · DWG {dwg} · {titleWord} · REV {rev}</span>
    </div>
  </section>

  {#if view !== 'idle'}
    <!-- Request timeline / recent jobs -->
    <section class="tline panel" class:jobs={jobsView}>
      <div class="tl-main">
        {#if jobsView}
          <div class="cap2">RECENT JOBS<span class="dimcap">{jobsCaption}</span></div>
          <div class="jobs-area"><JobsChart jobs={img?.recent ?? []} fault={view === 'fault' && !!img} /></div>
        {:else}
          <div class="cap2">REQUEST TIMELINE<span class="dimcap">{tlCaption}</span></div>
          <div class="bars">
            {#each reqSlots as r, i (i)}
              {#if r}
                {@const tot = r.prefillS + r.decodeS}
                <span class="bar" class:cur={i === reqN - 1 && view !== 'fault'} title={reqTitle(r)}
                  ><span class="pre" style="width:{tot > 0 ? (r.prefillS / tot) * 100 : 0}%"></span><span class="dec"></span></span
                >
              {:else}<span class="bar empty"></span>{/if}
            {/each}
            {#if view === 'fault' && llm}<span class="bar failed" title={diedInRequest ? 'The server died during this request' : 'The server died with no request in flight'}></span>{/if}
          </div>
        {/if}
      </div>
      <div class="legend">
        {#if jobsView}
          <span><i class="sw dec"></i>PLAIN</span>
          <span><i class="sw edit"></i>EDIT</span>
        {:else}
          <span><i class="sw pre"></i>PREFILL</span>
          <span><i class="sw dec"></i>DECODE</span>
        {/if}
        {#if view === 'fault' && faultHadWork}<span><i class="sw flt"></i>FAULT</span>{/if}
      </div>
    </section>
  {/if}

  <!-- Controls + console line -->
  <section class="ctrl panel">
    <div class="btnrow">
      {#if view === 'idle' || view === 'fault'}
        <span class="chip off wide" title="No server is running">
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1" /><path d="M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1" /></svg
          >
          Endpoint offline
        </span>
      {:else}
        <button class="chip" onclick={() => actions.copyEndpoint()} disabled={!s} title={s ? `Copy http://${s.endpoint.host}:${s.endpoint.port}` : 'No endpoint'}>
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1" /><path d="M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1" /></svg
          >
          <span class="mono">{s ? `:${s.endpoint.port}` : '—'}</span>
        </button>
        <button class="chip key" onclick={() => actions.copyApiKey()} disabled={!s?.apiKeySet} title={s?.apiKeySet ? 'Copy API key' : 'No API key set'}>
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path
              d="M15 2.5a6.5 6.5 0 0 0-6.2 8.4L2.5 17.2V21.5h4.3v-2.2H9v-2.2h2.2l1.9-1.9A6.5 6.5 0 1 0 15 2.5Zm2.1 6.1a1.7 1.7 0 1 1 0-3.4 1.7 1.7 0 0 1 0 3.4Z"
              fill="currentColor"
              stroke="none"
            /></svg
          >
          {#if s?.apiKeySet}<span class="dots">•••••••••</span><span class="copy">copy</span>{:else}<span class="copy">no key</span>{/if}
        </button>
      {/if}
      <span class="grow"></span>
      {#if view === 'fault'}
        <button class="btn danger" onclick={() => actions.restart()}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.34-5.66" /><path d="M20 4v5h-5" /></svg>Restart {sessionSlot?.label ?? ''}
        </button>
        <button class="btn" onclick={() => actions.toggleConsole(true)}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4" /><path d="M9 12h6M9 16h6" /></svg>Show full log
        </button>
        <button class="btn" onclick={() => actions.dismiss?.()}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12H5" /><path d="M11 6l-6 6 6 6" /></svg>Back to launcher
        </button>
      {:else if view === 'idle'}
        <button class="btn" onclick={() => actions.toggleConsole()}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7l5 5-5 5" /><path d="M12 18h8" /></svg>Console
        </button>
      {:else}
        <button class="btn stop" onclick={() => actions.stop()} disabled={view === 'stopping'}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="6" width="12" height="12" fill="currentColor" stroke="none" /></svg>{view === 'loading' ? 'Cancel' : 'Stop'}
        </button>
        <button class="btn" onclick={() => actions.restart()} disabled={view === 'loading' || view === 'stopping'}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.34-5.66" /><path d="M20 4v5h-5" /></svg>Restart
        </button>
        <button class="btn" onclick={() => actions.openEndpoint()} disabled={phase !== 'live'}>
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg
          >{kind === 'image' ? 'Open web UI' : 'Open endpoint'}
        </button>
        <button class="btn" onclick={() => actions.toggleConsole()}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7l5 5-5 5" /><path d="M12 18h8" /></svg>Console
        </button>
      {/if}
    </div>
    <button class="cline" onclick={() => actions.toggleConsole(true)} title="Open console">
      <span class="caret">&gt;</span><span class="ltxt">{lastLine}</span>
    </button>
  </section>
</div>

<style>
  .full {
    --bg: #0d1115;
    --panel: #0e151b;
    --line: #2c3c49;
    --line2: #33424f;
    --cyan: #5ab6eb;
    --hot: #f4faff;
    --ink: #dbe6ee;
    --muted: #7c8f9e;
    --label: #86c3e6;
    --red: #ff5a36;
    --amber: #f2b33a;
    --mint: #4fd6a4;
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    gap: calc(var(--u) * 9);
    padding: 0 calc(var(--u) * 13) calc(var(--u) * 13);
    background: var(--bg);
    color: var(--ink);
    font-family: 'JetBrains Mono Variable', 'JetBrains Mono', ui-monospace, monospace;
    font-size: max(11px, calc(var(--u) * 15.5));
    overflow: hidden;
    user-select: none;
  }
  .full > :global(*) {
    min-width: 0;
    flex: none;
  }
  .panel {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: calc(var(--u) * 3);
    min-width: 0;
    min-height: 0;
  }
  b {
    color: var(--hot);
    font-weight: 600;
  }
  .muted {
    color: var(--muted);
  }
  .red {
    color: var(--red);
  }
  .sm {
    font-size: 0.86em;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 1px solid var(--cyan);
    outline-offset: 2px;
  }
  button:disabled {
    cursor: default;
    opacity: 0.4;
  }

  /* Header */
  .hdr {
    position: relative;
    height: calc(var(--u) * 78);
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--line);
    margin: 0 calc(var(--u) * -13);
    padding: 0 calc(var(--u) * 13);
    min-width: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 16);
    min-width: 0;
  }
  .mark {
    width: calc(var(--u) * 64);
    height: calc(var(--u) * 64);
    border: 1px solid #2f4150;
    border-radius: calc(var(--u) * 9);
    display: grid;
    place-items: center;
    background: #0a0f13;
    flex: none;
  }
  .mark img {
    width: 100%;
    height: 100%;
    display: block;
    pointer-events: none;
  }
  .wordmark {
    font-size: calc(var(--u) * 34);
    font-weight: 800;
    color: var(--hot);
    line-height: 1;
    letter-spacing: 0.02em;
  }
  .tagline {
    margin-top: calc(var(--u) * 8);
    font-size: max(10px, calc(var(--u) * 14.5));
    letter-spacing: 0.05em;
    color: #cfe0ea;
    white-space: nowrap;
  }
  .status {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 12);
    font-size: max(11px, calc(var(--u) * 17));
    white-space: nowrap;
    padding-top: calc(var(--u) * 6);
  }
  .frameless .status {
    padding-top: calc(var(--u) * 28);
  }
  .dot {
    width: calc(var(--u) * 15);
    height: calc(var(--u) * 15);
    border-radius: 50%;
    background: #4a5a66;
  }
  .dot.live {
    background: var(--cyan);
  }
  .dot.amber {
    background: var(--amber);
  }
  .dot.pulse {
    animation: si-pulse 1s ease-in-out infinite;
  }
  @media (prefers-reduced-motion: reduce) {
    .dot.pulse {
      animation: none;
    }
  }
  .dot.fault {
    background: var(--red);
  }
  .phase {
    font-weight: 700;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .phase.live {
    color: var(--cyan);
  }
  .phase.amber {
    color: var(--amber);
  }
  .phase.fault {
    color: var(--red);
  }
  .vsep {
    width: 1px;
    height: calc(var(--u) * 30);
    background: var(--line2);
    margin: 0 calc(var(--u) * 14);
  }
  .uptime {
    color: #dfe9f0;
  }
  .uptime b {
    color: var(--cyan);
    font-weight: 500;
    letter-spacing: 0.04em;
  }
  .uptime.ver {
    color: var(--muted);
    letter-spacing: 0.04em;
  }
  /* Panel-mode button: same hairline chip as the control row, glyph + a short mono label. */
  .pbtn {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 9);
    height: calc(var(--u) * 34);
    padding: 0 calc(var(--u) * 14) 0 calc(var(--u) * 11);
    border: 1px solid #3a4d5b;
    border-radius: calc(var(--u) * 3);
    background: #0b1116;
    color: var(--label);
    font-family: 'Iosevka', 'JetBrains Mono Variable', monospace;
    font-weight: 500;
    font-size: max(10.5px, calc(var(--u) * 14.5));
    letter-spacing: 0.08em;
    white-space: nowrap;
    flex: none;
  }
  .pbtn:hover {
    border-color: #6a8aa0;
    color: var(--hot);
  }
  .pbtn.on {
    border-color: var(--cyan);
    color: var(--cyan);
  }
  .pbtn svg {
    width: calc(var(--u) * 20);
    height: calc(var(--u) * 20);
    min-width: 14px;
    min-height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.7;
    stroke-linecap: round;
    stroke-linejoin: round;
    flex: none;
  }
  .pbtn.compact {
    padding: 0 calc(var(--u) * 10);
  }
  .pbtn.compact .pl {
    display: none;
  }

  /* Idle: tier cards */
  .cards {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    grid-auto-rows: calc(var(--u) * 80);
    gap: calc(var(--u) * 9);
  }
  .card {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 9);
    padding: 0 calc(var(--u) * 20);
    border: 1px solid #33475a;
    border-radius: calc(var(--u) * 3);
    background: var(--panel);
    text-align: left;
    min-width: 0;
  }
  .card:hover:not(.sel) {
    border-color: #557086;
  }
  .card.sel {
    border-color: var(--cyan);
    background: linear-gradient(180deg, #11263a, #0f1d29);
    box-shadow: inset 0 0 0 1px rgba(90, 182, 235, 0.35);
  }
  .c-top {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: calc(var(--u) * 12);
    min-width: 0;
  }
  .c-l {
    font-family: 'Iosevka', 'JetBrains Mono Variable', monospace;
    font-weight: 700;
    font-size: calc(var(--u) * 28);
    letter-spacing: 0.06em;
    color: var(--hot);
    line-height: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .c-a {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 9);
    font-size: max(10.5px, calc(var(--u) * 16));
    color: var(--label);
    white-space: nowrap;
    flex: none;
  }
  .c-a i {
    width: calc(var(--u) * 12);
    height: calc(var(--u) * 12);
    border-radius: 50%;
    background: var(--mint);
  }
  .c-a.bad {
    color: var(--amber);
  }
  .c-a.bad i {
    background: transparent;
    border: 1.5px solid var(--amber);
  }
  .c-m {
    font-size: max(10.5px, calc(var(--u) * 16));
    color: #d4e1ea;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .card.na .c-l {
    color: var(--muted);
  }
  .card.na .c-m {
    color: #8394a1;
  }

  /* Tabs */
  .tabs {
    height: calc(var(--u) * 42);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: calc(var(--u) * 9);
  }
  .tab {
    border: 1px solid #3a4d5b;
    border-radius: calc(var(--u) * 3);
    background: var(--panel);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: calc(var(--u) * 10);
    min-width: 0;
    padding: 0 calc(var(--u) * 6);
  }
  .tab:hover:not(.sel):not(:disabled) {
    border-color: #557086;
  }
  .tab:disabled {
    opacity: 1;
  }
  .tt {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: calc(var(--u) * 2);
    min-width: 0;
  }
  .tl {
    font-size: max(11px, calc(var(--u) * 15.5));
    font-weight: 400;
    letter-spacing: 0.12em;
    color: #e6eef4;
    white-space: nowrap;
  }
  /* Narrow face so "model · quant · ctx" fits a quarter-width tab (as in the B-krea / B-fault sheets). */
  .tm {
    font-family: 'Iosevka', 'JetBrains Mono Variable', monospace;
    font-weight: 500;
    font-size: max(9.5px, calc(var(--u) * 12));
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  /* Loading / stopping: taller tabs, the model may take two lines (B-loading). */
  .tabs.tall {
    height: calc(var(--u) * 58);
  }
  .tabs.tall .tm {
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    line-height: 1.25;
    text-align: center;
  }
  .tabs.tall .tab.locked .tm {
    text-align: left;
  }
  .tm.bad {
    color: var(--amber);
  }
  .tab.sel {
    background: var(--cyan);
    border-color: var(--cyan);
  }
  .tab.sel .tl {
    color: #04121b;
    font-weight: 500;
  }
  .tab.sel .tm {
    color: #0b3550;
  }
  .tab.fault {
    background: var(--red);
    border-color: var(--red);
  }
  .tab.fault .tl {
    color: #1a0602;
    font-weight: 600;
  }
  .tab.fault .tm {
    color: #4a1206;
  }
  .tab.na .tl,
  .tab.locked .tl {
    color: var(--muted);
  }
  .tab.locked {
    border-color: #2a3945;
    justify-content: flex-start;
    padding-left: calc(var(--u) * 14);
  }
  .tab.locked .tt {
    align-items: flex-start;
  }
  .tab.locked .tm {
    color: #5f7280;
  }
  .lock {
    width: calc(var(--u) * 15);
    height: calc(var(--u) * 17);
    flex: none;
    fill: none;
    stroke: #8296a5;
    stroke-width: 1.5;
  }
  .run {
    display: inline-block;
    width: calc(var(--u) * 7);
    height: calc(var(--u) * 7);
    border-radius: 50%;
    background: var(--cyan);
    margin-right: calc(var(--u) * 6);
    vertical-align: middle;
  }

  /* Model line */
  .mline {
    height: calc(var(--u) * 35);
    display: flex;
    align-items: center;
    padding: 0 calc(var(--u) * 16);
    font-size: max(11px, calc(var(--u) * 16.5));
    color: #edf3f7;
    white-space: nowrap;
    overflow: hidden;
  }
  .mline span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Hero panel (decode speed / prefill progress / diffusion progress / waiting) */
  .speed {
    display: grid;
    grid-template-columns: calc(var(--u) * 282) minmax(0, 1fr);
    height: calc(var(--u) * 86);
  }
  .sp-l {
    border-right: 1px solid var(--line);
    padding: calc(var(--u) * 10) calc(var(--u) * 14) 0;
    min-width: 0;
  }
  .lbl {
    font-family: 'Iosevka', monospace;
    font-weight: 500;
    font-size: max(10px, calc(var(--u) * 15));
    letter-spacing: 0.08em;
    color: var(--label);
    white-space: nowrap;
  }
  .lbl.ink {
    color: #cfdde6;
  }
  .hero {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 18);
    margin-top: calc(var(--u) * 1);
    white-space: nowrap;
  }
  .hero b {
    font-family: 'Iosevka', 'JetBrains Mono Variable', monospace;
    font-size: calc(var(--u) * 56);
    font-weight: 700;
    line-height: 1.05;
    letter-spacing: -0.01em;
  }
  .hero .unit {
    font-family: 'Iosevka', 'JetBrains Mono Variable', monospace;
    font-size: calc(var(--u) * 50);
    font-weight: 500;
    color: var(--hot);
  }
  .hero.dimh b,
  .hero.dimh .unit {
    color: #5f7280;
  }
  /* GPU asleep: the last figure, not a live one. */
  .hero.faded b,
  .hero.faded .unit {
    color: #7f93a1;
  }
  .amb {
    color: var(--amber);
  }
  .hero.dh {
    gap: calc(var(--u) * 13);
  }
  .hero.pct {
    gap: calc(var(--u) * 4);
  }
  .hero.dh .unit.pre {
    font-size: calc(var(--u) * 44);
  }
  .sp-r {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(var(--u) * 6) calc(var(--u) * 10) calc(var(--u) * 8) calc(var(--u) * 12);
    min-width: 0;
  }
  .cap {
    font-size: max(9.5px, calc(var(--u) * 12));
    color: #c3d4df;
    letter-spacing: 0.03em;
    margin-bottom: calc(var(--u) * 4);
    white-space: nowrap;
    overflow: hidden;
  }
  .dimcap {
    color: #5f7280;
  }
  .chart {
    position: relative;
    min-height: 0;
  }
  .emptyframe {
    position: absolute;
    inset: 0;
    border: 1px dashed #26333d;
  }
  .waiting {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    font-family: 'Iosevka', monospace;
    font-weight: 500;
    font-size: max(10px, calc(var(--u) * 14));
    letter-spacing: 0.14em;
    color: #9fb2bf;
  }

  /* Prefill hero: 64 segments, one per CU tile */
  .pfr {
    grid-template-rows: auto auto auto;
    align-content: space-between;
    padding-bottom: calc(var(--u) * 7);
  }
  .segs {
    display: grid;
    grid-template-columns: repeat(64, minmax(0, 1fr));
    gap: max(1px, calc(var(--u) * 2));
    height: calc(var(--u) * 20);
    padding: max(1px, calc(var(--u) * 2));
    border: 1px solid #3a4c5a;
    border-radius: calc(var(--u) * 2);
    background: #0b1116;
  }
  .segs i {
    background: #1a2833;
  }
  .segs i:nth-child(16n) {
    box-shadow: 1px 0 0 #4e6779;
  }
  .segs i.on {
    background: var(--cyan);
  }
  .pf-stats {
    display: flex;
    gap: calc(var(--u) * 26);
    font-size: max(11px, calc(var(--u) * 17));
    white-space: nowrap;
    overflow: hidden;
    color: #c7d6e0;
  }
  .pf-stats b {
    font-weight: 600;
  }

  /* Diffusion hero */
  .dgr {
    grid-template-rows: auto auto;
    align-content: center;
    row-gap: calc(var(--u) * 10);
    padding: calc(var(--u) * 8) calc(var(--u) * 16) calc(var(--u) * 8) calc(var(--u) * 22);
  }
  .d-stats {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 40);
    font-size: max(12px, calc(var(--u) * 21));
    white-space: nowrap;
    overflow: hidden;
    color: #dbe6ee;
  }
  .d-stats .sm {
    font-size: max(10px, calc(var(--u) * 14));
    margin-left: auto;
  }
  .d-bar {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 16);
  }
  .d-meter {
    flex: 1 1 auto;
    min-width: 0;
  }
  .d-pct {
    font-size: max(12px, calc(var(--u) * 22));
    min-width: 3.2em;
    text-align: right;
  }
  .krow {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: calc(var(--u) * 9);
    height: calc(var(--u) * 35);
  }
  .kc {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 18);
    padding: 0 calc(var(--u) * 14);
    white-space: nowrap;
    overflow: hidden;
    font-size: max(11px, calc(var(--u) * 16));
  }
  .k3 {
    color: #cfdde6;
    letter-spacing: 0.04em;
  }
  .kc b {
    font-size: max(12px, calc(var(--u) * 18));
  }

  /* Data rows */
  .row {
    display: grid;
    height: calc(var(--u) * 33);
    align-items: stretch;
  }
  .r-pre {
    grid-template-columns: calc(var(--u) * 80) minmax(0, 1fr) calc(var(--u) * 74) minmax(0, 1fr);
  }
  .r-ctx {
    grid-template-columns: calc(var(--u) * 80) minmax(0, 480fr) calc(var(--u) * 166) minmax(0, 272fr);
  }
  .row .k {
    display: flex;
    align-items: center;
    padding-left: calc(var(--u) * 12);
    font-family: 'Iosevka', monospace;
    font-weight: 500;
    font-size: max(10px, calc(var(--u) * 14.5));
    letter-spacing: 0.03em;
    color: var(--label);
    border-right: 1px solid var(--line);
    white-space: nowrap;
    overflow: hidden;
  }
  .row .v {
    font-size: max(10.5px, calc(var(--u) * 15));
    display: flex;
    align-items: center;
    padding: 0 calc(var(--u) * 14);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
    border-right: 1px solid var(--line);
    min-width: 0;
  }
  .row .v:last-child {
    border-right: 0;
  }
  .row .v b {
    font-weight: 500;
  }
  .v.ctx {
    gap: calc(var(--u) * 14);
  }
  .ctxt small {
    font-size: max(9.5px, calc(var(--u) * 13.5));
    color: #c7d6e0;
  }
  .ctxbar {
    flex: 1 1 auto;
    min-width: calc(var(--u) * 40);
  }

  /* Loading: startup + steps */
  .boot {
    height: calc(var(--u) * 112);
    display: grid;
    grid-template-columns: calc(var(--u) * 180) minmax(0, 1.25fr) minmax(0, 1fr);
  }
  .b-l {
    padding: calc(var(--u) * 12) calc(var(--u) * 14) 0;
    border-right: 1px solid var(--line);
  }
  .b-pct {
    display: flex;
    align-items: baseline;
    margin-top: calc(var(--u) * 4);
    font-family: 'Iosevka', 'JetBrains Mono Variable', monospace;
  }
  .b-pct b {
    font-size: calc(var(--u) * 62);
    font-weight: 700;
    line-height: 1;
  }
  .b-pct span {
    font-size: calc(var(--u) * 50);
    font-weight: 600;
    color: var(--hot);
    margin-left: calc(var(--u) * 4);
  }
  .b-col {
    list-style: none;
    margin: 0;
    padding: calc(var(--u) * 12) calc(var(--u) * 18);
    display: grid;
    grid-auto-rows: min-content;
    row-gap: calc(var(--u) * 9);
    min-width: 0;
    font-size: max(10.5px, calc(var(--u) * 15));
  }
  .b-col + .b-col {
    border-left: 1px solid var(--line);
  }
  .b-col li {
    display: grid;
    grid-template-columns: calc(var(--u) * 20) minmax(0, 1fr);
    column-gap: calc(var(--u) * 12);
    align-items: center;
    color: var(--muted);
    min-width: 0;
  }
  .st-l {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .st-l em {
    font-style: normal;
    color: #8fa6b6;
    margin-left: calc(var(--u) * 12);
  }
  .b-col li.done {
    color: #d4e1ea;
  }
  .b-col li.active {
    color: var(--hot);
  }
  .b-col li.active em {
    color: #dfe9f0;
  }
  .st-bar {
    grid-column: 2;
    margin-top: calc(var(--u) * 5);
    max-width: calc(var(--u) * 400);
  }
  .si {
    position: relative;
    width: calc(var(--u) * 18);
    height: calc(var(--u) * 18);
    border-radius: 50%;
    border: 1.5px solid #4e6779;
    box-sizing: border-box;
  }
  .done .si {
    background: var(--cyan);
    border-color: var(--cyan);
  }
  .done .si::after {
    content: '';
    position: absolute;
    left: 30%;
    top: 16%;
    width: 28%;
    height: 50%;
    border: solid #04121b;
    border-width: 0 2px 2px 0;
    transform: rotate(45deg);
  }
  .active .si {
    border-color: var(--cyan);
  }
  .active .si::after {
    content: '';
    position: absolute;
    inset: 22%;
    border-radius: 50%;
    background: var(--cyan);
    animation: si-pulse 1.6s ease-in-out infinite;
  }
  .failed .si {
    border-color: var(--red);
  }
  .failed .si::before,
  .failed .si::after {
    content: '';
    position: absolute;
    left: 45%;
    top: 18%;
    width: 1.5px;
    height: 64%;
    background: var(--red);
    transform: rotate(45deg);
  }
  .failed .si::before {
    transform: rotate(-45deg);
  }
  @keyframes si-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  /* Stopping / other */
  .state {
    height: calc(var(--u) * 120);
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 24);
    padding: calc(var(--u) * 14) calc(var(--u) * 18);
    overflow: hidden;
  }
  .st-main {
    flex: 1 1 auto;
    min-width: 0;
  }
  .st-big {
    font-size: calc(var(--u) * 46);
    font-weight: 700;
    color: var(--hot);
    margin-top: calc(var(--u) * 4);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .st-sub {
    margin-top: calc(var(--u) * 6);
    color: #c7d6e0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Fault panel */
  .fpanel {
    height: calc(var(--u) * 186);
    border-color: rgba(255, 90, 54, 0.85);
    background: linear-gradient(180deg, #1a1210, #120f0f 60%, #0f1214);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(var(--u) * 14) calc(var(--u) * 18) calc(var(--u) * 10);
    overflow: hidden;
  }
  .f-head {
    display: grid;
    grid-template-columns: calc(var(--u) * 54) minmax(0, 1fr) auto;
    column-gap: calc(var(--u) * 18);
    align-items: center;
    padding-bottom: calc(var(--u) * 11);
    border-bottom: 1px solid rgba(255, 90, 54, 0.75);
  }
  .f-ico {
    width: calc(var(--u) * 52);
    height: calc(var(--u) * 48);
    fill: none;
    stroke: var(--red);
    stroke-width: 2.6;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .f-ico .fdot {
    fill: var(--red);
    stroke: none;
  }
  .f-t {
    min-width: 0;
  }
  .f-title {
    font-family: 'Iosevka', 'JetBrains Mono Variable', monospace;
    font-weight: 700;
    font-size: calc(var(--u) * 27);
    color: var(--hot);
    line-height: 1.1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .f-sub {
    margin-top: calc(var(--u) * 5);
    font-size: max(10.5px, calc(var(--u) * 16));
    color: #e8eef2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .f-ago {
    align-self: end;
    font-size: max(11px, calc(var(--u) * 18));
    color: var(--red);
    white-space: nowrap;
  }
  .f-body {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    column-gap: calc(var(--u) * 18);
    min-height: 0;
    padding-top: calc(var(--u) * 9);
  }
  .f-body.withsteps {
    grid-template-columns: minmax(0, 1fr) calc(var(--u) * 372);
  }
  .f-log {
    min-width: 0;
    display: grid;
    align-content: start;
    row-gap: calc(var(--u) * 3);
    font-size: max(10px, calc(var(--u) * 14.5));
    color: #e2e9ee;
  }
  .f-line {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .f-steps {
    list-style: none;
    margin: 0;
    padding: 0 0 0 calc(var(--u) * 16);
    border-left: 1px solid rgba(255, 90, 54, 0.35);
    display: grid;
    grid-template-rows: repeat(3, auto);
    grid-auto-flow: column;
    grid-auto-columns: minmax(0, 1fr);
    align-content: start;
    row-gap: calc(var(--u) * 7);
    column-gap: calc(var(--u) * 14);
    font-size: max(10px, calc(var(--u) * 13.5));
    color: var(--muted);
    min-width: 0;
  }
  .f-steps li {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 9);
    white-space: nowrap;
    overflow: hidden;
  }
  .f-steps li.done {
    color: #c7d6e0;
  }
  .f-steps li.failed {
    color: var(--red);
  }
  .f-steps .si {
    width: calc(var(--u) * 13);
    height: calc(var(--u) * 13);
    flex: none;
  }

  /* Drawing */
  .draw {
    flex: 1 1 0;
    min-height: calc(var(--u) * 200);
    display: grid;
    grid-template-columns: minmax(0, 1fr) calc(var(--u) * 304);
    overflow: hidden;
  }
  .die-wrap {
    position: relative;
    min-height: 0;
    background-color: #0c1318;
    background-image: linear-gradient(#161f26 1px, transparent 1px), linear-gradient(90deg, #161f26 1px, transparent 1px);
    background-size: calc(var(--u) * 26) calc(var(--u) * 26);
    background-position: calc(var(--u) * 12) calc(var(--u) * 8);
  }
  .dtitle {
    position: absolute;
    left: calc(var(--u) * 13);
    top: calc(var(--u) * 8);
    font-size: max(10px, calc(var(--u) * 14));
    letter-spacing: 0.05em;
    color: #e3ecf2;
    z-index: 1;
  }
  .die-canvas {
    position: absolute;
    inset: calc(var(--u) * 4) calc(var(--u) * 4) 0;
  }
  .col-wrap {
    display: grid;
    grid-template-rows: calc(var(--u) * 37) minmax(0, 1fr);
    border-left: 1px solid var(--line);
    min-height: 0;
    background-color: #0c1318;
    background-image: linear-gradient(#141c22 1px, transparent 1px), linear-gradient(90deg, #141c22 1px, transparent 1px);
    background-size: calc(var(--u) * 26) calc(var(--u) * 26);
  }
  .chdr {
    display: flex;
    align-items: center;
    padding: 0 calc(var(--u) * 13);
    border-bottom: 1px solid var(--line);
    font-size: max(10.5px, calc(var(--u) * 15));
    white-space: nowrap;
    overflow: hidden;
    color: var(--hot);
    background: var(--panel);
    letter-spacing: 0.02em;
  }
  .chdr b {
    font-weight: 500;
  }
  .chdr b.amber {
    color: var(--amber);
  }
  .chdr b.red {
    color: var(--red);
  }
  .chdr .fp {
    font-family: 'Iosevka', monospace;
    font-weight: 500;
    letter-spacing: 0.06em;
    color: var(--label);
  }
  .chdr .fp.amb {
    color: var(--amber);
  }
  .col-body {
    min-height: 0;
    padding-top: calc(var(--u) * 4);
  }

  /* Idle: launch + last session */
  .launch {
    display: grid;
    grid-template-columns: minmax(0, 2.3fr) minmax(0, 1fr);
    gap: calc(var(--u) * 13);
    height: calc(var(--u) * 62);
  }
  .btn.big {
    height: 100%;
    justify-content: center;
    font-size: max(13px, calc(var(--u) * 24));
    gap: calc(var(--u) * 18);
    border-radius: calc(var(--u) * 3);
  }
  .btn.big svg {
    width: calc(var(--u) * 28);
    height: calc(var(--u) * 28);
  }
  .btn.primary.big {
    font-weight: 500;
    letter-spacing: 0.02em;
  }
  .btn.primary.big b {
    color: #04121b;
    font-family: 'Iosevka', 'JetBrains Mono Variable', monospace;
    font-weight: 700;
    letter-spacing: 0.05em;
  }
  .btn.primary:disabled {
    opacity: 1;
    background: #1a2731;
    border-color: #2f4150;
    color: var(--amber);
  }
  .btn.primary:disabled b {
    color: #c7d6e0;
  }
  .btn.tune {
    border-color: var(--cyan);
    color: var(--cyan);
    background: #0b1116;
  }
  .btn.tune svg {
    stroke-width: 1.6;
  }
  .btn.tune .knob {
    fill: #0b1116;
  }
  .lastrow {
    grid-template-columns: calc(var(--u) * 150) minmax(0, 1fr);
    height: calc(var(--u) * 35);
  }
  .lastrow .k {
    letter-spacing: 0.06em;
  }
  .lastrow .v b {
    font-weight: 600;
  }
  .ls {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* System row */
  .sys {
    height: calc(var(--u) * 40);
    display: grid;
    grid-template-columns: minmax(0, 358fr) minmax(0, 352fr) minmax(0, 286fr);
  }
  .cell {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 14);
    padding: 0 calc(var(--u) * 13);
    border-right: 1px solid var(--line);
    min-width: 0;
    white-space: nowrap;
  }
  .cell:last-child {
    border-right: 0;
  }
  .k2 {
    font-family: 'Iosevka', monospace;
    font-weight: 500;
    color: #dfe9f0;
    font-size: max(10px, calc(var(--u) * 15));
    letter-spacing: 0.05em;
  }
  .v2 {
    color: #edf3f7;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .m2 {
    flex: 1 1 auto;
    min-width: calc(var(--u) * 30);
  }
  .tb-cell {
    justify-content: center;
    padding: 0 calc(var(--u) * 8);
  }
  .titleblock {
    font-family: 'Iosevka', monospace;
    font-weight: 500;
    border: 1px solid #3f5363;
    padding: calc(var(--u) * 4) calc(var(--u) * 9);
    font-size: max(9.5px, calc(var(--u) * 13));
    letter-spacing: 0.02em;
    color: #e6eef4;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .titleblock.long {
    font-size: max(9px, calc(var(--u) * 11.5));
    letter-spacing: 0;
  }

  /* Timeline */
  .tline {
    height: calc(var(--u) * 62);
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: stretch;
  }
  .tline.jobs {
    height: calc(var(--u) * 108);
  }
  .tl-main {
    padding: calc(var(--u) * 9) calc(var(--u) * 16) 0 calc(var(--u) * 14);
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .tline.jobs .tl-main {
    padding-bottom: calc(var(--u) * 10);
  }
  .jobs-area {
    flex: 1 1 auto;
    min-height: 0;
    margin-top: calc(var(--u) * 6);
  }
  .cap2 {
    font-family: 'Iosevka', monospace;
    font-weight: 500;
    font-size: max(10px, calc(var(--u) * 14));
    letter-spacing: 0.06em;
    color: #e3ecf2;
    white-space: nowrap;
    overflow: hidden;
  }
  .cap2 .dimcap {
    font-family: 'JetBrains Mono Variable', 'JetBrains Mono', monospace;
    font-weight: 400;
    font-size: max(9.5px, calc(var(--u) * 12));
    letter-spacing: 0.02em;
  }
  .bars {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: calc(var(--u) * 14);
    margin-top: calc(var(--u) * 9);
  }
  .bar {
    display: flex;
    height: calc(var(--u) * 16);
    border: 1px solid #2f5d7c;
    background: #0b1116;
    overflow: hidden;
  }
  .bar .pre {
    background: #1d6596;
    height: 100%;
    border-right: 1px solid #0e151b;
  }
  .bar .dec {
    flex: 1 1 auto;
    background: var(--cyan);
  }
  .bar.cur {
    border-color: #c9dde9;
  }
  .bar.empty {
    background: transparent;
    border: 1px dashed #2c3c49;
  }
  .bar.failed {
    border: 1px dashed var(--red);
    background-color: rgba(255, 90, 54, 0.08);
    background-image: repeating-linear-gradient(-45deg, rgba(255, 90, 54, 0.55) 0 1px, transparent 1px 6px);
  }
  .legend {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 22);
    padding: 0 calc(var(--u) * 22);
    border-left: 1px solid var(--line);
    margin: calc(var(--u) * 10) 0;
    font-family: 'Iosevka', monospace;
    font-weight: 500;
    font-size: max(10px, calc(var(--u) * 14.5));
    letter-spacing: 0.05em;
    color: #dfe9f0;
    white-space: nowrap;
  }
  .sw {
    display: inline-block;
    width: calc(var(--u) * 11);
    height: calc(var(--u) * 11);
    margin-right: calc(var(--u) * 10);
    vertical-align: -1px;
  }
  .sw.pre {
    background: #1d6596;
  }
  .sw.dec {
    background: var(--cyan);
  }
  .sw.edit {
    background-color: #3e9ad0;
    background-image: repeating-linear-gradient(-45deg, rgba(4, 18, 27, 0.6) 0 1.5px, transparent 1.5px 4px);
  }
  .sw.flt {
    background: var(--red);
  }

  /* Controls */
  .ctrl {
    display: grid;
    grid-template-rows: calc(var(--u) * 58) calc(var(--u) * 41);
  }
  .btnrow {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 13);
    padding: 0 calc(var(--u) * 12);
    border-bottom: 1px solid var(--line);
    min-width: 0;
  }
  .grow {
    flex: 1 1 auto;
  }
  .chip,
  .btn {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 10);
    height: calc(var(--u) * 41);
    border: 1px solid #3a4d5b;
    border-radius: calc(var(--u) * 3);
    padding: 0 calc(var(--u) * 15);
    background: #0b1116;
    white-space: nowrap;
    font-size: max(10.5px, calc(var(--u) * 14));
    flex: none;
  }
  .chip {
    padding: 0 calc(var(--u) * 22) 0 calc(var(--u) * 20);
  }
  .chip .mono {
    letter-spacing: 0.06em;
  }
  .chip.key {
    gap: calc(var(--u) * 14);
  }
  .chip.off {
    color: #6f8392;
    border-color: #2c3c49;
    cursor: default;
  }
  .chip.wide {
    flex: 0 1 calc(var(--u) * 360);
    min-width: 0;
    gap: calc(var(--u) * 16);
  }
  .dots {
    letter-spacing: 0.02em;
    color: var(--hot);
  }
  .copy {
    color: #c7d6e0;
  }
  .chip:hover:not(:disabled):not(.off),
  .btn:hover:not(:disabled) {
    border-color: #6a8aa0;
  }
  .btn svg,
  .chip svg {
    width: calc(var(--u) * 21);
    height: calc(var(--u) * 21);
    fill: none;
    stroke: currentColor;
    stroke-width: 1.9;
    stroke-linecap: round;
    stroke-linejoin: round;
    flex: none;
  }
  .btn.stop {
    border-color: var(--red);
    color: var(--red);
    padding: 0 calc(var(--u) * 26) 0 calc(var(--u) * 22);
  }
  .btn.stop svg {
    width: calc(var(--u) * 16);
    height: calc(var(--u) * 16);
  }
  .btn.danger {
    border-color: var(--red);
    color: var(--red);
    background: #1a100e;
    padding: 0 calc(var(--u) * 24) 0 calc(var(--u) * 20);
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .btn.danger:hover {
    border-color: #ff8a6e;
  }
  .btn.primary {
    border-color: var(--cyan);
    background: var(--cyan);
    color: #04121b;
    font-weight: 600;
  }
  .cline {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 20);
    padding: 0 calc(var(--u) * 16);
    text-align: left;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
  }
  .cline:hover .ltxt {
    color: var(--hot);
  }
  .caret {
    color: #c7d6e0;
  }
  .ltxt {
    overflow: hidden;
    text-overflow: ellipsis;
    color: #e6eef4;
  }
</style>
