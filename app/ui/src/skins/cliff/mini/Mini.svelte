<script lang="ts">
  // Cliff, mini panel (960x640, read from 1 m). Read-only. A fixed 960x640 sheet scaled to fit, laid out like
  // the full window in miniature, and the same in every state (only the contents change):
  //   header      KLIF · status · uptime / elapsed (the version when idle)
  //   system strip  one tab per System (status dot, short label), scrolls sideways when it overflows
  //   model line  the running model, or the selected System's
  //   cliff (left) the VRAM cliff, the skin's signature; hero (right): one big figure with its label and a
  //               status line; rows (right): two fixed facts per kind (LLM: context, prefill; image: last
  //               image, images)
  //   VRAM strip  numbers only (the drawing is the gauge): VRAM / FITS (idle) / RESIDENT (dormant) / SPILL
  // A fault covers the hero and the rows. Dormant GPU: amber status, the last figure faded.
  // Smallest text 24 px (labels), values 34 px and up.
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtClock, fmtCtx, fmtGiB, fmtInt, fmtPct, fmtTps } from '../../../lib/model/format';
  import { EXTERNAL_TITLE, KIND_LABEL, canStop, doLaunch, idleState, isPendingLaunch, launchCtl, systemLabel } from '../../../lib/model/systems';
  import { strip as scrollStrip } from '../../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../../lib/shell/SystemTabs/tabs';
  import Cliff from '../Cliff.svelte';
  import type { SceneMode } from '../paint';
  import type { GpuView } from '../power';
  import { gpuStatus } from '../power';
  import { blockView, fmtAgo, fmtEta, fmtSpan, releasedGiB, selectedSystem, sessionKind, statusOf, sumGiB, tierWord } from '../util';

  let { vm, actions, gpu = null }: { vm: ViewModel; actions: Actions; gpu?: GpuView | null } = $props();

  // "Back to window": the one control on the panel, shown while the pointer is over it or it has focus.
  const canLeave = $derived(!!(vm.host?.panel?.available || vm.host?.panel?.active));

  const SW = 960;
  const SH = 640;
  let w = $state(SW);
  let h = $state(SH);
  const k = $derived(Math.max(0.1, Math.min(w / SW, h / SH)));
  const ox = $derived((w - SW * k) / 2);
  const oy = $derived((h - SH * k) / 2);

  const s = $derived(vm.session);
  const view = $derived(blockView(vm));
  const kind = $derived(sessionKind(vm));
  const slot = $derived(selectedSystem(vm));
  const tabs = $derived(tabsFor(vm));
  const model = $derived(s?.model ?? slot?.model ?? null);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);
  const idle = $derived(view === 'idle');
  const faulted = $derived(view === 'fault');
  const st = $derived(statusOf(vm));
  const live = $derived(view === 'live');

  const prefilling = $derived(live && !!llm?.prefill && llm.activity === 'prefill');
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || 0);
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? Math.min(1, llm.context.usedTokens / llm.context.totalTokens) : 0);
  const preFrac = $derived(prefilling && llm?.prefill && llm.prefill.tokens > 0 ? Math.min(1, llm.prefill.doneTokens / llm.prefill.tokens) : 0);
  const imgGen = $derived(live && !!img && img.activity === 'generating' && img.steps > 0);
  const lastJob = $derived(img && img.recent.length ? img.recent[img.recent.length - 1] : null);
  const activeStep = $derived(s?.loading?.steps.find((x) => x.state === 'active') ?? null);

  const tone = $derived(gpu || st.tone === 'busy' ? 'busy' : st.tone);
  const statusText = $derived(gpu ? gpuStatus(gpu) : st.text);
  const clock = $derived(s ? fmtClock(view === 'loading' ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS) : '');

  /** The model line: name · quant · context or size · mode. */
  const modelText = $derived.by(() => {
    if (!model) return '';
    const parts = [model.name, model.quant];
    if (model.ctxTokens) parts.push(fmtCtx(model.ctxTokens));
    if (model.imageSize) parts.push(model.imageSize);
    if (model.mode) parts.push(model.mode);
    return parts.filter(Boolean).join(' · ');
  });

  // ---- the drawing ---------------------------------------------------------------------------------------
  const mode = $derived<SceneMode>(idle ? 'fit' : faulted ? 'fault' : view === 'loading' ? 'building' : 'live');
  const baseGiB = $derived(vm.vram.baselineGiB ?? (s ? 0 : vm.vram.usedGiB));
  const layers = $derived(mode === 'fit' ? (slot?.expectedVram ?? []) : vm.vram.layers);

  // ---- hero: label, figure + unit, status line ---------------------------------------------------------------
  type Tone = '' | 'amb' | 'red';
  type Hero = { label: string; tone: Tone; fig: string; unit: string; pre?: string; dim: boolean; faded: boolean; line: string; lineTone: Tone };
  const idleWhy = $derived(idleState(slot));
  const last = $derived(vm.lastSession);
  const lastText = $derived.by(() => {
    if (!last) return '';
    const label = tierWord(systemLabel(vm, last.system) || last.model.name);
    return `last ${label} · ${fmtSpan(last.uptimeS)}${last.ended === 'fault' ? ' · fault' : ''} · ${fmtAgo(last.endedAgoS)}`;
  });
  const hero = $derived.by<Hero>(() => {
    const base = { tone: '' as Tone, dim: false, faded: false, lineTone: '' as Tone };
    const kindLabel = kind === 'image' ? 'DIFFUSION STEP' : kind === 'llm' ? 'DECODE SPEED' : KIND_LABEL[kind].toUpperCase();
    if (gpu && live) {
      if (gpu.phase === 'waking')
        return { ...base, label: 'GPU WAKING', tone: 'amb', fig: String(Math.round(gpu.frac * 100)), unit: '%', line: `${fmtGiB(gpu.pagedOutGiB)} GiB still in system RAM` };
      const ps = gpu.powerState ? ` · ${gpu.powerState}` : '';
      const fig = llm ? fmtTps(llm.decodeTps) : '—';
      return {
        ...base,
        label: `${gpuStatus(gpu)}${ps}`,
        tone: 'amb',
        fig,
        unit: llm ? 'tok/s' : '',
        faded: true,
        line: `${fmtGiB(gpu.pagedOutGiB)} GiB paged out · ${fmtSpan(gpu.sinceS)}`,
      };
    }
    if (view === 'loading' && s) {
      const step = activeStep ? `${activeStep.label}${activeStep.detail ? ` · ${activeStep.detail}` : ''}` : s.phase === 'starting' ? 'starting' : 'loading';
      return { ...base, label: 'STARTUP', tone: 'amb', fig: s.loading ? String(Math.floor(s.loading.fraction * 100)) : '—', unit: '%', line: step };
    }
    if (view === 'stopping') return { ...base, label: 'STOPPING', tone: 'amb', fig: '—', unit: '', dim: true, line: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB` };
    if (live && llm) {
      if (prefilling && llm.prefill)
        return { ...base, label: 'PREFILL PROGRESS', fig: String(Math.floor(preFrac * 100)), unit: '%', line: `${fmtInt(llm.prefill.doneTokens)} / ${fmtInt(llm.prefill.tokens)} tok` };
      return {
        ...base,
        label: llm.activity === 'decode' ? 'DECODE SPEED' : 'LAST DECODE SPEED',
        fig: fmtTps(llm.decodeTps),
        unit: 'tok/s',
        line: `${fmtInt(llm.generatedTokens)} tok generated${llm.activity === 'idle' ? ' · idle' : ''}`,
      };
    }
    if (live && img) {
      if (imgGen)
        return { ...base, label: `DIFFUSION STEP${img.edit ? ' · EDIT' : ''}`, pre: 'step', fig: String(img.step), unit: `/ ${img.steps}`, line: `${img.sPerIt.toFixed(2)} s/it · ${img.width}×${img.height}` };
      return { ...base, label: kindLabel, pre: 'step', fig: '—', unit: `/ ${img.steps}`, dim: true, line: 'waiting for the next job' };
    }
    if (live && gen) {
      const inFlight = gen.requestsInFlight ?? 0;
      return {
        ...base,
        label: inFlight > 0 ? `${kindLabel} · WORKING` : kindLabel,
        fig: gen.requestsTotal !== undefined ? fmtInt(gen.requestsTotal) : '—',
        unit: 'requests',
        line: inFlight > 0 ? `${inFlight} in flight` : gen.lastActivityS !== undefined ? `last activity ${fmtSpan(gen.lastActivityS)} ago` : 'waiting for a request',
      };
    }
    if (s) return { ...base, label: kindLabel, fig: '—', unit: '', dim: true, line: 'waiting for data' };
    // Idle: the selected System, not running.
    return {
      ...base,
      label: kindLabel,
      pre: kind === 'image' ? 'step' : undefined,
      fig: '—',
      unit: kind === 'llm' ? 'tok/s' : '',
      dim: true,
      line: idleWhy.warn || slot?.status === 'not-set' ? (slot?.reason ?? idleWhy.text) : lastText || idleWhy.text,
      lineTone: idleWhy.warn ? 'amb' : '',
    };
  });

  // ---- rows: two fixed facts per kind, the same labels in every phase --------------------------------------
  type Row = { k: string; v: string; sub: string; red?: boolean };
  const rows = $derived.by<Row[]>(() => {
    if (kind === 'image') {
      return [
        { k: 'LAST IMAGE', v: lastJob ? `${lastJob.seconds.toFixed(1)} s` : '—', sub: lastJob?.edit ? 'edit' : '' },
        { k: 'IMAGES', v: img ? fmtInt(img.imagesThisSession) : '—', sub: img ? 'this session' : '' },
      ];
    }
    if (kind !== 'llm') {
      return [
        { k: 'IN FLIGHT', v: gen ? fmtInt(gen.requestsInFlight ?? 0) : '—', sub: '' },
        { k: 'LAST REQ', v: gen?.lastActivityS !== undefined ? fmtSpan(gen.lastActivityS) : '—', sub: gen?.lastActivityS !== undefined ? 'ago' : '' },
      ];
    }
    const pf = llm?.prefill;
    return [
      { k: 'CONTEXT', v: llm ? fmtPct(ctxFrac) : '—', sub: ctxTotal ? `of ${fmtCtx(ctxTotal)}` : '', red: ctxFrac >= 0.95 },
      prefilling && pf
        ? { k: 'PREFILL', v: fmtInt(pf.tps), sub: `tok/s · ${fmtEta(pf.etaS)} left` }
        : { k: 'PREFILL', v: pf ? fmtInt(pf.tps) : '—', sub: pf ? 'tok/s' : '' },
    ];
  });

  // ---- fault -----------------------------------------------------------------------------------------------
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const exitText = $derived(fault ? (fault.exitCode === undefined ? 'no exit code' : `exit ${fault.exitCodeHex ?? fault.exitCode}`) : '');

  // ---- VRAM strip: numbers only --------------------------------------------------------------------------
  type Strip = { k: string; v: string; aux: string; tone: Tone };
  const strip = $derived.by<Strip>(() => {
    const used = `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)} GiB`;
    if (idle && slot?.expectedVram?.length && !slot.external) {
      const top = baseGiB + sumGiB(slot.expectedVram);
      const spare = vm.vram.totalGiB - top;
      const aux = `${fmtGiB(top)} / ${fmtGiB(vm.vram.totalGiB)} expected`;
      return spare >= 0 ? { k: 'FITS', v: `${fmtGiB(spare)} GiB spare`, aux, tone: '' } : { k: 'OVER', v: `by ${fmtGiB(-spare)} GiB`, aux, tone: 'red' };
    }
    if (vm.vram.spillMiB > 0) return { k: 'SPILL', v: `${fmtInt(vm.vram.spillMiB)} MiB`, aux: used, tone: 'red' };
    if (gpu) return { k: 'RESIDENT', v: used, aux: `${fmtGiB(gpu.pagedOutGiB)} GiB in RAM`, tone: 'amb' };
    if (faulted) {
      const rel = releasedGiB(vm);
      return { k: 'VRAM', v: used, aux: rel > 0.004 ? `${fmtGiB(rel)} GiB released` : '', tone: '' };
    }
    const free = vm.vram.totalGiB - vm.vram.usedGiB;
    return { k: 'VRAM', v: used, aux: `${fmtGiB(Math.max(0, free))} GiB free`, tone: s && free < vm.vram.warnBelowGiB ? 'amb' : '' };
  });
  // The panel's one control, in the header: Launch the selected System (idle; with conflicts it reads
  // "STOP S1 & LAUNCH" and stops them first), Cancel (loading), Stop (live), Restart (fault). Systems are picked
  // on the strip at any time: selecting never stops anything.
  const act = $derived.by(() => {
    const ph = vm.session?.phase;
    const pick = slot;
    const mine = !!pick && pick.controllable && !pick.external;
    // An external server: a quiet note, never Launch / Stop (KLIF only watches it). A launch that waits for
    // other Systems to stop (starting, no session yet): Cancel.
    if (pick?.external) return { kind: 'ext', text: 'EXTERNAL SERVER', title: EXTERNAL_TITLE, disabled: true, run: () => {} };
    if (!ph && isPendingLaunch(pick)) {
      return { kind: 'stop', text: 'CANCEL', title: pick?.reason ?? 'Cancel the launch', disabled: !canStop(pick), run: () => void actions?.stop(pick?.id) };
    }
    if (!ph) {
      const ctl = launchCtl(vm, pick, { short: true });
      const word = tierWord(pick?.label ?? '');
      return {
        kind: 'go',
        text: ctl.stopOthers ? ctl.text.toUpperCase() : `LAUNCH ${word}`,
        title: ctl.enabled ? (ctl.stopOthers ? ctl.text : `Launch ${pick?.label ?? ''}`) : `${pick?.label ?? ''} cannot launch: ${ctl.blocked}`,
        disabled: !ctl.enabled,
        run: () => doLaunch(actions, pick, ctl),
      };
    }
    if (ph === 'fault') return { kind: 'hot', text: 'RESTART', title: 'Restart the System that failed', disabled: !mine, run: () => void actions?.restart(pick?.id) };
    if (ph === 'stopping') return { kind: 'stop', text: 'STOPPING', title: 'Stopping', disabled: true, run: () => {} };
    return {
      kind: 'stop',
      text: ph === 'live' ? 'STOP' : 'CANCEL',
      title: !canStop(pick) ? 'External server: it runs where it was started' : ph === 'live' ? 'Stop the server' : 'Cancel the launch',
      disabled: !canStop(pick),
      run: () => void actions?.stop(pick?.id),
    };
  });
</script>

<div class="mini" bind:clientWidth={w} bind:clientHeight={h}>
  <div class="sheet" style="transform: translate({ox}px, {oy}px) scale({k})">
    <!-- Header -->
    <div class="hdr">
      <span class="klif">KLIF</span>
      <span class="sep"></span>
      <span class="st {tone}">{#key vm.now}<i class="dot"></i>{/key}{statusText}</span>
      <span class="grow"></span>
      {#if s}<span class="clock">{clock}</span>{:else if vm.host?.appVersion}<span class="clock ver">v{vm.host.appVersion}</span>{/if}
      <button class="act {act.kind}" type="button" onclick={act.run} disabled={act.disabled || !actions} title={act.title}>
        <svg viewBox="0 0 12 12" aria-hidden="true"
          >{#if act.kind === 'go'}<path d="M3 1.5 10.5 6 3 10.5z" class="fill" />{:else if act.kind === 'hot'}<path d="M10 6a4 4 0 1 1-1.17-2.83" /><path
              d="M10 1.5v2.5H7.5"
            />{:else}<rect x="2.5" y="2.5" width="7" height="7" class="fill" />{/if}</svg
        ><span>{act.text}</span>
      </button>
      {#if canLeave}
        <button class="back" onclick={() => actions.togglePanel?.()} title="Leave panel mode" aria-label="Leave panel mode">
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M7.5 3v4.5H3M12.5 3v4.5H17M7.5 17v-4.5H3M12.5 17v-4.5H17" /></svg>
        </button>
      {/if}
    </div>

    <!-- System strip -->
    <div class="tiers" use:scrollStrip={vm.selected}>
      {#each tabs as t (t.id)}
        {@const sel = t.id === vm.selected}
        {@const na = t.system.status === 'invalid' || t.system.status === 'unreachable'}
        <button type="button" class="tier" class:sel class:na class:fault={t.status === 'fault'} data-sel={sel} onclick={() => void actions?.select(t.id)}>
          <TabLabel tab={t} short dotSize={13} />
        </button>
      {/each}
    </div>

    <!-- Model line -->
    <div class="mline panel" class:dim={idle}><span>{modelText}</span></div>

    <!-- The cliff -->
    <div class="draw panel">
      <Cliff
        variant="mini"
        totalGiB={vm.vram.totalGiB}
        usedGiB={vm.vram.usedGiB}
        {layers}
        spillMiB={vm.vram.spillMiB}
        warnBelowGiB={vm.vram.warnBelowGiB}
        device={vm.vram.device}
        {mode}
        {baseGiB}
        faultSinceS={s?.fault?.sinceS}
        releasedGiB={releasedGiB(vm)}
        {gpu}
      />
    </div>

    {#if faulted}
      <!-- Fault: covers the hero and the rows -->
      <div class="fpanel panel" role="alert">
        <div class="lbl red">FAULT · {fmtAgo(fault?.sinceS ?? 0)}</div>
        <div class="ftitle">{fault?.title ?? 'The server stopped unexpectedly.'}</div>
        <div class="fexit">{exitText}</div>
      </div>
    {:else}
      <!-- Hero -->
      <div class="hero panel">
        <div class="lbl {hero.tone}">{hero.label}</div>
        <div class="fig" class:dim={hero.dim} class:faded={hero.faded}>
          {#if hero.pre}<span class="unit">{hero.pre}</span>{/if}<b>{hero.fig}</b>{#if hero.unit}<span class="unit">{hero.unit}</span>{/if}
        </div>
        <div class="line {hero.lineTone}">{hero.line}</div>
      </div>

      <!-- Rows -->
      <div class="rows panel" class:dim={!live} class:img={kind === 'image'}>
        {#each rows as r (r.k)}
          <div class="row">
            <span class="k">{r.k}</span>
            <span class="v"><b class:red={r.red}>{r.v}</b>{#if r.sub}<small>{r.sub}</small>{/if}</span>
          </div>
        {/each}
      </div>
    {/if}

    <!-- VRAM strip -->
    <div class="vram panel">
      <span class="k {strip.tone}">{strip.k}</span>
      <span class="v {strip.tone}">{strip.v}</span>
      {#if strip.aux}<span class="aux">{strip.aux}</span>{/if}
    </div>
  </div>
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    z-index: 1;
    overflow: hidden;
    user-select: none;
  }
  .sheet {
    position: absolute;
    left: 0;
    top: 0;
    width: 960px;
    height: 640px;
    transform-origin: 0 0;
    font-family: var(--f-ui);
    font-variant-numeric: tabular-nums;
    color: var(--foam);
  }
  .panel {
    position: absolute;
    box-sizing: border-box;
    background: var(--panel);
    border: 1px solid var(--edge);
    border-radius: 10px;
  }
  .red {
    color: var(--danger);
  }
  .amb {
    color: var(--amber);
  }

  /* Header: 0..62 */
  .hdr {
    position: absolute;
    left: 24px;
    right: 24px;
    top: 0;
    height: 62px;
    display: flex;
    align-items: center;
    gap: 20px;
    white-space: nowrap;
    border-bottom: 1px solid var(--s1);
  }
  .klif {
    font-family: var(--f-disp);
    font-stretch: 125%;
    font-weight: 600;
    font-size: 36px;
    letter-spacing: 0.12em;
    line-height: 1;
    color: var(--sky);
  }
  .sep {
    width: 1px;
    height: 30px;
    background: var(--s2);
    flex: none;
  }
  .st {
    display: flex;
    align-items: center;
    gap: 14px;
    font-size: 30px;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: var(--sky);
  }
  .dot {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: currentColor;
    animation: beat 700ms ease-out 1;
  }
  @keyframes beat {
    0% {
      opacity: 0.45;
    }
    100% {
      opacity: 1;
    }
  }
  .st.busy {
    color: var(--amber);
  }
  .st.idle,
  .st.stop {
    color: var(--muted);
  }
  .st.idle .dot,
  .st.stop .dot {
    background: transparent;
    box-shadow: inset 0 0 0 3px currentColor;
  }
  .st.fault {
    color: var(--danger);
  }
  .grow {
    flex: 1 1 auto;
  }
  .clock {
    font-size: 34px;
    font-weight: 400;
    letter-spacing: 0.02em;
    color: var(--foam);
  }
  .clock.ver {
    font-size: 28px;
    color: var(--muted);
  }

  /* Tier strip: 72..128 */
  .tiers {
    position: absolute;
    left: 24px;
    right: 24px;
    top: 72px;
    height: 56px;
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: minmax(150px, 1fr);
    gap: 10px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tier {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 18px;
    border: 1px solid var(--edge);
    border-radius: 10px;
    background: var(--panel);
    font-size: 28px;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: var(--mist);
    white-space: nowrap;
    overflow: hidden;
  }
  .tier.sel {
    border-color: var(--sky);
    background: rgba(90, 182, 235, 0.1);
    box-shadow: inset 0 0 0 1px rgba(90, 182, 235, 0.3);
    color: var(--foam);
  }
  .tier.na {
    color: #6c7d86;
  }
  .tier.fault {
    border-color: var(--danger);
    background: rgba(232, 100, 90, 0.12);
    color: var(--danger);
  }
  /* Model line: 138..178 */
  .mline {
    left: 24px;
    right: 24px;
    top: 138px;
    height: 40px;
    display: flex;
    align-items: center;
    padding: 0 18px;
    font-size: 26px;
    color: var(--foam);
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim {
    color: #a9bcc6;
  }
  .mline span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The cliff: 188..550, left. The drawing runs to the box's edges, like an inset map. */
  .draw {
    left: 24px;
    top: 188px;
    width: 412px;
    height: 362px;
    overflow: hidden;
    background: rgba(15, 19, 22, 0.6);
  }

  /* Hero: 188..398, right */
  .hero {
    left: 448px;
    right: 24px;
    top: 188px;
    height: 210px;
    padding: 14px 22px 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
  }
  .lbl {
    font-size: 24px;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lbl.amb {
    color: var(--amber);
  }
  .lbl.red {
    color: var(--danger);
  }
  .fig {
    display: flex;
    align-items: baseline;
    gap: 14px;
    align-self: center;
    white-space: nowrap;
    overflow: hidden;
    line-height: 1;
    font-family: var(--f-disp);
    font-stretch: 100%;
  }
  .fig b {
    font-size: 116px;
    font-weight: 300;
    letter-spacing: -0.01em;
    color: #f2f9fc;
  }
  .unit {
    font-size: 38px;
    font-weight: 400;
    color: var(--muted);
  }
  .fig.dim b,
  .fig.dim .unit {
    color: #4b5a62;
  }
  .fig.faded b,
  .fig.faded .unit {
    color: #75878f;
  }
  .line {
    height: 48px;
    display: flex;
    align-items: center;
    border-top: 1px solid var(--edge);
    margin: 0 -22px;
    padding: 0 22px;
    font-size: 26px;
    color: #a9bcc6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .line.amb {
    color: var(--amber);
  }

  /* Rows: 408..550, right */
  .rows {
    left: 448px;
    right: 24px;
    top: 408px;
    height: 142px;
    display: grid;
    grid-template-rows: repeat(2, minmax(0, 1fr));
  }
  .row {
    display: grid;
    grid-template-columns: 164px minmax(0, 1fr);
    align-items: center;
    min-height: 0;
  }
  .img .row {
    grid-template-columns: 206px minmax(0, 1fr);
  }
  .row + .row {
    border-top: 1px solid var(--edge);
  }
  .row .k {
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: 22px;
    border-right: 1px solid var(--edge);
    font-size: 24px;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: var(--label);
    white-space: nowrap;
  }
  .row .v {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 0 18px;
    white-space: nowrap;
    overflow: hidden;
  }
  .row .v b {
    font-size: 40px;
    font-weight: 500;
    color: var(--foam);
  }
  .row .v small {
    font-size: 26px;
    color: #a9bcc6;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v b {
    color: #5d6e77;
  }

  /* Fault: the hero and the rows together */
  .fpanel {
    left: 448px;
    right: 24px;
    top: 188px;
    height: 362px;
    padding: 16px 22px;
    border-color: rgba(232, 100, 90, 0.85);
    background: linear-gradient(180deg, rgba(48, 22, 20, 0.85), rgba(21, 24, 27, 0.88) 70%);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    row-gap: 12px;
  }
  .ftitle {
    font-size: 36px;
    font-weight: 500;
    line-height: 1.2;
    color: #f2f9fc;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 5;
    line-clamp: 5;
    -webkit-box-orient: vertical;
  }
  .fexit {
    font-size: 30px;
    color: var(--danger);
  }

  /* VRAM strip: 560..620 */
  .vram {
    left: 24px;
    right: 24px;
    top: 560px;
    height: 60px;
    display: flex;
    align-items: baseline;
    gap: 24px;
    padding: 0 22px;
    line-height: 58px;
    white-space: nowrap;
  }
  .vram .k {
    width: 150px;
    flex: none;
    font-size: 24px;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: var(--label);
  }
  .vram .k.red,
  .vram .v.red {
    color: var(--danger);
  }
  .vram .k.amb,
  .vram .v.amb {
    color: var(--amber);
  }
  .vram .v {
    font-size: 34px;
    font-weight: 500;
    color: var(--foam);
  }
  .vram .aux {
    margin-left: auto;
    font-size: 34px;
    color: #a9bcc6;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Leave panel mode: shown while the pointer is over the panel or it has focus. */
  .back {
    width: 48px;
    height: 40px;
    display: grid;
    place-items: center;
    border: 1px solid var(--s2);
    border-radius: 8px;
    background: var(--slate);
    color: var(--mist);
    opacity: 0;
    transition: opacity 0.15s ease;
    flex: none;
  }
  .mini:hover .back,
  .back:focus-visible {
    opacity: 1;
  }
  .back:hover {
    border-color: var(--sky);
    color: var(--foam);
  }
  .back svg {
    width: 24px;
    height: 24px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  /* Launch / Cancel / Stop / Restart: one width, so nothing in the header moves. */
  .act {
    flex: none;
    width: 292px;
    height: 44px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 0 14px;
    border: 1.5px solid var(--sky);
    border-radius: 8px;
    background: var(--sky);
    color: var(--basalt);
    font-family: inherit;
    font-size: 24px;
    font-weight: 600;
    font-stretch: 87.5%;
    letter-spacing: 0.08em;
    white-space: nowrap;
    cursor: pointer;
    overflow: hidden;
  }
  .act span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .act svg {
    width: 18px;
    height: 18px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .act svg .fill {
    fill: currentColor;
    stroke: none;
  }
  .act.stop {
    background: transparent;
    border-color: var(--amber);
    color: var(--amber);
  }
  /* An external server: a quiet note in the control's place. */
  .act.ext,
  .act.ext:disabled {
    background: transparent;
    border-color: var(--edge);
    color: var(--muted);
    opacity: 1;
    box-shadow: none;
  }
  .act.ext svg {
    display: none;
  }
  .act.hot {
    background: var(--danger);
    border-color: var(--danger);
    color: var(--basalt);
  }
  .act:disabled {
    cursor: default;
    opacity: 0.45;
  }
  .act.go:disabled {
    opacity: 1;
    background: var(--slate-2);
    border-color: var(--s2);
    color: var(--muted);
  }
  button.tier:not(:disabled):not(.sel):hover {
    border-color: var(--s3);
  }
  button.tier {
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }
</style>
