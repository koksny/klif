<script lang="ts">
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtFixed } from '../../../lib/model/format';
  import Cliff from '../Cliff.svelte';
  import type { SceneMode } from '../paint';
  import type { GpuView } from '../power';
  import { releasedGiB, selectedSlot, sessionKind, sessionSlot, viewState } from '../util';
  import Bottom from './Bottom.svelte';
  import FaultPanel from './FaultPanel.svelte';
  import Header from './Header.svelte';
  import IdlePanel from './IdlePanel.svelte';
  import ImagePanel from './ImagePanel.svelte';
  import LlmPanel from './LlmPanel.svelte';
  import LoadingPanel from './LoadingPanel.svelte';
  import ModelLine from './ModelLine.svelte';
  import RecentJobs from './RecentJobs.svelte';
  import Tabs from './Tabs.svelte';
  import Timeline from './Timeline.svelte';

  let { vm, actions, gpu = null }: { vm: ViewModel; actions: Actions; gpu?: GpuView | null } = $props();

  const s = $derived(vm.session);
  const view = $derived(viewState(vm));
  const kind = $derived(sessionKind(vm));
  const sel = $derived(selectedSlot(vm));
  const running = $derived(sessionSlot(vm));
  const model = $derived(s?.model ?? sel?.model ?? null);
  const mode = $derived<SceneMode>(
    view === 'idle' ? 'fit' : view === 'fault' ? 'fault' : view === 'loading' ? 'building' : 'live',
  );
  const fit = $derived(mode === 'fit');
  /** VRAM held by others; with nothing running that is everything in use (older cores lack the field). */
  const baseGiB = $derived(vm.vram.baselineGiB ?? (s ? 0 : vm.vram.usedGiB));
  const cliffLayers = $derived(fit ? (sel?.expectedVram ?? []) : vm.vram.layers);
  /** The GPU is asleep with nothing in flight: the numbers on the hero are the last request's, not live. */
  const stale = $derived(!!gpu && gpu.phase === 'asleep' && (s?.llm ? s.llm.activity === 'idle' : s?.image?.activity === 'idle'));
  const kicker = $derived(
    fit
      ? `VRAM cliff · fit preview for ${sel?.label ?? 'job'}`
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
</script>

<div class="full">
  <div class="pad top">
    <Header {vm} {actions} {gpu} />
    <Tabs {vm} {actions} />
  </div>

  {#if view === 'idle'}
    <div class="pad"><IdlePanel {vm} {actions} /></div>
    <div class="pad"><div class="c-rule"></div></div>
  {:else if view === 'fault' && s}
    <div class="pad"><FaultPanel session={s} history={vm.vram.history} totalGiB={vm.vram.totalGiB} /></div>
    <div class="pad"><div class="c-rule"></div></div>
  {:else}
    {#if model}
      <div class="pad"><ModelLine {model} {kind} label={s ? 'Active model' : 'Selected model'} /></div>
      <div class="pad"><div class="c-rule"></div></div>
    {/if}
    <div class="pad mid">
      {#if view === 'llm' && s?.llm}
        <LlmPanel llm={s.llm} {stale} />
      {:else if view === 'image' && s?.image}
        <ImagePanel image={s.image} />
      {:else if s}
        <LoadingPanel session={s} label={running?.label ?? s.model.name} {kind} />
      {/if}
    </div>
    <div class="pad"><div class="c-rule"></div></div>
  {/if}

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

  <div class="pad"><div class="c-rule strong"></div></div>
  <div class="pad sys">
    <span class="k">RAM</span>
    <span class="c-data v">{fmtFixed(vm.system.ramUsedGiB, 1)} / {fmtFixed(vm.system.ramTotalGiB, 1)} GiB</span>
    <span class="vr" aria-hidden="true"></span>
    <span class="k">CPU</span>
    <span class="c-data v">{vm.system.cpuName} · {Math.round(vm.system.cpuPct)}%</span>
  </div>
  <div class="pad"><div class="c-rule"></div></div>

  {#if view === 'llm' && s?.llm}
    <div class="pad"><Timeline requests={s.llm.requests} /></div>
    <div class="pad"><div class="c-rule"></div></div>
  {:else if view === 'image' && s?.image}
    <div class="pad"><RecentJobs recent={s.image.recent} /></div>
    <div class="pad"><div class="c-rule"></div></div>
  {/if}

  <div class="pad"><Bottom {vm} {actions} /></div>
</div>

<style>
  .full {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .pad {
    padding-inline: max(16px, 2.35cqi);
    flex: none;
    min-width: 0;
  }
  .scene {
    flex: 1 1 0;
    min-height: max(200px, calc(var(--u) * 250));
    position: relative;
  }
  .c-rule.strong {
    background: #2c373e;
  }
  .sys {
    display: flex;
    align-items: baseline;
    gap: max(12px, calc(var(--u) * 26));
    padding-top: max(10px, calc(var(--u) * 15));
    padding-bottom: max(10px, calc(var(--u) * 15));
    font-size: max(13px, calc(var(--u) * 18));
  }
  .sys .k {
    font-family: var(--f-ui);
    font-weight: 500;
    color: var(--foam);
    letter-spacing: 0.04em;
  }
  .sys .v {
    color: var(--foam);
    letter-spacing: 0.04em;
  }
  .sys .vr {
    align-self: stretch;
    width: 1px;
    background: var(--rule);
    margin: 0 max(10px, calc(var(--u) * 26));
  }
</style>
