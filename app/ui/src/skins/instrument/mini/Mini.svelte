<script lang="ts">
  // Mini panel (960 x 640, read from 1 m): read-only. A fixed 960 x 640 sheet scaled to fit, laid out like the
  // full window in miniature and the same in every state (only the contents change):
  //   header      KLIF · status lamp + phase · uptime / elapsed (version while idle)
  //   tier strip  HIGH · MEDIUM · LOW · KREA with their state lamps (selected, running, cannot launch, locked)
  //   model line  the running model, or the selected tier's
  //   left        the VRAM dial (idle: fit preview of the selected tier) over the backend toggle
  //   right       hero plate: label, the drum counter + unit, a status line; two fixed rows per kind
  //               (LLM: context, prefill; image: last image, images)
  //   VRAM strip  VRAM / FITS (idle) / RESIDENT (dormant) / SPILL with an LED meter and the value
  // A fault covers the hero and the rows. Dormant GPU: amber lamp and label, the last figure faded.
  // Smallest text 24 px (labels), values 34 px and up.
  import { onDestroy } from 'svelte';
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtClock, fmtCtx, fmtGiB, fmtInt, fmtTps } from '../../../lib/model/format';
  import {
    PHASE_LABEL,
    availLabel,
    fmtAgo,
    fmtDur,
    fmtJobS,
    fmtLeft,
    frac,
    kindOf,
    modelShort,
    phaseOf,
    pointerSlot,
    sessionSpanS,
    sessionVram,
    shortLabel,
    sleepOf,
    slotById,
    viewOf,
  } from '../theme';
  import Drum from '../parts/Drum.svelte';
  import StepDrum from '../parts/StepDrum.svelte';
  import Led from '../parts/Led.svelte';
  import LedBar from '../parts/LedBar.svelte';
  import MiniVram from '../parts/MiniVram.svelte';
  import BackendToggle from '../parts/BackendToggle.svelte';
  import Icon from '../parts/Icon.svelte';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  // "Leave panel mode": the only control on this read-only layout. Invisible until the pointer is over the
  // panel (hover) or it takes keyboard focus; a touch/pen tap reveals it for a few seconds.
  const canLeave = $derived(!!(vm.host?.panel?.available || vm.host?.panel?.active));
  let tapped = $state(false);
  let tapTimer: ReturnType<typeof setTimeout> | undefined;
  function onTap(e: PointerEvent) {
    if (e.pointerType === 'mouse') return;
    tapped = true;
    clearTimeout(tapTimer);
    tapTimer = setTimeout(() => (tapped = false), 4500);
  }
  onDestroy(() => clearTimeout(tapTimer));

  const s = $derived(vm.session);
  const phase = $derived(phaseOf(vm));
  const view = $derived(viewOf(vm));
  const kind = $derived(kindOf(vm));
  const ptr = $derived(pointerSlot(vm));
  const slot = $derived(slotById(vm, ptr));
  const model = $derived(s?.model ?? slot?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const busy = $derived(view === 'loading' || view === 'stopping');
  const ready = $derived(slot?.availability === 'ready');

  const pf = $derived(llm?.prefill ?? null);
  const prefilling = $derived(view === 'live-llm' && !!llm && llm.activity === 'prefill' && !!pf && pf.tokens > 0);
  const generating = $derived(view === 'live-img' && !!img && img.activity === 'generating' && img.steps > 0);
  const lastJob = $derived(img?.recent.length ? img.recent[img.recent.length - 1] : null);
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || slot?.recipe?.ctxTokens || 0);
  const ctxFrac = $derived(llm ? frac(llm.context.usedTokens, llm.context.totalTokens) : 0);

  // GPU dormant: asleep between requests, or waking while the VRAM is restored from system RAM.
  const sleep = $derived(s?.phase === 'live' ? sleepOf(vm.vram) : null);
  const waking = $derived(sleep?.phase === 'waking');
  const tone = $derived(phase === 'fault' ? 'orange' : sleep || view === 'loading' ? 'amber' : 'cyan');
  const statusWord = $derived(sleep ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : PHASE_LABEL[phase]);

  const lastText = $derived.by(() => {
    const ls = vm.lastSession;
    if (!ls) return '';
    const who = shortLabel(slotById(vm, ls.slot)?.label ?? ls.model.name);
    return `last ${who} · ${fmtDur(ls.uptimeS)}${ls.ended === 'fault' ? ' · fault' : ''} · ${fmtAgo(ls.endedAgoS)}`;
  });

  // ---- hero: label, drum figure + unit, status line ---------------------------------------------------
  type Fig = { kind: 'drum'; value: number; int: number; blank: boolean } | { kind: 'step'; step: number; steps: number; blank: boolean };
  type Hero = { label: string; tone: '' | 'amber'; fig: Fig; unit: string; dim: boolean; line: string; lineTone: '' | 'amber' };
  const blankDrum: Fig = { kind: 'drum', value: 0, int: 2, blank: true };
  const pctDrum = (f: number): Fig => ({ kind: 'drum', value: f * 100, int: f >= 0.9995 ? 3 : 2, blank: false });
  const tpsDrum = (v: number): Fig => ({ kind: 'drum', value: v, int: v >= 99.95 ? 3 : 2, blank: false });
  const hero = $derived.by<Hero>(() => {
    const base = { tone: '' as const, dim: false, lineTone: '' as const };
    const idleLabel = kind === 'image' ? 'DIFFUSION STEP' : 'DECODE SPEED';
    const idleFig: Fig = kind === 'image' ? { kind: 'step', step: 0, steps: img?.steps ?? 8, blank: true } : blankDrum;
    if (sleep) {
      if (waking) return { ...base, label: 'GPU WAKING', tone: 'amber', fig: pctDrum(sleep.restored), unit: '%', line: `${fmtGiB(sleep.pagedGiB)} GiB still in system RAM`, lineTone: 'amber' };
      return {
        ...base,
        label: sleep.power ? `GPU ASLEEP · ${sleep.power}` : 'GPU ASLEEP',
        tone: 'amber',
        fig: llm ? tpsDrum(llm.decodeTps) : idleFig,
        unit: llm ? 'tok/s' : '',
        dim: true,
        line: `${fmtGiB(sleep.pagedGiB)} GiB paged out · ${fmtDur(sleep.sinceS)}`,
        lineTone: 'amber',
      };
    }
    if (view === 'loading' && s) {
      const st = s.loading?.steps.find((x) => x.state === 'active');
      return { ...base, label: 'STARTUP', tone: 'amber', fig: pctDrum(frac(s.loading?.fraction ?? 0, 1)), unit: '%', line: st ? `${st.label}${st.detail ? ` · ${st.detail}` : ''}` : 'starting the server' };
    }
    if (view === 'stopping') return { ...base, label: 'STOPPING', fig: idleFig, unit: kind === 'image' ? '' : 'tok/s', dim: true, line: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB`, lineTone: 'amber' };
    if (view === 'live-llm' && llm) {
      if (prefilling && pf) return { ...base, label: 'PREFILL PROGRESS', fig: pctDrum(frac(pf.doneTokens, pf.tokens)), unit: '%', line: `${fmtInt(pf.doneTokens)} / ${fmtInt(pf.tokens)} tok` };
      return { ...base, label: 'DECODE SPEED', fig: tpsDrum(llm.decodeTps), unit: 'tok/s', line: `${fmtInt(llm.generatedTokens)} tok generated${llm.activity === 'idle' ? ' · idle' : ''}` };
    }
    if (view === 'live-img' && img) {
      const fig: Fig = { kind: 'step', step: img.step, steps: img.steps, blank: !generating };
      if (generating) return { ...base, label: `DIFFUSION STEP${img.edit ? ' · EDIT' : ''}`, fig, unit: '', line: `${img.sPerIt.toFixed(2)} s/it · ${img.width}×${img.height}` };
      return { ...base, label: 'DIFFUSION STEP', fig, unit: '', dim: true, line: 'waiting for the next job' };
    }
    if (s) return { ...base, label: idleLabel, fig: idleFig, unit: kind === 'image' ? '' : 'tok/s', dim: true, line: 'waiting for data' };
    // Idle: the selected tier, not running.
    return {
      ...base,
      label: idleLabel,
      fig: idleFig,
      unit: kind === 'image' ? '' : 'tok/s',
      dim: true,
      line: ready ? lastText || 'not running' : `cannot launch · ${slot?.reason ?? availLabel(slot?.availability ?? 'unsupported')}`,
      lineTone: ready ? '' : 'amber',
    };
  });

  // ---- rows: two fixed facts per kind, the same labels in every phase -----------------------------------
  type Row = { k: string; v: string; sub: string; hot?: boolean };
  const rows = $derived.by<Row[]>(() => {
    if (kind === 'image') {
      return [
        { k: 'LAST IMAGE', v: lastJob ? fmtJobS(lastJob.seconds) : '—', sub: lastJob ? (lastJob.edit ? 'edit' : 'new image') : '' },
        { k: 'IMAGES', v: img ? fmtInt(img.imagesThisSession) : '—', sub: img ? 'this session' : '' },
      ];
    }
    return [
      { k: 'CONTEXT', v: llm ? `${Math.round(ctxFrac * 100)}%` : '—', sub: ctxTotal ? `of ${fmtCtx(ctxTotal)}` : '', hot: ctxFrac >= 0.95 },
      prefilling && pf
        ? { k: 'PREFILL', v: fmtInt(pf.tps), sub: `tok/s · ${fmtLeft(pf.etaS)} left` }
        : { k: 'PREFILL', v: pf ? fmtInt(pf.tps) : '—', sub: pf ? 'tok/s' : '' },
    ];
  });

  // ---- fault ----------------------------------------------------------------------------------------------
  const fault = $derived(view === 'fault' ? (s?.fault ?? null) : null);
  const exitText = $derived(fault ? (fault.exitCode === undefined ? 'no exit code reported' : `exit code ${fault.exitCodeHex ?? fault.exitCode}`) : '');
  const logLine = $derived.by(() => {
    const t = fault?.logTail ?? [];
    for (let i = t.length - 1; i >= 0; i--) if (t[i].trim()) return t[i].trim();
    return '';
  });

  // ---- VRAM strip -----------------------------------------------------------------------------------------
  const total = $derived(Math.max(0.01, vm.vram.totalGiB));
  const baseline = $derived(vm.vram.baselineGiB ?? vm.vram.usedGiB);
  const expected = $derived(view === 'idle' ? (slot?.expectedVram ?? []) : []);
  const fitTotal = $derived(baseline + expected.reduce((a, l) => a + l.gib, 0));
  const spare = $derived(vm.vram.totalGiB - fitTotal);
  const spillGiB = $derived(vm.vram.spillMiB / 1024);
  const lowFree = $derived(!!s && vm.vram.totalGiB - vm.vram.usedGiB < vm.vram.warnBelowGiB);
  const strip = $derived.by(() => {
    if (expected.length)
      return spare >= 0
        ? { k: 'FITS', v: `${fmtGiB(spare)} GiB spare`, f: fitTotal / total, tone: '' }
        : { k: 'OVER', v: `by ${fmtGiB(-spare)} GiB`, f: 1, tone: 'hot' };
    if (spillGiB > 0) return { k: 'SPILL', v: `${fmtGiB(spillGiB)} GiB`, f: vm.vram.usedGiB / total, tone: 'hot' };
    if (sleep) return { k: 'RESIDENT', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)} GiB`, f: vm.vram.usedGiB / total, tone: 'amber' };
    return { k: 'VRAM', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)} GiB`, f: vm.vram.usedGiB / total, tone: lowFree ? 'amber' : '' };
  });
</script>

<svelte:window onpointerdown={onTap} />

{#snippet figure(f: Fig, dim: boolean)}
  {#if f.kind === 'step'}
    <div class="sdrum" class:dim style="--cells:{2 * String(f.steps).length}"><StepDrum step={f.step} steps={f.steps} variant="mini" blank={f.blank} /></div>
  {:else}
    <div class="drum" class:dim style="--cells:{f.int + 1}"><Drum value={f.value} intDigits={f.int} variant="mini" blank={f.blank} /></div>
  {/if}
{/snippet}

<div class="mini">
  <!-- header -->
  <div class="hdr">
    <span class="klif">KLIF</span>
    <span class="sep"></span>
    <Led on={phase !== 'idle'} {tone} size="calc(18 * var(--u))" pulse={waking || view === 'loading'} />
    <span class="ph {tone}" class:off={phase === 'idle'}>{statusWord}</span>
    <span class="grow"></span>
    {#if s}
      <span class="clock">{fmtClock(view === 'loading' ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS)}</span>
    {:else if vm.host?.appVersion}
      <span class="clock ver">v{vm.host.appVersion}</span>
    {/if}
    {#if canLeave}
      <button class="back" class:tapped type="button" onclick={() => actions.togglePanel?.()} title="Leave panel mode" aria-label="Leave panel mode">
        <Icon name="window" size="calc(24 * var(--u))" />
      </button>
    {/if}
  </div>

  <!-- tier strip -->
  <div class="tiers">
    {#each vm.slots as t (t.id)}
      {@const sel = t.id === ptr}
      {@const na = t.availability !== 'ready'}
      {@const running = !!s && s.slot === t.id}
      <div class="tier" class:sel class:na class:locked={busy && !running} class:fault={running && view === 'fault'}>
        <Led on={running || na || (sel && !s)} tone={running ? tone : na ? 'orange' : 'cyan'} size="calc(14 * var(--u))" pulse={running && view === 'loading'} />
        <span>{shortLabel(t.label)}</span>
      </div>
    {/each}
  </div>

  <!-- model line -->
  <div class="panel mline" class:dim={!s}><span>{modelShort(model)}</span></div>

  <!-- left: the VRAM dial over the backend toggle -->
  <div class="panel left">
    <div class="dial">
      {#if view === 'idle'}
        <MiniVram vram={vm.vram} mode="fit" fit={{ baseline, layers: expected }} />
      {:else}
        <MiniVram vram={sessionVram(vm.vram, sessionSpanS(s))} mode={view === 'fault' ? 'fault' : 'live'} />
      {/if}
    </div>
    <span class="rule"></span>
    <div class="be"><BackendToggle backend={model?.backend ?? null} variant="mini" /></div>
  </div>

  {#if view === 'fault'}
    <!-- fault: covers the hero and the rows -->
    <div class="panel fpanel" role="alert">
      <div class="hl hot">FAULT · {fmtAgo(fault?.sinceS ?? 0)}</div>
      <div class="ftitle">{fault?.title ?? 'The server stopped'}</div>
      <div class="fexit">{exitText}</div>
      {#if logLine}<div class="flog">{logLine}</div>{/if}
    </div>
  {:else}
    <!-- hero -->
    <div class="panel hero">
      <div class="hl {hero.tone}">{hero.label}</div>
      <div class="fig" class:dim={hero.dim}>
        {@render figure(hero.fig, hero.dim)}
        {#if hero.unit}<span class="unit">{hero.unit}</span>{/if}
      </div>
      <div class="line {hero.lineTone}">{hero.line}</div>
    </div>

    <!-- rows -->
    <div class="panel rows" class:dim={!s || view === 'loading'}>
      {#each rows as r (r.k)}
        <div class="row">
          <span class="k">{r.k}</span>
          <span class="v"><b class:hot={r.hot}>{r.v}</b>{#if r.sub}<small>{r.sub}</small>{/if}</span>
        </div>
      {/each}
    </div>
  {/if}

  <!-- VRAM strip -->
  <div class="panel vram">
    <span class="k {strip.tone}">{strip.k}</span>
    <div class="vbar"><LedBar fraction={strip.f} segments={16} tone={sleep ? 'amber' : 'cyan'} label="VRAM" /></div>
    <span class="v {strip.tone}">{strip.v}</span>
  </div>
</div>

<style>
  /* The sheet: 960 x 640 design px, scaled by --u (the root sets --k = min(w / 960, h / 640)), centred. */
  .mini {
    position: absolute;
    left: 50%;
    top: 50%;
    width: calc(960 * var(--u));
    height: calc(640 * var(--u));
    transform: translate(-50%, -50%);
    background-color: #1e1f22;
    background-image: var(--tex, none);
    background-size: 384px 384px;
    font-family: var(--font-text);
    color: var(--cream);
  }
  .mini > .hdr,
  .mini > .tiers,
  .mini > .panel {
    position: absolute;
  }
  .mini > .panel {
    border-radius: calc(6 * var(--u));
  }
  .hot {
    color: var(--orange);
  }
  .amber {
    color: var(--amber);
  }

  /* header: 0..60 */
  .hdr {
    left: calc(24 * var(--u));
    right: calc(24 * var(--u));
    top: 0;
    height: calc(60 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(16 * var(--u));
    white-space: nowrap;
    border-bottom: 2px solid rgba(237, 230, 214, 0.18);
  }
  .klif {
    font-family: var(--font-label);
    font-weight: 700;
    font-size: calc(42 * var(--u));
    letter-spacing: 0.04em;
    line-height: 1;
  }
  .sep {
    width: 2px;
    height: calc(32 * var(--u));
    background: rgba(237, 230, 214, 0.25);
    margin: 0 calc(4 * var(--u));
  }
  .ph {
    font-family: var(--font-label);
    font-weight: 600;
    font-size: calc(32 * var(--u));
    letter-spacing: 0.1em;
    line-height: 1;
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
  .grow {
    flex: 1 1 auto;
  }
  .clock {
    font-weight: 500;
    font-size: calc(34 * var(--u));
    line-height: 1;
    letter-spacing: 0.02em;
  }
  .clock.ver {
    font-size: calc(28 * var(--u));
    color: var(--muted);
  }
  /* leave panel mode: shown while the pointer is over the panel, it has focus, or after a tap */
  .back {
    width: calc(46 * var(--u));
    height: calc(38 * var(--u));
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid #08090a;
    border-radius: calc(5 * var(--u));
    background: linear-gradient(180deg, #2c2d31, #1d1e21);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.07);
    color: var(--cream);
    cursor: pointer;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.18s ease;
    flex: none;
  }
  .back :global(.ic) {
    stroke-width: 1.5;
  }
  .back.tapped,
  .back:focus-visible {
    opacity: 1;
    pointer-events: auto;
  }
  @media (hover: hover) {
    .mini:hover .back {
      opacity: 1;
      pointer-events: auto;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .back {
      transition: none;
    }
  }

  /* tier strip: 70..126 */
  .tiers {
    left: calc(24 * var(--u));
    right: calc(24 * var(--u));
    top: calc(70 * var(--u));
    height: calc(56 * var(--u));
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: calc(8 * var(--u));
  }
  .tier {
    display: flex;
    align-items: center;
    gap: calc(14 * var(--u));
    padding: 0 calc(18 * var(--u));
    border-radius: calc(6 * var(--u));
    border: 1px solid #08090a;
    background: linear-gradient(180deg, #2a2b2f, #1f2023);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.06);
    font-family: var(--font-label);
    font-weight: 600;
    font-size: calc(30 * var(--u));
    letter-spacing: 0.08em;
    color: rgba(237, 230, 214, 0.72);
    white-space: nowrap;
    overflow: hidden;
  }
  .tier.sel {
    border-color: var(--cyan);
    color: var(--cream);
    box-shadow:
      inset 0 0 0 1px rgba(90, 182, 235, 0.35),
      0 0 calc(10 * var(--u)) rgba(90, 182, 235, 0.22);
  }
  .tier.na,
  .tier.locked {
    color: rgba(237, 230, 214, 0.4);
  }
  .tier.fault {
    border-color: var(--orange);
    color: var(--orange);
    box-shadow: inset 0 0 0 1px rgba(255, 107, 44, 0.35);
  }

  /* model line: 136..178 */
  .mline {
    left: calc(24 * var(--u));
    right: calc(24 * var(--u));
    top: calc(136 * var(--u));
    height: calc(42 * var(--u));
    display: flex;
    align-items: center;
    padding: 0 calc(18 * var(--u));
    font-weight: 500;
    font-size: calc(26 * var(--u));
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim {
    color: rgba(237, 230, 214, 0.8);
  }
  .mline span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* left: VRAM dial over the backend toggle, 188..550 */
  .left {
    left: calc(24 * var(--u));
    top: calc(188 * var(--u));
    width: calc(412 * var(--u));
    height: calc(362 * var(--u));
  }
  .dial {
    position: absolute;
    left: calc(14 * var(--u));
    right: calc(14 * var(--u));
    top: calc(14 * var(--u));
    height: calc(234 * var(--u));
  }
  .rule {
    position: absolute;
    left: calc(20 * var(--u));
    right: calc(20 * var(--u));
    top: calc(262 * var(--u));
    height: 2px;
    background: rgba(237, 230, 214, 0.14);
  }
  .be {
    position: absolute;
    left: 0;
    right: 0;
    top: calc(264 * var(--u));
    bottom: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* hero: 188..398, right */
  .hero {
    left: calc(448 * var(--u));
    right: calc(24 * var(--u));
    top: calc(188 * var(--u));
    height: calc(210 * var(--u));
    padding: calc(16 * var(--u)) calc(22 * var(--u)) 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
  }
  .hl {
    font-family: var(--font-label);
    font-weight: 600;
    font-size: calc(26 * var(--u));
    letter-spacing: 0.1em;
    line-height: 1;
    color: var(--cream-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hl.amber {
    color: var(--amber);
  }
  .hl.hot {
    color: var(--orange);
  }
  .fig {
    display: flex;
    align-items: flex-end;
    gap: calc(16 * var(--u));
    align-self: center;
    min-width: 0;
  }
  .drum,
  .sdrum {
    height: calc(96 * var(--u));
    flex: 0 0 auto;
    font-size: calc(84 * var(--u));
    --drum-gap: calc(4 * var(--u));
    --drum-pad: calc(5 * var(--u));
    --drum-radius: calc(7 * var(--u));
    --digit-dy: -0.02em;
  }
  .drum {
    width: calc((var(--cells) * 60 + 28 + 26) * var(--u));
    --dot-w: calc(28 * var(--u));
  }
  .sdrum {
    width: calc((var(--cells) * 62 + 44 + 26) * var(--u));
    --slash-w: calc(44 * var(--u));
  }
  .unit {
    margin-bottom: calc(10 * var(--u));
    font-weight: 500;
    font-size: calc(36 * var(--u));
    line-height: 1;
    white-space: nowrap;
  }
  .fig.dim .drum,
  .fig.dim .sdrum,
  .fig.dim .unit {
    opacity: 0.42;
  }
  .line {
    height: calc(48 * var(--u));
    display: flex;
    align-items: center;
    border-top: 1px solid rgba(0, 0, 0, 0.55);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
    margin: 0 calc(-22 * var(--u));
    padding: 0 calc(22 * var(--u));
    font-size: calc(26 * var(--u));
    color: var(--cream-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .line.amber {
    color: var(--amber);
  }

  /* rows: 408..550, right */
  .rows {
    left: calc(448 * var(--u));
    right: calc(24 * var(--u));
    top: calc(408 * var(--u));
    height: calc(142 * var(--u));
    display: grid;
    grid-template-rows: repeat(2, minmax(0, 1fr));
  }
  .row {
    display: grid;
    grid-template-columns: calc(186 * var(--u)) minmax(0, 1fr);
    align-items: center;
    min-height: 0;
  }
  .row + .row {
    border-top: 1px solid rgba(0, 0, 0, 0.55);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
  }
  .row .k {
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: calc(22 * var(--u));
    border-right: 1px solid rgba(237, 230, 214, 0.1);
    font-family: var(--font-label);
    font-weight: 600;
    font-size: calc(24 * var(--u));
    letter-spacing: 0.1em;
    color: var(--cream-2);
    white-space: nowrap;
  }
  .row .v {
    display: flex;
    align-items: baseline;
    gap: calc(12 * var(--u));
    padding: 0 calc(20 * var(--u));
    white-space: nowrap;
    overflow: hidden;
  }
  .row .v b {
    font-size: calc(40 * var(--u));
    font-weight: 500;
  }
  .row .v small {
    font-size: calc(26 * var(--u));
    color: var(--cream-2);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v b {
    color: rgba(237, 230, 214, 0.42);
  }

  /* fault: the hero and the rows together */
  .fpanel {
    left: calc(448 * var(--u));
    right: calc(24 * var(--u));
    top: calc(188 * var(--u));
    height: calc(362 * var(--u));
    padding: calc(18 * var(--u)) calc(22 * var(--u));
    display: flex;
    flex-direction: column;
    gap: calc(14 * var(--u));
    border: 2px solid var(--orange);
    background-color: #2a221f;
    background-image: var(--tex, none), linear-gradient(180deg, rgba(255, 107, 44, 0.16), rgba(255, 107, 44, 0.05));
    box-shadow: 0 0 calc(14 * var(--u)) rgba(255, 107, 44, 0.2);
    overflow: hidden;
  }
  .ftitle {
    font-weight: 500;
    font-size: calc(36 * var(--u));
    line-height: 1.18;
    color: var(--cream);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
  }
  .fexit {
    margin-top: auto;
    font-weight: 500;
    font-size: calc(30 * var(--u));
    color: var(--orange);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .flog {
    font-family: var(--font-mono);
    font-size: calc(24 * var(--u));
    color: var(--cream-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* VRAM strip: 560..620 */
  .vram {
    left: calc(24 * var(--u));
    right: calc(24 * var(--u));
    top: calc(560 * var(--u));
    height: calc(60 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(20 * var(--u));
    padding: 0 calc(22 * var(--u));
  }
  .vram .k {
    flex: 0 0 calc(132 * var(--u));
    font-family: var(--font-label);
    font-weight: 600;
    font-size: calc(26 * var(--u));
    letter-spacing: 0.1em;
    color: var(--cream-2);
  }
  .vram .k.hot {
    color: var(--orange);
  }
  .vram .k.amber {
    color: var(--amber);
  }
  .vbar {
    flex: 1 1 auto;
    min-width: 0;
    height: calc(24 * var(--u));
    --seg-gap: calc(5 * var(--u));
    --seg-radius: calc(3 * var(--u));
    --seg-glow: calc(7 * var(--u));
  }
  .vram .v {
    font-weight: 500;
    font-size: calc(34 * var(--u));
    white-space: nowrap;
  }
  .vram .v.hot {
    color: var(--orange);
  }
  .vram .v.amber {
    color: var(--amber);
  }
</style>
