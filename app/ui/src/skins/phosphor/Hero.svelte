<script lang="ts">
  // The status block's hero: one fixed box whose contents follow the phase. Left: the label and the big
  // figure; right: what the figure moves with.
  //   decode   : decode tok/s (current or last request) beside the 1 Hz scope trace with its afterglow
  //   prefill  : prompt prefill % in amber, tok/s and ETA over a progress bar (never a stale 0.0 tok/s)
  //   image    : sampling step n / m, s/it, elapsed and size over the step bar
  //   loading  : startup %, the active step and VRAM so far over the overall bar
  //   wait     : nothing to measure (idle, stopping, no data yet): the kind's own hero dimmed ("—" and a
  //              flat trace) with one word over it, amber when the selected tier cannot launch or it stops
  // GPU dormant: the figure is the last one (faded), the trace is afterglow only, the tag says asleep / waking.
  import type { GenericLive, GpuMemory, ImageLive, LlmLive, LoadProgress, SystemKind } from '../../lib/model/types';
  import { KIND_LABEL } from '../../lib/model/systems';
  import { fmtClock, fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../lib/model/format';
  import Scope from './Scope.svelte';
  import { clamp, fmtSPerIt, type GpuSleep } from './geom';

  let {
    mode,
    kind,
    llm = null,
    img = null,
    generic = null,
    loading = null,
    vram,
    gpu = null,
    word = '',
    amber = false,
  }: {
    mode: 'llm' | 'image' | 'generic' | 'loading' | 'wait';
    kind: SystemKind;
    llm?: LlmLive | null;
    img?: ImageLive | null;
    generic?: GenericLive | null;
    loading?: LoadProgress | null;
    vram: GpuMemory;
    gpu?: GpuSleep | null;
    /** wait: the word over the dimmed hero. */
    word?: string;
    amber?: boolean;
  } = $props();

  const ACT = { idle: 'idle', prefill: 'prefill', decode: 'decoding' } as const;

  /** Prompt processing in progress: it becomes the hero instead of a stale decode speed. */
  const pf = $derived(llm && llm.activity === 'prefill' && llm.prefill && llm.prefill.tokens > 0 ? llm.prefill : null);
  const pfFrac = $derived(pf ? clamp(pf.doneTokens / pf.tokens, 0, 1) : 0);
  /** A prefill that has not started because the GPU is still being restored. */
  const pfWaits = $derived(!!gpu && !!pf && pf.doneTokens === 0);

  const gen = $derived(!!img && img.activity === 'generating' && img.steps > 0);
  const imgFrac = $derived(gen && img ? clamp(img.step / img.steps, 0, 1) : 0);

  const working = $derived((generic?.requestsInFlight ?? 0) > 0);

  const loadFrac = $derived(clamp(loading?.fraction ?? 0, 0, 1));
  const activeStep = $derived(loading?.steps.find((x) => x.state === 'active') ?? null);
</script>

{#if mode === 'llm' && llm && pf}
  <section class="panel grat hero pf" aria-label="Prompt prefill">
    <div class="hl">
      <div class="hhead"><span class="lbl amb">Prompt prefill</span></div>
      <div class="fig amb" class:faded={pfWaits}><span class="num">{Math.floor(pfFrac * 100)}</span><span class="unit pc">%</span></div>
    </div>
    <div class="hr bars">
      <div class="stat amb">
        {#if pfWaits && gpu}
          <span>waiting for the GPU to wake</span><span class="mut sm">{fmtGiB(gpu.pagedOutGiB)} GiB still in system RAM</span>
        {:else}
          <span><b>{fmtInt(pf.tps)}</b> tok/s</span><span>{pf.etaS > 0 ? `eta ${fmtSeconds(pf.etaS)}` : 'finishing'}</span>
        {/if}
      </div>
      <span class="track amb" role="img" aria-label="Prefill {Math.floor(pfFrac * 100)}%"><span class="fill" style="transform:scaleX({pfFrac.toFixed(4)})"></span></span>
      <div class="sub">{fmtInt(pf.doneTokens)} / {fmtInt(pf.tokens)} tok{pf.cachedTokens ? ` · ${fmtInt(pf.cachedTokens)} from the prompt cache` : ''}</div>
    </div>
  </section>
{:else if mode === 'llm' && llm}
  <section class="panel grat hero" aria-label="Decode speed">
    <div class="hl">
      <div class="hhead">
        <span class="lbl">{gpu ? 'Last decode speed' : 'Decode speed'}</span>
        {#if gpu}<span class="act gpuw">{gpu.state === 'waking' ? 'gpu waking' : 'gpu asleep'}</span>{:else}<span class="act" data-act={llm.activity}>{ACT[llm.activity]}</span>{/if}
      </div>
      <div class="fig" class:faded={!!gpu || llm.activity !== 'decode'} title={llm.activity === 'decode' ? 'Current request' : 'Last request (not decoding now)'}>
        <span class="num">{fmtTps(llm.decodeTps)}</span><span class="unit">tok/s</span>
      </div>
    </div>
    <div class="hr sc">
      <span class="cap">5-minute history</span>
      <div class="scope"><Scope history={llm.decodeHistory} dim={!!gpu} /></div>
    </div>
  </section>
{:else if mode === 'image' && img}
  <section class="panel grat hero" aria-label="Image generation">
    <div class="hl">
      <div class="hhead">
        <span class="lbl">{gen && img.edit ? 'Editing image' : gen ? 'Generating image' : 'Image server'}</span>
        <span class="act" class:on={gen}>{gen ? 'sampling' : 'idle'}</span>
      </div>
      <div class="fig" class:faded={!gen}>
        <span class="unit pre">step</span><span class="num">{gen ? img.step : '—'}</span><span class="unit">/ {img.steps}</span>
      </div>
    </div>
    <div class="hr bars">
      <div class="stat">
        {#if gen}
          <span><b>{fmtSPerIt(img.sPerIt)}</b> s/it</span><span>elapsed <b>{fmtSeconds(img.elapsedS)}</b></span><span class="mut sm end">{img.width}x{img.height}{img.edit ? ' · edit' : ''}</span>
        {:else}
          <span class="mut">waiting for the next job</span>
        {/if}
      </div>
      <div class="trow">
        <span class="track" role="img" aria-label="Sampling step {img.step} of {img.steps}">
          <span class="fill" style="transform:scaleX({imgFrac.toFixed(4)})"></span>
          {#if gen}<span class="hw" style="transform:translateX({(imgFrac * 100).toFixed(2)}%)"><span class="head"></span></span>{/if}
        </span>
        <span class="pct" class:dim={!gen}>{gen ? `${Math.round(imgFrac * 100)}%` : '—'}</span>
      </div>
    </div>
  </section>
{:else if mode === 'generic' && generic}
  <section class="panel grat hero" aria-label={KIND_LABEL[kind]}>
    <div class="hl">
      <div class="hhead">
        <span class="lbl">{KIND_LABEL[kind]}</span>
        <span class="act" class:on={working}>{working ? 'working' : 'idle'}</span>
      </div>
      <div class="fig" class:faded={!working}>
        <span class="num">{generic.requestsTotal !== undefined ? fmtInt(generic.requestsTotal) : '—'}</span><span class="unit">requests</span>
      </div>
    </div>
    <div class="hr bars">
      <div class="stat">
        {#if working}
          <span><b>{generic.requestsInFlight}</b> in flight</span>
        {:else}
          <span class="mut">waiting for the next request</span>
        {/if}
        {#if generic.lastActivityS !== undefined}<span class="mut sm end">last activity {fmtSeconds(generic.lastActivityS)} ago</span>{/if}
      </div>
      <div class="trow">
        <span class="track" role="img" aria-label={working ? 'A request is running' : 'Idle'}>
          <span class="fill" style="transform:scaleX({working ? 1 : 0})"></span>
        </span>
        <span class="pct" class:dim={!working}>{working ? 'busy' : '—'}</span>
      </div>
    </div>
  </section>
{:else if mode === 'loading'}
  <section class="panel grat hero" aria-label="Startup">
    <div class="hl">
      <div class="hhead"><span class="lbl amb">Startup</span><span class="act amb">{activeStep ? 'loading' : 'starting'}</span></div>
      <div class="fig amb"><span class="num">{Math.floor(loadFrac * 100)}</span><span class="unit pc">%</span></div>
    </div>
    <div class="hr bars">
      <div class="stat">
        <span class="cur">{activeStep?.label ?? 'starting the server'}{#if activeStep?.detail}<span class="mut">{` · ${activeStep.detail}`}</span>{/if}</span>
        <span class="mut sm end">VRAM <b>{fmtGiB(vram.usedGiB)}</b> / {fmtGiB(vram.totalGiB)} GiB</span>
      </div>
      <div class="trow">
        <span class="track amb" role="img" aria-label="Startup {Math.floor(loadFrac * 100)}%"><span class="fill" style="transform:scaleX({loadFrac.toFixed(4)})"></span></span>
        <span class="pct amb">{fmtClock(loading?.elapsedS ?? 0)}</span>
      </div>
    </div>
  </section>
{:else}
  <!-- nothing to measure: the kind's own hero, dimmed, with a word over it -->
  <section class="panel grat hero wait" aria-label={word}>
    <div class="hl">
      <div class="hhead"><span class="lbl dimlbl">{kind === 'image' ? 'Image generation' : kind === 'llm' ? 'Decode speed' : KIND_LABEL[kind]}</span></div>
      <div class="fig dim">
        {#if kind === 'image'}<span class="unit pre">step</span><span class="num">—</span>{:else if kind === 'llm'}<span class="num">—</span><span class="unit">tok/s</span>{:else}<span class="num">—</span><span class="unit">requests</span>{/if}
      </div>
    </div>
    <div class="hr wr">
      <span class="word" class:amb={amber}>{word}</span>
      {#if kind === 'image' || (kind !== 'llm')}
        <div class="trow"><span class="track"></span><span class="pct dim">—</span></div>
      {:else}
        <div class="flat" aria-hidden="true"></div>
      {/if}
    </div>
  </section>
{/if}

<style>
  .hero {
    height: 100%;
    display: grid;
    grid-template-columns: calc(270px * var(--k)) minmax(0, 1fr);
    overflow: hidden;
  }
  .hl {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(11px * var(--k)) calc(16px * var(--k)) calc(8px * var(--k));
    border-right: 1px solid var(--ph-grat);
    min-width: 0;
  }
  .hhead {
    display: flex;
    align-items: baseline;
    gap: calc(12px * var(--k));
    min-width: 0;
  }
  .lbl.amb {
    color: var(--ph-amber);
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.45);
  }
  .lbl.dimlbl {
    color: var(--ph-muted);
    text-shadow: none;
  }
  .act {
    font-size: var(--ph-fs-xs);
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ph-muted);
    white-space: nowrap;
  }
  .act[data-act='decode'],
  .act.on {
    color: var(--ph-cyan);
  }
  .act[data-act='prefill'],
  .act.amb,
  .act.gpuw {
    color: var(--ph-amber);
  }
  .act.gpuw {
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.4);
  }

  /* the big figure */
  .fig {
    align-self: center;
    display: flex;
    align-items: baseline;
    gap: calc(10px * var(--k));
    font-family: var(--ph-display);
    color: var(--ph-cyan);
    line-height: 1;
    white-space: nowrap;
    min-width: 0;
  }
  .num {
    font-size: calc(54px * var(--k));
    font-weight: 330;
    font-stretch: 108%;
    letter-spacing: 0.02em;
    text-shadow:
      0 0 10px rgba(127, 227, 255, 0.55),
      0 0 28px rgba(90, 182, 235, 0.3);
  }
  .unit {
    font-size: calc(20px * var(--k));
    font-weight: 350;
    font-stretch: 100%;
    letter-spacing: 0.04em;
    color: var(--ph-brand);
    text-shadow: 0 0 6px rgba(127, 227, 255, 0.3);
  }
  .unit.pc {
    margin-left: calc(-6px * var(--k));
    font-size: calc(26px * var(--k));
  }
  .unit.pre {
    font-size: calc(18px * var(--k));
  }
  .fig.amb,
  .fig.amb .unit {
    color: var(--ph-amber);
  }
  .fig.amb .num {
    text-shadow:
      0 0 10px rgba(232, 176, 74, 0.5),
      0 0 28px rgba(232, 176, 74, 0.22);
  }
  /* the last figure, not a live one (between requests, GPU asleep) */
  .fig.faded .num,
  .fig.faded .unit {
    color: var(--ph-muted);
    text-shadow: 0 0 8px rgba(79, 152, 180, 0.25);
  }
  .fig.dim .num,
  .fig.dim .unit {
    color: #2f6377;
    text-shadow: none;
  }

  .hr {
    position: relative;
    min-width: 0;
    min-height: 0;
  }
  /* decode: the scope fills the right side, its caption on top */
  .hr.sc {
    padding: calc(4px * var(--k)) calc(10px * var(--k)) calc(4px * var(--k)) calc(12px * var(--k));
  }
  .cap {
    position: absolute;
    top: calc(8px * var(--k));
    right: calc(14px * var(--k));
    font-size: var(--ph-fs-xs);
    color: var(--ph-muted);
    z-index: 1;
  }
  .scope {
    position: absolute;
    inset: calc(6px * var(--k)) calc(10px * var(--k)) calc(4px * var(--k)) calc(12px * var(--k));
  }

  /* bar heroes: a stats line over a progress bar */
  .hr.bars {
    display: grid;
    grid-template-rows: auto auto auto;
    align-content: center;
    row-gap: calc(10px * var(--k));
    padding: 0 calc(18px * var(--k)) 0 calc(20px * var(--k));
  }
  .stat {
    display: flex;
    align-items: baseline;
    gap: calc(26px * var(--k));
    font-size: var(--ph-fs-l);
    color: var(--ph-ink);
    text-shadow: var(--ph-glow-soft);
    white-space: nowrap;
    overflow: hidden;
    min-width: 0;
  }
  .stat.amb {
    color: #f3d9a4;
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.35);
  }
  .stat b {
    font-weight: 500;
    color: var(--ph-hot);
  }
  .stat.amb b {
    color: #ffe9bf;
  }
  .stat .sm {
    font-size: var(--ph-fs-m);
    text-shadow: none;
  }
  .stat .end {
    margin-left: auto;
  }
  .stat .cur {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .sub {
    font-size: var(--ph-fs-m);
    color: var(--ph-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .trow {
    display: flex;
    align-items: center;
    gap: calc(14px * var(--k));
  }
  .track {
    position: relative;
    display: block;
    flex: 1;
    height: calc(12px * var(--k));
    border: 1px solid #2a7f93;
    border-radius: 3px;
    overflow: hidden;
    background: rgba(3, 9, 12, 0.75);
  }
  .track.amb {
    border-color: rgba(232, 176, 74, 0.5);
  }
  .fill {
    position: absolute;
    inset: 2px;
    transform-origin: left center;
    border-radius: 1px;
    background: linear-gradient(90deg, rgba(90, 182, 235, 0.75), var(--ph-cyan) 70%, #c8f5ff);
    box-shadow: 0 0 12px rgba(127, 227, 255, 0.55);
    transition: transform 0.45s ease-out;
  }
  .track.amb .fill {
    background: linear-gradient(90deg, rgba(232, 176, 74, 0.5), var(--ph-amber));
    box-shadow: 0 0 10px rgba(232, 176, 74, 0.5);
  }
  .hw {
    position: absolute;
    inset: 0;
    transition: transform 0.45s ease-out;
  }
  .head {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -3px;
    width: 3px;
    background: #ffe2a8;
    box-shadow:
      0 0 8px var(--ph-amber),
      0 0 14px rgba(232, 176, 74, 0.6);
  }
  .pct {
    flex: none;
    min-width: 3.6em;
    text-align: right;
    font-size: var(--ph-fs-l);
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
  }
  .pct.amb {
    color: var(--ph-amber);
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.4);
  }
  .pct.dim {
    color: #2f6377;
    text-shadow: none;
  }

  /* wait: a flat trace (or an empty bar) with the word over it */
  .hr.wr {
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    padding: calc(10px * var(--k)) calc(18px * var(--k)) calc(16px * var(--k)) calc(20px * var(--k));
  }
  .word {
    align-self: center;
    justify-self: center;
    font-size: var(--ph-fs-s);
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.22em;
    text-transform: uppercase;
    color: var(--ph-muted);
    white-space: nowrap;
  }
  .word.amb {
    color: var(--ph-amber);
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.4);
  }
  .flat {
    height: 0;
    border-top: 1.5px solid rgba(127, 227, 255, 0.3);
    box-shadow: 0 0 8px rgba(127, 227, 255, 0.2);
  }
</style>
