<script lang="ts">
  // Full window: one hardware front panel on one fixed skeleton. Top to bottom every region keeps its place
  // and height in every phase (idle, loading, live LLM, live image, stopping, fault, dormant); a phase only
  // changes what the panels read:
  //   header        brand, status lamp + phase, uptime / elapsed (version while idle), panel + window keys
  //   System control  the rotary MODE selector (one detent per System, + adds one) with its state column, the BACKEND toggle (opens Tune)
  //   model line    the running model, or (idle) the selected tier's
  //   status block  hero plate + two detail rows, or the fault plate over both (see Status)
  //   dial area     the VRAM dial (idle: fit preview of the selected tier) + the context / generation dial
  //   system row    RAM and CPU with their LED meters
  //   timeline      request timeline / recent jobs; idle: empty frame, the last session in the caption
  //   controls      endpoint + API key chips, then Launch / Cancel / Stop / Restart (one place, one width),
  //                 Restart or Dismiss, Tune (always: a running tier gets "Restart to apply" in the drawer),
  //                 Records (icon only), Endpoint / Web UI, Console / Full log; the console line under it
  import { onMount, tick } from 'svelte';
  import type { Actions, SystemId, ViewModel } from '../../../lib/model/types';
  import { EXTERNAL_NOTE, EXTERNAL_TITLE, KIND_LABEL, canLaunch, canStop, doLaunch, idleState, isPendingLaunch, launchCtl } from '../../../lib/model/systems';
  import { fmtClock, fmtCtx, fmtInt } from '../../../lib/model/format';
  import {
    PHASE_LABEL,
    kindOf,
    lastSessionText,
    modelLine,
    phaseOf,
    pointerSlot,
    sessionSpanS,
    sessionVram,
    sleepOf,
    slotById,
    viewOf,
  } from '../theme';
  import Mark from '../parts/Mark.svelte';
  import Led from '../parts/Led.svelte';
  import LedBar from '../parts/LedBar.svelte';
  import Selector from '../parts/Selector.svelte';
  import BackendToggle from '../parts/BackendToggle.svelte';
  import VramDial from '../parts/VramDial.svelte';
  import ContextDial from '../parts/ContextDial.svelte';
  import Timeline from '../parts/Timeline.svelte';
  import RecentJobs from '../parts/RecentJobs.svelte';
  import Icon from '../parts/Icon.svelte';
  import WinCtl from '../parts/WinCtl.svelte';
  import Status from './Status.svelte';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  const s = $derived(vm.session);
  const phase = $derived(phaseOf(vm));
  const view = $derived(viewOf(vm));
  const kind = $derived(kindOf(vm));
  const ptr = $derived(pointerSlot(vm));
  const ptrSlot = $derived(slotById(vm, ptr));
  const selSlot = $derived(slotById(vm, ptr));
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);
  const busy = $derived(view === 'loading' || view === 'stopping');
  const online = $derived(s?.phase === 'live');
  /** The launch control: with conflicts it reads "Stop System 1 & launch" and sends stopOthers. */
  const ctl = $derived(launchCtl(vm, selSlot));
  /** We may stop / restart it (not an external server, not a node that only lets us look). */
  const mine = $derived(!!selSlot && selSlot.controllable && !selSlot.external);
  const heldWhy = $derived(selSlot?.external ? 'External server: it runs where it was started.' : !selSlot?.controllable ? 'This node does not allow launching.' : '');
  const runLabel = $derived(ptrSlot?.label ?? s?.system ?? '');
  const backend = $derived(s?.model.backend ?? ptrSlot?.model.backend ?? null);
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const frameless = $derived(vm.host?.frameless ?? false);

  // GPU dormant (live session, vm.vram.dormant set): amber lamp and words in the header and the selector.
  const sleep = $derived(s?.phase === 'live' ? sleepOf(vm.vram) : null);
  const statusWord = $derived(sleep ? (sleep.phase === 'waking' ? 'GPU WAKING' : 'GPU ASLEEP') : PHASE_LABEL[phase]);
  const statusTone = $derived(phase === 'fault' ? 'orange' : sleep ? 'amber' : 'cyan');
  /** The selected System's GPU is dormant: its state word. */
  const sleepWord = $derived(sleep ? (sleep.phase === 'waking' ? 'waking' : 'asleep') : null);

  // Double-click on a System launches it when it is ready and nothing is in its way.
  function launchTier(id: SystemId) {
    const sl = slotById(vm, id);
    if (sl && canLaunch(sl) && sl.conflicts.length === 0) void actions.launch(id).catch(() => {});
  }

  // Endpoint chip: the session's port, or the selected System's configured one.
  const port = $derived(s?.endpoint.port ?? ptrSlot?.command?.port);

  // Dial area: the VRAM dial follows the phase; a young session's cliff spans the session.
  const expected = $derived(selSlot?.expectedVram ?? []);
  const baseline = $derived(vm.vram.baselineGiB ?? vm.vram.usedGiB);
  const target = $derived.by(() => {
    if (view !== 'loading' || !ptrSlot?.expectedVram?.length) return null;
    const base = vm.vram.baselineGiB ?? vm.vram.layers.find((l) => l.id === 'other')?.gib ?? 0;
    return base + ptrSlot.expectedVram.reduce((a, l) => a + l.gib, 0);
  });
  const dialVram = $derived(s ? sessionVram(vm.vram, sessionSpanS(s)) : vm.vram);

  // Context dial (LLM): the live or last reading, else the configured window with the needle at rest.
  const model = $derived(s?.model ?? ptrSlot?.model);
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || 0);
  const ctxUsed = $derived(llm?.context.usedTokens ?? 0);
  const generating = $derived(view === 'live-img' && !!img && img.activity === 'generating');
  // The dial and its readout are one centred group; the dial takes what the panel leaves, up to 330 u.
  let cw = $state(0);
  let ch = $state(0);
  let rh = $state(0);

  // Timeline / recent jobs: one place, one height; the caption says what the frame holds.
  const jobsView = $derived(kind === 'image');
  const faultHadWork = $derived(view === 'fault' && (!!llm || !!img || !!gen));
  const diedInRequest = $derived(view === 'fault' && !!llm && llm.activity !== 'idle');
  const last = $derived(view === 'idle' ? lastSessionText(vm) : null);
  const tlCaption = $derived.by(() => {
    if (view === 'idle') return 'not running';
    if (view === 'loading') return jobsView ? 'no jobs yet' : 'no requests yet';
    if (kind !== 'llm' && kind !== 'image') return view === 'fault' ? 'failed' : 'no per-request log for this kind';
    if (jobsView) {
      if (!img) return view === 'fault' ? 'no jobs: failed during startup' : 'no jobs yet';
      return `seconds per image · ${fmtInt(img.imagesThisSession)} this session${view === 'fault' ? ', then the fault' : ''}`;
    }
    if (!llm) return view === 'fault' ? 'no requests: failed during startup' : 'no requests yet';
    const head = `session: ${fmtInt(llm.totals.requests)} requests · ${fmtInt(llm.totals.generatedTokens)} tok generated`;
    return view === 'fault' ? `${head}, then ${diedInRequest ? 'the request that died' : 'the fault'}` : head;
  });

  // Panel mode (read-only mini layout on the small status screen): always offered when a target exists.
  const panelAvailable = $derived(vm.host?.panel?.available ?? false);
  const panelActive = $derived(vm.host?.panel?.active ?? false);
  const panelTip = $derived(
    panelActive
      ? 'Leave panel mode'
      : `Panel mode: show on the small screen${vm.host?.panel?.target ? ` (${vm.host.panel.target})` : ''}`,
  );
  // The panel key carries a PANEL label while the header has room for it. When the brand tagline would be
  // clipped (frameless window controls, long phase names, narrow windows) it drops to the bare glyph;
  // title and aria-label stay. Re-measured whenever the header width, phase or fonts change.
  let hdrW = $state(0);
  let fontsTick = $state(0);
  let tagEl = $state<HTMLElement>();
  let compactPm = $state(false);
  onMount(() => {
    const bump = () => fontsTick++;
    document.fonts?.addEventListener('loadingdone', bump);
    void document.fonts?.ready.then(bump);
    return () => document.fonts?.removeEventListener('loadingdone', bump);
  });
  $effect(() => {
    void hdrW;
    void fontsTick;
    void statusWord;
    void frameless;
    void panelAvailable;
    void panelActive;
    compactPm = false;
    void tick().then(() => {
      if (tagEl && tagEl.scrollWidth > tagEl.clientWidth + 1) compactPm = true;
    });
  });
</script>

{#snippet panelBtn()}
  <button class="pm" class:bare={compactPm} type="button" onclick={() => actions.togglePanel?.()} title={panelTip} aria-label={panelTip}>
    <Icon name="screen" size="calc(18 * var(--u))" />
    {#if !compactPm}<span class="pl">{panelActive ? 'WINDOW' : 'PANEL'}</span>{/if}
  </button>
{/snippet}

<div class="full" data-view={view}>
  <!-- header (window drag region; draws its own window controls when the host is frameless) -->
  <header class="hdr" data-tauri-drag-region bind:clientWidth={hdrW}>
    <div class="brand" data-tauri-drag-region>
      <span class="mk"><Mark /></span>
      <span class="word">KLIF</span>
      <span class="tag" bind:this={tagEl}>Koksny.com LOCAL INFERENCE FORNICATOR</span>
    </div>
    <div class="status" data-tauri-drag-region>
      <Led on={phase !== 'idle'} tone={statusTone} size="calc(11 * var(--u))" pulse={sleep?.phase === 'waking'} />
      <span class="ph {statusTone}" class:off={phase === 'idle'}>{statusWord}</span>
      <span class="vsep"></span>
      {#if s}
        <span class="up">{view === 'loading' ? 'elapsed' : 'uptime'} <b>{fmtClock(view === 'loading' ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS)}</b></span>
      {:else}
        <span class="up ver">v{vm.host?.appVersion ?? ''}</span>
      {/if}
      {#if frameless}
        <span class="vsep wsep"></span>
        <div class="grp">
          {#if panelAvailable}{@render panelBtn()}{/if}
          <WinCtl
            maximized={vm.host?.maximized ?? false}
            onmin={() => actions.minimize?.()}
            onmax={() => actions.toggleMaximize?.()}
            onclose={() => actions.closeWindow?.()}
          />
        </div>
      {:else if panelAvailable}
        <span class="vsep"></span>
        {@render panelBtn()}
      {/if}
    </div>
  </header>

  <!-- tier control: one layout and one height in every phase -->
  <div class="tier">
    <section class="panel sel">
      <span class="lbl corner">MODE</span>
      <button class="addsys" type="button" title="Add a System" aria-label="Add a System" onclick={() => actions.openTune(undefined, { add: true })}>
        <Icon name="plus" size="calc(13 * var(--u))" />
      </button>
      <div class="selbox">
        <Selector {vm} pointer={ptr} tone={statusTone} {sleepWord} onselect={(id) => actions.select(id)} onlaunch={launchTier} />
      </div>
    </section>
    <section class="panel be">
      <span class="lbl corner">BACKEND</span>
      <div class="bebox">
        <BackendToggle {backend} onclick={() => actions.openTune(selSlot?.id)} />
      </div>
    </section>
  </div>

  <!-- model line -->
  <section class="panel model" class:dim={!s}>
    <span class="lbl">{s ? 'ACTIVE MODEL' : 'SELECTED MODEL'}</span>
    <span class="mline" title={modelLine(s, ptrSlot)}>{modelLine(s, ptrSlot)}</span>
  </section>

  <!-- status block: hero + two rows, or the fault plate over both -->
  <Status {vm} />

  <!-- dial area: the same two panels in every phase -->
  <div class="dials">
    <section class="panel vram">
      {#if view === 'idle'}
        <VramDial vram={vm.vram} mode="fit" fit={{ baseline, layers: expected }} />
      {:else}
        <VramDial vram={dialVram} mode={view === 'fault' ? 'fault' : 'live'} {target} />
      {/if}
    </section>
    <section class="panel ctx" bind:clientWidth={cw} bind:clientHeight={ch}>
      <span class="lbl corner">{kind === 'image' ? 'GENERATION' : kind === 'llm' ? 'CONTEXT' : KIND_LABEL[kind].toUpperCase()}</span>
      <div class="cgroup" style="--d: max(calc(60 * var(--u)), min(calc(330 * var(--u)), calc({cw}px - 48 * var(--u)), calc({ch}px - {rh}px - 72 * var(--u))))">
        <div class="cdial">
          {#if kind === 'image'}
            {@const st = img?.steps ?? 0}
            <ContextDial
              used={generating ? (img?.step ?? 0) : 0}
              total={st}
              hiLabel={st ? String(st) : '—'}
              majors={st && st <= 24 ? st : 4}
              minorPer={st && st <= 12 ? 4 : st && st <= 24 ? 2 : 10}
              dim={!generating}
              label="Generation"
            />
          {:else if kind !== 'llm'}
            <ContextDial used={0} total={0} dim label="Requests" />
          {:else}
            <!-- idle / loading: the configured window with the needle at rest; at a fault the last reading, unlit -->
            <ContextDial used={ctxUsed} total={ctxTotal} dim={!llm || view === 'fault' || view === 'stopping' || (!!sleep && llm.activity === 'idle')} />
          {/if}
        </div>
        <div class="cread" bind:clientHeight={rh}>
          {#if kind === 'image'}
            {#if generating && img}
              <span class="cv"><b>step {img.step}</b> / {img.steps}</span>
              <span class="cs">{img.width} × {img.height} · {img.edit ? 'edit job' : 'new image'}</span>
            {:else if img}
              <span class="cv dimv">no job running</span>
              <span class="cs">{img.steps} sampling steps per image</span>
            {:else}
              <span class="cv dimv">{view === 'loading' ? 'server loading' : view === 'fault' ? 'no job ran' : 'not running'}</span>
              <span class="cs">{model?.imageSize ? `default size ${model.imageSize.replace(/\s*x\s*/i, ' × ')}` : ''}{model?.mode ? ` · ${model.mode}` : ''}</span>
            {/if}
          {:else if kind !== 'llm'}
            {#if gen}
              <span class="cv"><b>{fmtInt(gen.requestsTotal ?? 0)}</b> requests</span>
              <span class="cs">{(gen.requestsInFlight ?? 0) > 0 ? `${gen.requestsInFlight} in flight` : 'idle'}{gen.lastActivityS !== undefined ? ` · last ${Math.round(gen.lastActivityS)} s ago` : ''}</span>
            {:else}
              <span class="cv dimv">{view === 'loading' ? 'server loading' : 'not running'}</span>
              <span class="cs">{model?.name ?? 'no preset'}</span>
            {/if}
          {:else if llm}
            <span class="cv"><b>{fmtInt(ctxUsed)}</b> / {fmtInt(ctxTotal)} tokens · {ctxTotal ? Math.round((ctxUsed / ctxTotal) * 100) : 0}%</span>
            <span class="cs">{view === 'fault' ? 'last reading before the fault' : `${fmtCtx(ctxTotal)} window${model?.kvType ? ` · KV ${model.kvType}` : ''}`}</span>
          {:else}
            <span class="cv dimv">— / {ctxTotal ? fmtInt(ctxTotal) : '—'} tokens</span>
            <span class="cs">{ctxTotal ? `${fmtCtx(ctxTotal)} preset` : 'no preset'}{model?.kvType ? ` · KV ${model.kvType}` : ''}</span>
          {/if}
        </div>
      </div>
    </section>
  </div>

  <!-- system row -->
  <section class="panel sys">
    <span class="lbl">RAM</span>
    <span class="si"><Icon name="ram" size="calc(20 * var(--u))" /></span>
    <span class="sv">{vm.machine.ramUsedGiB.toFixed(1)} / {vm.machine.ramTotalGiB.toFixed(1)} GiB{vm.machine.ramType ? ` ${vm.machine.ramType}` : ''}</span>
    <span class="sm"><LedBar fraction={vm.machine.ramTotalGiB > 0 ? vm.machine.ramUsedGiB / vm.machine.ramTotalGiB : 0} segments={16} label="System RAM used" /></span>
    <span class="vsep"></span>
    <span class="lbl">CPU</span>
    <span class="si"><Icon name="cpu" size="calc(20 * var(--u))" /></span>
    <span class="sv">{vm.machine.cpuName} · {Math.round(vm.machine.cpuPct)}%</span>
    <span class="sm"><LedBar fraction={vm.machine.cpuPct / 100} segments={16} label="CPU load" /></span>
  </section>

  <!-- request timeline / recent jobs: one height for both, present in every phase -->
  <section class="panel tline">
    <div class="tmain">
      <div class="thead">
        <span class="lbl">{jobsView ? 'RECENT JOBS' : 'REQUEST TIMELINE'}</span>
        <span class="tcap">{tlCaption}</span>
        {#if last}<span class="lastcap" class:hot={last.fault} title={last.text}>{last.text}</span>{/if}
      </div>
      <div class="tbody">
        {#if jobsView}
          <RecentJobs jobs={img?.recent ?? []} none={img && view !== 'fault' ? 'no images yet' : ''} />
        {:else}
          <Timeline
            requests={llm?.requests ?? []}
            failed={view === 'fault' && !!llm}
            failedTitle={diedInRequest ? 'The server died during this request' : 'The server died with no request in flight'}
          />
        {/if}
      </div>
    </div>
    <div class="legend">
      {#if jobsView}
        <span><i class="sw dc"></i>new</span>
        <span><i class="sw pf"></i>edit</span>
      {:else if kind === 'llm'}
        <span><i class="sw pf"></i>prefill</span>
        <span><i class="sw dc"></i>decode</span>
      {/if}
      {#if faultHadWork && !jobsView}<span><i class="sw ft"></i>fault</span>{/if}
    </div>
  </section>

  <!-- controls: fixed order and places -->
  <section class="panel bar">
    <span class="lbl">ENDPOINT</span>
    <button
      class="chip ep"
      disabled={!online}
      onclick={() => actions.copyEndpoint(selSlot?.id)}
      title={online && s ? `Copy ${s.endpoint.host}:${s.endpoint.port}` : 'The server is not running'}
    >
      <span class="port mono">{port ? `:${port}` : '—'}</span>{#if !online}<span class="off">offline</span>{/if}
    </button>
    <span class="vsep"></span>
    <span class="lbl">API KEY</span>
    <button class="chip key" disabled={!online || !s?.apiKeySet} onclick={() => actions.copyApiKey()} aria-label="Copy API key" title={s?.apiKeySet ? 'Copy API key' : 'No API key set'}>
      {#if s?.apiKeySet}<span class="dots mono">•••••••</span><span class="copy">copy</span>{:else}<span class="dots none">none</span>{/if}
    </button>
    <span class="grow"></span>

    <!-- primary: Launch / Cancel / Stop / Restart after a fault, always in this place and width -->
    {#if selSlot?.external}
      <button class="btn act ext" disabled title={EXTERNAL_TITLE}><span class="al">{EXTERNAL_NOTE}</span></button>
    {:else if view === 'idle' && isPendingLaunch(selSlot)}
      <button class="btn act stop" onclick={() => actions.stop(selSlot?.id)} disabled={!canStop(selSlot)} title={selSlot?.reason ?? 'Cancel the launch'}>
        <span class="sq"></span><span class="al">Cancel <b>{selSlot?.label ?? ''}</b></span>
      </button>
    {:else if view === 'idle'}
      <button
        class="btn act launch primary"
        disabled={!ctl.enabled}
        onclick={() => doLaunch(actions, selSlot, ctl)}
        title={ctl.enabled ? (ctl.stopOthers ? `${ctl.text}: ${selSlot?.reason ?? ''}` : `Launch ${selSlot?.label ?? ''}`) : `${selSlot?.label ?? ''}: ${ctl.blocked}`}
      >
        <span class="play"><Icon name="play" size="calc(16 * var(--u))" /></span><span class="al">{#if ctl.stopOthers}{ctl.text}{:else}Launch <b>{selSlot?.label ?? ''}</b>{/if}</span>
      </button>
    {:else if view === 'fault'}
      <button class="btn act hot" onclick={() => actions.restart(selSlot?.id)} disabled={!mine}>
        <Icon name="restart" size="calc(17 * var(--u))" /><span class="al">Restart <b>{runLabel}</b></span>
      </button>
    {:else}
      <button class="btn act stop" onclick={() => actions.stop(selSlot?.id)} disabled={view === 'stopping' || !canStop(selSlot)} title={heldWhy}>
        <span class="sq"></span><span class="al">{view === 'loading' ? 'Cancel' : view === 'stopping' ? 'Stopping' : 'Stop'} <b>{runLabel}</b></span>
      </button>
    {/if}

    {#if view === 'fault'}
      <button class="btn rs" onclick={() => actions.dismiss(selSlot?.id)} disabled={!mine} title="Back to the launcher"><Icon name="back" size="calc(17 * var(--u))" />Dismiss</button>
    {:else}
      <button class="btn rs" onclick={() => actions.restart(selSlot?.id)} disabled={!s || busy || !mine} title={heldWhy || 'Stop and launch again with the current settings'}>
        <Icon name="restart" size="calc(17 * var(--u))" />Restart
      </button>
    {/if}
    <button
      class="btn tune"
      onclick={() => actions.openTune(selSlot?.id)}
      title={s && view !== 'fault' ? 'Change the settings; Restart to apply them' : 'Change what this System launches'}
    >
      <Icon name="tune" size="calc(17 * var(--u))" />Tune
    </button>
    <!-- Icon only: the row is full (the endpoint and key chips are what gives), so Records takes just its icon. -->
    <button class="btn rec" onclick={() => actions.openRecords()} title="Records: the best each model reached" aria-label="Records">
      <Icon name="records" size="calc(17 * var(--u))" />
    </button>
    <button class="btn op" onclick={() => actions.openEndpoint(selSlot?.id)} disabled={!online} title={kind === 'image' ? 'Open the sd-server web UI' : 'Open the endpoint'}>
      <Icon name="open" size="calc(16 * var(--u))" />{kind === 'image' ? 'Web UI' : 'Endpoint'}
    </button>
    <button class="btn cn" onclick={() => actions.toggleConsole(view === 'fault' ? true : undefined)}>
      <Icon name={view === 'fault' ? 'log' : 'console'} size="calc(17 * var(--u))" />{view === 'fault' ? 'Full log' : 'Console'}
    </button>
  </section>

  <button class="console" onclick={() => actions.toggleConsole(true)} title="Open console">
    <Icon name="chev" size="calc(13 * var(--u))" />
    <span class="cl mono">{lastLine || 'no output yet'}</span>
  </button>
</div>

<style>
  .full {
    display: flex;
    flex-direction: column;
    gap: calc(4 * var(--u));
    height: 100%;
    padding: 0 calc(4 * var(--u)) calc(6 * var(--u));
    font-family: var(--font-text);
    font-size: var(--fs-body);
  }
  .full > :global(*) {
    flex: 0 0 auto;
    min-width: 0;
  }
  .corner {
    position: absolute;
    left: calc(18 * var(--u));
    top: calc(14 * var(--u));
  }
  /* "+" beside the MODE caption: adds a System (opens Tune in add mode) */
  .addsys {
    position: absolute;
    left: calc(58 * var(--u));
    top: calc(10 * var(--u));
    z-index: 2;
    display: grid;
    place-items: center;
    width: calc(20 * var(--u));
    height: calc(20 * var(--u));
    padding: 0;
    border-radius: 50%;
    border: calc(1.5 * var(--u)) solid rgba(237, 230, 214, 0.45);
    background: #141517;
    color: rgba(237, 230, 214, 0.75);
    cursor: pointer;
  }
  .addsys:hover {
    color: var(--cyan);
    border-color: var(--cyan);
  }

  /* header */
  .hdr {
    height: calc(52 * var(--u));
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(16 * var(--u));
    padding: 0 calc(14 * var(--u)) 0 calc(16 * var(--u));
    border-bottom: 1px solid #0a0b0c;
    box-shadow: 0 1px 0 rgba(255, 255, 255, 0.03);
    margin: 0 calc(-4 * var(--u));
  }
  .brand {
    display: flex;
    align-items: center;
    gap: calc(12 * var(--u));
    min-width: 0;
  }
  .mk {
    display: block;
    flex: 0 0 auto;
    width: calc(32 * var(--u));
    height: calc(32 * var(--u));
  }
  .word {
    font-family: var(--font-label);
    font-weight: 700;
    font-size: calc(34 * var(--u));
    line-height: 1;
    letter-spacing: 0.02em;
    color: var(--cream);
    margin-top: calc(2 * var(--u));
  }
  .tag {
    font-family: var(--font-label);
    font-weight: 600;
    font-size: var(--fs-lbl);
    letter-spacing: 0.1em;
    color: rgba(237, 230, 214, 0.66);
    margin-top: calc(7 * var(--u));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .status {
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    height: calc(32 * var(--u));
    flex: 0 0 auto;
  }
  .status .vsep {
    height: calc(22 * var(--u));
    align-self: center;
  }
  .wsep {
    margin-left: calc(2 * var(--u));
  }
  .ph {
    font-family: var(--font-label);
    font-weight: 600;
    font-size: var(--fs-tier);
    letter-spacing: 0.12em;
    color: var(--cream);
  }
  .ph.off {
    color: var(--cream-2);
  }
  .ph.orange {
    color: var(--orange);
  }
  .ph.amber {
    color: var(--amber);
  }
  .up {
    font-size: var(--fs-body);
    color: var(--cream-2);
    white-space: nowrap;
  }
  .up b {
    font-weight: 500;
    color: var(--cream);
    margin-left: calc(3 * var(--u));
  }
  .up.ver {
    color: var(--muted);
  }
  /* frameless: the panel key joins the window controls as one cluster */
  .grp {
    display: flex;
    align-items: center;
    gap: calc(4 * var(--u));
  }
  .pm {
    display: inline-flex;
    align-items: center;
    gap: calc(7 * var(--u));
    height: calc(28 * var(--u));
    padding: 0 calc(9 * var(--u)) 0 calc(8 * var(--u));
    border: 1px solid rgba(237, 230, 214, 0.2);
    border-radius: calc(4 * var(--u));
    background: transparent;
    color: rgba(237, 230, 214, 0.86);
    cursor: pointer;
    flex: 0 0 auto;
    white-space: nowrap;
  }
  .pm.bare {
    width: calc(34 * var(--u));
    padding: 0;
    justify-content: center;
  }
  .pm :global(.ic) {
    stroke-width: 1.5;
  }
  .pm:hover {
    background: rgba(237, 230, 214, 0.08);
    border-color: rgba(237, 230, 214, 0.34);
    color: var(--cream);
  }
  .pm:active {
    background: rgba(0, 0, 0, 0.25);
  }
  .pl {
    font-family: var(--font-label);
    font-weight: 600;
    font-size: var(--fs-lbl);
    letter-spacing: 0.1em;
    line-height: 1;
  }

  /* tier control */
  .tier {
    height: calc(118 * var(--u));
    display: flex;
    gap: calc(4 * var(--u));
  }
  .sel {
    flex: 1 1 auto;
  }
  .selbox {
    position: absolute;
    left: calc(62 * var(--u));
    top: calc(6 * var(--u));
    right: 0;
  }
  .be {
    flex: 0 0 calc(168 * var(--u));
  }
  .bebox {
    position: absolute;
    left: 0;
    right: 0;
    top: calc(38 * var(--u));
    display: flex;
    justify-content: center;
  }

  /* model line */
  .model {
    height: calc(32 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(16 * var(--u));
    padding: 0 calc(18 * var(--u));
  }
  .model .lbl {
    flex: 0 0 calc(96 * var(--u));
  }
  .mline {
    font-weight: 500;
    font-size: var(--fs-body);
    letter-spacing: 0.01em;
    color: var(--cream);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .model.dim .mline {
    color: rgba(237, 230, 214, 0.75);
  }

  /* dial area: absorbs the remaining height, the same two panels in every phase */
  .full > .dials {
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
  .ctx {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(30 * var(--u)) calc(20 * var(--u)) calc(14 * var(--u));
    overflow: hidden;
  }
  .cgroup {
    display: flex;
    flex-direction: column;
    align-items: center;
    min-width: 0;
    max-width: 100%;
  }
  .cdial {
    flex: 0 0 auto;
    width: var(--d);
    height: var(--d);
  }
  .cread {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: calc(6 * var(--u));
    padding-top: calc(14 * var(--u));
    text-align: center;
    white-space: nowrap;
    min-width: 0;
  }
  .cv {
    font-size: var(--fs-read);
    font-weight: 400;
    color: var(--cream);
    line-height: 1.1;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .cv b {
    font-weight: 600;
  }
  .cv.dimv {
    color: rgba(237, 230, 214, 0.5);
  }
  .cs {
    font-size: var(--fs-small);
    color: rgba(237, 230, 214, 0.62);
    line-height: 1.1;
    min-height: 1.1em;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  /* system row */
  .sys {
    height: calc(38 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(12 * var(--u));
    padding: 0 calc(18 * var(--u));
  }
  .sys .vsep {
    height: calc(22 * var(--u));
    align-self: center;
    margin: 0 calc(8 * var(--u));
  }
  .si {
    display: flex;
    color: rgba(237, 230, 214, 0.75);
  }
  .sv {
    white-space: nowrap;
    color: var(--cream);
  }
  .sm {
    flex: 1 1 0;
    min-width: calc(40 * var(--u));
    height: calc(10 * var(--u));
    --seg-gap: calc(2 * var(--u));
    --seg-glow: calc(3 * var(--u));
  }

  /* timeline / recent jobs */
  .tline {
    height: calc(94 * var(--u));
    display: flex;
    gap: calc(26 * var(--u));
    padding: calc(12 * var(--u)) calc(20 * var(--u)) calc(10 * var(--u)) calc(18 * var(--u));
  }
  .tmain {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: calc(10 * var(--u));
  }
  .thead {
    display: flex;
    align-items: baseline;
    gap: calc(14 * var(--u));
    min-width: 0;
    white-space: nowrap;
  }
  .tcap {
    font-size: var(--fs-small);
    color: rgba(237, 230, 214, 0.55);
    flex: 0 0 auto;
  }
  .lastcap {
    margin-left: auto;
    padding-left: calc(12 * var(--u));
    font-size: var(--fs-small);
    color: rgba(237, 230, 214, 0.72);
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .lastcap.hot {
    color: var(--orange);
  }
  .tbody {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }
  .legend {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(8 * var(--u));
    font-size: var(--fs-small);
    color: var(--cream-2);
    min-width: calc(58 * var(--u));
  }
  .legend span {
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
  }
  .sw {
    display: inline-block;
    width: calc(10 * var(--u));
    height: calc(10 * var(--u));
    border-radius: 2px;
  }
  .sw.pf {
    background: #e9e1d0;
  }
  .sw.dc {
    background: #4fb2ea;
  }
  .sw.ft {
    background: var(--orange);
  }

  /* controls */
  .bar {
    height: calc(50 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(8 * var(--u));
    padding: 0 calc(12 * var(--u)) 0 calc(18 * var(--u));
  }
  .bar .vsep {
    height: calc(28 * var(--u));
    align-self: center;
    margin: 0 calc(4 * var(--u));
  }
  .bar .btn {
    flex: 0 0 auto;
  }
  .ep {
    width: calc(108 * var(--u));
    padding: 0 calc(10 * var(--u));
    gap: calc(8 * var(--u));
  }
  .port {
    font-size: var(--fs-body);
    letter-spacing: 0.02em;
  }
  .off {
    font-size: var(--fs-small);
    color: var(--muted);
  }
  .chip[disabled] .port {
    color: rgba(237, 230, 214, 0.55);
  }
  .key {
    width: calc(112 * var(--u));
    /* The row is full: when it is short of room the endpoint chip (its port has space to spare) gives, not this one. */
    flex-shrink: 0;
  }
  .key[disabled] .dots,
  .key[disabled] .copy {
    color: rgba(237, 230, 214, 0.42);
  }
  .key .dots {
    flex: 1 1 auto;
    padding: 0 calc(10 * var(--u));
    letter-spacing: 0.06em;
  }
  .key .dots.none {
    font-size: var(--fs-small);
    color: var(--muted);
    letter-spacing: 0.01em;
  }
  .key .copy {
    align-self: stretch;
    display: flex;
    align-items: center;
    padding: 0 calc(9 * var(--u));
    border-left: 1px solid #0a0b0c;
    background: linear-gradient(180deg, #2a2b2f, #1e1f22);
    font-size: var(--fs-small);
    color: var(--cream-2);
  }
  .grow {
    flex: 1 1 auto;
  }
  /* The primary place: one width for Launch / Cancel / Stop / Restart, so nothing beside it moves. */
  .act {
    width: calc(196 * var(--u));
    justify-content: flex-start;
  }
  /* An external server: a quiet engraved note in the primary place, not a push button. */
  .btn.act.ext[disabled] {
    justify-content: center;
    background: transparent;
    border-color: rgba(237, 230, 214, 0.16);
    box-shadow: none;
    color: var(--muted);
    opacity: 1;
    cursor: default;
  }
  .al {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .act b {
    font-family: var(--font-label);
    font-weight: 600;
    font-size: var(--fs-tier);
    letter-spacing: 0.07em;
    margin-left: calc(2 * var(--u));
  }
  .play {
    display: flex;
    color: var(--cyan);
    filter: drop-shadow(0 0 calc(4 * var(--u)) rgba(90, 182, 235, 0.55));
  }
  /* A tier that cannot launch: an inert key, its label stays legible. */
  .launch[disabled] {
    opacity: 1;
    border-color: rgba(237, 230, 214, 0.14);
    box-shadow: none;
    color: var(--muted);
  }
  .launch[disabled] .play {
    color: var(--muted);
    filter: none;
  }
  .sq {
    flex: 0 0 auto;
    width: calc(11 * var(--u));
    height: calc(11 * var(--u));
    border-radius: 2px;
    background: var(--cyan);
    box-shadow: 0 0 calc(6 * var(--u)) rgba(90, 182, 235, 0.7);
  }
  .stop[disabled] .sq {
    background: var(--muted);
    box-shadow: none;
  }
  .act.hot {
    border-color: var(--orange);
    color: #ff8a55;
    box-shadow:
      inset 0 0 0 1px rgba(255, 107, 44, 0.35),
      0 0 calc(10 * var(--u)) rgba(255, 107, 44, 0.3);
  }
  .rs {
    width: calc(94 * var(--u));
  }
  .rec {
    width: calc(30 * var(--u));
    padding: 0;
    justify-content: center;
  }
  .op {
    /* "Endpoint" / "Web UI" (was "Open endpoint": the row is full and Records needed its room). */
    width: calc(98 * var(--u));
  }
  .cn {
    width: calc(98 * var(--u));
  }
  .tune {
    color: var(--cyan);
  }
  .tune :global(.ic) {
    stroke-width: 1.8;
  }

  /* collapsed console */
  .console {
    height: calc(28 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    padding: 0 calc(12 * var(--u));
    margin: 0 calc(10 * var(--u));
    border-radius: calc(4 * var(--u));
    border: 1px solid #0a0b0c;
    background: #18191b;
    box-shadow: inset 0 1px 3px rgba(0, 0, 0, 0.6);
    color: var(--cream-2);
    cursor: pointer;
    text-align: left;
    min-width: 0;
  }
  .console:hover {
    background: #1c1d20;
  }
  .cl {
    font-size: var(--fs-small);
    letter-spacing: 0.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
</style>
