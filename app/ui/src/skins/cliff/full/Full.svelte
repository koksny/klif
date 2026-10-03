<script lang="ts">
  // Cliff, full window. One fixed skeleton in every phase, scaled by --u (1 design px): header, tier strip,
  // model line, status block (hero + two detail rows, or the fault panel in the same box), the cliff scene
  // (absorbs the remaining height), system row, request timeline / recent jobs, controls. A phase change
  // only changes what the regions say, never where they are:
  //   idle      - the selected tier: dim hero and rows with its configured values, fit preview, Launch
  //   loading   - startup % in the hero, the six startup steps in the rows, other tiers locked
  //   live      - decode speed (prefill progress while prefilling) / diffusion progress, the scene live
  //   stopping  - dim hero "STOPPING · RELEASING x GiB"; waiting = live with no telemetry yet
  //   fault     - fault panel over the status block, debris off the emptied face, Restart / Dismiss
  //   dormant   - (live, vram.dormant set) the GPU asleep or waking: amber status, the last figure faded
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtFixed, fmtGiB } from '../../../lib/model/format';
  import Cliff from '../Cliff.svelte';
  import type { SceneMode } from '../paint';
  import type { GpuView } from '../power';
  import { idleState } from '../../../lib/model/systems';
  import { blockView, lastSessionLine, releasedGiB, selectedSystem, sessionKind } from '../util';
  import Bottom from './Bottom.svelte';
  import FaultPanel from './FaultPanel.svelte';
  import GenericLog from './GenericLog.svelte';
  import GenericPanel from './GenericPanel.svelte';
  import Header from './Header.svelte';
  import ImagePanel from './ImagePanel.svelte';
  import LlmPanel from './LlmPanel.svelte';
  import LoadingPanel from './LoadingPanel.svelte';
  import ModelLine from './ModelLine.svelte';
  import RecentJobs from './RecentJobs.svelte';
  import Tabs from './Tabs.svelte';
  import Timeline from './Timeline.svelte';

  let { vm, actions, gpu = null }: { vm: ViewModel; actions: Actions; gpu?: GpuView | null } = $props();

  const s = $derived(vm.session);
  const view = $derived(blockView(vm));
  /** The selected System's kind (vm.session is always its session). */
  const kind = $derived(sessionKind(vm));
  const sel = $derived(selectedSystem(vm));
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);
  /** The status block follows the session, or (idle) the selected System with its configured values. */
  const model = $derived(s?.model ?? sel?.model ?? null);
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || 0);

  const mode = $derived<SceneMode>(view === 'idle' ? 'fit' : view === 'fault' ? 'fault' : view === 'loading' ? 'building' : 'live');
  const fit = $derived(mode === 'fit');
  /** VRAM held by others; with nothing running that is everything in use (older cores lack the field). */
  const baseGiB = $derived(vm.vram.baselineGiB ?? (s ? 0 : vm.vram.usedGiB));
  const cliffLayers = $derived(fit ? (sel?.expectedVram ?? []) : vm.vram.layers);
  const kicker = $derived(
    fit
      ? `VRAM cliff · fit preview for ${sel?.label ?? 'System'}`
      : mode === 'building'
        ? 'VRAM cliff · filling'
        : mode === 'fault'
          ? 'VRAM cliff · released'
          : gpu
            ? gpu.phase === 'waking'
              ? 'VRAM cliff · restoring from system RAM'
              : gpu.phase === 'sleeping'
                ? 'VRAM cliff · paging out to system RAM'
                : 'VRAM cliff · paged out to system RAM'
            : 'VRAM cliff',
  );

  // The hero's word when there is nothing to measure.
  const idle = $derived(idleState(sel));
  const word = $derived(
    view === 'idle'
      ? idle.text.toUpperCase()
      : view === 'stopping'
        ? `STOPPING · RELEASING ${fmtGiB(vm.vram.usedGiB)} GiB`
        : view === 'waiting'
          ? 'WAITING FOR DATA'
          : null,
  );
  const wordAmber = $derived((view === 'idle' && idle.warn) || view === 'stopping');
  /** Image GPU asleep with nothing in flight: the last job's figures, faded. */
  const imgStale = $derived(!!gpu && img?.activity === 'idle');

  // Timeline / recent jobs.
  const hadWork = $derived(view === 'fault' && (!!llm || !!img || !!gen));
  const died = $derived(view === 'fault' && !!llm && llm.activity !== 'idle');
  const lastText = $derived(vm.lastSession ? lastSessionLine(vm.lastSession, vm.systems) : '');
  const lastFault = $derived(vm.lastSession?.ended === 'fault');
  const ramFrac = $derived(vm.machine.ramTotalGiB > 0 ? Math.min(1, vm.machine.ramUsedGiB / vm.machine.ramTotalGiB) : 0);
</script>

<div class="full" data-view={view}>
  <div class="pad"><Header {vm} {actions} {gpu} /></div>
  <div class="pad"><Tabs {vm} {actions} /></div>
  <div class="pad"><ModelLine {model} {kind} label={s ? 'Active model' : 'Selected'} dim={!s} /></div>

  <!-- Status block: its box never moves; a fault covers it whole. -->
  <div class="pad block">
    {#if view === 'fault' && s}
      <FaultPanel session={s} label={sel?.label ?? s.model.name} history={vm.vram.history} totalGiB={vm.vram.totalGiB} />
    {:else if view === 'loading' && s}
      <LoadingPanel session={s} usedGiB={vm.vram.usedGiB} totalGiB={vm.vram.totalGiB} />
    {:else if kind === 'image'}
      <ImagePanel image={img} {model} {word} {wordAmber} stale={imgStale} />
    {:else if kind === 'llm'}
      <LlmPanel {llm} {model} {ctxTotal} {word} {wordAmber} {gpu} />
    {:else}
      <GenericPanel generic={gen} {model} {kind} port={s?.endpoint.port ?? sel?.command?.port} {word} {wordAmber} />
    {/if}
  </div>

  <div class="scene">
    <Cliff
      variant="full"
      totalGiB={vm.vram.totalGiB}
      usedGiB={vm.vram.usedGiB}
      layers={cliffLayers}
      spillMiB={vm.vram.spillMiB}
      warnBelowGiB={vm.vram.warnBelowGiB}
      device={vm.vram.device}
      {mode}
      {baseGiB}
      faultSinceS={s?.fault?.sinceS}
      releasedGiB={releasedGiB(vm)}
      {kicker}
      {gpu}
    />
  </div>

  <div class="pad sys">
    <span class="c-lbl">RAM</span>
    <span class="v">{fmtFixed(vm.machine.ramUsedGiB, 1)} / {fmtFixed(vm.machine.ramTotalGiB, 1)} GiB{vm.machine.ramType ? ` ${vm.machine.ramType}` : ''}</span>
    <span class="c-bar" role="meter" aria-label="System RAM" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(ramFrac * 100)}
      ><i style:transform="scaleX({ramFrac})"></i></span
    >
    <span class="vr" aria-hidden="true"></span>
    <span class="c-lbl">CPU</span>
    <span class="v">{vm.machine.cpuName} · {Math.round(vm.machine.cpuPct)}%</span>
    <span class="c-bar" role="meter" aria-label="CPU" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(vm.machine.cpuPct)}
      ><i style:transform="scaleX({Math.min(1, vm.machine.cpuPct / 100)})"></i></span
    >
  </div>

  <div class="pad">
    {#if kind === 'image'}
      <RecentJobs recent={img?.recent ?? []} total={img?.imagesThisSession ?? 0} {view} {hadWork} {lastText} {lastFault} />
    {:else if kind === 'llm'}
      <Timeline requests={llm?.requests ?? []} {view} {hadWork} {died} {lastText} {lastFault} />
    {:else}
      <GenericLog {view} requests={gen?.requestsTotal ?? 0} {lastText} {lastFault} />
    {/if}
  </div>

  <div class="pad last"><Bottom {vm} {actions} /></div>
</div>

<style>
  .full {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    height: 100%;
    min-height: 0;
    font-size: var(--fs-m);
  }
  .pad {
    padding-inline: max(14px, calc(var(--u) * 20));
    flex: none;
    min-width: 0;
  }
  .last {
    padding-bottom: max(10px, calc(var(--u) * 12));
  }
  .block {
    display: grid;
    grid-template-rows: max(76px, calc(var(--u) * 92)) minmax(0, 1fr);
    row-gap: var(--gap);
    height: max(152px, calc(var(--u) * 172));
  }
  /* the drawing absorbs the remaining height; it runs edge to edge like a chart's neat line */
  .scene {
    flex: 1 1 0;
    min-height: max(200px, calc(var(--u) * 240));
    position: relative;
    border-top: 1px solid var(--rule);
    border-bottom: 1px solid #2c373e;
  }
  .sys {
    display: flex;
    align-items: center;
    gap: max(10px, calc(var(--u) * 12));
    height: max(24px, calc(var(--u) * 26));
    white-space: nowrap;
  }
  .sys .v {
    color: var(--foam);
    flex: none;
  }
  .sys .c-bar {
    flex: 0 1 max(60px, calc(var(--u) * 120));
  }
  .sys .vr {
    align-self: stretch;
    width: 1px;
    margin: 0 max(8px, calc(var(--u) * 14));
    background: var(--rule);
  }
</style>
