<script lang="ts">
  // Mini panel (960x640 at ~330 ppi, read from 1 m). Read-only, smallest text >= 30 px.
  // One composition for every state: wordmark + status on top, the tier/model line, one hero number,
  // one sub line, and the VRAM cliff across the lower half.
  import type { ViewModel } from '../../../lib/model/types';
  import { fmtInt, fmtSeconds, fmtTps } from '../../../lib/model/format';
  import Cliff from '../Cliff.svelte';
  import type { SceneMode } from '../paint';
  import Swell from '../Swell.svelte';
  import { availabilityText, fmtAgo, fmtSpan, releasedGiB, selectedSlot, sessionSlot, statusOf, viewState } from '../util';

  let { vm }: { vm: ViewModel } = $props();

  const s = $derived(vm.session);
  const view = $derived(viewState(vm));
  const st = $derived(statusOf(vm));
  const sel = $derived(selectedSlot(vm));
  const running = $derived(sessionSlot(vm));
  const mode = $derived<SceneMode>(
    view === 'idle' ? 'fit' : view === 'fault' ? 'fault' : view === 'loading' ? 'building' : 'live',
  );
  const fit = $derived(mode === 'fit');
  const baseGiB = $derived(vm.vram.baselineGiB ?? (s ? 0 : vm.vram.usedGiB));
  const layers = $derived(fit ? (sel?.expectedVram ?? []) : vm.vram.layers);
  const ctxFrac = $derived(
    s?.llm && s.llm.context.totalTokens > 0 ? Math.min(1, s.llm.context.usedTokens / s.llm.context.totalTokens) : 0,
  );
  const pf = $derived(
    s?.llm && s.llm.activity === 'prefill' && s.llm.prefill && s.llm.prefill.tokens > 0 ? s.llm.prefill : null,
  );
  const pfFrac = $derived(pf ? Math.min(1, pf.doneTokens / pf.tokens) : 0);
  const img = $derived(s?.image ?? null);
  const gen = $derived(img?.activity === 'generating');
  const jobFrac = $derived(img && gen && img.steps > 0 ? Math.min(1, img.step / img.steps) : 0);
  const lastJob = $derived(img && img.recent.length ? img.recent[img.recent.length - 1] : null);
  const why = $derived(sel ? availabilityText(sel.availability) : null);
  const activeStep = $derived(s?.loading?.steps.find((x) => x.state === 'active') ?? null);
  const last = $derived(vm.lastSession ?? null);
  const lastLabel = $derived(last ? (vm.slots.find((x) => x.id === last.slot)?.label ?? last.model.name) : '');
  const f = $derived(s?.fault ?? null);
  const R = 70;
  const CIRC = 2 * Math.PI * R;
</script>

{#snippet ring(frac: number | null, k: string, v: number | string, unit: string = '')}
  <div class="ring" role={frac === null ? undefined : 'meter'} aria-label={k} aria-valuemin="0" aria-valuemax="100" aria-valuenow={frac === null ? undefined : Math.round(frac * 100)}>
    <svg viewBox="0 0 170 170" aria-hidden="true">
      <circle cx="85" cy="85" r={R} class="trk" class:plain={frac === null} />
      {#if frac !== null}
        <circle
          cx="85"
          cy="85"
          r={R}
          class="arc"
          stroke-dasharray="{(frac * CIRC).toFixed(1)} {CIRC.toFixed(1)}"
          transform="rotate(-90 85 85)"
        />
      {/if}
    </svg>
    <div class="rt"><span class="rk">{k}</span><span class="rv" class:long={`${v}${unit}`.length > 3}>{v}{unit}</span></div>
  </div>
{/snippet}

<div class="mini">
  <div class="cliff-wrap">
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
      faultSinceS={f?.sinceS}
      releasedGiB={releasedGiB(vm)}
    />
  </div>

  <div class="word">KLIF</div>
  <div class="status {st.tone}">{#key vm.now}<i class="dot"></i>{/key}<span>{st.text}</span></div>

  {#if view === 'llm' && s?.llm}
    <div class="model">{s.model.name}</div>
    {#if pf}
      <div class="hero"><span class="big">{Math.round(pfFrac * 100)}</span><span class="unit">%</span><span class="unit2">prefill</span></div>
      <div class="sub"><span class="hv">{fmtTps(pf.tps)}</span> tok/s · <span class="hv">{fmtSeconds(pf.etaS)}</span> left</div>
      {@render ring(ctxFrac, 'ctx', Math.round(ctxFrac * 100), '%')}
    {:else}
      <div class="hero"><span class="big">{fmtTps(s.llm.decodeTps)}</span><span class="unit">tok/s</span></div>
      <div class="swell"><Swell history={s.llm.decodeHistory} variant="mini" /></div>
      {@render ring(ctxFrac, 'ctx', Math.round(ctxFrac * 100), '%')}
    {/if}
  {:else if view === 'image' && img}
    <div class="model">{s?.model.name}</div>
    {#if gen}
      <div class="hero"><span class="unit2 lead">step</span><span class="big">{img.step}/{img.steps}</span></div>
      <div class="sub">
        {#if img.sPerIt > 0}<span class="hv">{img.sPerIt.toFixed(2)}</span>{' s/it · '}{/if}{'last image '}<span class="hv"
          >{lastJob ? `${lastJob.seconds.toFixed(1)} s` : '—'}</span
        >
      </div>
      {@render ring(jobFrac, 'job', Math.round(jobFrac * 100), '%')}
    {:else}
      <div class="hero"><span class="big dim">idle</span></div>
      <div class="sub">{'last image '}<span class="hv">{lastJob ? `${lastJob.seconds.toFixed(1)} s` : '—'}</span></div>
      {@render ring(null, 'images', fmtInt(img.imagesThisSession))}
    {/if}
  {:else if view === 'loading' && s}
    <div class="model">{running?.label ?? s.model.name}</div>
    <div class="hero">
      <span class="big">{s.loading ? Math.round(s.loading.fraction * 100) : '—'}</span><span class="unit">{s.loading ? '%' : ''}</span>
    </div>
    <div class="sub">
      {activeStep?.label ?? (s.phase === 'starting' ? 'starting' : 'loading')}{#if activeStep?.detail}<span class="dt"
          >{` · ${activeStep.detail}`}</span
        >{/if}
    </div>
  {:else if view === 'fault' && s}
    <div class="model">{running?.label ?? s.model.name}</div>
    <div class="hero fault">
      <svg class="warn" viewBox="0 0 48 44" aria-hidden="true"
        ><path d="M24 4 45 40H3Z" /><path d="M24 17v11" /><circle cx="24" cy="33.5" r="2.2" /></svg
      ><span class="big">FAULT</span>
    </div>
    <div class="sub fault-t">{f?.title ?? 'The server stopped unexpectedly.'}</div>
    <div class="fcode">
      {#if f?.exitCodeHex || f?.exitCode !== undefined}{'exit '}<span class="hv">{f?.exitCodeHex ?? f?.exitCode}</span
        >{' · '}{/if}{f ? fmtAgo(f.sinceS) : ''}
    </div>
  {:else}
    <div class="model">{sel?.label ?? 'KLIF'}</div>
    <div class="hero idle"><span class="big sm">{sel?.model.name ?? ''}</span></div>
    <div class="sub" class:na={!!why}><i class="av"></i>{why ?? 'ready to launch'}</div>
    {#if last}
      <div class="last">
        last · {lastLabel} · {fmtSpan(last.uptimeS)}{#if last.requests !== undefined}{` · ${fmtInt(last.requests)} req`}{/if}{#if last.images !== undefined}{` · ${fmtInt(last.images)} img`}{/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    z-index: 1;
    font-family: var(--f-ui);
    color: var(--foam);
  }
  .cliff-wrap {
    position: absolute;
    inset: 0;
  }
  .word {
    position: absolute;
    left: calc(var(--m) * 26);
    top: calc(var(--m) * 16);
    font-family: var(--f-disp);
    font-stretch: 125%;
    font-weight: 500;
    font-size: max(30px, calc(var(--m) * 30));
    letter-spacing: 0.16em;
    color: var(--sky);
  }
  .status {
    position: absolute;
    right: calc(var(--m) * 24);
    top: calc(var(--m) * 16);
    display: flex;
    align-items: center;
    gap: calc(var(--m) * 16);
    font-family: var(--f-disp);
    font-stretch: 112%;
    font-weight: 600;
    font-size: max(30px, calc(var(--m) * 30));
    letter-spacing: 0.07em;
    color: var(--sky);
  }
  .dot {
    width: calc(var(--m) * 24);
    height: calc(var(--m) * 24);
    border-radius: 50%;
    background: var(--sky);
    animation: beat 700ms ease-out 1;
  }
  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
  @keyframes beat {
    0% {
      opacity: 0.45;
    }
    100% {
      opacity: 1;
    }
  }
  .idle .dot,
  .stop .dot {
    background: transparent;
    box-shadow: inset 0 0 0 3px currentColor;
  }
  .idle,
  .stop {
    color: var(--muted);
  }
  .busy {
    color: var(--amber);
  }
  .busy .dot {
    background: var(--amber);
  }
  .status.fault {
    color: var(--amber);
  }
  .status.fault .dot {
    background: var(--amber);
  }
  .model {
    position: absolute;
    left: calc(var(--m) * 24);
    top: calc(var(--m) * 70);
    max-width: calc(var(--m) * 900);
    font-family: var(--f-disp);
    font-stretch: 122%;
    font-weight: 650;
    font-size: max(30px, calc(var(--m) * 64));
    line-height: 1.1;
    letter-spacing: 0.005em;
    color: var(--foam);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hero {
    position: absolute;
    left: calc(var(--m) * 20);
    top: calc(var(--m) * 150);
    display: flex;
    align-items: baseline;
    gap: calc(var(--m) * 22);
    white-space: nowrap;
  }
  .big {
    font-family: var(--f-disp);
    font-stretch: 125%;
    font-weight: 800;
    font-size: calc(var(--m) * 150);
    line-height: 1;
    letter-spacing: -0.012em;
    color: #f2f9fc;
  }
  .big.sm {
    font-size: calc(var(--m) * 72);
    font-stretch: 112%;
  }
  .big.dim {
    color: #5d6e77;
  }
  .hero.fault {
    align-items: center;
    gap: calc(var(--m) * 26);
    top: calc(var(--m) * 148);
  }
  .hero.fault .big {
    font-size: calc(var(--m) * 128);
    color: var(--amber);
  }
  .warn {
    width: calc(var(--m) * 108);
    height: calc(var(--m) * 99);
    flex: none;
  }
  .warn path {
    fill: none;
    stroke: var(--amber);
    stroke-width: 3.6;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .warn circle {
    fill: var(--amber);
  }
  .unit {
    font-family: var(--f-disp);
    font-stretch: 100%;
    font-weight: 700;
    font-size: calc(var(--m) * 82);
    color: var(--sky);
  }
  .unit2 {
    font-family: var(--f-disp);
    font-stretch: 100%;
    font-weight: 600;
    font-size: calc(var(--m) * 52);
    color: var(--sky);
  }
  .unit2.lead {
    color: #c3d3db;
  }
  /* stops short of the lip label (the cliff's "N GiB free" sits right of the lip at the same height) */
  .sub {
    position: absolute;
    left: calc(var(--m) * 26);
    top: calc(var(--m) * 300);
    max-width: calc(var(--m) * 600);
    font-size: max(30px, calc(var(--m) * 32));
    color: #c3d3db;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub .hv {
    color: var(--foam);
    font-family: var(--f-data);
    font-weight: 500;
  }
  .sub .dt {
    font-family: var(--f-data);
    color: var(--foam);
  }
  .sub .av {
    display: inline-block;
    width: calc(var(--m) * 18);
    height: calc(var(--m) * 18);
    margin-right: calc(var(--m) * 14);
    border-radius: 50%;
    background: var(--sky);
    vertical-align: 0.05em;
  }
  .sub.na {
    color: var(--amber);
  }
  .sub.na .av {
    background: var(--amber);
  }
  .fault-t {
    top: calc(var(--m) * 282);
    color: var(--foam);
    max-width: calc(var(--m) * 910);
  }
  .fcode {
    position: absolute;
    left: calc(var(--m) * 26);
    top: calc(var(--m) * 372);
    font-size: max(30px, calc(var(--m) * 32));
    color: #c3d3db;
    white-space: nowrap;
    paint-order: stroke;
    -webkit-text-stroke: 0;
    text-shadow: 0 0 6px rgba(12, 16, 19, 0.9);
  }
  .fcode .hv {
    font-family: var(--f-data);
    font-weight: 500;
    color: var(--amber);
  }
  .last {
    position: absolute;
    left: calc(var(--m) * 26);
    bottom: calc(var(--m) * 14);
    max-width: calc(var(--m) * 600);
    font-size: max(30px, calc(var(--m) * 30));
    color: #c3d3db;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-shadow:
      0 0 4px rgba(12, 16, 19, 0.95),
      0 0 10px rgba(12, 16, 19, 0.8);
  }
  /* Kept above the cap height of "tok/s" so a low trend never runs through the unit. */
  .swell {
    position: absolute;
    left: calc(var(--m) * 430);
    top: calc(var(--m) * 134);
    width: calc(var(--m) * 330);
    height: calc(var(--m) * 68);
  }
  .ring {
    position: absolute;
    left: calc(var(--m) * 768);
    top: calc(var(--m) * 120);
    width: calc(var(--m) * 170);
    height: calc(var(--m) * 170);
  }
  .ring svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .trk {
    fill: rgba(15, 19, 22, 0.55);
    stroke: #2a353c;
    stroke-width: 11;
  }
  .trk.plain {
    stroke-width: 3;
  }
  .arc {
    fill: none;
    stroke: var(--sky);
    stroke-width: 11;
    stroke-linecap: butt;
  }
  .rt {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    line-height: 1;
    gap: calc(var(--m) * 4);
  }
  .rk {
    font-family: var(--f-ui);
    font-weight: 500;
    font-size: max(30px, calc(var(--m) * 30));
    color: var(--muted);
    letter-spacing: 0.04em;
  }
  .rv {
    font-family: var(--f-disp);
    font-stretch: 112%;
    font-weight: 700;
    font-size: max(30px, calc(var(--m) * 46));
    color: var(--foam);
  }
  /* "100%" and longer counts stay inside the ring */
  .rv.long {
    font-stretch: 100%;
    font-size: max(30px, calc(var(--m) * 38));
  }
</style>
