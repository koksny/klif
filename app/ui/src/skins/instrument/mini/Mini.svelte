<script lang="ts">
  // Mini panel (960 x 640, read from 1 m): read-only. Hero drum, model name, VRAM cliff gauge,
  // context dial, selector + backend as indicators, one labelled LED bar.
  // States share the composition and swap what each region reads:
  //   idle     hero = selected tier + availability + fit, gauge = fit preview, centre = context preset,
  //            bottom row = last session one-liner
  //   loading  hero = overall %, current step; centre = step n / N; bottom = load bar
  //   prefill  hero = prompt % on the drum, tok/s beside it; bottom = prefill bar + time left
  //   image    hero = step n / M on drums, s/it; centre = last image; bottom = step bar
  //   fault    hero = FAULT + title + since; centre = exit code; bottom = last log line
  import type { ViewModel } from '../../../lib/model/types';
  import { fmtGiB, fmtCtx, fmtTps } from '../../../lib/model/format';
  import { PHASE_LABEL, availLabel, clamp, fmtAgo, fmtDur, fmtJobS, fmtLeft, frac, phaseOf, sessionSpanS, sessionVram, pointerSlot, shortLabel, slotById } from '../theme';
  import Drum from '../parts/Drum.svelte';
  import StepDrum from '../parts/StepDrum.svelte';
  import Led from '../parts/Led.svelte';
  import LedBar from '../parts/LedBar.svelte';
  import MiniVram from '../parts/MiniVram.svelte';
  import ContextDial from '../parts/ContextDial.svelte';
  import Selector from '../parts/Selector.svelte';
  import BackendToggle from '../parts/BackendToggle.svelte';

  let { vm }: { vm: ViewModel } = $props();

  const s = $derived(vm.session);
  const phase = $derived(phaseOf(vm));
  const ptr = $derived(pointerSlot(vm));
  const ptrSlot = $derived(slotById(vm, ptr));
  const model = $derived(s?.model ?? ptrSlot?.model);
  const llm = $derived(s && (s.phase === 'live' || s.phase === 'stopping') ? s.llm : null);
  const img = $derived(s && (s.phase === 'live' || s.phase === 'stopping') ? s.image : null);
  const pf = $derived(llm?.prefill ?? null);
  const prefilling = $derived(!!llm && llm.activity === 'prefill' && !!pf && pf.tokens > 0);
  const mode = $derived(
    !s ? 'idle' : s.phase === 'fault' ? 'fault' : llm ? (prefilling ? 'prefill' : 'llm') : img ? 'image' : 'loading',
  );

  // idle: fit preview of the selected tier
  const expected = $derived(ptrSlot?.expectedVram ?? []);
  const baseline = $derived(vm.vram.baselineGiB ?? vm.vram.usedGiB);
  const fitTotal = $derived(baseline + expected.reduce((a, l) => a + l.gib, 0));
  const spare = $derived(vm.vram.totalGiB - fitTotal);
  const ready = $derived(ptrSlot?.availability === 'ready');
  const ls = $derived(vm.lastSession ?? null);

  // loading
  const steps = $derived(s?.loading?.steps ?? []);
  const active = $derived(steps.find((x) => x.state === 'active') ?? null);
  const doneN = $derived(steps.filter((x) => x.state === 'done').length);
  const loadPct = $derived(Math.round(clamp(s?.loading?.fraction ?? 0) * 100));

  // image
  const generating = $derived(!!img && img.activity === 'generating');
  const lastJob = $derived(img?.recent.length ? img.recent[img.recent.length - 1] : null);

  // fault
  const f = $derived(s?.fault ?? null);
  const logLine = $derived.by(() => {
    const t = f?.logTail ?? [];
    for (let i = t.length - 1; i >= 0; i--) if (t[i].trim()) return t[i].trim();
    return '';
  });

  // The bottom LED bar.
  const bar = $derived.by(() => {
    if (llm) {
      if (prefilling && pf) {
        const f = frac(pf.doneTokens, pf.tokens);
        return { label: 'PREFILL', f, value: `${fmtLeft(pf.etaS)} left` };
      }
      const f = frac(llm.context.usedTokens, llm.context.totalTokens);
      return { label: 'CTX', f, value: `${Math.round(f * 100)}%` };
    }
    if (img) {
      const f = frac(img.step, img.steps);
      return { label: 'STEP', f: generating ? f : 0, value: generating ? `${img.step}/${img.steps}` : 'idle' };
    }
    if (s?.loading) {
      const f = clamp(s.loading.fraction);
      return { label: 'LOAD', f, value: `${Math.round(f * 100)}%` };
    }
    const f = frac(vm.vram.usedGiB, vm.vram.totalGiB);
    return { label: 'VRAM', f, value: `${Math.round(f * 100)}%` };
  });

  const nameSize = $derived.by(() => {
    const n = (model?.name ?? '').length || 1;
    return Math.min(70, Math.floor(900 / (n * 0.62)));
  });
  const fit = (text: string, base: number, room: number) => Math.min(base, Math.floor(room / Math.max(1, text.length * 0.6)));

  const lastText = $derived.by(() => {
    if (!ls) return '';
    const who = shortLabel(slotById(vm, ls.slot)?.label ?? ls.slot);
    const speed = ls.decodeTps !== undefined ? `${fmtTps(ls.decodeTps)} tok/s` : ls.secondsPerImage !== undefined ? `${fmtJobS(ls.secondsPerImage)}/image` : '';
    return [who, fmtDur(ls.uptimeS), speed].filter(Boolean).join(' · ');
  });

  const plate = $derived.by(() => {
    if (mode === 'idle') {
      if (ptrSlot?.kind === 'image') return { k: 'SIZE', v: ptrSlot.model.imageSize ?? '—', sub: 'default' };
      return { k: 'CONTEXT', v: ptrSlot?.model.ctxTokens ? fmtCtx(ptrSlot.model.ctxTokens) : '—', sub: 'preset' };
    }
    if (mode === 'loading') return { k: 'STEP', v: `${Math.min(steps.length, doneN + 1)}/${steps.length || '—'}`, sub: active ? 'running' : 'starting' };
    if (mode === 'image') return { k: 'LAST IMAGE', v: lastJob ? fmtJobS(lastJob.seconds) : '—', sub: `${img?.imagesThisSession ?? 0} images` };
    if (mode === 'fault') {
      if (!f || f.exitCode === undefined) return { k: 'EXIT CODE', v: '—', sub: 'not reported' };
      return { k: 'EXIT CODE', v: f.exitCodeHex ?? String(f.exitCode), sub: f.exitCodeHex ? String(f.exitCode) : '' };
    }
    return null;
  });
</script>

<div class="mini">
  <div class="top">
    <span class="klif">KLIF</span>
    <span class="tsep"></span>
    <Led on={phase !== 'idle'} tone={phase === 'fault' ? 'orange' : 'cyan'} size="calc(24 * var(--u))" />
    <span class="ph" class:fault={phase === 'fault'}>{PHASE_LABEL[phase]}</span>
    <span class="rule"></span>
  </div>

  <div class="model" style="font-size: calc({nameSize} * var(--u))">{model?.name ?? '—'}</div>

  <div class="hero">
    {#if mode === 'llm' && llm}
      <div class="drum"><Drum value={llm.decodeTps} intDigits={llm.decodeTps >= 99.95 ? 3 : 2} variant="mini" /></div>
      <span class="unit">tok/s</span>
    {:else if mode === 'prefill' && pf}
      {@const p = frac(pf.doneTokens, pf.tokens) * 100}
      <div class="drum"><Drum value={p} intDigits={p >= 99.95 ? 3 : 2} variant="mini" /></div>
      <div class="side tight">
        <span class="unit pc">%</span>
        <span class="sl cy">PREFILL</span>
        <span class="sl">{Math.round(pf.tps)} tok/s</span>
      </div>
    {:else if mode === 'image' && img}
      <div class="sdrum" style="--n:{String(img.steps).length}"><StepDrum step={img.step} steps={img.steps} variant="mini" blank={!generating} /></div>
      <div class="side">
        <span class="sv">{generating && img.sPerIt > 0 ? img.sPerIt.toFixed(1) : '—'}<small>&nbsp;s/it</small></span>
        <span class="sl">{generating ? `${fmtJobS(img.elapsedS)} elapsed` : 'waiting for a job'}</span>
      </div>
    {:else if mode === 'fault' && f}
      <div class="msg fault">
        <span class="bigrow"><span class="big">FAULT</span><span class="ago">{fmtAgo(f.sinceS)}</span></span>
        <span class="line">{f.title}</span>
      </div>
    {:else if mode === 'loading'}
      <div class="msg">
        <span class="big">{PHASE_LABEL[s?.phase ?? 'loading']} {loadPct}%</span>
        <span class="line">{active ? `${active.label}${active.detail ? ` · ${active.detail}` : ''}` : 'starting the server'}</span>
      </div>
    {:else}
      <div class="msg">
        <span class="big">{ptrSlot ? shortLabel(ptrSlot.label) : '—'}</span>
        <span class="line" class:bad={!ready || spare < 0}>
          {#if !ready && ptrSlot}{availLabel(ptrSlot.availability)} · cannot launch
          {:else if expected.length}ready · {spare >= 0 ? `fits · ${fmtGiB(spare)} GiB spare` : `${fmtGiB(-spare)} GiB over the edge`}
          {:else}ready{/if}
        </span>
      </div>
    {/if}
  </div>

  <div class="lower">
    <div class="vg">
      <div class="vgdial">
        {#if mode === 'idle'}
          <MiniVram vram={vm.vram} mode="fit" fit={{ baseline, layers: expected }} />
        {:else}
          <MiniVram vram={sessionVram(vm.vram, sessionSpanS(s))} mode={mode === 'fault' ? 'fault' : 'live'} />
        {/if}
      </div>
      {#if mode !== 'idle' && vm.vram.spillMiB > 0}
        <div class="read hotv">+{Math.round(vm.vram.spillMiB)} MiB spill</div>
      {:else}
        <div class="read" class:hotv={mode === 'idle' && expected.length > 0 && spare < 0}>{fmtGiB(mode === 'idle' ? fitTotal : vm.vram.usedGiB)} / {fmtGiB(vm.vram.totalGiB)} GiB</div>
      {/if}
    </div>
    <div class="cd">
      {#if llm}
        <ContextDial used={llm.context.usedTokens} total={llm.context.totalTokens} variant="mini" />
      {:else if plate}
        <div class="plate" class:hot={mode === 'fault'}>
          <span class="pk">{plate.k}</span>
          <span class="pv" style="font-size: calc({fit(plate.v, 64, 210)} * var(--u))">{plate.v}</span>
          {#if plate.sub}<span class="ps">{plate.sub}</span>{/if}
        </div>
      {/if}
    </div>
    <span class="dsep"></span>
    <div class="right">
      <div class="selw">
        <Selector slots={vm.slots} pointer={ptr} selected={vm.selected} running={s?.slot ?? null} variant="mini" tone={mode === 'fault' ? 'orange' : 'cyan'} />
      </div>
      <span class="hrule"></span>
      <div class="tg"><BackendToggle backend={model?.backend ?? null} variant="mini" /></div>
    </div>
  </div>

  <div class="ledrow">
    {#if mode === 'idle'}
      <span class="bl">LAST</span>
      <span class="ltxt">{ls ? lastText : 'no previous session'}</span>
      {#if ls}<span class="bv dim" class:hotv={ls.ended === 'fault'}>{ls.ended === 'fault' ? 'faulted' : 'stopped'} {fmtAgo(ls.endedAgoS)}</span>{/if}
    {:else if mode === 'fault'}
      <span class="bl hotv">LOG</span>
      <span class="ltxt mono">{logLine || 'no log output'}</span>
    {:else}
      <span class="bl">{bar.label}</span>
      <div class="bar"><LedBar fraction={bar.f} segments={36} label={bar.label} /></div>
      <span class="bv">{bar.value}</span>
    {/if}
  </div>
</div>

<style>
  .mini {
    position: relative;
    width: calc(960 * var(--u));
    height: calc(640 * var(--u));
    margin: 0 auto;
    background-color: #1e1f22;
    background-image: var(--tex, none);
    background-size: 384px 384px;
  }
  .top {
    position: absolute;
    left: calc(26 * var(--u));
    right: calc(24 * var(--u));
    top: calc(12 * var(--u));
    height: calc(48 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(20 * var(--u));
  }
  .klif {
    font-family: var(--font-hero);
    font-weight: 800;
    font-size: calc(40 * var(--u));
    line-height: 1;
    letter-spacing: 0.01em;
  }
  .tsep {
    width: 2px;
    height: calc(38 * var(--u));
    background: rgba(237, 230, 214, 0.28);
    margin: 0 calc(6 * var(--u));
  }
  .ph {
    font-weight: 600;
    font-size: calc(34 * var(--u));
    letter-spacing: 0.05em;
    line-height: 1;
  }
  .ph.fault {
    color: var(--orange);
  }
  .rule {
    flex: 1 1 auto;
    height: 2px;
    background: rgba(237, 230, 214, 0.22);
    margin-left: calc(16 * var(--u));
  }
  .model {
    position: absolute;
    left: calc(24 * var(--u));
    right: calc(24 * var(--u));
    top: calc(58 * var(--u));
    height: calc(72 * var(--u));
    font-family: var(--font-hero);
    font-weight: 800;
    line-height: 1.05;
    letter-spacing: -0.005em;
    white-space: nowrap;
    overflow: hidden;
  }
  .hero {
    position: absolute;
    left: calc(24 * var(--u));
    right: calc(24 * var(--u));
    top: calc(132 * var(--u));
    height: calc(186 * var(--u));
    display: flex;
    align-items: center;
  }
  .drum {
    width: calc(642 * var(--u));
    height: 100%;
    font-size: calc(216 * var(--u));
    --drum-gap: calc(5 * var(--u));
    --drum-pad: calc(6 * var(--u));
    --drum-radius: calc(8 * var(--u));
    --dot-w: calc(56 * var(--u));
    --digit-dy: -0.02em;
  }
  .sdrum {
    width: calc((var(--n) * 2 * 150 + 96 + 30) * var(--u));
    height: 100%;
    font-size: calc(216 * var(--u));
    --drum-gap: calc(5 * var(--u));
    --drum-pad: calc(6 * var(--u));
    --drum-radius: calc(8 * var(--u));
    --slash-w: calc(96 * var(--u));
    --digit-dy: -0.02em;
  }
  .unit {
    margin-left: auto;
    margin-top: calc(72 * var(--u));
    font-family: var(--font-hero);
    font-weight: 800;
    font-size: calc(90 * var(--u));
    line-height: 1;
    letter-spacing: -0.01em;
  }
  .side {
    margin-left: auto;
    align-self: stretch;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    justify-content: flex-end;
    gap: calc(14 * var(--u));
    padding-bottom: calc(10 * var(--u));
    min-width: 0;
  }
  .side .unit {
    margin: 0;
  }
  .unit.pc {
    font-size: calc(96 * var(--u));
  }
  .side.tight {
    gap: calc(10 * var(--u));
    padding-bottom: calc(4 * var(--u));
  }
  .sl.cy {
    color: var(--cyan);
    letter-spacing: 0.06em;
  }
  .sv {
    font-family: var(--font-hero);
    font-weight: 800;
    font-size: calc(84 * var(--u));
    line-height: 1;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }
  .sv small {
    font-size: calc(54 * var(--u));
  }
  .sl {
    font-weight: 600;
    font-size: calc(38 * var(--u));
    color: var(--cream-2);
    line-height: 1;
    white-space: nowrap;
  }
  .msg {
    display: flex;
    flex-direction: column;
    gap: calc(14 * var(--u));
    min-width: 0;
    width: 100%;
  }
  .msg .big {
    font-family: var(--font-hero);
    font-weight: 800;
    font-size: calc(110 * var(--u));
    line-height: 1;
    white-space: nowrap;
  }
  .msg .line {
    font-size: calc(40 * var(--u));
    color: var(--cream-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .msg .line.bad {
    color: var(--orange);
  }
  .msg.fault .big {
    color: var(--orange);
  }
  .bigrow {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: calc(24 * var(--u));
  }
  .ago {
    font-weight: 600;
    font-size: calc(44 * var(--u));
    color: var(--cream);
    white-space: nowrap;
  }
  .msg.fault .line {
    color: var(--cream);
  }
  .lower {
    position: absolute;
    left: calc(24 * var(--u));
    right: calc(24 * var(--u));
    top: calc(326 * var(--u));
    height: calc(236 * var(--u));
    display: flex;
  }
  .vg {
    width: calc(296 * var(--u));
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .vgdial {
    width: calc(280 * var(--u));
    height: calc(171 * var(--u));
  }
  .read {
    margin-top: calc(14 * var(--u));
    font-family: var(--font-hero);
    font-weight: 700;
    font-stretch: 82%;
    font-size: calc(42 * var(--u));
    line-height: 1;
    white-space: nowrap;
    letter-spacing: -0.01em;
  }
  .cd {
    width: calc(224 * var(--u));
    height: calc(224 * var(--u));
    margin-left: calc(28 * var(--u));
    margin-top: calc(-2 * var(--u));
  }
  .plate {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: calc(10 * var(--u));
    border-radius: calc(14 * var(--u));
    background: #121315;
    box-shadow:
      inset 0 2px 6px rgba(0, 0, 0, 0.8),
      0 1px 0 rgba(255, 255, 255, 0.05);
    text-align: center;
  }
  .pk {
    font-weight: 600;
    font-size: calc(31 * var(--u));
    letter-spacing: 0.06em;
    color: var(--cream-2);
    line-height: 1;
    white-space: nowrap;
  }
  .pv {
    font-family: var(--font-hero);
    font-weight: 800;
    font-stretch: 85%;
    line-height: 1;
    white-space: nowrap;
  }
  .ps {
    font-weight: 600;
    font-size: calc(31 * var(--u));
    color: var(--cream-2);
    line-height: 1;
    white-space: nowrap;
  }
  .plate.hot .pk,
  .plate.hot .pv {
    color: var(--orange);
  }
  .dsep {
    width: 2px;
    height: calc(220 * var(--u));
    background: rgba(237, 230, 214, 0.2);
    margin: 0 calc(22 * var(--u)) 0 calc(24 * var(--u));
  }
  .right {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .selw {
    height: calc(156 * var(--u));
  }
  .hrule {
    height: 2px;
    background: rgba(237, 230, 214, 0.2);
    margin: calc(10 * var(--u)) 0 calc(14 * var(--u));
  }
  .tg {
    display: flex;
    justify-content: flex-end;
  }
  .ledrow {
    position: absolute;
    left: calc(24 * var(--u));
    right: calc(24 * var(--u));
    bottom: calc(20 * var(--u));
    height: calc(48 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(16 * var(--u));
    padding: calc(6 * var(--u)) calc(14 * var(--u));
    border-radius: calc(8 * var(--u));
    background: #0e0f10;
    box-shadow:
      inset 0 2px 5px rgba(0, 0, 0, 0.8),
      0 1px 0 rgba(255, 255, 255, 0.05);
  }
  .bl,
  .bv {
    font-weight: 600;
    font-size: calc(32 * var(--u));
    line-height: 1;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }
  .bv {
    min-width: calc(70 * var(--u));
    text-align: right;
  }
  .bv.dim {
    color: var(--cream-2);
  }
  .hotv,
  .bv.dim.hotv {
    color: var(--orange);
  }
  .ltxt {
    flex: 1 1 auto;
    min-width: 0;
    font-weight: 500;
    font-size: calc(32 * var(--u));
    line-height: 1;
    letter-spacing: 0.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ltxt.mono {
    font-family: var(--font-mono);
    font-size: calc(30 * var(--u));
    letter-spacing: 0;
  }
  .bar {
    flex: 1 1 auto;
    height: calc(30 * var(--u));
    --seg-gap: calc(5 * var(--u));
    --seg-radius: calc(3 * var(--u));
    --seg-glow: calc(8 * var(--u));
  }
</style>
