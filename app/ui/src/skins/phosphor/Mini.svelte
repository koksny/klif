<script lang="ts">
  // Mini panel: a 3.5" 960x640 screen read from a metre away. Read-only, huge type, smallest
  // text 30 px (at k = 1). Everything is laid out on a 960x640 design grid scaled by --k.
  // Rows: brand/phase · model · big readout · line or trace · cliff (+ side gauge) · bottom strip.
  import type { ViewModel } from '../../lib/model/types';
  import { fmtClock, fmtGiB, fmtInt, fmtSeconds, fmtTps } from '../../lib/model/format';
  import Scope from './Scope.svelte';
  import Radar from './Radar.svelte';
  import VramCliff from './VramCliff.svelte';
  import FitCliff from './FitCliff.svelte';
  import Timeline from './Timeline.svelte';
  import Jobs from './Jobs.svelte';
  import { AVAIL_TEXT, clamp, fmtAgo, fmtDur, fmtSPerIt, loadSpanS } from './geom';

  let { vm, k }: { vm: ViewModel; k: number } = $props();

  const s = $derived(vm.session);
  const llm = $derived(s && s.phase === 'live' ? s.llm : null);
  const img = $derived(s && s.phase === 'live' ? s.image : null);
  const slot = $derived(vm.slots.find((x) => x.id === (s?.slot ?? vm.selected)));
  const booting = $derived(!!s && (s.phase === 'starting' || s.phase === 'loading'));
  const fault = $derived(s && s.phase === 'fault' ? s.fault : null);

  const PHASE: Record<string, { word: string; tone: string }> = {
    starting: { word: 'START', tone: 'amber' },
    loading: { word: 'LOAD', tone: 'amber' },
    live: { word: 'LIVE', tone: 'cyan' },
    stopping: { word: 'STOP', tone: 'muted' },
    fault: { word: 'FAULT', tone: 'danger' },
  };
  const st = $derived(s ? PHASE[s.phase] : { word: 'IDLE', tone: 'muted' });

  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0);
  const sweep = $derived.by(() => {
    if (!llm || llm.activity === 'idle') return 0;
    const tps = llm.decodeTps > 0 ? llm.decodeTps : (llm.prefill?.tps ?? 0) / 16;
    return tps / 180;
  });
  const pf = $derived(llm && llm.activity === 'prefill' && llm.prefill && llm.prefill.tokens > 0 ? llm.prefill : null);
  const pfFrac = $derived(pf ? clamp(pf.doneTokens / pf.tokens, 0, 1) : 0);

  const steps = $derived(s?.loading?.steps ?? []);
  const activeStep = $derived(steps.find((x) => x.state === 'active'));
  const doneSteps = $derived(steps.filter((x) => x.state === 'done').length);
  const loadFrac = $derived(clamp(s?.loading?.fraction ?? 0, 0, 1));
  const spill = $derived(Math.round(vm.vram.spillMiB));
  /** Free VRAM under the configured warning headroom (or spilling): the readout turns amber. */
  const vramTight = $derived(spill > 0 || vm.vram.totalGiB - vm.vram.usedGiB < vm.vram.warnBelowGiB);

  // idle fit preview: spare = total - baseline - expected
  const ready = $derived(!!slot && slot.availability === 'ready');
  const base = $derived(Math.max(0, vm.vram.baselineGiB ?? vm.vram.usedGiB));
  const need = $derived((slot?.expectedVram ?? []).reduce((a, l) => a + l.gib, 0));
  const spare = $derived(vm.vram.totalGiB - base - need);
  const last = $derived(vm.lastSession ?? null);
  const lastLine = $derived.by(() => {
    if (!last) return '';
    const p = [vm.slots.find((x) => x.id === last.slot)?.label ?? last.slot, fmtDur(last.uptimeS)];
    if (last.requests !== undefined) p.push(`${fmtInt(last.requests)} req`);
    if (last.images !== undefined) p.push(`${fmtInt(last.images)} img`);
    if (last.ended !== 'fault') p.push(fmtAgo(last.endedAgoS));
    return p.join(' · ');
  });
  /** Shrink long model names so they fit the 916 px row. */
  const nameSize = (name: string, max: number) => Math.min(max, 900 / Math.max(1, name.length * 0.62));

  const imgFrac = $derived(img && img.activity === 'generating' && img.steps > 0 ? clamp(img.step / img.steps, 0, 1) : 0);
  const lastJob = $derived(img && img.recent.length ? img.recent[img.recent.length - 1] : null);
</script>

<div class="mini">
  <div class="top">
    <span class="brand">KLIF</span>
    <span class="phase {st.tone}">{st.word}</span>
  </div>

  {#if !s}
    <!-- idle launcher: selected tier, its model, availability, fit preview, last session -->
    <div class="model tier">{slot?.label ?? ''}</div>
    <div class="namebig" style="font-size:calc({nameSize(slot?.model.name ?? '', 84).toFixed(1)}px * var(--k))">{slot?.model.name ?? ''}</div>
    <div class="trace line" class:amber={!ready}>
      {#if ready}<span class="dot" aria-hidden="true"></span>ready{:else}{slot ? (AVAIL_TEXT[slot.availability] ?? slot.availability) : ''}{/if}{#if slot?.model.quant}<span class="mut">{` · ${slot.model.quant}`}</span>{/if}
    </div>
    <div class="lower">
      <div class="cliff">
        <FitCliff vram={vm.vram} {slot} variant="mini" {k} />
        <div class="gib" class:bad={spare < 0}>
          {#if need > 0}{spare >= 0 ? `fits · ${fmtGiB(spare)} GiB spare` : `over the edge · ${fmtGiB(-spare)} GiB`}{:else}{fmtGiB(base)} GiB in use{/if}
        </div>
      </div>
    </div>
    <div class="req">
      <span class="reql">LAST</span>
      <span class="reqt">
        {#if last}{lastLine}{#if last.ended === 'fault'}<span class="mut">{' · '}</span><span class="danger">fault</span>{/if}{:else}no previous session{/if}
      </span>
    </div>
  {:else}
    <div class="model">{s.model.name}</div>

    {#if pf && llm}
      <!-- prompt processing: its progress is the hero, never a stale decode speed -->
      <div class="readout">
        <span class="num amber">{Math.floor(pfFrac * 100)}<span class="of">%</span></span><span class="unit amber">prefill</span>
      </div>
      <div class="trace pfl">
        <span class="pft">{fmtInt(pf.tps)} tok/s · {pf.etaS > 0 ? `eta ${fmtSeconds(pf.etaS)}` : 'finishing'}</span>
        <span class="pbar amber"><span class="pfill" style="transform:scaleX({pfFrac.toFixed(4)})"></span></span>
      </div>
    {:else if llm}
      <div class="readout" class:dim={llm.activity !== 'decode'}>
        <span class="num">{fmtTps(llm.decodeTps)}</span><span class="unit">tok/s</span>
      </div>
      <div class="trace"><Scope history={llm.decodeHistory} variant="mini" /></div>
    {:else if img}
      <div class="readout">
        {#if img.activity === 'generating'}
          <span class="num">{img.step}<span class="of">/{img.steps}</span></span><span class="unit">{fmtSPerIt(img.sPerIt)} s/it</span>
        {:else}
          <span class="num dim small">idle</span><span class="unit dim">{fmtInt(img.imagesThisSession)} img</span>
        {/if}
      </div>
      <div class="trace pfl">
        <span class="pft">{img.activity === 'generating' ? `elapsed ${fmtSeconds(img.elapsedS)} · ${Math.round(imgFrac * 100)}%` : 'waiting for a job'}</span>
        <span class="pbar"><span class="pfill" style="transform:scaleX({imgFrac.toFixed(4)})"></span></span>
      </div>
    {:else if booting}
      <!-- the % and the bar are both loading.fraction; the line names the active step -->
      <div class="readout"><span class="num amber">{Math.round(loadFrac * 100)}<span class="of">%</span></span></div>
      <div class="trace pfl">
        <span class="pft">{activeStep ? `${activeStep.label}${activeStep.detail ? ` · ${activeStep.detail}` : ''}` : (s.phase === 'starting' ? 'starting' : '')}</span>
        <span class="pbar amber"><span class="pfill" style="transform:scaleX({loadFrac.toFixed(4)})"></span></span>
      </div>
    {:else if fault}
      <div class="readout">
        <span class="num danger fault">FAULT</span>{#if fault.exitCodeHex || fault.exitCode !== undefined}<span class="unit danger code">{fault.exitCodeHex ?? fault.exitCode}</span>{/if}
      </div>
      <div class="trace line danger ftitle">{fault.title}</div>
    {:else if s.phase === 'stopping'}
      <div class="readout"><span class="num dim small">stopping</span></div>
      <div class="trace"></div>
    {:else}
      <div class="readout"><span class="num dim small">{slot?.label ?? ''}</span></div>
      <div class="trace"></div>
    {/if}

    <div class="lower">
      <div class="cliff">
        <VramCliff
          vram={vm.vram}
          variant="mini"
          {k}
          spanS={booting ? loadSpanS(s.loading?.elapsedS ?? s.uptimeS) : 300}
          markAgoS={fault ? fault.sinceS : null}
        />
        <div class="gib" class:low={booting || !!fault} class:tight={vramTight}>
          {fmtGiB(vm.vram.usedGiB)} / {fmtGiB(vm.vram.totalGiB)} GiB
          {#if spill > 0}<span class="spill">+{spill} MiB spill</span>{/if}
        </div>
      </div>
      {#if llm}
        <div class="radar">
          <Radar fraction={ctxFrac} turnsPerSec={sweep} size={Math.round(212 * k)} tone="amber">
            <div class="ctx"><span class="ctxl">CTX</span><span class="ctxv">{Math.round(ctxFrac * 100)}%</span></div>
          </Radar>
        </div>
      {:else if booting}
        <div class="side">
          <span class="sl">ELAPSED</span>
          <span class="sv">{fmtClock(s.loading?.elapsedS ?? s.uptimeS).slice(3)}</span>
        </div>
      {:else if img}
        <div class="side">
          <span class="sl">LAST</span>
          <span class="sv">{lastJob ? `${lastJob.seconds.toFixed(1)} s` : '—'}</span>
          <span class="sl">{fmtInt(img.imagesThisSession)} IMG</span>
        </div>
      {:else if fault}
        <div class="side">
          <span class="sv">{fmtAgo(fault.sinceS).replace(' ago', '')}</span>
          <span class="sl">AGO</span>
        </div>
      {/if}
    </div>

    {#if llm}
      <div class="req">
        <span class="reql">REQ</span>
        <div class="reqs"><Timeline requests={llm.requests} variant="mini" minCells={Math.max(1, llm.requests.length)} /></div>
      </div>
    {:else if img}
      <div class="req">
        <span class="reql">JOBS</span>
        <div class="reqs">
          <Jobs
            jobs={img.recent}
            current={img.activity === 'generating' ? { elapsedS: img.elapsedS, edit: img.edit } : null}
            variant="mini"
            maxShown={10}
          />
        </div>
      </div>
    {:else if booting}
      <div class="req">
        <span class="reql">STEP</span>
        <div class="pips" role="img" aria-label="{doneSteps} of {steps.length} load steps done">
          {#each steps as x (x.id)}<span class="pip {x.state}"></span>{/each}
        </div>
        <span class="reqt num2">{doneSteps}/{steps.length}</span>
      </div>
    {:else if fault}
      <div class="req">
        <span class="reqt mut">{fault.logTail.length ? fault.logTail[fault.logTail.length - 1].trim() : ''}</span>
      </div>
    {:else}
      <div class="req"></div>
    {/if}
  {/if}
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    display: grid;
    grid-template-rows:
      calc(60px * var(--k)) calc(70px * var(--k)) calc(128px * var(--k)) calc(84px * var(--k))
      minmax(0, 1fr) calc(46px * var(--k));
    padding: calc(12px * var(--k)) calc(22px * var(--k)) calc(10px * var(--k));
    color: var(--ph-cyan);
    background-image:
      linear-gradient(to right, rgba(18, 48, 58, 0.42) 1px, transparent 1px),
      linear-gradient(to bottom, rgba(18, 48, 58, 0.42) 1px, transparent 1px);
    background-size: calc(48px * var(--k)) calc(48px * var(--k));
  }
  .top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-family: var(--ph-display);
    font-stretch: 125%;
    font-weight: 400;
    font-size: calc(58px * var(--k));
    line-height: 1;
    letter-spacing: 0.04em;
  }
  .brand {
    color: var(--ph-brand);
    text-shadow:
      0 0 8px rgba(90, 182, 235, 0.6),
      0 0 22px rgba(90, 182, 235, 0.3);
  }
  .phase {
    color: var(--ph-cyan);
    text-shadow:
      0 0 8px rgba(127, 227, 255, 0.6),
      0 0 22px rgba(127, 227, 255, 0.25);
  }
  .phase.amber,
  .num.amber,
  .unit.amber,
  .line.amber {
    color: var(--ph-amber);
    text-shadow: 0 0 10px rgba(232, 176, 74, 0.45);
  }
  .phase.danger,
  .danger {
    color: var(--ph-danger);
    text-shadow: 0 0 10px rgba(229, 97, 92, 0.45);
  }
  .phase.muted {
    color: var(--ph-muted);
    text-shadow: none;
  }
  .model {
    align-self: center;
    text-align: center;
    font-family: var(--ph-display);
    font-stretch: 122%;
    font-weight: 330;
    font-size: calc(64px * var(--k));
    line-height: 1;
    letter-spacing: 0.04em;
    color: var(--ph-cyan);
    text-shadow:
      0 0 8px rgba(127, 227, 255, 0.5),
      0 0 22px rgba(90, 182, 235, 0.25);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .model.tier {
    font-weight: 400;
    letter-spacing: 0.08em;
    color: #b9f1ff;
  }
  .namebig {
    align-self: center;
    text-align: center;
    font-family: var(--ph-display);
    font-stretch: 112%;
    font-weight: 330;
    line-height: 1;
    letter-spacing: 0.03em;
    color: #c9f4ff;
    text-shadow:
      0 0 12px rgba(127, 227, 255, 0.5),
      0 0 30px rgba(90, 182, 235, 0.25);
    white-space: nowrap;
    overflow: hidden;
  }
  .readout {
    display: flex;
    justify-content: center;
    align-items: baseline;
    gap: calc(34px * var(--k));
    font-family: var(--ph-display);
    color: #c9f4ff;
    line-height: 0.74;
    padding-top: calc(10px * var(--k));
    white-space: nowrap;
  }
  .readout.dim .num,
  .readout.dim .unit {
    color: var(--ph-muted);
    text-shadow: none;
  }
  .num {
    font-size: calc(176px * var(--k));
    font-weight: 330;
    font-stretch: 116%;
    letter-spacing: 0.03em;
    text-shadow:
      0 0 12px rgba(127, 227, 255, 0.55),
      0 0 34px rgba(90, 182, 235, 0.3);
  }
  .num.small {
    font-size: calc(96px * var(--k));
  }
  .num.fault {
    font-size: calc(132px * var(--k));
    font-weight: 400;
    letter-spacing: 0.06em;
  }
  .num.dim,
  .unit.dim {
    color: var(--ph-muted);
    text-shadow: none;
  }
  .of {
    font-size: 0.45em;
  }
  .unit {
    font-size: calc(76px * var(--k));
    font-weight: 340;
    font-stretch: 110%;
    text-shadow:
      0 0 10px rgba(127, 227, 255, 0.5),
      0 0 26px rgba(90, 182, 235, 0.25);
  }
  .unit.code {
    font-size: calc(50px * var(--k));
    font-stretch: 100%;
  }
  .trace {
    min-height: 0;
    margin: 0 calc(-12px * var(--k)) 0 calc(-16px * var(--k));
  }
  .trace.line {
    margin: 0;
    align-self: center;
    text-align: center;
    font-size: calc(34px * var(--k));
    line-height: 1.1;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
  .trace.line.ftitle {
    font-size: calc(30px * var(--k));
    text-wrap: balance;
  }
  .trace.line .mut {
    color: var(--ph-muted);
    text-shadow: none;
  }
  .dot {
    display: inline-block;
    width: calc(18px * var(--k));
    height: calc(18px * var(--k));
    margin-right: calc(14px * var(--k));
    border-radius: 50%;
    background: var(--ph-cyan);
    box-shadow: 0 0 10px var(--ph-cyan);
    vertical-align: 0.05em;
  }
  /* a text line over a progress bar (prefill, image steps) */
  .trace.pfl {
    margin: 0;
    display: grid;
    align-content: start;
    gap: calc(10px * var(--k));
    justify-items: center;
    padding-top: calc(16px * var(--k));
  }
  .pft {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: calc(34px * var(--k));
    line-height: 1.05;
    letter-spacing: 0.03em;
    white-space: nowrap;
    color: #f3d9a4;
    text-shadow: 0 0 10px rgba(232, 176, 74, 0.4);
  }
  .pbar {
    position: relative;
    width: 100%;
    height: calc(14px * var(--k));
    border: 1px solid #2a7f93;
    border-radius: 3px;
    overflow: hidden;
  }
  .pbar.amber {
    border-color: rgba(232, 176, 74, 0.55);
  }
  .pfill {
    position: absolute;
    inset: 0;
    transform-origin: left center;
    background: linear-gradient(90deg, rgba(90, 182, 235, 0.7), var(--ph-cyan));
    box-shadow: 0 0 12px rgba(127, 227, 255, 0.55);
    transition: transform 0.45s ease-out;
  }
  .amber .pfill {
    background: linear-gradient(90deg, rgba(232, 176, 74, 0.55), var(--ph-amber));
    box-shadow: 0 0 12px rgba(232, 176, 74, 0.55);
  }
  .trace.pfl:not(:has(.amber)) .pft {
    color: var(--ph-cyan);
    text-shadow: 0 0 10px rgba(127, 227, 255, 0.4);
  }
  .lower {
    display: flex;
    gap: calc(26px * var(--k));
    min-height: 0;
    padding-top: calc(14px * var(--k));
  }
  .cliff {
    position: relative;
    flex: 1;
    min-width: 0;
    border-left: 1px solid var(--ph-grat);
  }
  .gib {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(10px * var(--k));
    text-align: center;
    font-family: var(--ph-display);
    font-stretch: 96%;
    font-size: calc(60px * var(--k));
    line-height: 1;
    color: var(--ph-cyan);
    text-shadow:
      0 0 3px var(--ph-glass),
      0 0 3px var(--ph-glass),
      0 0 12px rgba(127, 227, 255, 0.45);
    white-space: nowrap;
  }
  .gib.bad {
    color: var(--ph-danger);
  }
  .gib.tight {
    color: var(--ph-amber);
    text-shadow:
      0 0 3px var(--ph-glass),
      0 0 3px var(--ph-glass),
      0 0 12px rgba(232, 176, 74, 0.45);
  }
  /* loading / fault: the trace runs along the floor under the readout, so cut it out around the glyphs */
  .gib.low {
    -webkit-text-stroke: calc(10px * var(--k)) var(--ph-glass);
    paint-order: stroke fill;
    text-shadow: 0 0 14px rgba(127, 227, 255, 0.35);
  }
  .spill {
    display: block;
    font-size: calc(32px * var(--k));
    color: var(--ph-amber);
  }
  .radar {
    flex: none;
    align-self: center;
    margin-right: calc(4px * var(--k));
  }
  .ctx {
    display: grid;
    justify-items: center;
    line-height: 1;
    padding: calc(6px * var(--k)) calc(10px * var(--k));
    background: radial-gradient(closest-side, rgba(5, 9, 11, 0.85), rgba(5, 9, 11, 0));
    transform: translateY(calc(30px * var(--k)));
  }
  .ctxl {
    font-family: var(--ph-ui);
    font-size: calc(30px * var(--k));
    letter-spacing: 0.1em;
    color: var(--ph-cyan);
  }
  .ctxv {
    font-family: var(--ph-display);
    font-size: calc(50px * var(--k));
    color: #d8f7ff;
    text-shadow: 0 0 10px rgba(127, 227, 255, 0.5);
  }
  /* side gauge (image, fault): label / big value / label */
  .side {
    flex: none;
    width: calc(212px * var(--k));
    align-self: center;
    display: grid;
    justify-items: center;
    gap: calc(10px * var(--k));
    border-left: 1px solid var(--ph-grat);
    padding: calc(8px * var(--k)) 0;
  }
  .sl {
    font-size: calc(30px * var(--k));
    letter-spacing: 0.1em;
    line-height: 1;
    color: var(--ph-cyan);
  }
  .sv {
    font-family: var(--ph-display);
    font-size: calc(62px * var(--k));
    line-height: 1;
    color: #d8f7ff;
    white-space: nowrap;
    text-shadow:
      0 0 10px rgba(127, 227, 255, 0.5),
      0 0 26px rgba(90, 182, 235, 0.25);
  }
  .req {
    display: flex;
    align-items: center;
    gap: calc(18px * var(--k));
    min-width: 0;
  }
  .reql {
    flex: none;
    font-size: calc(30px * var(--k));
    letter-spacing: 0.1em;
    color: var(--ph-cyan);
    opacity: 0.85;
  }
  .reqs {
    flex: 1;
    height: 100%;
    min-width: 0;
  }
  .reqt {
    flex: 1;
    min-width: 0;
    font-size: calc(30px * var(--k));
    letter-spacing: 0.03em;
    color: var(--ph-ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .reqt.mut {
    color: var(--ph-muted);
  }
  .reqt.num2 {
    flex: none;
    color: var(--ph-amber);
  }
  .pips {
    flex: 1;
    display: flex;
    gap: calc(10px * var(--k));
    height: calc(18px * var(--k));
  }
  .pip {
    flex: 1;
    border-radius: 3px;
    border: 1.5px solid var(--ph-muted);
    opacity: 0.7;
  }
  .pip.done {
    background: var(--ph-cyan);
    border-color: var(--ph-cyan);
    box-shadow: 0 0 10px rgba(127, 227, 255, 0.5);
    opacity: 1;
  }
  .pip.active {
    background: rgba(232, 176, 74, 0.45);
    border-color: var(--ph-amber);
    box-shadow: 0 0 10px rgba(232, 176, 74, 0.5);
    opacity: 1;
  }
  .pip.failed {
    background: var(--ph-danger);
    border-color: var(--ph-danger);
    opacity: 1;
  }
</style>
