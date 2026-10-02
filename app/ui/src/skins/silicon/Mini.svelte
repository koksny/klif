<script lang="ts">
  // Silicon, mini panel (960x640, read from 1 m). Read-only. A fixed 960x640 sheet scaled to fit, laid out
  // like the full window in miniature, and the same in every state (only the contents change):
  //   header      KLIF · status · uptime / elapsed
  //   tier strip  the four tiers (selected, running, cannot launch, locked while starting)
  //   model line  the running model, or the selected tier's
  //   die (left)  the GPU die; hero (right): one big figure with its label and a status line;
  //               rows (right): two fixed facts per kind (LLM: context, prefill; image: last image, images)
  //   VRAM strip  the 8-segment gauge at true scale, used / total (idle: the selected tier's fit)
  // A fault covers the hero and the rows. Dormant GPU: amber status, the last figure faded, paged-out
  // allocations hatched in the die and the gauge. Smallest text 24 px (labels), values 34 px and up.
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtCtx, fmtGiB, fmtInt, fmtPct, fmtTps } from '../../lib/model/format';
  import DieCanvas from './DieCanvas.svelte';
  import { held } from './held.svelte';
  import { useSleep } from './sleep.svelte';
  import { availabilityText, baselineOf, fmtAgo, fmtDur, fmtEta } from './text';

  let { vm, actions }: { vm: ViewModel; actions?: Actions } = $props();

  // "Back to window": the one control on the panel, shown while the pointer is over the panel or it has focus.
  const canLeave = $derived(!!(vm.host?.panel?.available || vm.host?.panel?.active));

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
  const prefilling = $derived(liveLlm && !!llm?.prefill && llm.activity === 'prefill');
  const tps = held(() => llm?.decodeTps ?? 0);
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || slot?.recipe?.ctxTokens || 0);
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0);
  const preFrac = $derived(prefilling && llm?.prefill && llm.prefill.tokens > 0 ? llm.prefill.doneTokens / llm.prefill.tokens : 0);
  const imgGen = $derived(liveImg && !!img && img.activity === 'generating' && img.steps > 0);
  const imgFill = $derived(imgGen && img ? img.step / img.steps : null);
  const lastJob = $derived(img && img.recent.length ? img.recent[img.recent.length - 1] : null);

  // GPU dormant: asleep between requests, or waking while the VRAM is restored from system RAM.
  const sleep = useSleep(() => vm);
  const dz = $derived(phase === 'live' ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);

  const tone = $derived(dz || loading ? 'amber' : faulted ? 'fault' : s && phase === 'live' ? 'live' : 'off');
  const phaseLabel = $derived(
    phase === 'idle' ? 'IDLE' : phase === 'live' ? 'LIVE' : phase === 'fault' ? 'FAULT' : phase === 'stopping' ? 'STOPPING' : phase === 'loading' ? 'LOADING' : 'STARTING',
  );
  const statusLabel = $derived(dz ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : phaseLabel);
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
  type Hero = { label: string; tone: '' | 'amber' | 'red'; fig: string; unit: string; dim: boolean; line: string; lineTone: '' | 'amber' | 'red' };
  const last = $derived(vm.lastSession);
  const lastText = $derived.by(() => {
    if (!last) return '';
    const label = short(vm.slots.find((x) => x.id === last.slot)?.label ?? last.model.name);
    return `last ${label} · ${fmtDur(last.uptimeS)}${last.ended === 'fault' ? ' · fault' : ''} · ${fmtAgo(last.endedAgoS)}`;
  });
  const ready = $derived(slot?.availability === 'ready');
  const hero = $derived.by<Hero>(() => {
    const base = { tone: '' as const, dim: false, lineTone: '' as const };
    if (dz) {
      if (waking) return { ...base, label: 'GPU WAKING', tone: 'amber', fig: String(Math.round(dz.restoredFrac * 100)), unit: '%', line: `${fmtGiB(dz.pagedOutGiB)} GiB still in system RAM` };
      return {
        ...base,
        label: dz.powerState ? `GPU ASLEEP · ${dz.powerState}` : 'GPU ASLEEP',
        tone: 'amber',
        fig: llm ? fmtTps(tps.current) : '—',
        unit: llm ? 'tok/s' : '',
        dim: true,
        line: `${fmtGiB(dz.pagedOutGiB)} GiB paged out · ${fmtDur(dz.sinceS)}`,
      };
    }
    if (loading && s) {
      const step = activeStep ? `${activeStep.label}${activeStep.detail ? ` · ${activeStep.detail}` : ''}` : phaseLabel.toLowerCase();
      return { ...base, label: 'STARTUP', tone: 'amber', fig: String(Math.floor((s.loading?.fraction ?? 0) * 100)), unit: '%', line: step };
    }
    if (phase === 'stopping') return { ...base, label: 'STOPPING', fig: '—', unit: '', dim: true, line: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB` };
    if (liveLlm && llm) {
      if (prefilling && llm.prefill)
        return { ...base, label: 'PREFILL PROGRESS', fig: String(Math.floor(preFrac * 100)), unit: '%', line: `${fmtInt(llm.prefill.doneTokens)} / ${fmtInt(llm.prefill.tokens)} tok` };
      return { ...base, label: 'DECODE SPEED', fig: fmtTps(tps.current), unit: 'tok/s', line: `${fmtInt(llm.generatedTokens)} tok generated${llm.activity === 'idle' ? ' · idle' : ''}` };
    }
    if (liveImg && img) {
      if (imgGen) return { ...base, label: `DIFFUSION STEP${img.edit ? ' · EDIT' : ''}`, fig: String(img.step), unit: `/ ${img.steps}`, line: `${img.sPerIt.toFixed(2)} s/it · ${img.width}×${img.height}` };
      return { ...base, label: 'DIFFUSION STEP', fig: '—', unit: `/ ${img.steps}`, dim: true, line: 'waiting for the next job' };
    }
    if (s) return { ...base, label: kind === 'image' ? 'DIFFUSION STEP' : 'DECODE SPEED', fig: '—', unit: '', dim: true, line: 'waiting for data' };
    // Idle: the selected tier, not running.
    return {
      ...base,
      label: kind === 'image' ? 'DIFFUSION STEP' : 'DECODE SPEED',
      fig: '—',
      unit: kind === 'image' ? '' : 'tok/s',
      dim: true,
      line: ready ? (lastText || 'not running') : (slot?.reason ?? availabilityText(slot?.availability ?? 'unsupported')),
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
        ? { k: 'PREFILL', v: `${fmtInt(pf.tps)}`, sub: `tok/s · ${fmtEta(pf.etaS)} left` }
        : { k: 'PREFILL', v: pf ? fmtInt(pf.tps) : '—', sub: pf ? 'tok/s' : '' },
    ];
  });

  // ---- fault ----------------------------------------------------------------------------------------------
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const exitText = $derived(fault ? (fault.exitCode === undefined ? 'no exit code' : `exit ${fault.exitCodeHex ?? fault.exitCode}`) : '');

  // ---- VRAM strip: 8 segments = the 8 GDDR6 blocks, true scale -------------------------------------------
  const GAUGE = { x: 0, w: 400, h: 34 };
  const total = $derived(Math.max(0.01, vm.vram.totalGiB));
  const fx = (gib: number) => (Math.min(total, Math.max(0, gib)) / total) * GAUGE.w;
  const usedC = $derived(Math.min(total, Math.max(0, vm.vram.usedGiB)));
  const segW = GAUGE.w / 8;
  const pagedEnd = $derived(dz ? Math.min(total, usedC + dz.pagedOutGiB) : usedC);
  const segs = $derived(
    Array.from({ length: 8 }, (_, i) => ({
      x: i * segW,
      f: Math.min(1, Math.max(0, usedC / (total / 8) - i)),
      p: dz ? Math.min(1, Math.max(0, pagedEnd / (total / 8) - i)) : 0,
    })),
  );
  const spillGiB = $derived(vm.vram.spillMiB / 1024);
  const lowFree = $derived(!!s && vm.vram.totalGiB - vm.vram.usedGiB < vm.vram.warnBelowGiB);
  // Idle: the selected tier's expected footprint on top of what is in use, hatched.
  const base = $derived(Math.max(baselineOf(vm.vram), vm.vram.usedGiB));
  const expected = $derived(idle && slot?.expectedVram?.length ? slot.expectedVram.reduce((a, l) => a + l.gib, 0) : null);
  const spare = $derived(expected !== null ? vm.vram.totalGiB - base - expected : 0);
  const ghost = $derived(expected !== null ? { x0: fx(base), x1: fx(base + expected), over: spare < 0 } : null);
  const vramText = $derived.by(() => {
    if (ghost) return spare >= 0 ? { k: 'FITS', v: `${fmtGiB(spare)} GiB spare`, tone: '' } : { k: 'OVER', v: `by ${fmtGiB(-spare)} GiB`, tone: 'red' };
    if (spillGiB > 0) return { k: 'SPILL', v: `${fmtGiB(spillGiB)} GiB`, tone: 'red' };
    if (dz) return { k: 'RESIDENT', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)} GiB`, tone: 'amber' };
    return { k: 'VRAM', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)} GiB`, tone: lowFree ? 'amber' : '' };
  });
</script>

<div class="mini" bind:clientWidth={w} bind:clientHeight={h}>
  <div class="sheet" style="transform: translate({ox}px, {oy}px) scale({u})">
    <!-- Header -->
    <div class="hdr">
      <span class="klif">KLIF</span>
      <span class="sep"></span>
      <span class="st {tone}"><i class="dot" class:pulse={waking || loading}></i>{statusLabel}</span>
      <span class="grow"></span>
      {#if s}<span class="clock">{clock}</span>{:else if vm.host?.appVersion}<span class="clock ver">v{vm.host.appVersion}</span>{/if}
      {#if canLeave}
        <button class="back" onclick={() => actions?.togglePanel?.()} title="Leave panel mode" aria-label="Leave panel mode">
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.5 1.5h7v7M1.5 3.5h7v7h-7z" /></svg>
        </button>
      {/if}
    </div>

    <!-- Tier strip -->
    <div class="tiers">
      {#each vm.slots as t (t.id)}
        {@const sel = t.id === (s?.slot ?? vm.selected)}
        {@const na = t.availability !== 'ready'}
        {@const running = !!s && s.slot === t.id && !faulted}
        <div class="tier" class:sel class:na class:locked={busy && !running} class:fault={faulted && s?.slot === t.id}>
          <i class="td" class:run={running} class:bad={na}></i>{short(t.label)}
        </div>
      {/each}
    </div>

    <!-- Model line -->
    <div class="mline panel" class:dim={idle}><span>{modelText}</span></div>

    <!-- Die -->
    <div class="diebox panel">
      <div class="die">
        <DieCanvas
          variant="mini"
          llm={liveLlm ? llm : null}
          live={liveLlm}
          usedGiB={vm.vram.usedGiB}
          totalGiB={vm.vram.totalGiB}
          cacheFrac={null}
          jobFill={imgFill}
          pagedOutGiB={dz ? dz.pagedOutGiB : null}
          pxScale={u}
          label={dz
            ? `GPU die, GPU ${waking ? 'waking' : 'asleep'}: compute-unit tiles dark; GDDR6 blocks show ${fmtGiB(dz.residentGiB)} GiB resident and ${fmtGiB(dz.pagedOutGiB)} GiB paged out as hatched outlines`
            : 'GPU die: compute-unit tiles show the token stream; GDDR6 blocks show VRAM used'}
        />
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
        <div class="fig" class:dim={hero.dim}><b>{hero.fig}</b>{#if hero.unit}<span class="unit">{hero.unit}</span>{/if}</div>
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
      <svg class="gauge" viewBox="-2 -2 {GAUGE.w + 4} {GAUGE.h + 4}" width={GAUGE.w + 4} height={GAUGE.h + 4} aria-hidden="true">
        <defs>
          <pattern id="sim-ghost" width="10" height="10" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <line x1="0" y1="0" x2="0" y2="10" stroke="rgba(110,190,240,0.6)" stroke-width="2.5" />
          </pattern>
        </defs>
        {#each segs as sg, i (i)}
          <rect x={sg.x + 1.5} y="0" width={segW - 3} height={GAUGE.h} class="seg" />
          {#if sg.f > 0}<rect x={sg.x + 1.5} y="0" width={(segW - 3) * sg.f} height={GAUGE.h} class="segfill" class:red={spillGiB > 0} />{/if}
          {#if sg.p - sg.f > 0.004}<rect x={sg.x + 1.5 + (segW - 3) * sg.f} y="0" width={(segW - 3) * (sg.p - sg.f)} height={GAUGE.h} fill="url(#sim-ghost)" class="paged" />{/if}
        {/each}
        {#if ghost}
          <rect x={ghost.x0} y="3" width={Math.max(0, ghost.x1 - ghost.x0)} height={GAUGE.h - 6} fill="url(#sim-ghost)" class="ghost" class:over={ghost.over} />
        {/if}
        <line x1={GAUGE.w} x2={GAUGE.w} y1="-2" y2={GAUGE.h + 2} class="limit" />
      </svg>
      <span class="v {vramText.tone}">{vramText.v}</span>
    </div>
  </div>
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background-color: #0d1115;
    background-image: linear-gradient(#18202780 1px, transparent 1px), linear-gradient(90deg, #18202780 1px, transparent 1px);
    background-size: 16px 16px;
    user-select: none;
  }
  .sheet {
    position: absolute;
    left: 0;
    top: 0;
    width: 960px;
    height: 640px;
    transform-origin: 0 0;
    font-family: 'Bahnschrift', 'DIN Alternate', 'Barlow', 'Segoe UI', sans-serif;
    font-variant-numeric: tabular-nums;
    color: #f4faff;
    --cyan: #5ab6eb;
    --label: #86c3e6;
    --muted: #7c8f9e;
    --amber: #f2b33a;
    --red: #ff5a36;
    --line: #2c3c49;
  }
  .panel {
    position: absolute;
    background: #0e151b;
    border: 1px solid var(--line);
    border-radius: 4px;
    box-sizing: border-box;
  }
  .red {
    color: var(--red);
  }
  .amber {
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
    border-bottom: 1px solid #3a4c5b;
  }
  .klif {
    font-size: 40px;
    font-weight: 700;
    letter-spacing: 0.14em;
    line-height: 1;
  }
  .sep {
    width: 1px;
    height: 32px;
    background: #33424f;
    flex: none;
  }
  .st {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 34px;
    font-weight: 600;
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
    color: var(--muted);
  }
  .st .dot {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: currentColor;
  }
  .st.live {
    color: var(--cyan);
  }
  .st.amber {
    color: var(--amber);
  }
  .st.fault {
    color: var(--red);
  }
  .st .dot.pulse {
    animation: zz-pulse 1s ease-in-out infinite;
  }
  @keyframes zz-pulse {
    0%,
    100% {
      opacity: 1;
    }
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
    font-weight: 300;
    letter-spacing: 0.04em;
    color: var(--cyan);
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
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 10px;
  }
  .tier {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    border: 1px solid #33475a;
    border-radius: 4px;
    background: #0e151b;
    font-size: 30px;
    font-weight: 600;
    font-stretch: 87.5%;
    letter-spacing: 0.08em;
    color: #c9d7e1;
    white-space: nowrap;
    overflow: hidden;
  }
  .tier.sel {
    border-color: var(--cyan);
    background: linear-gradient(180deg, #11263a, #0f1d29);
    color: #f4faff;
  }
  .tier.na,
  .tier.locked {
    color: #6c7f8d;
  }
  .tier.locked {
    border-color: #26333d;
  }
  .tier.fault {
    border-color: var(--red);
    background: linear-gradient(180deg, #2a1410, #1a100e);
  }
  .td {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #4fd6a4;
    flex: none;
  }
  .td.bad {
    background: transparent;
    box-shadow: inset 0 0 0 2px var(--amber);
  }
  .td.run {
    background: var(--cyan);
    box-shadow: 0 0 0 4px rgba(90, 182, 235, 0.25);
  }
  .tier.locked .td:not(.run) {
    background: #3a4a56;
  }
  .tier.fault .td {
    background: var(--red);
    box-shadow: none;
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
    color: #e3ecf2;
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim {
    color: #a9bccb;
  }
  .mline span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Die: 188..550, left */
  .diebox {
    left: 24px;
    top: 188px;
    width: 412px;
    height: 362px;
    background-color: #0c1318;
    background-image: linear-gradient(#161f26 1px, transparent 1px), linear-gradient(90deg, #161f26 1px, transparent 1px);
    background-size: 16px 16px;
  }
  .die {
    position: absolute;
    inset: 12px;
  }

  /* Hero: 188..398, right */
  .hero {
    left: 448px;
    right: 24px;
    top: 188px;
    height: 210px;
    padding: 14px 20px 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
  }
  .lbl {
    font-size: 26px;
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lbl.amber {
    color: var(--amber);
  }
  .lbl.red {
    color: var(--red);
  }
  .fig {
    display: flex;
    align-items: baseline;
    gap: 14px;
    align-self: center;
    white-space: nowrap;
    overflow: hidden;
    line-height: 1;
  }
  .fig b {
    font-size: 116px;
    font-weight: 300;
    letter-spacing: 0;
  }
  .unit {
    font-size: 36px;
    font-weight: 300;
    font-stretch: 87.5%;
    letter-spacing: 0.04em;
    color: #9fb4c3;
  }
  .fig.dim b,
  .fig.dim .unit {
    color: #4f6170;
  }
  .line {
    height: 46px;
    display: flex;
    align-items: center;
    border-top: 1px solid var(--line);
    margin: 0 -20px;
    padding: 0 20px;
    font-size: 26px;
    color: #a9bccb;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .line.amber {
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
    grid-template-columns: 180px minmax(0, 1fr);
    align-items: center;
    min-height: 0;
  }
  .row + .row {
    border-top: 1px solid var(--line);
  }
  .row .k {
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: 20px;
    border-right: 1px solid var(--line);
    font-size: 24px;
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
    color: var(--label);
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
  }
  .row .v small {
    font-size: 26px;
    color: #a9bccb;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v b {
    color: #5f7280;
  }

  /* Fault: the hero and the rows together */
  .fpanel {
    left: 448px;
    right: 24px;
    top: 188px;
    height: 362px;
    padding: 16px 20px;
    border-color: rgba(255, 90, 54, 0.85);
    background: linear-gradient(180deg, #1a1210, #120f0f 60%, #0f1214);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    row-gap: 12px;
  }
  .ftitle {
    font-size: 36px;
    font-weight: 500;
    line-height: 1.2;
    color: #f4faff;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 5;
    line-clamp: 5;
    -webkit-box-orient: vertical;
  }
  .fexit {
    font-size: 28px;
    color: var(--red);
  }

  /* VRAM strip: 560..616 */
  .vram {
    left: 24px;
    right: 24px;
    top: 560px;
    height: 60px;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 0 20px;
  }
  .vram .k {
    width: 130px;
    flex: none;
    font-size: 24px;
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
    color: var(--label);
  }
  .vram .k.red {
    color: var(--red);
  }
  .vram .k.amber {
    color: var(--amber);
  }
  .gauge {
    flex: none;
    overflow: visible;
  }
  .seg {
    fill: #1a2631;
    stroke: #3d5161;
    stroke-width: 1;
  }
  .segfill {
    fill: var(--cyan);
  }
  .segfill.red {
    fill: var(--red);
  }
  .ghost {
    stroke: #8fd0f5;
    stroke-width: 2;
    stroke-dasharray: 8 5;
  }
  .ghost.over {
    stroke: var(--red);
  }
  .paged {
    stroke: var(--amber);
    stroke-width: 1.5;
    stroke-dasharray: 6 4;
  }
  .limit {
    stroke: var(--red);
    stroke-width: 2;
    stroke-dasharray: 5 4;
  }
  .vram .v {
    margin-left: auto;
    font-size: 32px;
    font-weight: 400;
    white-space: nowrap;
  }
  .vram .v.red {
    color: var(--red);
  }
  .vram .v.amber {
    color: var(--amber);
  }

  /* Leave panel mode: shown while the pointer is over the panel or it has focus. */
  .back {
    width: 44px;
    height: 34px;
    display: grid;
    place-items: center;
    border: 1px solid #3a4d5b;
    border-radius: 4px;
    background: #0b1116;
    color: #86c3e6;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.15s ease;
    flex: none;
  }
  .mini:hover .back,
  .back:focus-visible {
    opacity: 1;
  }
  .back svg {
    width: 20px;
    height: 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
  }
</style>
