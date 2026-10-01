<script lang="ts">
  // Full window. The state picks the middle of the window; the header, the VRAM cliff, the system
  // strip and the console line are shared.
  //   idle    : 2x2 tier cards, VRAM cliff in fit preview, Launch / Tune, last session
  //   loading : locked tabs, dormant decode area, radar + load steps, VRAM staircase, Cancel
  //   llm     : decode scope (or prefill progress), context radar, cliff, request timeline (approved LIVE)
  //   image   : generating image (step / steps), last image, cliff, recent jobs
  //   fault   : fault panel (title, exit code, trace ending in a red drop, log lines, ways out), cliff
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../lib/model/format';
  import Header from './Header.svelte';
  import Tabs from './Tabs.svelte';
  import Controls from './Controls.svelte';
  import Scope from './Scope.svelte';
  import Radar from './Radar.svelte';
  import VramCliff from './VramCliff.svelte';
  import FitCliff from './FitCliff.svelte';
  import Timeline from './Timeline.svelte';
  import Jobs from './Jobs.svelte';
  import TierCards from './TierCards.svelte';
  import Launcher from './Launcher.svelte';
  import LoadPanel from './LoadPanel.svelte';
  import ImageHero from './ImageHero.svelte';
  import FaultPanel from './FaultPanel.svelte';
  import FullState from './FullState.svelte';
  import { loadSpanS, recipeLine } from './geom';

  let { vm, actions, k }: { vm: ViewModel; actions: Actions; k: number } = $props();

  const s = $derived(vm.session);
  const mode = $derived.by(() => {
    if (!s) return 'idle';
    switch (s.phase) {
      case 'fault':
        return 'fault';
      case 'starting':
      case 'loading':
        return 'loading';
      case 'stopping':
        return 'stopping';
      default:
        return s.llm ? 'llm' : s.image ? 'image' : 'wait';
    }
  });
  const llm = $derived(mode === 'llm' ? s?.llm ?? null : null);
  const img = $derived(mode === 'image' ? s?.image ?? null : null);
  const slot = $derived(vm.slots.find((x) => x.id === (s?.slot ?? vm.selected)));
  const model = $derived(s?.model ?? slot?.model);
  const recipe = $derived(model ? recipeLine(model) : []);
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const imageSlot = $derived(slot?.kind === 'image');

  const ACT = { idle: 'idle', prefill: 'prefill', decode: 'decoding' } as const;

  /** Prompt processing in progress: it becomes the hero instead of a stale decode speed. */
  const pf = $derived(llm && llm.activity === 'prefill' && llm.prefill && llm.prefill.tokens > 0 ? llm.prefill : null);
  const pfFrac = $derived(pf ? Math.max(0, Math.min(1, pf.doneTokens / pf.tokens)) : 0);

  const prefillText = $derived.by(() => {
    const p = llm?.prefill;
    if (!p) return '—';
    if (llm?.activity === 'prefill' && p.etaS > 0) {
      return `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s · eta ${fmtSeconds(p.etaS)}`;
    }
    return `${fmtInt(p.tokens)} tok · ${fmtInt(p.tps)} tok/s · done in ${fmtSeconds(p.elapsedS)}`;
  });

  /** The current request has not produced a token yet, so there is no acceptance to show. */
  const specPending = $derived(!!llm && llm.activity === 'prefill' && llm.generatedTokens === 0);
  /** Character count of the speculative line (monospace), for its fit-to-width font size. */
  const specChars = $derived.by(() => {
    const sp = llm?.spec;
    if (!sp) return 3;
    return (specPending ? '— accepted · ' : `${Math.round(sp.acceptancePct)}% accepted · `).length + sp.mode.length;
  });

  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0);
  /** Sweep speed: 2 degrees per second per decode tok/s (1/180 turn); static while idle. */
  const sweep = $derived.by(() => {
    if (!llm || llm.activity === 'idle') return 0;
    const tps = llm.decodeTps > 0 ? llm.decodeTps : (llm.prefill?.tps ?? 0) / 16;
    return tps / 180;
  });

  const span = $derived(mode === 'loading' ? loadSpanS(s?.loading?.elapsedS ?? s?.uptimeS ?? 0) : 300);
  const markAgo = $derived(mode === 'fault' ? (s?.fault?.sinceS ?? null) : null);
</script>

<div class="full m-{mode}">
  <Header session={s} host={vm.host} {actions} {k} />

  {#if mode === 'idle'}
    <TierCards slots={vm.slots} selected={vm.selected} {actions} />

    <section class="panel grat vram fitp" aria-label="VRAM cliff, fit preview">
      <div class="vhead">
        <span class="lbl">VRAM cliff</span>
        <span class="lbl sub">Fit preview</span>
        <span class="val dev">{vm.vram.device} · {fmtGiB(vm.vram.usedGiB)} GiB in use</span>
      </div>
      <div class="cliffbox"><FitCliff vram={vm.vram} {slot} {k} /></div>
    </section>

    <Launcher {slot} slots={vm.slots} last={vm.lastSession} {actions} />
  {:else}
    <Tabs
      slots={vm.slots}
      selected={vm.selected}
      running={s?.slot ?? null}
      {actions}
      locked={mode === 'loading' || mode === 'stopping'}
      tone={mode === 'fault' ? 'danger' : 'cyan'}
    />
    {#if mode !== 'fault'}
      <div class="recipe" title={recipe.join(' · ')}>
        {#each recipe as part, i}
          {#if i > 0}<span class="sep" aria-hidden="true">·</span>{/if}<span class="part">{part}</span>
        {/each}
      </div>
    {/if}

    {#if s && llm}
      <section class="panel grat decode" aria-label={pf ? 'Prompt prefill' : 'Decode speed'}>
        <div class="dhead">
          <span class="lbl">{pf ? 'Prompt prefill' : 'Decode speed'}</span>
          {#if !pf}<span class="act" data-act={llm.activity}>{ACT[llm.activity]}</span>{/if}
          <span class="note">{pf ? 'decode · 5-minute history' : '5-minute history'}</span>
        </div>
        {#if pf}
          <div class="readout pf" title="Prompt processing of the current request">
            <span class="num">{Math.floor(pfFrac * 100)}<span class="pc">%</span></span>
            <div class="pfx">
              <span class="pfa">{fmtInt(pf.tps)} tok/s · {pf.etaS > 0 ? `eta ${fmtSeconds(pf.etaS)}` : 'finishing'}</span>
              <span class="pfbar" role="img" aria-label="Prefill {Math.floor(pfFrac * 100)}%"><span class="pff" style="transform:scaleX({pfFrac.toFixed(4)})"></span></span>
              <span class="pfb">{fmtInt(pf.doneTokens)} / {fmtInt(pf.tokens)} tok{pf.cachedTokens ? ` · ${fmtInt(pf.cachedTokens)} cached` : ''}</span>
            </div>
          </div>
        {:else}
          <div class="readout" class:dim={llm.activity !== 'decode'} title={llm.activity === 'decode' ? 'Current request' : 'Last request (not decoding now)'}>
            <span class="num">{fmtTps(llm.decodeTps)}</span><span class="unit">tok/s</span>
          </div>
        {/if}
        <div class="scope"><Scope history={llm.decodeHistory} /></div>
        <div class="dfoot">
          <span class="seg"><span class="lbl">Prefill</span><span class="val">{prefillText}</span></span>
          <span class="vrule" aria-hidden="true"></span>
          <span class="seg"><span class="lbl">Decode</span><span class="val">{fmtInt(llm.generatedTokens)} tok generated</span></span>
        </div>
      </section>

      <section class="panel ctx">
        <Radar fraction={ctxFrac} turnsPerSec={sweep} size={Math.round(118 * k)} />
        <div class="cell">
          <div class="lbl">Context fill</div>
          <div class="big val">
            {fmtInt(llm.context.usedTokens)} / {fmtInt(llm.context.totalTokens)} tokens · {Math.round(ctxFrac * 100)}%
          </div>
        </div>
        <span class="vrule" aria-hidden="true"></span>
        <div class="cell spec">
          <div class="lbl">Speculative decoding</div>
          <!-- shrinks (never clips) when the context figure next to it is long -->
          <div class="big val" style="--n:{specChars}">
            {#if !llm.spec}<span class="mut">off</span>{:else if specPending}<span class="mut">— accepted</span> · {llm.spec.mode}{:else}{Math.round(llm.spec.acceptancePct)}% accepted · {llm.spec.mode}{/if}
          </div>
        </div>
      </section>
    {:else if s && img}
      <ImageHero {img} />
    {:else if s && mode === 'loading'}
      <section class="panel grat dorm" aria-label={imageSlot ? 'Image generation (not started)' : 'Decode speed (not started)'}>
        <div class="dhead">
          <span class="lbl">{imageSlot ? 'Image generation' : 'Decode speed'}</span>
          <span class="act">not serving yet</span>
          <span class="note">starts when the model is ready</span>
        </div>
        <div class="readout dim"><span class="num">—</span><span class="unit">{imageSlot ? 's/it' : 'tok/s'}</span></div>
        <div class="flat" aria-hidden="true"></div>
      </section>
      <LoadPanel loading={s.loading} {slot} vram={vm.vram} {k} />
    {:else if s && mode === 'fault'}
      <FaultPanel session={s} {slot} vram={vm.vram} {actions} {k} />
    {:else}
      <FullState {vm} />
    {/if}

    <section class="panel grat vram" aria-label="VRAM cliff">
      <div class="vhead">
        <span class="lbl">VRAM cliff</span>
        <span class="val">{vm.vram.device} · {fmtGiB(vm.vram.usedGiB)} / {fmtGiB(vm.vram.totalGiB)} GiB</span>
      </div>
      <div class="cliffbox"><VramCliff vram={vm.vram} {k} spanS={span} markAgoS={markAgo} /></div>
    </section>
  {/if}

  <section class="panel sys">
    <span class="seg"
      ><span class="lbl">RAM</span><span class="val"
        >{vm.system.ramUsedGiB.toFixed(1)} / {vm.system.ramTotalGiB.toFixed(1)} GiB{#if vm.system.ramType}<span class="mut">{` · ${vm.system.ramType}`}</span>{/if}</span
      ></span
    >
    <span class="vrule" aria-hidden="true"></span>
    <span class="seg"><span class="lbl">CPU</span><span class="val">{vm.system.cpuName} · {Math.round(vm.system.cpuPct)}%</span></span>
  </section>

  {#if s && llm}
    <section class="panel tlp" aria-label="Request timeline">
      <div class="thead">
        <span class="lbl">Request timeline</span>
        <span class="legend"><i class="sw a"></i>prefill<i class="sw c"></i>decode</span>
      </div>
      <div class="tlbox"><Timeline requests={llm.requests} /></div>
    </section>
  {:else if s && (img || (mode === 'fault' && s.image))}
    {@const im = img ?? s.image}
    {#if im}
      <section class="panel tlp jobsp" aria-label="Recent jobs">
        <div class="thead">
          <span class="lbl">Recent jobs</span>
          <span class="legend"><i class="sw c"></i>plain<i class="sw a"></i>edit<span class="mut wnote">width = seconds</span></span>
        </div>
        <div class="tlbox">
          <Jobs
            jobs={im.recent}
            current={mode === 'image' && im.activity === 'generating' ? { elapsedS: im.elapsedS, edit: im.edit } : null}
            fault={mode === 'fault'}
          />
        </div>
      </section>
    {/if}
  {:else if s && mode === 'fault' && s.llm && s.llm.requests.length}
    <section class="panel tlp" aria-label="Request timeline">
      <div class="thead">
        <span class="lbl">Request timeline</span>
        <span class="legend"><i class="sw a"></i>prefill<i class="sw c"></i>decode</span>
      </div>
      <div class="tlbox"><Timeline requests={s.llm.requests} fault /></div>
    </section>
  {/if}

  {#if mode !== 'idle'}
    <Controls session={s} {slot} {actions} {mode} />
  {/if}

  <button class="panel cons" onclick={() => actions.toggleConsole()} aria-label="Toggle console">
    {#if lastLine}<span class="line">{lastLine}</span>{:else}<span class="line mut">Console · no output yet</span>{/if}
    <svg class="chev" viewBox="0 0 20 12" aria-hidden="true"><path d="M1 1 L10 11 L19 1 Z" /></svg>
  </button>
</div>

<style>
  .full {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: calc(10px * var(--k));
    padding: calc(12px * var(--k)) calc(22px * var(--k)) calc(14px * var(--k));
  }

  .recipe {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: calc(9px * var(--k));
    row-gap: 2px;
    font-size: calc(18px * var(--k));
    letter-spacing: 0.035em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow-soft);
    padding: 0 calc(6px * var(--k));
    max-height: calc(52px * var(--k));
    overflow: hidden;
  }
  .recipe .sep {
    color: var(--ph-muted);
  }
  .part {
    white-space: nowrap;
  }

  /* decode */
  .decode {
    flex: 1.3 1 0;
    min-height: calc(150px * var(--k));
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr) auto;
    padding: calc(14px * var(--k)) calc(22px * var(--k)) 0;
  }
  .dhead {
    display: flex;
    align-items: baseline;
    gap: calc(16px * var(--k));
  }
  .act {
    font-size: calc(13px * var(--k));
    letter-spacing: 0.1em;
    color: var(--ph-muted);
    text-transform: uppercase;
  }
  .act[data-act='decode'] {
    color: var(--ph-cyan);
  }
  .act[data-act='prefill'] {
    color: var(--ph-amber);
  }
  .note {
    margin-left: auto;
    font-size: calc(15px * var(--k));
    letter-spacing: 0.04em;
    color: var(--ph-cyan);
    opacity: 0.85;
  }
  .readout {
    display: flex;
    align-items: baseline;
    gap: calc(26px * var(--k));
    margin-top: calc(-4px * var(--k));
    font-family: var(--ph-display);
    color: var(--ph-cyan);
    line-height: 1;
    white-space: nowrap;
  }
  .readout.dim {
    color: var(--ph-muted);
  }
  .readout.dim .num,
  .readout.dim .unit {
    text-shadow: none;
  }
  .num {
    font-size: calc(82px * var(--k));
    font-weight: 380;
    font-stretch: 112%;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.03em;
    text-shadow:
      0 0 10px rgba(127, 227, 255, 0.55),
      0 0 30px rgba(90, 182, 235, 0.3);
  }
  .unit {
    font-size: calc(74px * var(--k));
    font-weight: 360;
    font-stretch: 108%;
    letter-spacing: 0.03em;
    text-shadow:
      0 0 10px rgba(127, 227, 255, 0.5),
      0 0 28px rgba(90, 182, 235, 0.26);
  }
  /* prefill as hero */
  .readout.pf {
    align-items: center;
    gap: calc(34px * var(--k));
    color: var(--ph-amber);
  }
  .readout.pf .num {
    text-shadow:
      0 0 10px rgba(232, 176, 74, 0.5),
      0 0 30px rgba(232, 176, 74, 0.22);
  }
  .pc {
    font-size: 0.6em;
    margin-left: 0.04em;
  }
  .pfx {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: calc(9px * var(--k));
    font-family: var(--ph-ui);
    padding-top: calc(6px * var(--k));
  }
  .pfa {
    font-size: calc(26px * var(--k));
    letter-spacing: 0.04em;
    color: #f3d9a4;
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.4);
  }
  .pfb {
    font-size: calc(17px * var(--k));
    letter-spacing: 0.04em;
    color: var(--ph-muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pfbar {
    position: relative;
    display: block;
    height: calc(8px * var(--k));
    border: 1px solid rgba(232, 176, 74, 0.45);
    border-radius: 2px;
    overflow: hidden;
  }
  .pff {
    position: absolute;
    inset: 0;
    transform-origin: left center;
    background: linear-gradient(90deg, rgba(232, 176, 74, 0.5), var(--ph-amber));
    box-shadow: 0 0 10px rgba(232, 176, 74, 0.5);
    transition: transform 0.45s ease-out;
  }
  .scope {
    min-height: 0;
    margin: 0 calc(-8px * var(--k)) 0 calc(-10px * var(--k));
  }
  .dfoot {
    display: flex;
    align-items: center;
    gap: calc(22px * var(--k));
    height: calc(44px * var(--k));
    border-top: 1px solid var(--ph-grat);
    font-size: calc(18px * var(--k));
    min-width: 0;
  }
  .seg {
    display: flex;
    align-items: baseline;
    gap: calc(22px * var(--k));
    min-width: 0;
  }
  .seg .lbl {
    font-size: calc(17px * var(--k));
  }
  .seg .val {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .vrule {
    flex: none;
    width: 1px;
    align-self: stretch;
    margin: calc(8px * var(--k)) 0;
    background: var(--ph-rule);
  }

  /* dormant decode area while loading */
  .dorm {
    flex: none;
    height: calc(152px * var(--k));
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    padding: calc(14px * var(--k)) calc(22px * var(--k)) calc(18px * var(--k));
  }
  .dorm .readout {
    margin-top: calc(6px * var(--k));
  }
  .dorm .num {
    font-size: calc(74px * var(--k));
  }
  .dorm .unit {
    font-size: calc(66px * var(--k));
  }
  .flat {
    align-self: end;
    height: 0;
    border-top: 1.5px solid rgba(127, 227, 255, 0.35);
    box-shadow: 0 0 8px rgba(127, 227, 255, 0.25);
  }

  /* context + speculative */
  .ctx {
    flex: none;
    height: calc(126px * var(--k));
    display: flex;
    align-items: center;
    gap: calc(28px * var(--k));
    padding: 0 calc(22px * var(--k)) 0 calc(16px * var(--k));
  }
  .ctx .cell {
    display: grid;
    gap: calc(12px * var(--k));
    min-width: 0;
  }
  .ctx .cell:last-child {
    flex: 1;
  }
  .ctx .cell.spec {
    container-type: inline-size;
  }
  /* Share Tech Mono advance + 0.05em tracking = 0.59em per character */
  .ctx .spec .big {
    font-size: min(calc(23px * var(--k)), calc(100cqw / (var(--n) * 0.59)));
  }
  .ctx .vrule {
    margin: calc(14px * var(--k)) 0;
  }
  .big {
    font-size: calc(23px * var(--k));
    letter-spacing: 0.05em;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* vram */
  .vram {
    flex: 1.15 1 0;
    min-height: calc(170px * var(--k));
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(12px * var(--k)) calc(16px * var(--k)) calc(6px * var(--k));
  }
  .m-fault .vram {
    flex: 1 1 0;
  }
  .m-loading .vram {
    flex: 1 1 0;
  }
  .fitp {
    flex: 1 1 0;
    min-height: calc(250px * var(--k));
    padding-bottom: calc(10px * var(--k));
  }
  .vhead {
    display: flex;
    align-items: baseline;
    gap: calc(48px * var(--k));
    padding-left: calc(6px * var(--k));
    font-size: calc(18px * var(--k));
    letter-spacing: 0.04em;
  }
  .vhead .sub {
    margin-left: calc(-12px * var(--k));
  }
  .vhead .dev {
    margin-left: auto;
    padding-right: calc(6px * var(--k));
  }
  .cliffbox {
    min-height: 0;
    margin-top: calc(4px * var(--k));
  }

  /* system strip */
  .sys {
    flex: none;
    height: calc(46px * var(--k));
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: calc(22px * var(--k));
    padding: 0 calc(22px * var(--k));
    font-size: calc(18px * var(--k));
    letter-spacing: 0.04em;
  }
  .sys .vrule {
    margin: calc(10px * var(--k)) 0;
  }

  /* request timeline / recent jobs */
  .tlp {
    flex: none;
    height: calc(96px * var(--k));
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: calc(10px * var(--k)) calc(18px * var(--k)) calc(10px * var(--k));
    gap: calc(8px * var(--k));
  }
  .jobsp {
    height: calc(122px * var(--k));
  }
  .thead {
    display: flex;
    align-items: baseline;
    padding-left: calc(4px * var(--k));
  }
  .legend {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: calc(10px * var(--k));
    font-size: calc(15px * var(--k));
    color: var(--ph-cyan);
  }
  .wnote {
    margin-left: calc(22px * var(--k));
  }
  .sw {
    display: inline-block;
    width: calc(20px * var(--k));
    height: 3px;
    border-radius: 2px;
    margin-left: calc(18px * var(--k));
  }
  .sw.a {
    background: var(--ph-amber);
    box-shadow: 0 0 6px rgba(232, 176, 74, 0.6);
  }
  .sw.c {
    background: var(--ph-cyan);
    box-shadow: 0 0 6px rgba(127, 227, 255, 0.6);
  }
  .tlbox {
    min-height: 0;
  }

  /* console line */
  .cons {
    flex: none;
    height: calc(42px * var(--k));
    display: flex;
    align-items: center;
    gap: calc(16px * var(--k));
    padding: 0 calc(20px * var(--k));
    text-align: left;
    font-size: calc(17px * var(--k));
    letter-spacing: 0.03em;
    color: var(--ph-ink);
  }
  .cons:hover {
    border-color: #2a7f93;
  }
  .cons .line {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chev {
    flex: none;
    width: calc(18px * var(--k));
    height: calc(11px * var(--k));
    fill: var(--ph-brand);
  }
</style>
