<script lang="ts">
  // Silicon, full window. One fixed skeleton in every phase, scaled by --u (1 design px): header, tier strip,
  // status block (model line + hero + two detail rows, or the fault panel in the same box), the drawing
  // (absorbs the remaining height), system row, request timeline / recent jobs, controls. A phase change only
  // changes what the panels say, never where they are:
  //   idle      - the selected tier: dim hero and rows with its configured values, empty timeline, Launch
  //   loading   - startup % in the hero, the six startup steps in the rows, other tiers locked
  //   live-llm  - decode speed (or prefill progress while prefilling), prefill/decode/context/speculative rows
  //   live-img  - diffusion progress, last image / images / size / mode rows, recent jobs
  //   fault     - fault panel over the status block, emptied column, failed request marked
  //   dormant   - (live, vram.dormant set) the GPU is asleep or waking: GDDR6 blocks and the section show
  //               the paged-out allocations as hatched dashed outlines, tiles dark, amber status
  // Controls keep their places: the primary button (Launch / Cancel / Stop / Restart after a fault), Restart,
  // Tune (always: on a running tier the drawer offers "Restart to apply"), Open endpoint / web UI, Console.
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
  import { availabilityText, baselineOf, fmtAgo, fmtDur, fmtEta, lastSessionParts, modelLine, modelShort, vramAtFault } from './text';

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
  const busy = $derived(view === 'loading' || view === 'stopping');
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

  // The status block follows the running session, or (idle) the selected tier with its configured values.
  const focusModel = $derived(s?.model ?? selectedSlot?.model);
  const focusRecipe = $derived((sessionSlot ?? selectedSlot)?.recipe);
  const endpointPort = $derived(s?.endpoint.port ?? focusRecipe?.port);
  const online = $derived(phase === 'live');

  // LLM: during prefill the hero is the prefill progress, never a bright 0.0 tok/s.
  const prefillActive = $derived(!!llm?.prefill && llm.activity === 'prefill');
  const tps = held(() => llm?.decodeTps ?? 0);
  const lastReq = $derived(llm && llm.requests.length ? llm.requests[llm.requests.length - 1] : null);
  const cacheFrac = $derived(lastReq && lastReq.promptTokens > 0 ? lastReq.cachedTokens / lastReq.promptTokens : null);
  const ctxTotal = $derived(llm?.context.totalTokens || focusModel?.ctxTokens || focusRecipe?.ctxTokens || 0);
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

  // Loading: the six startup steps fill the two detail rows (three each).
  const steps = $derived(s?.loading?.steps ?? []);
  const stepsA = $derived(steps.slice(0, 3));
  const stepsB = $derived(steps.slice(3, 6));
  const activeStep = $derived(steps.find((x) => x.state === 'active') ?? null);

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

  // LLM request timeline: last 8 (with a fault, the last slot marks the request that died). Empty when idle.
  const reqN = $derived(view === 'fault' && llm ? 7 : 8);
  const requests = $derived(llm ? llm.requests.slice(-reqN) : []);
  const reqSlots = $derived(Array.from({ length: reqN }, (_, i) => requests[i - (reqN - requests.length)] ?? null));
  // Image slots show recent jobs in every state (empty while idle or loading), never a request timeline.
  const jobsView = $derived(kind === 'image');
  // Was a request in flight when the LLM server died? (activity as last reported)
  const diedInRequest = $derived(view === 'fault' && !!llm && llm.activity !== 'idle');
  const last = $derived(vm.lastSession ? lastSessionParts(vm.lastSession, vm.slots) : null);
  const lastText = $derived.by(() => {
    if (!vm.lastSession || !last) return '';
    const end = vm.lastSession.ended === 'fault' ? `ended in a fault ${fmtAgo(vm.lastSession.endedAgoS)}` : `stopped ${fmtAgo(vm.lastSession.endedAgoS)}`;
    return `last session: ${last.label} · ${last.facts.join(' · ')} · ${end}`;
  });
  const tlCaption = $derived.by(() => {
    const n = requests.length;
    if (view === 'idle') return ' · not running';
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
    if (view === 'idle') return ' · not running';
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
  const selReady = $derived(selectedSlot?.availability === 'ready');
  // The hero's overlay word when nothing runs: the selected tier's state.
  const idleWord = $derived(selReady ? 'NOT RUNNING' : availabilityText(selectedSlot?.availability ?? 'unsupported').toUpperCase());

  /** Lets a "a · b · c" line wrap only after a separator (spaces inside each part do not break). */
  function wrapAtDots(t: string): string {
    return t
      .split(' · ')
      .map((p) => p.replace(/ /g, ' '))
      .join(' · ');
  }

  function reqTitle(r: NonNullable<(typeof requests)[number]>): string {
    return `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${fmtSeconds(r.prefillS)} · ${fmtInt(r.generatedTokens)} tok in ${fmtSeconds(r.decodeS)}`;
  }
</script>

<!-- A hero with nothing to measure (idle, stopping, no data yet): the kind's own panel, dimmed, with a word over it. -->
{#snippet waitHero(word: string, amber: boolean)}
  {#if kind === 'image'}
    <section class="speed panel diff">
      <div class="sp-l">
        <div class="lbl">DIFFUSION PROGRESS</div>
        <div class="hero dh dimh"><span class="unit pre">step</span><b>—</b></div>
      </div>
      <div class="sp-r dgr">
        <div class="d-stats"><span class="wword" class:amb={amber}>{word}</span></div>
        <div class="d-bar">
          <span class="d-meter"><Meter value={0} tall /></span>
          <b class="d-pct dimv">—</b>
        </div>
      </div>
    </section>
  {:else}
    <section class="speed panel">
      <div class="sp-l">
        <div class="lbl">DECODE SPEED</div>
        <div class="hero dimh"><b>—</b><span class="unit">tok/s</span></div>
      </div>
      <div class="sp-r">
        <div class="cap">5-minute history <span class="dimcap">· grid 10 tok/s × 30 s</span></div>
        <div class="chart">
          <Sparkline values={[]} />
          <span class="waiting" class:amb={amber}>{word}</span>
        </div>
      </div>
    </section>
  {/if}
{/snippet}

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

  <!-- Tier strip: the same in every phase. Select, double-click to launch; locked while a session starts or stops. -->
  <div class="tabs" role="tablist" aria-label="Tier">
    {#each vm.slots as slot (slot.id)}
      {@const sel = slot.id === vm.selected}
      {@const locked = busy && slot.id !== s?.slot}
      {@const na = slot.availability !== 'ready'}
      {@const running = !!s && s.slot === slot.id && view !== 'fault'}
      <button
        class="tab"
        class:sel
        class:fault={sel && view === 'fault' && s?.slot === slot.id}
        class:locked
        class:na
        role="tab"
        aria-selected={sel}
        disabled={locked}
        title={locked ? `${slot.label}: locked while ${s?.model.name ?? 'the session'} is ${phaseLabel.toLowerCase()}` : na ? `${slot.label}: ${slot.reason ?? availabilityText(slot.availability)}` : `${slot.label}: ${slot.model.name}`}
        onclick={() => actions.select(slot.id)}
        ondblclick={() => {
          if ((!s || view === 'fault') && !na) actions.launch(slot.id);
        }}
      >
        <span class="tl">
          {#if locked}
            <svg class="lock" viewBox="0 0 16 18" aria-hidden="true"><rect x="2" y="8" width="12" height="9" rx="1.5" /><path d="M5 8V5.5a3 3 0 0 1 6 0V8" /></svg>
          {:else}
            <i class="st" class:run={running} class:bad={na} class:pulse={running && view === 'loading'}></i>
          {/if}
          {slot.label}
        </span>
        <span class="tm" class:bad={na}>{wrapAtDots(na ? `${slot.model.name} · ${availabilityText(slot.availability)}` : modelShort(slot.model))}</span>
      </button>
    {/each}
  </div>

  <!-- Status block: model line + hero + detail rows. Its box never moves; a fault covers it whole. -->
  <div class="block">
    {#if view === 'fault' && fault}
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
    {:else}
      <div class="mline panel" class:dim={view === 'idle'}><span>{modelLine(focusModel, kind, img?.steps)}</span></div>

      <!-- Hero -->
      {#if view === 'live-llm' && llm}
        {#if prefillActive && llm.prefill}
          {@const p = llm.prefill}
          <section class="speed panel pf">
            <div class="sp-l">
              <div class="lbl">PREFILL PROGRESS</div>
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
              <div class="lbl">{dz ? 'LAST DECODE SPEED' : 'DECODE SPEED'}</div>
              <div class="hero" class:faded={!!dz}><b>{fmtTps(tps.current)}</b><span class="unit">tok/s</span></div>
            </div>
            <div class="sp-r">
              <div class="cap">5-minute history <span class="dimcap">· grid 10 tok/s × 30 s</span></div>
              <div class="chart"><Sparkline values={llm.decodeHistory} /></div>
            </div>
          </section>
        {/if}
      {:else if view === 'live-img' && img}
        <section class="speed panel diff">
          <div class="sp-l">
            <div class="lbl">DIFFUSION PROGRESS{imgGen && img.edit ? ' · EDIT' : ''}</div>
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
      {:else if view === 'loading' && s}
        <section class="speed panel diff boot">
          <div class="sp-l">
            <div class="lbl">STARTUP</div>
            <div class="hero pct"><b>{Math.floor((s.loading?.fraction ?? 0) * 100)}</b><span class="unit">%</span></div>
          </div>
          <div class="sp-r dgr">
            <div class="d-stats">
              <span class="cur">{activeStep?.label ?? 'starting'}{#if activeStep?.detail}<span class="muted">{` · ${activeStep.detail}`}</span>{/if}</span>
              <span class="muted sm">VRAM <b>{fmtGiB(vm.vram.usedGiB)}</b> / {fmtGiB(vm.vram.totalGiB)} GiB</span>
            </div>
            <div class="d-bar">
              <span class="d-meter"><Meter value={s.loading?.fraction ?? 0} tall /></span>
              <b class="d-pct">{fmtClock(s.loading?.elapsedS ?? s.uptimeS)}</b>
            </div>
          </div>
        </section>
      {:else if view === 'stopping'}
        {@render waitHero(`STOPPING · RELEASING ${fmtGiB(vm.vram.usedGiB)} GiB`, true)}
      {:else if view === 'other'}
        {@render waitHero('WAITING FOR DATA', false)}
      {:else}
        {@render waitHero(idleWord, !selReady)}
      {/if}

      <!-- Detail rows -->
      {#if view === 'loading'}
        <section class="rows panel steps">
          {#each [stepsA, stepsB] as line, li (li)}
            <div class="srow">
              {#each line as st (st.id)}
                <div class="sc {st.state}">
                  <span class="si"></span>
                  <span class="st-l">{st.label}{#if st.detail}<em>{st.detail}</em>{/if}</span>
                </div>
              {/each}
            </div>
          {/each}
        </section>
      {:else if kind === 'image'}
        <section class="rows panel" class:dim={!img}>
          <div class="row r-img">
            <div class="k">LAST IMAGE</div>
            <div class="v">
              {#if lastJob}<b>{fmtSeconds(lastJob.seconds)}</b><span class="muted">{` · ${lastJob.width}×${lastJob.height}${lastJob.edit ? ' · edit' : ''}`}</span>{:else}<span class="muted">{img ? 'none yet' : '—'}</span>{/if}
            </div>
            <div class="k">IMAGES</div>
            <div class="v">{#if img}<b>{fmtInt(img.imagesThisSession)}</b> this session{:else}<span class="muted">—</span>{/if}</div>
          </div>
          <div class="row r-img">
            <div class="k">SIZE</div>
            <div class="v">{#if focusModel?.imageSize}<b>{focusModel.imageSize}</b>{:else}<span class="muted">—</span>{/if}</div>
            <div class="k">MODE</div>
            <div class="v">{#if focusModel?.mode}<b>{focusModel.mode}</b>{:else}<span class="muted">—</span>{/if}</div>
          </div>
        </section>
      {:else}
        <section class="rows panel" class:dim={!llm}>
          <div class="row r-pre">
            <div class="k">PREFILL</div>
            <div class="v">
              {#if llm?.prefill}
                {#if prefillActive}
                  {#if llm.prefill.cachedTokens !== undefined}<b>{fmtInt(llm.prefill.cachedTokens)}</b>{` tok from prompt cache · `}{/if}{`running ${fmtSeconds(llm.prefill.elapsedS)}`}
                {:else}
                  <b>{fmtInt(llm.prefill.tokens)}</b> tok · <b>{fmtInt(llm.prefill.tps)}</b> tok/s · done in {fmtSeconds(llm.prefill.elapsedS)}
                {/if}
              {:else}<span class="muted">{llm ? 'no request yet' : '—'}</span>{/if}
            </div>
            <div class="k">DECODE</div>
            <div class="v">
              {#if !llm}<span class="muted">—</span>{:else if prefillActive}<span class="muted">waiting for prefill</span>{:else}<b>{fmtInt(llm.generatedTokens)}</b> tok generated{llm.activity === 'idle' ? ' · idle' : ''}{/if}
            </div>
          </div>
          <div class="row r-ctx">
            <div class="k">CONTEXT</div>
            <div class="v ctx">
              <span class="ctxt"
                >{#if llm}<b>{fmtInt(llm.context.usedTokens)}</b>{:else}<span class="muted">—</span>{/if} <small>/ {ctxTotal ? fmtInt(ctxTotal) : '—'} tokens</small>{#if llm}
                  · <b class:red={ctxFrac >= 0.95}>{fmtPct(ctxFrac)}</b>{/if}</span
              >
              <span class="ctxbar"><Meter value={ctxFrac} warn={ctxFrac >= 0.95} /></span>
            </div>
            <div class="k">SPECULATIVE</div>
            <div class="v">
              {#if llm?.spec}<b>{Math.round(llm.spec.acceptancePct)}%</b> accepted · {llm.spec.mode}{:else if llm}<span class="muted">off</span>{:else}<span class="muted">{focusModel?.specMode ?? 'off'}</span>{/if}
            </div>
          </div>
        </section>
      {/if}
    {/if}
  </div>

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
      <span class="titleblock">KLIF · DWG {dwg} · {titleWord} · REV {rev}</span>
    </div>
  </section>

  <!-- Request timeline / recent jobs (empty frames while nothing runs; the last session in the caption) -->
  <section class="tline panel" class:jobs={jobsView}>
    <div class="tl-main">
      <div class="cap2">
        {jobsView ? 'RECENT JOBS' : 'REQUEST TIMELINE'}<span class="dimcap">{jobsView ? jobsCaption : tlCaption}</span>
        {#if view === 'idle' && lastText}<span class="lastcap" class:red={vm.lastSession?.ended === 'fault'} title={vm.lastSession?.model.name}>{lastText}</span>{/if}
      </div>
      {#if jobsView}
        <div class="jobs-area"><JobsChart jobs={img?.recent ?? []} fault={view === 'fault' && !!img} /></div>
      {:else}
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

  <!-- Controls (fixed places) + console line -->
  <section class="ctrl panel">
    <div class="btnrow">
      <button class="chip" onclick={() => actions.copyEndpoint()} disabled={!online} title={online && s ? `Copy http://${s.endpoint.host}:${s.endpoint.port}` : 'Endpoint offline'}>
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1" /><path d="M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1" /></svg
        >
        <span class="mono">{endpointPort ? `:${endpointPort}` : '—'}</span>{#if !online}<span class="copy">offline</span>{/if}
      </button>
      <button class="chip key" onclick={() => actions.copyApiKey()} disabled={!online || !s?.apiKeySet} title={s?.apiKeySet ? 'Copy API key' : 'No API key set'}>
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path
            d="M15 2.5a6.5 6.5 0 0 0-6.2 8.4L2.5 17.2V21.5h4.3v-2.2H9v-2.2h2.2l1.9-1.9A6.5 6.5 0 1 0 15 2.5Zm2.1 6.1a1.7 1.7 0 1 1 0-3.4 1.7 1.7 0 0 1 0 3.4Z"
            fill="currentColor"
            stroke="none"
          /></svg
        >
        {#if s?.apiKeySet}<span class="dots mono">•••••••</span><span class="copy">copy</span>{:else}<span class="copy">no key</span>{/if}
      </button>
      <span class="grow"></span>

      <!-- Primary: Launch / Cancel / Stop / Restart after a fault, always in this place -->
      {#if view === 'idle'}
        <button class="btn act primary" onclick={() => actions.launch(vm.selected)} disabled={!selReady} title={selReady ? `Launch ${selectedSlot?.label}` : `${selectedSlot?.label}: ${selectedSlot?.reason ?? availabilityText(selectedSlot?.availability ?? 'unsupported')}`}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 4l14 8-14 8z" fill="currentColor" stroke="none" /></svg><span class="al">Launch <b>{selectedSlot?.label ?? ''}</b></span>
        </button>
      {:else if view === 'fault'}
        <button class="btn act danger" onclick={() => actions.restart()}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.34-5.66" /><path d="M20 4v5h-5" /></svg><span class="al">Restart <b>{sessionSlot?.label ?? ''}</b></span>
        </button>
      {:else}
        <button class="btn act stop" onclick={() => actions.stop()} disabled={view === 'stopping'}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="6" width="12" height="12" fill="currentColor" stroke="none" /></svg><span class="al"
            >{view === 'loading' ? 'Cancel' : view === 'stopping' ? 'Stopping' : 'Stop'} <b>{sessionSlot?.label ?? ''}</b></span
          >
        </button>
      {/if}

      {#if view === 'fault'}
        <button class="btn" onclick={() => actions.dismiss?.()} title="Back to the launcher">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12H5" /><path d="M11 6l-6 6 6 6" /></svg>Dismiss
        </button>
      {:else}
        <button class="btn" onclick={() => actions.restart()} disabled={!s || busy} title="Stop and launch again with the current settings">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 12a8 8 0 1 1-2.34-5.66" /><path d="M20 4v5h-5" /></svg>Restart
        </button>
      {/if}
      <button class="btn tune" onclick={() => actions.openTune(vm.selected)} title={s && s.slot === vm.selected && view !== 'fault' ? 'Change the settings; Restart to apply them' : 'Change what this tier launches'}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h18M3 12h18M3 18h18" /><circle cx="15" cy="6" r="2.2" class="knob" /><circle cx="8" cy="12" r="2.2" class="knob" /><circle cx="14" cy="18" r="2.2" class="knob" /></svg>Tune
      </button>
      <button class="btn" onclick={() => actions.openEndpoint()} disabled={!online} title={kind === 'image' ? 'Open the sd-server web UI' : 'Open the endpoint'}>
        <svg viewBox="0 0 24 24" aria-hidden="true"
          ><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg
        >{kind === 'image' ? 'Web UI' : 'Endpoint'}
      </button>
      <button class="btn" onclick={() => actions.toggleConsole(view === 'fault' ? true : undefined)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7l5 5-5 5" /><path d="M12 18h8" /></svg>{view === 'fault' ? 'Full log' : 'Console'}
      </button>
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
    /* DIN 1451 (Bahnschrift, part of Windows): the lettering standard of technical drawings. Mono only for
       console text, ports and keys. Labels use the semi-condensed width in caps. */
    --f-ui: 'Bahnschrift', 'DIN Alternate', 'Barlow', 'Segoe UI', sans-serif;
    --f-mono: 'JetBrains Mono Variable', 'JetBrains Mono', ui-monospace, monospace;
    --fs-xs: max(10px, calc(var(--u) * 9.5));
    --fs-s: max(10.5px, calc(var(--u) * 10.5));
    --fs-m: max(11.5px, calc(var(--u) * 11.75));
    --fs-l: max(12.5px, calc(var(--u) * 13));
    --gap: calc(var(--u) * 8);
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    padding: 0 calc(var(--u) * 12) calc(var(--u) * 12);
    background: var(--bg);
    color: var(--ink);
    font-family: var(--f-ui);
    font-size: var(--fs-m);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
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
    font-weight: 500;
  }
  .muted {
    color: var(--muted);
  }
  .red {
    color: var(--red);
  }
  .amb {
    color: var(--amber);
  }
  .sm {
    font-size: var(--fs-s);
  }
  .mono {
    font-family: var(--f-mono);
    font-size: 0.92em;
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
  /* Small-caps drafting labels. */
  .lbl,
  .row .k,
  .cap2,
  .legend,
  .k2,
  .titleblock,
  .chdr .fp,
  .waiting,
  .wword {
    font-stretch: 87.5%;
    text-transform: uppercase;
    letter-spacing: 0.09em;
    font-weight: 400;
  }

  /* Header */
  .hdr {
    position: relative;
    height: calc(var(--u) * 58);
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--line);
    margin: 0 calc(var(--u) * -12);
    padding: 0 calc(var(--u) * 12);
    min-width: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 12);
    min-width: 0;
  }
  .mark {
    width: calc(var(--u) * 42);
    height: calc(var(--u) * 42);
    border: 1px solid #2f4150;
    border-radius: calc(var(--u) * 7);
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
    font-size: calc(var(--u) * 22);
    font-weight: 700;
    color: var(--hot);
    line-height: 1;
    letter-spacing: 0.12em;
  }
  .tagline {
    margin-top: calc(var(--u) * 4);
    font-size: var(--fs-xs);
    font-stretch: 87.5%;
    letter-spacing: 0.08em;
    color: #9fb4c3;
    white-space: nowrap;
  }
  .status {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 9);
    font-size: var(--fs-m);
    white-space: nowrap;
  }
  .frameless .status {
    padding-top: calc(var(--u) * 20);
  }
  .dot {
    width: calc(var(--u) * 9);
    height: calc(var(--u) * 9);
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
  .dot.fault {
    background: var(--red);
  }
  .phase {
    font-weight: 600;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
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
    height: calc(var(--u) * 18);
    background: var(--line2);
    margin: 0 calc(var(--u) * 6);
  }
  .uptime {
    color: #b9cad6;
  }
  .uptime b {
    color: var(--cyan);
    font-weight: 400;
  }
  .uptime.ver {
    color: var(--muted);
  }
  /* Panel-mode button: same hairline chip as the control row. */
  .pbtn {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--u) * 7);
    height: calc(var(--u) * 28);
    padding: 0 calc(var(--u) * 11) 0 calc(var(--u) * 9);
    border: 1px solid #3a4d5b;
    border-radius: calc(var(--u) * 3);
    background: #0b1116;
    color: var(--label);
    font-size: var(--fs-s);
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
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
    width: calc(var(--u) * 16);
    height: calc(var(--u) * 16);
    min-width: 13px;
    min-height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.7;
    stroke-linecap: round;
    stroke-linejoin: round;
    flex: none;
  }
  .pbtn.compact {
    padding: 0 calc(var(--u) * 8);
  }
  .pbtn.compact .pl {
    display: none;
  }

  /* Tier strip */
  .tabs {
    height: calc(var(--u) * 50);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--gap);
  }
  .tab {
    border: 1px solid #33475a;
    border-radius: calc(var(--u) * 3);
    background: var(--panel);
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 4);
    min-width: 0;
    padding: 0 calc(var(--u) * 12);
    text-align: left;
  }
  .tab:hover:not(.sel):not(:disabled) {
    border-color: #557086;
  }
  .tab:disabled {
    opacity: 1;
  }
  .tl {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 7);
    font-size: var(--fs-l);
    font-weight: 600;
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
    color: #e6eef4;
    white-space: nowrap;
    overflow: hidden;
  }
  .tm {
    font-size: var(--fs-s);
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .tm.bad {
    color: var(--amber);
  }
  /* Tier state: mint = ready, amber ring = cannot launch, cyan = this tier runs. */
  .st {
    width: calc(var(--u) * 7);
    height: calc(var(--u) * 7);
    min-width: 6px;
    min-height: 6px;
    border-radius: 50%;
    background: var(--mint);
    flex: none;
  }
  .st.bad {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--amber);
  }
  .st.run {
    background: var(--cyan);
    box-shadow: 0 0 0 calc(var(--u) * 2.5) rgba(90, 182, 235, 0.25);
  }
  .st.pulse {
    animation: si-pulse 1s ease-in-out infinite;
  }
  .tab.sel {
    border-color: var(--cyan);
    background: linear-gradient(180deg, #11263a, #0f1d29);
    box-shadow: inset 0 0 0 1px rgba(90, 182, 235, 0.3);
  }
  .tab.sel .tl {
    color: var(--hot);
  }
  .tab.sel .tm {
    color: #b8d3e4;
  }
  .tab.sel .st.run {
    background: var(--hot);
  }
  .tab.fault {
    border-color: var(--red);
    background: linear-gradient(180deg, #2a1410, #1a100e);
    box-shadow: none;
  }
  .tab.fault .st {
    background: var(--red);
  }
  .tab.na .tl,
  .tab.locked .tl {
    color: var(--muted);
  }
  .tab.locked {
    border-color: #2a3945;
  }
  .tab.locked .tm {
    color: #5f7280;
  }
  .lock {
    width: calc(var(--u) * 10);
    height: calc(var(--u) * 12);
    flex: none;
    fill: none;
    stroke: #8296a5;
    stroke-width: 1.5;
  }

  /* Status block: one fixed box in every phase */
  .block {
    height: calc(var(--u) * 192);
    display: grid;
    grid-template-rows: calc(var(--u) * 28) calc(var(--u) * 84) minmax(0, 1fr);
    gap: var(--gap);
  }
  .mline {
    display: flex;
    align-items: center;
    padding: 0 calc(var(--u) * 14);
    font-size: var(--fs-m);
    color: #e3ecf2;
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim {
    color: #9fb4c3;
  }
  .mline span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Hero panel */
  .speed {
    display: grid;
    grid-template-columns: calc(var(--u) * 250) minmax(0, 1fr);
    min-height: 0;
  }
  .sp-l {
    border-right: 1px solid var(--line);
    padding: calc(var(--u) * 10) calc(var(--u) * 14) 0;
    min-width: 0;
  }
  .lbl {
    font-size: var(--fs-s);
    color: var(--label);
    white-space: nowrap;
  }
  .hero {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 10);
    margin-top: calc(var(--u) * 2);
    white-space: nowrap;
  }
  .hero b {
    font-size: calc(var(--u) * 46);
    font-weight: 300;
    line-height: 1.05;
    letter-spacing: 0.01em;
  }
  .hero .unit {
    font-size: calc(var(--u) * 20);
    font-weight: 300;
    font-stretch: 87.5%;
    letter-spacing: 0.04em;
    color: #9fb4c3;
  }
  .hero.dimh b,
  .hero.dimh .unit {
    color: #4f6170;
  }
  /* GPU asleep: the last figure, not a live one. */
  .hero.faded b,
  .hero.faded .unit {
    color: #7f93a1;
  }
  .hero.dh {
    gap: calc(var(--u) * 8);
  }
  .hero.pct {
    gap: calc(var(--u) * 3);
  }
  .sp-r {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(var(--u) * 8) calc(var(--u) * 10) calc(var(--u) * 8) calc(var(--u) * 12);
    min-width: 0;
    min-height: 0;
  }
  .cap {
    font-size: var(--fs-xs);
    color: #a9bccb;
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
  .waiting {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    font-size: var(--fs-s);
    letter-spacing: 0.18em;
    color: #8296a5;
  }
  .wword {
    font-size: var(--fs-s);
    letter-spacing: 0.18em;
    color: #8296a5;
  }

  /* Prefill hero: 64 segments, one per CU tile */
  .pfr {
    grid-template-rows: auto auto auto;
    align-content: space-between;
  }
  .segs {
    display: grid;
    grid-template-columns: repeat(64, minmax(0, 1fr));
    gap: max(1px, calc(var(--u) * 2));
    height: calc(var(--u) * 16);
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
    gap: calc(var(--u) * 22);
    font-size: var(--fs-m);
    white-space: nowrap;
    overflow: hidden;
    color: #c7d6e0;
  }

  /* Diffusion / startup hero */
  .dgr {
    grid-template-rows: auto auto;
    align-content: center;
    row-gap: calc(var(--u) * 10);
    padding: calc(var(--u) * 8) calc(var(--u) * 14) calc(var(--u) * 8) calc(var(--u) * 16);
  }
  .d-stats {
    display: flex;
    align-items: baseline;
    gap: calc(var(--u) * 28);
    font-size: var(--fs-l);
    white-space: nowrap;
    overflow: hidden;
    color: #dbe6ee;
  }
  .d-stats .sm {
    margin-left: auto;
  }
  .d-stats .cur {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .d-bar {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 14);
  }
  .d-meter {
    flex: 1 1 auto;
    min-width: 0;
  }
  .d-pct {
    font-size: var(--fs-l);
    min-width: 3.4em;
    text-align: right;
  }
  .d-pct.dimv {
    color: #4f6170;
  }

  /* Detail rows: two rows in one panel */
  .rows {
    display: grid;
    grid-template-rows: repeat(2, minmax(0, 1fr));
  }
  .rows.dim .v {
    color: #8296a5;
  }
  .row {
    display: grid;
    align-items: stretch;
    min-height: 0;
  }
  .row + .row {
    border-top: 1px solid var(--line);
  }
  .r-pre,
  .r-img {
    grid-template-columns: calc(var(--u) * 96) minmax(0, 1fr) calc(var(--u) * 96) minmax(0, 1fr);
  }
  .r-ctx {
    grid-template-columns: calc(var(--u) * 96) minmax(0, 1.55fr) calc(var(--u) * 96) minmax(0, 1fr);
  }
  .row .k {
    display: flex;
    align-items: center;
    padding-left: calc(var(--u) * 12);
    font-size: var(--fs-xs);
    color: var(--label);
    border-right: 1px solid var(--line);
    white-space: nowrap;
    overflow: hidden;
  }
  .row .v {
    display: flex;
    align-items: center;
    padding: 0 calc(var(--u) * 12);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
    border-right: 1px solid var(--line);
    min-width: 0;
  }
  .row .v:last-child {
    border-right: 0;
  }
  .v.ctx {
    gap: calc(var(--u) * 12);
  }
  .ctxt small {
    font-size: var(--fs-s);
    color: #a9bccb;
  }
  .ctxbar {
    flex: 1 1 auto;
    min-width: calc(var(--u) * 40);
  }

  /* Loading: the six startup steps in the rows */
  .steps .srow {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    min-height: 0;
  }
  .steps .srow + .srow {
    border-top: 1px solid var(--line);
  }
  .sc {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 9);
    padding: 0 calc(var(--u) * 12);
    color: var(--muted);
    min-width: 0;
    border-right: 1px solid var(--line);
  }
  .sc:last-child {
    border-right: 0;
  }
  .sc.done {
    color: #c7d6e0;
  }
  .sc.active {
    color: var(--hot);
  }
  .sc.failed {
    color: var(--red);
  }
  .st-l {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .st-l em {
    font-style: normal;
    color: #8fa6b6;
    margin-left: calc(var(--u) * 8);
  }
  .sc.active .st-l em {
    color: #dfe9f0;
  }
  .si {
    position: relative;
    width: calc(var(--u) * 13);
    height: calc(var(--u) * 13);
    min-width: 11px;
    min-height: 11px;
    border-radius: 50%;
    border: 1.5px solid #4e6779;
    box-sizing: border-box;
    flex: none;
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
    border-width: 0 1.5px 1.5px 0;
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
  @media (prefers-reduced-motion: reduce) {
    .dot.pulse,
    .st.pulse,
    .active .si::after {
      animation: none;
    }
  }

  /* Fault panel: covers the whole status block */
  .fpanel {
    grid-row: 1 / -1;
    border-color: rgba(255, 90, 54, 0.85);
    background: linear-gradient(180deg, #1a1210, #120f0f 60%, #0f1214);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(var(--u) * 12) calc(var(--u) * 16) calc(var(--u) * 10);
    overflow: hidden;
  }
  .f-head {
    display: grid;
    grid-template-columns: calc(var(--u) * 40) minmax(0, 1fr) auto;
    column-gap: calc(var(--u) * 14);
    align-items: center;
    padding-bottom: calc(var(--u) * 10);
    border-bottom: 1px solid rgba(255, 90, 54, 0.75);
  }
  .f-ico {
    width: calc(var(--u) * 38);
    height: calc(var(--u) * 35);
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
    font-weight: 600;
    font-size: calc(var(--u) * 19);
    color: var(--hot);
    line-height: 1.15;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .f-sub {
    margin-top: calc(var(--u) * 4);
    color: #e8eef2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .f-ago {
    align-self: end;
    color: var(--red);
    white-space: nowrap;
  }
  .f-body {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    column-gap: calc(var(--u) * 16);
    min-height: 0;
    padding-top: calc(var(--u) * 9);
  }
  .f-body.withsteps {
    grid-template-columns: minmax(0, 1fr) calc(var(--u) * 330);
  }
  .f-log {
    min-width: 0;
    display: grid;
    align-content: start;
    row-gap: calc(var(--u) * 4);
    font-family: var(--f-mono);
    font-size: var(--fs-xs);
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
    padding: 0 0 0 calc(var(--u) * 14);
    border-left: 1px solid rgba(255, 90, 54, 0.35);
    display: grid;
    grid-template-rows: repeat(3, auto);
    grid-auto-flow: column;
    grid-auto-columns: minmax(0, 1fr);
    align-content: start;
    row-gap: calc(var(--u) * 7);
    column-gap: calc(var(--u) * 12);
    font-size: var(--fs-s);
    color: var(--muted);
    min-width: 0;
  }
  .f-steps li {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8);
    white-space: nowrap;
    overflow: hidden;
  }
  .f-steps li.done {
    color: #c7d6e0;
  }
  .f-steps li.failed {
    color: var(--red);
  }

  /* Drawing */
  .draw {
    flex: 1 1 0;
    min-height: calc(var(--u) * 200);
    display: grid;
    grid-template-columns: minmax(0, 1fr) calc(var(--u) * 300);
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
    left: calc(var(--u) * 12);
    top: calc(var(--u) * 8);
    font-size: var(--fs-xs);
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
    color: #cfdde6;
    z-index: 1;
  }
  .die-canvas {
    position: absolute;
    inset: calc(var(--u) * 4) calc(var(--u) * 4) 0;
  }
  .col-wrap {
    display: grid;
    grid-template-rows: calc(var(--u) * 32) minmax(0, 1fr);
    border-left: 1px solid var(--line);
    min-height: 0;
    background-color: #0c1318;
    background-image: linear-gradient(#141c22 1px, transparent 1px), linear-gradient(90deg, #141c22 1px, transparent 1px);
    background-size: calc(var(--u) * 26) calc(var(--u) * 26);
  }
  .chdr {
    display: flex;
    align-items: center;
    padding: 0 calc(var(--u) * 12);
    border-bottom: 1px solid var(--line);
    font-size: var(--fs-s);
    white-space: nowrap;
    overflow: hidden;
    color: var(--hot);
    background: var(--panel);
  }
  .chdr b.amber {
    color: var(--amber);
  }
  .chdr b.red {
    color: var(--red);
  }
  .chdr .fp {
    color: var(--label);
  }
  .chdr .fp.amb {
    color: var(--amber);
  }
  .col-body {
    min-height: 0;
    padding-top: calc(var(--u) * 4);
  }

  /* System row */
  .sys {
    height: calc(var(--u) * 34);
    display: grid;
    grid-template-columns: minmax(0, 358fr) minmax(0, 352fr) minmax(0, 286fr);
  }
  .cell {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 12);
    padding: 0 calc(var(--u) * 12);
    border-right: 1px solid var(--line);
    min-width: 0;
    white-space: nowrap;
  }
  .cell:last-child {
    border-right: 0;
  }
  .k2 {
    font-size: var(--fs-xs);
    color: var(--label);
  }
  .v2 {
    color: #e3ecf2;
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
    border: 1px solid #3f5363;
    padding: calc(var(--u) * 3) calc(var(--u) * 8);
    font-size: var(--fs-xs);
    letter-spacing: 0.06em;
    color: #cfdde6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Timeline / recent jobs: one height for both */
  .tline {
    height: calc(var(--u) * 84);
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: stretch;
  }
  .tl-main {
    padding: calc(var(--u) * 9) calc(var(--u) * 14) calc(var(--u) * 10) calc(var(--u) * 12);
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .jobs-area {
    flex: 1 1 auto;
    min-height: 0;
    margin-top: calc(var(--u) * 6);
  }
  .cap2 {
    display: flex;
    align-items: baseline;
    min-width: 0;
    font-size: var(--fs-xs);
    color: #cfdde6;
    white-space: nowrap;
    overflow: hidden;
  }
  .cap2 .dimcap {
    /* A flex item drops its leading space: keep the gap before " · ..." explicitly. */
    padding-left: 0.35em;
    text-transform: none;
    letter-spacing: 0.02em;
    font-stretch: 100%;
  }
  .lastcap {
    margin-left: auto;
    padding-left: calc(var(--u) * 16);
    text-transform: none;
    letter-spacing: 0.01em;
    font-stretch: 100%;
    color: #8fa6b6;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .lastcap.red {
    color: var(--red);
  }
  .bars {
    flex: 1 1 auto;
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    align-items: center;
    gap: calc(var(--u) * 12);
  }
  .bar {
    display: flex;
    height: calc(var(--u) * 20);
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
    flex-direction: column;
    justify-content: center;
    gap: calc(var(--u) * 7);
    padding: 0 calc(var(--u) * 18);
    border-left: 1px solid var(--line);
    margin: calc(var(--u) * 10) 0;
    font-size: var(--fs-xs);
    color: #cfdde6;
    white-space: nowrap;
  }
  .sw {
    display: inline-block;
    width: calc(var(--u) * 9);
    height: calc(var(--u) * 9);
    margin-right: calc(var(--u) * 8);
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
    grid-template-rows: calc(var(--u) * 52) calc(var(--u) * 30);
  }
  .btnrow {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 8);
    padding: 0 calc(var(--u) * 10);
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
    gap: calc(var(--u) * 8);
    height: calc(var(--u) * 34);
    border: 1px solid #3a4d5b;
    border-radius: calc(var(--u) * 3);
    padding: 0 calc(var(--u) * 12);
    background: #0b1116;
    white-space: nowrap;
    font-size: var(--fs-m);
    flex: none;
  }
  .chip {
    padding: 0 calc(var(--u) * 14) 0 calc(var(--u) * 12);
    color: #c7d6e0;
  }
  .chip.key {
    gap: calc(var(--u) * 10);
  }
  .chip:disabled {
    opacity: 0.55;
  }
  .dots {
    letter-spacing: 0.02em;
    color: var(--hot);
  }
  .copy {
    color: #8fa6b6;
  }
  .chip:hover:not(:disabled),
  .btn:hover:not(:disabled) {
    border-color: #6a8aa0;
  }
  .btn svg,
  .chip svg {
    width: calc(var(--u) * 16);
    height: calc(var(--u) * 16);
    min-width: 13px;
    min-height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    flex: none;
  }
  /* The primary place: one width for Launch / Cancel / Stop / Restart, so nothing beside it moves. */
  .btn.act {
    width: calc(var(--u) * 210);
    justify-content: flex-start;
    font-weight: 500;
  }
  .al {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .btn.act b {
    font-weight: 600;
    font-stretch: 87.5%;
    letter-spacing: 0.08em;
    color: inherit;
  }
  .btn.primary {
    border-color: var(--cyan);
    background: var(--cyan);
    color: #04121b;
  }
  .btn.primary:disabled {
    opacity: 1;
    background: #1a2731;
    border-color: #2f4150;
    color: #8296a5;
  }
  .btn.stop {
    border-color: var(--red);
    color: var(--red);
  }
  .btn.stop svg {
    width: calc(var(--u) * 12);
    height: calc(var(--u) * 12);
    min-width: 10px;
    min-height: 10px;
  }
  .btn.danger {
    border-color: var(--red);
    color: var(--red);
    background: #1a100e;
  }
  .btn.danger:hover {
    border-color: #ff8a6e;
  }
  .btn.tune {
    border-color: #2f6f96;
    color: var(--cyan);
  }
  .btn.tune svg {
    stroke-width: 1.6;
  }
  .btn.tune .knob {
    fill: #0b1116;
  }
  .cline {
    display: flex;
    align-items: center;
    gap: calc(var(--u) * 12);
    padding: 0 calc(var(--u) * 14);
    text-align: left;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    font-family: var(--f-mono);
    font-size: var(--fs-xs);
  }
  .cline:hover .ltxt {
    color: var(--hot);
  }
  .caret {
    color: #8fa6b6;
  }
  .ltxt {
    overflow: hidden;
    text-overflow: ellipsis;
    color: #b9cad6;
  }
</style>
