<script lang="ts">
  // Mini panel (960x640, a 3.5" screen read from 1 m). Read-only. A fixed 960x640 sheet scaled to fit, laid
  // out like the full window in miniature, and the same in every state (only the contents change):
  //   header      KLIF · status dot + phase · uptime / elapsed (the app version when idle)
  //   tier strip  the four tiers with their state dots (selected, running, cannot launch, locked)
  //   model line  the running model, or the selected tier's
  //   cliff (left) the VRAM cliff trace (idle: the selected tier's fit preview); hero (right): one big figure
  //               with its label and a status line; rows (right): two fixed facts per kind
  //               (LLM: context, prefill; image: last image, images)
  //   VRAM strip  what the cliff measures, in words: VRAM / FITS (idle) / RESIDENT (dormant) / SPILL
  // A fault covers the hero and the rows. Dormant GPU: amber status, the last figure faded, the allocations
  // drawn paged out on the cliff. Smallest text 24 px (labels), values 34 px and up.
  import { onDestroy } from 'svelte';
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtCtx, fmtGiB, fmtInt, fmtPct, fmtTps } from '../../lib/model/format';
  import VramCliff from './VramCliff.svelte';
  import FitCliff from './FitCliff.svelte';
  import { availText, clamp, fmtAgo, fmtDur, fmtEta, fmtSPerIt, gpuSleep, loadSpanS } from './geom';

  let { vm, actions }: { vm: ViewModel; actions?: Actions } = $props();

  // Panel mode is the only reason this read-only layout has a control: a "back to window" button that is
  // invisible until the panel is hovered, tapped or keyboard-focused.
  const canLeave = $derived(!!(vm.host?.panel?.available || vm.host?.panel?.active));
  let tapped = $state(false);
  let tapTimer: ReturnType<typeof setTimeout> | undefined;
  function reveal() {
    tapped = true;
    clearTimeout(tapTimer);
    tapTimer = setTimeout(() => (tapped = false), 4000);
  }
  onDestroy(() => clearTimeout(tapTimer));

  const SW = 960;
  const SH = 640;
  let w = $state(SW);
  let h = $state(SH);
  const u = $derived(Math.max(0.1, Math.min(w / SW, h / SH)));
  const ox = $derived((w - SW * u) / 2);
  const oy = $derived((h - SH * u) / 2);

  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const slot = $derived(vm.slots.find((x) => x.id === (s?.slot ?? vm.selected)));
  const kind = $derived(slot?.kind ?? (s?.image ? 'image' : 'llm'));
  const model = $derived(s?.model ?? slot?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const liveLlm = $derived(!!s && phase === 'live' && kind === 'llm' && !!llm);
  const liveImg = $derived(!!s && phase === 'live' && kind === 'image' && !!img);
  const idle = $derived(!s);
  const loading = $derived(!!s && (phase === 'starting' || phase === 'loading'));
  const faulted = $derived(!!s && phase === 'fault');
  const busy = $derived(loading || phase === 'stopping');

  // During prefill the decode speed sits at 0 for minutes: the hero is the prefill progress.
  const prefilling = $derived(liveLlm && !!llm?.prefill && llm.activity === 'prefill' && llm.prefill.tokens > 0);
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || slot?.recipe?.ctxTokens || 0);
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0);
  const preFrac = $derived(prefilling && llm?.prefill ? clamp(llm.prefill.doneTokens / llm.prefill.tokens, 0, 1) : 0);
  const imgGen = $derived(liveImg && !!img && img.activity === 'generating' && img.steps > 0);
  const lastJob = $derived(img && img.recent.length ? img.recent[img.recent.length - 1] : null);

  /** GPU dormant (vm.vram.dormant): asleep, or waking while its VRAM is restored from system RAM. */
  const gpu = $derived(
    phase === 'live' ? gpuSleep(vm.vram, (!!llm && llm.activity !== 'idle') || (!!img && img.activity === 'generating')) : null,
  );
  const waking = $derived(gpu?.state === 'waking');

  const tone = $derived(gpu || loading ? 'amber' : faulted ? 'danger' : s && phase === 'live' ? 'cyan' : 'muted');
  const PHASE: Record<string, string> = { starting: 'STARTING', loading: 'LOADING', live: 'LIVE', stopping: 'STOPPING', fault: 'FAULT' };
  const statusLabel = $derived(gpu ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : s ? PHASE[s.phase] : 'IDLE');
  const activeStep = $derived(s?.loading?.steps.find((x) => x.state === 'active') ?? null);
  const clock = $derived.by(() => {
    if (!s) return '';
    const t = loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS;
    const hh = Math.floor(t / 3600);
    const mm = Math.floor((t % 3600) / 60);
    const ss = Math.floor(t % 60);
    return `${String(hh).padStart(2, '0')}:${String(mm).padStart(2, '0')}:${String(ss).padStart(2, '0')}`;
  });

  // Tier strip: the short tier word ("AGENT MEDIUM" -> "MEDIUM").
  const short = (label: string) => label.replace(/^AGENT\s+/i, '');

  /** The model line: name · quant · context or size · mode. */
  const modelText = $derived.by(() => {
    if (!model) return '';
    const parts = [model.name, model.quant];
    if (model.ctxTokens) parts.push(fmtCtx(model.ctxTokens));
    if (model.imageSize) parts.push(model.imageSize);
    if (model.mode) parts.push(model.mode);
    return parts.join(' · ');
  });

  // ---- hero: label, figure + unit, status line --------------------------------------------------------
  type Tone = '' | 'amber' | 'red';
  type Hero = { label: string; tone: Tone; fig: string; unit: string; dim: boolean; faded: boolean; line: string; lineTone: Tone };
  const last = $derived(vm.lastSession);
  const lastText = $derived.by(() => {
    if (!last) return '';
    const label = short(vm.slots.find((x) => x.id === last.slot)?.label ?? last.model.name);
    return `last ${label} · ${fmtDur(last.uptimeS)}${last.ended === 'fault' ? ' · fault' : ''} · ${fmtAgo(last.endedAgoS)}`;
  });
  const ready = $derived(slot?.availability === 'ready');
  const hero = $derived.by<Hero>(() => {
    const base = { tone: '' as Tone, dim: false, faded: false, lineTone: '' as Tone };
    if (gpu) {
      if (waking) return { ...base, label: 'GPU WAKING', tone: 'amber', fig: String(Math.round(gpu.restoredFrac * 100)), unit: '%', line: `${fmtGiB(gpu.pagedOutGiB)} GiB still in system RAM`, lineTone: 'amber' };
      return {
        ...base,
        label: gpu.powerState ? `GPU ASLEEP · ${gpu.powerState}` : 'GPU ASLEEP',
        tone: 'amber',
        fig: llm ? fmtTps(llm.decodeTps) : '—',
        unit: llm ? 'tok/s' : '',
        faded: true,
        line: `${fmtGiB(gpu.pagedOutGiB)} GiB paged out · ${fmtDur(gpu.sinceS)}`,
      };
    }
    if (loading && s) {
      const step = activeStep ? `${activeStep.label}${activeStep.detail ? ` · ${activeStep.detail}` : ''}` : 'starting the server';
      return { ...base, label: 'STARTUP', tone: 'amber', fig: String(Math.floor((s.loading?.fraction ?? 0) * 100)), unit: '%', line: step };
    }
    if (phase === 'stopping') return { ...base, label: 'STOPPING', fig: '—', unit: '', dim: true, line: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB`, lineTone: 'amber' };
    if (liveLlm && llm) {
      if (prefilling && llm.prefill)
        return { ...base, label: 'PROMPT PREFILL', tone: 'amber', fig: String(Math.floor(preFrac * 100)), unit: '%', line: `${fmtInt(llm.prefill.doneTokens)} / ${fmtInt(llm.prefill.tokens)} tok` };
      return {
        ...base,
        label: 'DECODE SPEED',
        fig: fmtTps(llm.decodeTps),
        unit: 'tok/s',
        faded: llm.activity !== 'decode',
        line: `${fmtInt(llm.generatedTokens)} tok generated${llm.activity === 'idle' ? ' · idle' : ''}`,
      };
    }
    if (liveImg && img) {
      if (imgGen)
        return { ...base, label: `DIFFUSION STEP${img.edit ? ' · EDIT' : ''}`, fig: String(img.step), unit: `/ ${img.steps}`, line: `${fmtSPerIt(img.sPerIt)} s/it · ${img.width}x${img.height}` };
      return { ...base, label: 'DIFFUSION STEP', fig: '—', unit: `/ ${img.steps}`, faded: true, line: 'waiting for the next job' };
    }
    if (s) return { ...base, label: kind === 'image' ? 'DIFFUSION STEP' : 'DECODE SPEED', fig: '—', unit: '', dim: true, line: 'waiting for data' };
    // Idle: the selected tier, not running.
    return {
      ...base,
      label: kind === 'image' ? 'DIFFUSION STEP' : 'DECODE SPEED',
      fig: '—',
      unit: kind === 'image' ? '' : 'tok/s',
      dim: true,
      line: ready ? lastText || 'not running' : (slot?.reason ?? availText(slot?.availability ?? 'unsupported')),
      lineTone: ready ? '' : 'amber',
    };
  });

  // ---- rows: two fixed facts per kind, the same labels in every phase -------------------------------------
  type Row = { k: string; v: string; sub: string; red?: boolean };
  const rows = $derived.by<Row[]>(() => {
    if (kind === 'image') {
      return [
        { k: 'LAST IMAGE', v: lastJob ? `${lastJob.seconds.toFixed(1)} s` : '—', sub: lastJob?.edit ? 'edit' : '' },
        { k: 'IMAGES', v: img ? fmtInt(img.imagesThisSession) : '—', sub: img ? 'this session' : '' },
      ];
    }
    const pf = llm?.prefill;
    return [
      { k: 'CONTEXT', v: llm ? fmtPct(ctxFrac) : '—', sub: ctxTotal ? `of ${fmtCtx(ctxTotal)}` : '', red: ctxFrac >= 0.95 },
      prefilling && pf
        ? { k: 'PREFILL', v: fmtInt(pf.tps), sub: `tok/s · ${pf.etaS > 0 ? `${fmtEta(pf.etaS)} left` : 'finishing'}` }
        : { k: 'PREFILL', v: pf ? fmtInt(pf.tps) : '—', sub: pf ? 'tok/s' : '' },
    ];
  });

  // ---- fault ----------------------------------------------------------------------------------------------
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const exitText = $derived(fault ? (fault.exitCode === undefined ? 'no exit code' : `exit ${fault.exitCodeHex ?? fault.exitCode}`) : '');

  // ---- VRAM strip: the cliff's numbers in words ------------------------------------------------------------
  const spillGiB = $derived(Math.max(0, vm.vram.spillMiB) / 1024);
  const free = $derived(Math.max(0, vm.vram.totalGiB - vm.vram.usedGiB));
  const lowFree = $derived(!!s && free < vm.vram.warnBelowGiB);
  // Idle: the selected tier's expected footprint on top of what is in use.
  const base = $derived(Math.max(0, vm.vram.baselineGiB ?? vm.vram.usedGiB));
  const expected = $derived(idle && slot?.expectedVram?.length ? slot.expectedVram.reduce((a, l) => a + l.gib, 0) : null);
  const spare = $derived(expected !== null ? vm.vram.totalGiB - base - expected : 0);
  const vramText = $derived.by(() => {
    const used = `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)} GiB`;
    if (expected !== null)
      return spare >= 0
        ? { k: 'FITS', v: `${fmtGiB(spare)} GiB spare`, side: `needs ${fmtGiB(expected)} GiB`, tone: spare < vm.vram.warnBelowGiB ? 'amber' : '' }
        : { k: 'OVER', v: `by ${fmtGiB(-spare)} GiB`, side: `needs ${fmtGiB(expected)} GiB`, tone: 'red' };
    if (idle) return { k: 'VRAM', v: `${fmtGiB(vm.vram.usedGiB)} GiB in use`, side: vm.vram.device, tone: '' };
    if (gpu) return { k: 'RESIDENT', v: used, side: `${fmtGiB(gpu.pagedOutGiB)} GiB paged out`, tone: 'amber' };
    if (spillGiB > 0) return { k: 'SPILL', v: `${fmtGiB(spillGiB)} GiB`, side: used, tone: 'red' };
    return { k: 'VRAM', v: used, side: `${fmtGiB(free)} GiB free`, tone: lowFree ? 'amber' : '' };
  });
</script>

<svelte:window onpointerdown={canLeave ? reveal : undefined} />

<div class="mini" class:tapped bind:clientWidth={w} bind:clientHeight={h}>
  <div class="sheet" style="transform: translate({ox}px, {oy}px) scale({u})">
    <!-- Header -->
    <div class="hdr">
      <span class="klif">KLIF</span>
      <span class="sep"></span>
      <span class="st {tone}"><i class="dot" class:pulse={waking || loading}></i>{statusLabel}</span>
      <span class="grow"></span>
      {#if canLeave}
        <button class="back" onclick={() => actions?.togglePanel?.()} title="Leave panel mode" aria-label="Leave panel mode">
          <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="1.75" y="2.75" width="12.5" height="10.5" /><path d="M1.75 6H14.25" /></svg>
          <span>WINDOW</span>
        </button>
      {/if}
      {#if s}<span class="clock">{clock}</span>{:else if vm.host?.appVersion}<span class="clock ver">v{vm.host.appVersion}</span>{/if}
    </div>

    <!-- Tier strip -->
    <div class="tiers">
      {#each vm.slots as t (t.id)}
        {@const sel = t.id === (s?.slot ?? vm.selected)}
        {@const na = t.availability !== 'ready'}
        {@const running = !!s && s.slot === t.id && !faulted}
        <div class="tier" class:sel class:na class:locked={busy && !running} class:fault={faulted && s?.slot === t.id}>
          <i class="td" class:run={running} class:bad={na && !running}></i>{short(t.label)}
        </div>
      {/each}
    </div>

    <!-- Model line -->
    <div class="mline panel" class:dim={idle}><span>{modelText}</span></div>

    <!-- The VRAM cliff -->
    <div class="cliffp panel">
      <div class="cap">{idle ? 'FIT PREVIEW' : 'VRAM CLIFF'}</div>
      <div class="cliff">
        {#if idle}
          <FitCliff vram={vm.vram} {slot} variant="mini" />
        {:else}
          <VramCliff
            vram={vm.vram}
            variant="mini"
            spanS={loading ? loadSpanS(s?.loading?.elapsedS ?? s?.uptimeS ?? 0) : 300}
            markAgoS={fault ? fault.sinceS : null}
            {gpu}
          />
        {/if}
      </div>
    </div>

    {#if faulted}
      <!-- Fault: covers the hero and the rows -->
      <div class="fpanel panel" role="alert">
        <div class="lbl red">FAULT · {fmtAgo(fault?.sinceS ?? 0)}</div>
        <div class="ftitle">{fault?.title ?? 'The server stopped'}</div>
        <div class="fexit">{exitText}</div>
      </div>
    {:else}
      <!-- Hero -->
      <div class="hero panel">
        <div class="lbl {hero.tone}">{hero.label}</div>
        <div class="fig {hero.tone}" class:dim={hero.dim} class:faded={hero.faded}><b>{hero.fig}</b>{#if hero.unit}<span class="unit">{hero.unit}</span>{/if}</div>
        <div class="line {hero.lineTone}">{hero.line}</div>
      </div>

      <!-- Rows -->
      <div class="rows panel" class:dim={idle}>
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
      <span class="k {vramText.tone}">{vramText.k}</span>
      <span class="v {vramText.tone}">{vramText.v}</span>
      <span class="side" class:amber={!!gpu}>{vramText.side}</span>
    </div>
  </div>
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    overflow: hidden;
    user-select: none;
    background-image:
      linear-gradient(to right, rgba(18, 48, 58, 0.32) 1px, transparent 1px),
      linear-gradient(to bottom, rgba(18, 48, 58, 0.32) 1px, transparent 1px);
    background-size: 48px 48px;
  }
  .sheet {
    --k: 1;
    position: absolute;
    left: 0;
    top: 0;
    width: 960px;
    height: 640px;
    transform-origin: 0 0;
    font-family: var(--ph-ui);
    font-variant-numeric: tabular-nums;
    color: var(--ph-ink);
  }
  .panel {
    position: absolute;
    box-sizing: border-box;
    border-radius: 6px;
  }
  .red {
    color: var(--ph-danger);
  }
  .amber {
    color: var(--ph-amber);
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
    border-bottom: 1px solid var(--ph-rule);
  }
  .klif {
    font-family: var(--ph-display);
    font-stretch: 125%;
    font-size: 40px;
    font-weight: 500;
    letter-spacing: 0.1em;
    line-height: 1;
    color: var(--ph-brand);
    text-shadow:
      0 0 8px rgba(90, 182, 235, 0.6),
      0 0 22px rgba(90, 182, 235, 0.3);
  }
  .sep {
    width: 1px;
    height: 32px;
    background: var(--ph-rule);
    flex: none;
  }
  .st {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 34px;
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    color: var(--ph-muted);
  }
  .st .dot {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 10px currentColor;
  }
  .st.cyan {
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
  }
  .st.amber {
    color: var(--ph-amber);
    text-shadow: 0 0 8px rgba(232, 176, 74, 0.45);
  }
  .st.danger {
    color: var(--ph-danger);
    text-shadow: 0 0 8px rgba(229, 97, 92, 0.5);
  }
  .st.muted .dot {
    box-shadow: none;
  }
  .st .dot.pulse {
    animation: mi-pulse 1s ease-in-out infinite;
  }
  @keyframes mi-pulse {
    50% {
      opacity: 0.35;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .st .dot.pulse {
      animation: none;
    }
  }
  .grow {
    flex: 1 1 auto;
  }
  .clock {
    font-size: 34px;
    font-weight: 400;
    letter-spacing: 0.03em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow-soft);
  }
  .clock.ver {
    font-size: 28px;
    color: var(--ph-muted);
    text-shadow: none;
  }

  /* Tier strip: 72..128 */
  .tiers {
    position: absolute;
    left: 24px;
    right: 24px;
    top: 72px;
    height: 56px;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 10px;
  }
  .tier {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    border: 1px solid var(--ph-rule);
    border-radius: 5px;
    background: rgba(3, 9, 12, 0.72);
    font-size: 30px;
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
    color: var(--ph-brand);
    white-space: nowrap;
    overflow: hidden;
  }
  .tier.sel {
    border: 2px solid var(--ph-cyan);
    background: rgba(18, 48, 58, 0.45);
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
    box-shadow:
      0 0 12px rgba(127, 227, 255, 0.28),
      inset 0 0 14px rgba(127, 227, 255, 0.12);
  }
  .tier.na:not(.sel),
  .tier.locked {
    color: var(--ph-muted);
  }
  .tier.locked {
    border-color: rgba(23, 79, 92, 0.55);
  }
  .tier.fault {
    border: 2px solid var(--ph-danger);
    background: rgba(60, 14, 14, 0.3);
    color: #ff8f88;
    text-shadow: 0 0 8px rgba(229, 97, 92, 0.5);
    box-shadow: 0 0 12px rgba(229, 97, 92, 0.3);
  }
  .td {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--ph-cyan);
    box-shadow: 0 0 6px rgba(127, 227, 255, 0.6);
    flex: none;
  }
  .td.bad {
    background: transparent;
    box-shadow: inset 0 0 0 2px var(--ph-amber);
  }
  .td.run {
    background: var(--ph-hot);
    box-shadow:
      0 0 0 4px rgba(127, 227, 255, 0.3),
      0 0 10px var(--ph-cyan);
  }
  .tier.locked .td:not(.run) {
    background: #2a5566;
    box-shadow: none;
  }
  .tier.fault .td {
    background: var(--ph-danger);
    box-shadow: 0 0 8px var(--ph-danger);
  }

  /* Model line: 138..178 */
  .mline {
    left: 24px;
    right: 24px;
    top: 138px;
    height: 40px;
    display: flex;
    align-items: center;
    padding: 0 16px;
    font-size: 26px;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow-soft);
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim {
    color: #9fd3e4;
  }
  .mline span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The VRAM cliff: 188..550, left */
  .cliffp {
    left: 24px;
    top: 188px;
    width: 412px;
    height: 362px;
    background-image:
      linear-gradient(to right, rgba(18, 48, 58, 0.5) 1px, transparent 1px),
      linear-gradient(to bottom, rgba(18, 48, 58, 0.5) 1px, transparent 1px);
    background-size: 40px 40px;
    background-position: -1px -1px;
  }
  .cap {
    position: absolute;
    left: 16px;
    top: 10px;
    font-size: 24px;
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
  }
  .cliff {
    position: absolute;
    left: 14px;
    right: 14px;
    top: 50px;
    bottom: 14px;
  }

  /* Hero: 188..398, right */
  .hero {
    left: 448px;
    right: 24px;
    top: 188px;
    height: 210px;
    padding: 12px 20px 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
  }
  .lbl {
    font-size: 26px;
    text-transform: none;
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lbl.amber {
    color: var(--ph-amber);
    text-shadow: 0 0 8px rgba(232, 176, 74, 0.45);
  }
  .lbl.red {
    color: var(--ph-danger);
    text-shadow: 0 0 8px rgba(229, 97, 92, 0.5);
  }
  .fig {
    display: flex;
    align-items: baseline;
    gap: 14px;
    align-self: center;
    white-space: nowrap;
    overflow: hidden;
    line-height: 1;
    color: var(--ph-cyan);
  }
  .fig b {
    font-family: var(--ph-display);
    font-size: 116px;
    font-weight: 320;
    font-stretch: 108%;
    letter-spacing: 0.01em;
    text-shadow:
      0 0 12px rgba(127, 227, 255, 0.55),
      0 0 34px rgba(90, 182, 235, 0.3);
  }
  .unit {
    font-size: 36px;
    font-weight: 350;
    letter-spacing: 0.04em;
    color: var(--ph-brand);
  }
  .fig.amber {
    color: var(--ph-amber);
  }
  .fig.amber b {
    text-shadow:
      0 0 12px rgba(232, 176, 74, 0.5),
      0 0 34px rgba(232, 176, 74, 0.22);
  }
  .fig.amber .unit {
    color: var(--ph-amber);
  }
  /* the last figure, not a live one */
  .fig.faded b,
  .fig.faded .unit {
    color: var(--ph-muted);
    text-shadow: 0 0 10px rgba(79, 152, 180, 0.25);
  }
  .fig.dim b,
  .fig.dim .unit {
    color: #2f6377;
    text-shadow: none;
  }
  .line {
    height: 46px;
    display: flex;
    align-items: center;
    border-top: 1px solid var(--ph-grat);
    margin: 0 -20px;
    padding: 0 20px;
    font-size: 26px;
    color: #9fd3e4;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .line.amber {
    color: var(--ph-amber);
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
    grid-template-columns: 180px minmax(0, 1fr);
    align-items: center;
    min-height: 0;
  }
  .row + .row {
    border-top: 1px solid var(--ph-grat);
  }
  .row .k {
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: 20px;
    border-right: 1px solid var(--ph-grat);
    font-size: 24px;
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
    white-space: nowrap;
  }
  .row .v {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 0 18px;
    white-space: nowrap;
    overflow: hidden;
  }
  .row .v b {
    font-size: 40px;
    font-weight: 400;
    color: var(--ph-hot);
    text-shadow: var(--ph-glow-soft);
  }
  .row .v b.red {
    color: var(--ph-danger);
  }
  .row .v small {
    font-size: 26px;
    color: #9fd3e4;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v b {
    color: #2f6377;
    text-shadow: none;
  }
  .rows.dim .k {
    color: #66c3de;
    text-shadow: none;
  }

  /* Fault: the hero and the rows together */
  .fpanel {
    left: 448px;
    right: 24px;
    top: 188px;
    height: 362px;
    padding: 16px 20px;
    border-color: rgba(229, 97, 92, 0.85);
    box-shadow:
      0 0 16px rgba(229, 97, 92, 0.25),
      inset 0 0 24px rgba(229, 97, 92, 0.1);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    row-gap: 12px;
  }
  .ftitle {
    font-size: 36px;
    font-weight: 400;
    line-height: 1.2;
    color: #ffb3ad;
    text-shadow: 0 0 10px rgba(229, 97, 92, 0.4);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 5;
    line-clamp: 5;
    -webkit-box-orient: vertical;
  }
  .fexit {
    font-size: 28px;
    color: var(--ph-danger);
  }

  /* VRAM strip: 560..620 */
  .vram {
    left: 24px;
    right: 24px;
    top: 560px;
    height: 60px;
    display: flex;
    align-items: baseline;
    gap: 22px;
    padding: 0 20px;
    line-height: 58px;
  }
  .vram .k {
    width: 150px;
    flex: none;
    font-size: 24px;
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
  }
  .vram .v {
    font-size: 36px;
    font-weight: 400;
    color: var(--ph-hot);
    text-shadow: var(--ph-glow-soft);
    white-space: nowrap;
  }
  .vram .k.red,
  .vram .v.red {
    color: var(--ph-danger);
    text-shadow: 0 0 8px rgba(229, 97, 92, 0.45);
  }
  .vram .k.amber,
  .vram .v.amber {
    color: var(--ph-amber);
    text-shadow: 0 0 8px rgba(232, 176, 74, 0.4);
  }
  .vram .side {
    margin-left: auto;
    font-size: 26px;
    color: #9fd3e4;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .vram .side.amber {
    color: var(--ph-amber);
  }

  /* Leave panel mode: shown while the pointer is over the panel, after a tap, or with focus. */
  .back {
    flex: none;
    height: 40px;
    display: inline-flex;
    align-items: center;
    gap: 10px;
    padding: 0 16px 0 12px;
    border-radius: 5px;
    background: rgba(3, 9, 12, 0.9);
    box-shadow: inset 0 0 0 1px var(--ph-rule);
    color: var(--ph-brand);
    font-size: 24px;
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.15s ease;
  }
  .mini:hover .back,
  .mini.tapped .back,
  .back:focus-visible {
    opacity: 1;
    pointer-events: auto;
  }
  .back:hover {
    color: var(--ph-hot);
    box-shadow: inset 0 0 0 1px var(--ph-brand);
  }
  .back svg {
    width: 24px;
    height: 24px;
    overflow: visible;
  }
  .back path,
  .back rect {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    vector-effect: non-scaling-stroke;
  }
  @media (prefers-reduced-motion: reduce) {
    .back {
      transition: none;
    }
  }
</style>
