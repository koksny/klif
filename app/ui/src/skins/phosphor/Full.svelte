<script lang="ts">
  // Full window. One fixed skeleton in every phase, scaled by --k: header, tier strip, model line, status
  // block (hero + two detail rows, or the fault panel in the same box), the VRAM cliff (absorbs the remaining
  // height), system row, request timeline / recent jobs, controls and the console line. A phase change only
  // changes what the panels say, never where they are:
  //   idle      : the selected tier: dim hero and rows with its configured values, fit preview on the cliff,
  //               empty timeline with the last session, Launch
  //   loading   : startup % in the hero, the six startup steps in the rows, other tiers locked, Cancel
  //   live llm  : decode speed on the scope (or prefill progress while prefilling), prefill / decode /
  //               context / speculative rows beside the context radar, request timeline
  //   live image: sampling step on the step bar, last image / images / size / mode rows, recent jobs
  //   fault     : the fault panel over the status block, the fault marked on the cliff and the timeline
  //   dormant   : (live, vm.vram.dormant set) the GPU is asleep or waking: amber tags, the last figure faded,
  //               the allocations drawn paged out on the cliff
  import type { Actions, Slot, ViewModel } from '../../lib/model/types';
  import { fmtGiB, fmtInt, fmtPct, fmtSeconds } from '../../lib/model/format';
  import Header from './Header.svelte';
  import Tabs from './Tabs.svelte';
  import Hero from './Hero.svelte';
  import Radar from './Radar.svelte';
  import Steps from './Steps.svelte';
  import FaultPanel from './FaultPanel.svelte';
  import VramCliff from './VramCliff.svelte';
  import FitCliff from './FitCliff.svelte';
  import Timeline from './Timeline.svelte';
  import Jobs from './Jobs.svelte';
  import Controls from './Controls.svelte';
  import { availText, clamp, fmtAgo, fmtDur, gpuSleep, lastSessionLine, loadSpanS, recipeLine } from './geom';

  let { vm, actions, k }: { vm: ViewModel; actions: Actions; k: number } = $props();

  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const slotOf = (id: string): Slot | undefined => vm.slots.find((x) => x.id === id);
  const sessionSlot = $derived(s ? slotOf(s.slot) : undefined);
  const selectedSlot = $derived(slotOf(vm.selected) ?? vm.slots[0]);
  // The running slot's kind, or (idle) the selected slot's: the empty hero and rows still say what they measure.
  const kind = $derived(sessionSlot?.kind ?? (s?.image ? 'image' : s?.llm ? 'llm' : (selectedSlot?.kind ?? 'llm')));
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);

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
            : kind === 'llm' && llm
              ? 'live-llm'
              : kind === 'image' && img
                ? 'live-img'
                : 'other',
  );
  const busy = $derived(view === 'loading' || view === 'stopping');
  const ctlView = $derived(view === 'live-llm' || view === 'live-img' || view === 'other' ? 'live' : view);

  /** GPU dormant (vm.vram.dormant): asleep, or waking while its VRAM is restored from system RAM. */
  const gpu = $derived(
    phase === 'live' ? gpuSleep(vm.vram, (!!llm && llm.activity !== 'idle') || (!!img && img.activity === 'generating')) : null,
  );

  // The model line and the rows follow the running session, or (idle) the selected tier's configured values.
  const focusModel = $derived(s?.model ?? selectedSlot?.model);
  const focusRecipe = $derived((sessionSlot ?? selectedSlot)?.recipe);
  const recipe = $derived(focusModel ? recipeLine(focusModel) : []);
  const port = $derived(s?.endpoint.port ?? focusRecipe?.port);
  const selReady = $derived(selectedSlot?.availability === 'ready');
  // The hero's word when nothing runs: the selected tier's state.
  const idleWord = $derived(selReady ? 'NOT RUNNING' : availText(selectedSlot?.availability ?? 'unsupported').toUpperCase());

  // LLM rows.
  const prefillActive = $derived(!!llm?.prefill && llm.activity === 'prefill');
  const specPending = $derived(!!llm && llm.activity === 'prefill' && llm.generatedTokens === 0);
  const ctxTotal = $derived(llm?.context.totalTokens || focusModel?.ctxTokens || focusRecipe?.ctxTokens || 0);
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0);
  /** Radar sweep: 2 degrees per second per decode tok/s (1/180 turn); during prefill 1/16 of its rate. */
  const sweep = $derived.by(() => {
    if (view !== 'live-llm' || !llm || llm.activity === 'idle' || gpu) return 0;
    const tps = llm.decodeTps > 0 ? llm.decodeTps : (llm.prefill?.tps ?? 0) / 16;
    return tps / 180;
  });

  // Image rows.
  const imgGen = $derived(view === 'live-img' && !!img && img.activity === 'generating' && img.steps > 0);
  const imgFrac = $derived(imgGen && img ? clamp(img.step / img.steps, 0, 1) : 0);
  const lastJob = $derived(img && img.recent.length ? img.recent[img.recent.length - 1] : null);

  // Loading: the six startup steps fill the detail rows (three per row), the dial is the overall fraction.
  const loadFrac = $derived(clamp(s?.loading?.fraction ?? 0, 0, 1));
  const steps = $derived(s?.loading?.steps ?? []);
  const weightsFrac = $derived.by(() => {
    const st = steps.find((x) => x.id === 'weights');
    if (!st || st.state === 'pending') return 0;
    if (st.state === 'done') return 1;
    const total = sessionSlot?.expectedVram?.find((l) => l.id === 'weights')?.gib ?? 0;
    const now = vm.vram.layers.find((l) => l.id === 'weights')?.gib ?? 0;
    return total > 0 ? clamp(now / total, 0, 1) : null;
  });

  // The dial at the left of the rows: context fill (LLM), sampling progress (image), startup (loading).
  const dial = $derived.by(() => {
    if (view === 'loading') return { frac: loadFrac, rate: 0.32, cap: 'LOAD', amber: true };
    if (kind === 'image') return { frac: imgFrac, rate: imgGen ? 0.25 : 0, cap: 'STEP', amber: false };
    return { frac: view === 'live-llm' || view === 'stopping' ? ctxFrac : 0, rate: sweep, cap: 'CTX', amber: false };
  });

  // Fault.
  const faultHadWork = $derived(view === 'fault' && (!!llm || !!img));
  /** Was a request in flight when the LLM server died? (activity as last reported) */
  const diedInRequest = $derived(view === 'fault' && !!llm && llm.activity !== 'idle');

  // Timeline / recent jobs: the running slot's kind, or (idle) the selected tier's. Empty frames while idle.
  const jobsView = $derived(kind === 'image');
  const requests = $derived(llm ? llm.requests.slice(view === 'fault' ? -6 : -8) : []);
  const jobs = $derived(img ? img.recent.slice(view === 'fault' ? -10 : -12) : []);
  const tlCaption = $derived.by(() => {
    const n = requests.length;
    if (view === 'idle') return 'not running';
    if (view === 'loading') return 'no requests yet';
    if (view === 'fault') {
      if (!faultHadWork) return 'no requests: failed during startup';
      const head = n === 0 ? 'no finished requests' : `last ${n}`;
      return `${head}, then ${diedInRequest ? 'the request that died' : 'the fault'}`;
    }
    if (n === 0) return 'no finished requests yet';
    return `${n < 8 ? `${n} so far` : `last ${n}`} · width = time`;
  });
  const jobsCaption = $derived.by(() => {
    if (view === 'idle') return 'not running';
    if (!img) return view === 'fault' ? 'no jobs: failed during startup' : 'no jobs yet';
    if (jobs.length === 0) return view === 'fault' ? 'no finished jobs, then the fault' : 'no finished jobs yet';
    return `last ${jobs.length} of ${fmtInt(img.imagesThisSession)} · width = seconds`;
  });
  const last = $derived(view === 'idle' && vm.lastSession ? vm.lastSession : null);

  const span = $derived(view === 'loading' ? loadSpanS(s?.loading?.elapsedS ?? s?.uptimeS ?? 0) : 300);
  const markAgo = $derived(view === 'fault' ? (s?.fault?.sinceS ?? null) : null);
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
</script>

<div class="full v-{view}">
  <Header session={s} host={vm.host} {actions} {k} {gpu} />

  <Tabs
    slots={vm.slots}
    selected={vm.selected}
    running={s?.slot ?? null}
    {actions}
    locked={busy}
    fault={view === 'fault'}
    canLaunch={!s || view === 'fault'}
  />

  <!-- model line: the running model, or the selected tier's -->
  <div class="panel recipe" class:dim={view === 'idle'} title={recipe.join(' · ')}>
    {#each recipe as part, i (i)}
      {#if i > 0}<span class="sep" aria-hidden="true">·</span>{/if}<span class="part">{part}</span>
    {/each}
  </div>

  <!-- status block: hero + detail rows; its box never moves, a fault covers it whole -->
  <div class="block">
    {#if view === 'fault' && s}
      <FaultPanel session={s} slot={sessionSlot} vram={vm.vram} {k} />
    {:else}
      <div class="hbox">
        {#if view === 'live-llm'}
          <Hero mode="llm" {kind} {llm} vram={vm.vram} {gpu} />
        {:else if view === 'live-img'}
          <Hero mode="image" {kind} {img} vram={vm.vram} {gpu} />
        {:else if view === 'loading'}
          <Hero mode="loading" {kind} loading={s?.loading ?? null} vram={vm.vram} />
        {:else if view === 'stopping'}
          <Hero mode="wait" {kind} vram={vm.vram} word="STOPPING · RELEASING {fmtGiB(vm.vram.usedGiB)} GiB" amber />
        {:else if view === 'other'}
          <Hero mode="wait" {kind} vram={vm.vram} word="WAITING FOR DATA" />
        {:else}
          <Hero mode="wait" {kind} vram={vm.vram} word={idleWord} amber={!selReady} />
        {/if}
      </div>

      <section class="panel rows" class:dim={view === 'idle'} class:load={view === 'loading'}>
        <div class="dial" class:off={dial.frac <= 0 && dial.rate <= 0}>
          <Radar fraction={dial.frac} turnsPerSec={dial.rate} size={Math.round(64 * k)} tone={dial.amber ? 'amber' : 'cyan'}>
            <span class="dcap" class:amb={dial.amber}>{dial.cap}</span>
          </Radar>
        </div>
        {#if view === 'loading'}
          <div class="cells"><Steps {steps} {weightsFrac} layout="grid" /></div>
        {:else if kind === 'image'}
          <div class="cells grid">
            <div class="k">Last image</div>
            <div class="v">
              {#if lastJob}<b>{fmtSeconds(lastJob.seconds)}</b><span class="mut">{` · ${lastJob.width}x${lastJob.height}${lastJob.edit ? ' · edit' : ''}`}</span>{:else}<span class="mut">{img ? 'none yet' : '—'}</span>{/if}
            </div>
            <div class="k">Images</div>
            <div class="v">{#if img}<b>{fmtInt(img.imagesThisSession)}</b> this session{:else}<span class="mut">—</span>{/if}</div>
            <div class="k">Size</div>
            <div class="v">{#if focusModel?.imageSize}<b>{focusModel.imageSize}</b>{:else}<span class="mut">—</span>{/if}</div>
            <div class="k">Mode</div>
            <div class="v">{#if focusModel?.mode}<b>{focusModel.mode}</b>{:else}<span class="mut">—</span>{/if}</div>
          </div>
        {:else}
          <div class="cells grid ctx">
            <div class="k">Prefill</div>
            <div class="v">
              {#if llm?.prefill}
                {#if prefillActive}
                  {#if llm.prefill.cachedTokens !== undefined}<b>{fmtInt(llm.prefill.cachedTokens)}</b>{` tok from the prompt cache · `}{/if}{`running ${fmtSeconds(llm.prefill.elapsedS)}`}
                {:else}
                  <b>{fmtInt(llm.prefill.tokens)}</b>{` tok · `}<b>{fmtInt(llm.prefill.tps)}</b>{` tok/s · done in ${fmtSeconds(llm.prefill.elapsedS)}`}
                {/if}
              {:else}<span class="mut">{llm ? 'no request yet' : '—'}</span>{/if}
            </div>
            <div class="k">Decode</div>
            <div class="v">
              {#if !llm}<span class="mut">—</span>{:else if prefillActive}<span class="mut">waiting for prefill</span>{:else}<b>{fmtInt(llm.generatedTokens)}</b>{` tok generated${llm.activity === 'idle' ? ' · idle' : ''}`}{/if}
            </div>
            <div class="k">Context</div>
            <div class="v">
              {#if llm}<b>{fmtInt(llm.context.usedTokens)}</b>{:else}<span class="mut">—</span>{/if}<span class="of">{` / ${ctxTotal ? fmtInt(ctxTotal) : '—'} tokens`}</span>{#if llm}{' · '}<b class:red={ctxFrac >= 0.95}>{fmtPct(ctxFrac)}</b>{/if}
            </div>
            <div class="k">Speculative</div>
            <div class="v">
              {#if llm?.spec}
                {#if specPending}<span class="mut">— accepted</span>{:else}<b>{Math.round(llm.spec.acceptancePct)}%</b> accepted{/if}{` · ${llm.spec.mode}`}
              {:else if llm}<span class="mut">off</span>{:else}{focusModel?.specMode ?? 'off'}{/if}
            </div>
          </div>
        {/if}
      </section>
    {/if}
  </div>

  <!-- the VRAM cliff: absorbs the remaining height; idle = the selected tier's fit preview -->
  <section class="panel grat vram" aria-label={view === 'idle' ? 'VRAM cliff, fit preview' : 'VRAM cliff'}>
    <div class="vhead">
      <span class="lbl">VRAM cliff</span>
      {#if view === 'idle'}
        <span class="lbl sub">Fit preview · {selectedSlot?.label ?? ''}</span>
        <span class="vv">{vm.vram.device} · <b>{fmtGiB(vm.vram.usedGiB)}</b> GiB in use</span>
      {:else}
        <span class="vv" class:spill={vm.vram.spillMiB > 0}>{vm.vram.device} · <b>{fmtGiB(vm.vram.usedGiB)}</b> / {fmtGiB(vm.vram.totalGiB)} GiB{gpu ? ' resident' : ''}</span>
        {#if gpu}
          <span class="gpu-tag" title="vm.vram.dormant: the GPU is powered down, the session's VRAM is paged out to system RAM">
            {gpu.powerState ?? 'dormant'} · {gpu.state === 'waking' ? `restoring ${Math.round(gpu.restoredFrac * 100)}%` : 'asleep'} · {fmtDur(gpu.sinceS)}
          </span>
        {/if}
      {/if}
    </div>
    <div class="cliffbox">
      {#if view === 'idle'}
        <FitCliff vram={vm.vram} slot={selectedSlot} {k} />
      {:else}
        <VramCliff vram={vm.vram} {k} spanS={span} markAgoS={markAgo} {gpu} />
      {/if}
    </div>
  </section>

  <!-- system row -->
  <section class="panel sys">
    <span class="seg">
      <span class="lbl">RAM</span>
      <span class="val">{vm.system.ramUsedGiB.toFixed(1)} / {vm.system.ramTotalGiB.toFixed(1)} GiB{#if vm.system.ramType}<span class="mut">{` · ${vm.system.ramType}`}</span>{/if}</span>
      <span class="meter" aria-hidden="true"><span style="transform:scaleX({(vm.system.ramTotalGiB > 0 ? clamp(vm.system.ramUsedGiB / vm.system.ramTotalGiB, 0, 1) : 0).toFixed(4)})"></span></span>
    </span>
    <span class="vrule" aria-hidden="true"></span>
    <span class="seg">
      <span class="lbl">CPU</span>
      <span class="val">{vm.system.cpuName} · {Math.round(vm.system.cpuPct)}%</span>
      <span class="meter" aria-hidden="true"><span style="transform:scaleX({clamp(vm.system.cpuPct / 100, 0, 1).toFixed(4)})"></span></span>
    </span>
  </section>

  <!-- request timeline / recent jobs: the same box in every phase -->
  <section class="panel tlp" aria-label={jobsView ? 'Recent jobs' : 'Request timeline'}>
    <div class="thead">
      <span class="lbl">{jobsView ? 'Recent jobs' : 'Request timeline'}</span>
      <span class="cap">{jobsView ? jobsCaption : tlCaption}</span>
      {#if last}
        <span class="lastcap" title={last.model.name}
          >last session: {lastSessionLine(last, vm.slots)} · <span class:bad={last.ended === 'fault'}
            >{last.ended === 'fault' ? 'ended in a fault' : 'stopped'} {fmtAgo(last.endedAgoS)}</span
          ></span
        >
      {/if}
      <span class="legend">
        {#if jobsView}<i class="sw c"></i>plain<i class="sw a"></i>edit{:else}<i class="sw a"></i>prefill<i class="sw c"></i>decode{/if}
        {#if view === 'fault' && faultHadWork}<i class="sw f"></i>fault{/if}
      </span>
    </div>
    <div class="tlbox">
      {#if jobsView}
        <Jobs
          {jobs}
          current={view === 'live-img' && img && img.activity === 'generating' ? { elapsedS: img.elapsedS, edit: img.edit } : null}
          fault={view === 'fault' && !!img}
        />
      {:else}
        <Timeline {requests} fault={view === 'fault' && !!llm} />
      {/if}
    </div>
  </section>

  <!-- controls (fixed places) + console line -->
  <section class="panel ctlp">
    <Controls view={ctlView} session={s} selected={selectedSlot} running={sessionSlot} {actions} {port} />
  </section>
  <button class="panel cons" onclick={() => actions.toggleConsole(true)} title="Open the console">
    <span class="caret mono">&gt;</span>
    {#if lastLine}<span class="line mono">{lastLine}</span>{:else}<span class="line mut">console · no output yet</span>{/if}
  </button>
</div>

<style>
  .full {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: calc(8px * var(--k));
    padding: 0 calc(14px * var(--k)) calc(12px * var(--k));
  }
  .full > :global(*) {
    flex: none;
    min-width: 0;
  }
  b {
    font-weight: 500;
    color: var(--ph-hot);
  }
  .red {
    color: var(--ph-danger);
  }

  /* model line */
  .recipe {
    height: calc(30px * var(--k));
    display: flex;
    align-items: center;
    column-gap: calc(8px * var(--k));
    padding: 0 calc(14px * var(--k));
    font-size: var(--ph-fs-m);
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow-soft);
    white-space: nowrap;
    overflow: hidden;
  }
  .recipe.dim {
    color: #9fd3e4;
  }
  .recipe .sep {
    color: var(--ph-muted);
    text-shadow: none;
  }
  .part {
    flex: none;
  }

  /* status block: one fixed box in every phase */
  .block {
    height: calc(216px * var(--k));
    display: grid;
    grid-template-rows: calc(128px * var(--k)) minmax(0, 1fr);
    gap: calc(8px * var(--k));
  }
  .block > :global(.fault) {
    grid-row: 1 / -1;
  }
  .hbox {
    min-height: 0;
  }

  /* detail rows: the dial, then two rows of label / value pairs */
  .rows {
    display: grid;
    grid-template-columns: calc(80px * var(--k)) minmax(0, 1fr);
    overflow: hidden;
  }
  .dial {
    display: grid;
    place-items: center;
    border-right: 1px solid var(--ph-grat);
  }
  .dial.off {
    opacity: 0.55;
  }
  .dcap {
    transform: translateY(calc(15px * var(--k)));
    padding: 0 calc(3px * var(--k));
    font-size: max(9.5px, calc(9px * var(--k)));
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    line-height: 1.2;
    color: var(--ph-cyan);
    background: radial-gradient(closest-side, rgba(3, 9, 12, 0.9), rgba(3, 9, 12, 0));
  }
  .dcap.amb {
    color: var(--ph-amber);
  }
  .cells {
    min-width: 0;
    min-height: 0;
  }
  .cells.grid {
    display: grid;
    grid-template-columns: calc(104px * var(--k)) minmax(0, 1fr) calc(112px * var(--k)) minmax(0, 1fr);
    grid-template-rows: repeat(2, minmax(0, 1fr));
  }
  .cells.grid.ctx {
    grid-template-columns: calc(104px * var(--k)) minmax(0, 1.35fr) calc(112px * var(--k)) minmax(0, 1fr);
  }
  .k,
  .v {
    display: flex;
    align-items: center;
    min-width: 0;
    border-right: 1px solid var(--ph-grat);
  }
  .k:nth-child(n + 5),
  .v:nth-child(n + 5) {
    border-top: 1px solid var(--ph-grat);
  }
  .k {
    padding-left: calc(14px * var(--k));
    font-size: var(--ph-fs-s);
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
    white-space: nowrap;
    overflow: hidden;
  }
  .v {
    padding: 0 calc(14px * var(--k));
    color: var(--ph-ink);
    text-shadow: var(--ph-glow-soft);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .v:nth-child(4n) {
    border-right: 0;
  }
  .v .of {
    color: var(--ph-brand);
  }
  .rows.dim .v,
  .rows.dim .v b,
  .rows.dim .v .of {
    color: #6fb2c9;
    text-shadow: none;
  }
  .rows.dim .k {
    color: #66c3de;
    text-shadow: none;
  }

  /* VRAM cliff */
  .vram {
    flex: 1 1 0;
    min-height: calc(150px * var(--k));
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(10px * var(--k)) calc(14px * var(--k)) calc(6px * var(--k));
  }
  .vhead {
    display: flex;
    align-items: baseline;
    gap: calc(16px * var(--k));
    padding-left: calc(4px * var(--k));
    min-width: 0;
  }
  .vhead .sub {
    color: var(--ph-brand);
  }
  .vv {
    margin-left: auto;
    padding-right: calc(4px * var(--k));
    color: var(--ph-ink);
    text-shadow: var(--ph-glow-soft);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .vv.spill b {
    color: var(--ph-amber);
  }
  .gpu-tag {
    padding-right: calc(4px * var(--k));
    color: var(--ph-amber);
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.4);
    white-space: nowrap;
  }
  .vv + .gpu-tag {
    margin-left: calc(-4px * var(--k));
  }
  .cliffbox {
    min-height: 0;
    margin-top: calc(2px * var(--k));
  }

  /* system row */
  .sys {
    height: calc(34px * var(--k));
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: calc(18px * var(--k));
    padding: 0 calc(16px * var(--k));
  }
  .seg {
    display: flex;
    align-items: center;
    gap: calc(14px * var(--k));
    min-width: 0;
  }
  .seg .val {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meter {
    position: relative;
    flex: 1 1 auto;
    min-width: calc(30px * var(--k));
    height: calc(4px * var(--k));
    min-height: 3px;
    border-radius: 2px;
    background: rgba(23, 79, 92, 0.45);
    overflow: hidden;
  }
  .meter span {
    position: absolute;
    inset: 0;
    transform-origin: left center;
    background: linear-gradient(90deg, rgba(90, 182, 235, 0.6), var(--ph-cyan));
    box-shadow: 0 0 8px rgba(127, 227, 255, 0.5);
    transition: transform 0.45s ease-out;
  }
  .vrule {
    width: 1px;
    align-self: stretch;
    margin: calc(8px * var(--k)) 0;
    background: var(--ph-rule);
  }

  /* request timeline / recent jobs: one height for both, in every phase */
  .tlp {
    height: calc(84px * var(--k));
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    gap: calc(6px * var(--k));
    padding: calc(9px * var(--k)) calc(14px * var(--k)) calc(8px * var(--k));
  }
  .thead {
    display: flex;
    align-items: baseline;
    gap: calc(12px * var(--k));
    padding-left: calc(4px * var(--k));
    min-width: 0;
    white-space: nowrap;
  }
  .cap {
    font-size: var(--ph-fs-xs);
    color: var(--ph-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 0 1 auto;
    min-width: 0;
  }
  .lastcap {
    margin-left: auto;
    min-width: 0;
    flex: 0 1 auto;
    font-size: var(--ph-fs-xs);
    color: #8fc4d6;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lastcap .bad {
    color: var(--ph-danger);
  }
  .legend {
    flex: none;
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: calc(7px * var(--k));
    font-size: var(--ph-fs-xs);
    color: var(--ph-cyan);
  }
  .lastcap + .legend {
    margin-left: calc(10px * var(--k));
  }
  .sw {
    display: inline-block;
    width: calc(16px * var(--k));
    height: 2px;
    border-radius: 2px;
    margin-left: calc(10px * var(--k));
  }
  .sw.a {
    background: var(--ph-amber);
    box-shadow: 0 0 6px rgba(232, 176, 74, 0.6);
  }
  .sw.c {
    background: var(--ph-cyan);
    box-shadow: 0 0 6px rgba(127, 227, 255, 0.6);
  }
  .sw.f {
    background: var(--ph-danger);
    box-shadow: 0 0 6px rgba(229, 97, 92, 0.6);
  }
  .tlbox {
    min-height: 0;
  }

  /* controls + console line */
  .ctlp {
    display: flex;
    flex-direction: column;
  }
  .cons {
    height: calc(30px * var(--k));
    display: flex;
    align-items: center;
    gap: calc(10px * var(--k));
    padding: 0 calc(14px * var(--k));
    text-align: left;
    font-size: max(10.5px, calc(11px * var(--k)));
    color: var(--ph-ink);
  }
  .cons:hover {
    border-color: #2a7f93;
  }
  .cons:hover .line {
    color: var(--ph-hot);
  }
  .caret {
    color: var(--ph-muted);
  }
  .cons .line {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cons .line.mut {
    font-size: var(--ph-fs-xs);
  }
</style>
