<script lang="ts">
  // Silicon, mini panel (960x640, read from 1 m). Read-only. A fixed 960x640 drawing sheet scaled
  // to fit; smallest text 30 px. Same composition in every state: title line, die (left), the
  // segmented VRAM gauge under it, readouts on the right, title block bottom right.
  // GPU dormant (live session, vram.dormant set): amber chip, hatched dashed outlines for what is paged
  // out (die blocks and gauge), and a big "GPU ASLEEP" / "GPU WAKING" in place of the speed.
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtGiB, fmtInt, fmtPct, fmtTps } from '../../lib/model/format';
  import DieCanvas from './DieCanvas.svelte';
  import { GEOM } from './die';
  import { held } from './held.svelte';
  import { useSleep } from './sleep.svelte';
  import { availabilityText, baselineOf, fmtAgo, fmtDur, fmtEta } from './text';

  let { vm, actions }: { vm: ViewModel; actions?: Actions } = $props();

  // "Back to window": the one control on the panel. Hidden until the pointer is over the panel (or it
  // is focused with the keyboard), then a small chip in the quiet strip under the title rule.
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
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const liveLlm = $derived(!!s && phase === 'live' && kind === 'llm' && !!llm);
  const liveImg = $derived(!!s && phase === 'live' && kind === 'image' && !!img);
  const name = $derived(s?.model.name ?? slot?.model.name ?? '');
  const idle = $derived(!s);
  const loading = $derived(!!s && (phase === 'starting' || phase === 'loading'));
  const faulted = $derived(!!s && phase === 'fault');

  // During prefill the decode speed sits at 0 for minutes: the hero is the prefill progress.
  const prefilling = $derived(liveLlm && !!llm?.prefill && llm.activity === 'prefill');
  const tps = held(() => llm?.decodeTps ?? 0);
  const heroText = $derived(fmtTps(tps.current));
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
  const titleWord = $derived(dz ? (waking ? 'WAKING' : 'DORMANT') : phaseLabel);
  /** The line under "x GiB paged out": how long and in which power state, or how far the restore is. */
  const sleepLine = $derived.by(() => {
    if (!dz) return '';
    if (waking) return `${Math.round(dz.restoredFrac * 100)}% restored`;
    return [dz.powerState, `asleep ${fmtDur(dz.sinceS)}`, llm ? `last ${fmtTps(tps.current)} tok/s` : null].filter(Boolean).join(' · ');
  });
  const dwg = $derived(vm.vram.device.replace(/^RX\s*/i, '').trim().replace(/\s+/g, '-'));
  // Drawing revision = the app's major.minor (host.appVersion "0.2.0" -> "0.2").
  const rev = $derived((vm.host?.appVersion ?? '').split('.').slice(0, 2).join('.') || '—');
  const activeStep = $derived(s?.loading?.steps.find((x) => x.state === 'active') ?? null);

  // Die placement on the sheet, and the leader anchor tile (token stream -> hero).
  const DIE = { x: 40, y: 80, w: 436, h: 388 };
  const g = GEOM.mini;
  const ds = Math.min(DIE.w / g.W, DIE.h / g.H);
  const dox = DIE.x + (DIE.w - g.W * ds) / 2;
  const doy = DIE.y + (DIE.h - g.H * ds) / 2;
  const lt = g.tiles[g.leaderTile];
  const tileX = dox + (lt.x + lt.w * 0.55) * ds;
  const tileY = doy + (lt.y + lt.h / 2) * ds;

  // Tagged heroes (PREFILL %, STEP n/m) are right-aligned and narrower than the decode figure: the
  // tile leader runs on to just short of the figure instead of stopping in empty space.
  let tagHeroW = $state(0);
  const leaderEnd = $derived((prefilling || imgGen) && tagHeroW > 0 ? Math.max(534, SW - 34 - tagHeroW - 16) : 534);

  // VRAM bar: the same 8 x totalGiB/8 gauge as the GDDR6 blocks, at true scale.
  const BAR = { x: 36, y: 504, w: 438, h: 48 };
  const total = $derived(Math.max(0.01, vm.vram.totalGiB));
  const xAt = (gib: number) => BAR.x + (Math.min(total, Math.max(0, gib)) / total) * BAR.w;
  const usedClamped = $derived(Math.min(total, Math.max(0, vm.vram.usedGiB)));
  const usedX = $derived(xAt(usedClamped));
  const segW = BAR.w / 8;
  // Dormant: the paged-out allocations occupy [resident, resident + paged out] of the gauge, same scale.
  const pagedEnd = $derived(dz ? Math.min(total, usedClamped + dz.pagedOutGiB) : usedClamped);
  const pagedX = $derived(xAt(pagedEnd));
  const segs = $derived(
    Array.from({ length: 8 }, (_, i) => {
      const f = Math.min(1, Math.max(0, usedClamped / (total / 8) - i));
      const ha = f;
      const hb = dz ? Math.min(1, Math.max(0, pagedEnd / (total / 8) - i)) : 0;
      return { x: BAR.x + i * segW, f, ha, hb };
    }),
  );
  const freeDotX = $derived(
    dz ? (usedX + pagedX) / 2 : Math.min(BAR.x + BAR.w - 6, Math.max(usedX + 3, (usedX + BAR.x + BAR.w) / 2)),
  );
  const spillGiB = $derived(vm.vram.spillMiB / 1024);
  const spillY = 464;
  const lowFree = $derived(!!s && vm.vram.totalGiB - vm.vram.usedGiB < vm.vram.warnBelowGiB);

  // Idle fit preview: expected layers of the selected tier on top of the baseline, at the same scale.
  const base = $derived(Math.max(baselineOf(vm.vram), vm.vram.usedGiB));
  const expected = $derived(idle && slot?.expectedVram?.length ? slot.expectedVram.reduce((a, l) => a + l.gib, 0) : null);
  const spare = $derived(expected !== null ? vm.vram.totalGiB - base - expected : 0);
  const ghost = $derived(expected !== null ? { x0: xAt(base), x1: xAt(base + expected), over: spare < 0 } : null);
  const spareDotX = $derived(ghost ? Math.min(BAR.x + BAR.w - 4, Math.max(ghost.x1 + 3, (ghost.x1 + BAR.x + BAR.w) / 2)) : 0);
  const ready = $derived(slot?.availability === 'ready');
  const labelWords = $derived((slot?.label ?? '').split(/\s+/).filter(Boolean).slice(0, 2));
  const last = $derived(vm.lastSession);
  const lastLabel = $derived(last ? (vm.slots.find((x) => x.id === last.slot)?.label ?? last.model.name) : '');

  // Fault.
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const exitText = $derived(fault ? (fault.exitCode === undefined ? 'no exit code' : `exit ${fault.exitCodeHex ?? fault.exitCode}`) : '');
</script>

<div class="mini" bind:clientWidth={w} bind:clientHeight={h}>
  <div class="sheet" style="transform: translate({ox}px, {oy}px) scale({u})">
    <!-- Title line -->
    <div class="top">
      <span class="klif">KLIF</span>
      <span class="sep"></span>
      <span class="st {tone}"><i class="dot" class:pulse={waking}></i>{statusLabel}</span>
      <span class="sep"></span>
      <span class="model" class:fit={statusLabel.length + name.length > 25}>{name}</span>
    </div>
    <div class="rule"></div>

    <!-- Die -->
    <div class="die" style="left:{DIE.x}px;top:{DIE.y}px;width:{DIE.w}px;height:{DIE.h}px">
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

    <!-- Linework overlay: VRAM bar, dimensions, leaders -->
    <svg class="lines" viewBox="0 0 {SW} {SH}" width={SW} height={SH} aria-hidden="true">
      <defs>
        <pattern id="sim-ghost" width="12" height="12" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="12" stroke="rgba(110,190,240,0.55)" stroke-width="2.5" />
        </pattern>
        <pattern id="sim-red" width="10" height="10" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="10" stroke="#FF5A36" stroke-width="2.5" />
        </pattern>
      </defs>
      <!-- total dimension -->
      <line x1={BAR.x} x2={BAR.x + BAR.w} y1="490" y2="490" class="hl" />
      <path d="M{BAR.x},490 l12,-5 v10 z M{BAR.x + BAR.w},490 l-12,-5 v10 z" class="ah" />
      <line x1={BAR.x} x2={BAR.x} y1="482" y2="498" class="hl" />
      <line x1={BAR.x + BAR.w} x2={BAR.x + BAR.w} y1="482" y2="498" class="hl" />
      <!-- gauge -->
      <rect x={BAR.x - 4} y={BAR.y - 4} width={BAR.w + 8} height={BAR.h + 8} class="frame" />
      {#each segs as sg, i (i)}
        <rect x={sg.x + 1.5} y={BAR.y} width={segW - 3} height={BAR.h} class="seg" />
        {#if sg.f > 0}<rect x={sg.x + 1.5} y={BAR.y} width={(segW - 3) * sg.f} height={BAR.h} class="segfill" />{/if}
        {#if sg.hb - sg.ha > 0.004}
          <rect x={sg.x + 1.5 + (segW - 3) * sg.ha} y={BAR.y} width={(segW - 3) * (sg.hb - sg.ha)} height={BAR.h} fill="url(#sim-ghost)" class="paged" />
        {/if}
      {/each}
      <!-- idle: the expected footprint of the selected tier, hatched, at true scale -->
      {#if ghost}
        <rect x={ghost.x0} y={BAR.y + 3} width={Math.max(0, ghost.x1 - ghost.x0)} height={BAR.h - 6} fill="url(#sim-ghost)" class="ghost" class:over={ghost.over} />
      {/if}
      <!-- spill: material pushed over the edge, drawn beyond it -->
      {#if spillGiB > 0}
        <rect x={BAR.x + BAR.w + 8} y={BAR.y} width={Math.max(4, Math.min(22, (spillGiB / total) * BAR.w))} height={BAR.h} fill="url(#sim-red)" class="spill" />
      {/if}
      <!-- used dimension (arrowheads only when the span can hold them) -->
      <line x1={BAR.x} x2={usedX} y1="566" y2="566" class="hl" />
      {#if usedX - BAR.x >= 30}<path d="M{BAR.x},566 l12,-5 v10 z M{usedX},566 l-12,-5 v10 z" class="ah" />{/if}
      <line x1={usedX} x2={usedX} y1={BAR.y + BAR.h + 4} y2="574" class="hl" />
      {#if dz && pagedX - usedX >= 2}
        <line x1={usedX} x2={pagedX} y1="566" y2="566" class="hl amb" />
        {#if pagedX - usedX >= 30}<path d="M{usedX},566 l12,-5 v10 z M{pagedX},566 l-12,-5 v10 z" class="ah amb" />{/if}
        <line x1={pagedX} x2={pagedX} y1={BAR.y + BAR.h + 4} y2="574" class="hl amb" />
      {/if}
      <!-- the cliff edge (limit) -->
      <line x1={BAR.x + BAR.w} x2={BAR.x + BAR.w} y1="478" y2="572" class="limit" />

      {#if ghost}
        <circle cx={spareDotX} cy={BAR.y + BAR.h / 2} r="5.5" class="pt" />
        <path d="M{spareDotX},{BAR.y + BAR.h / 2} L505,497 H544" class="leader" />
      {:else if vm.vram.usedGiB < vm.vram.totalGiB}
        <circle cx={freeDotX} cy={BAR.y + BAR.h / 2} r="5.5" class="pt" />
        <path d="M{freeDotX},{BAR.y + BAR.h / 2} L505,509 H544" class="leader" />
      {:else if spillGiB > 0}
        <path d="M{BAR.x + BAR.w + 19},{BAR.y + BAR.h / 2} L505,{spillY} H544" class="leader red" />
      {/if}
      {#if (liveLlm || imgGen) && !dz}
        <circle cx={tileX} cy={tileY} r="5.5" class="pt" />
        <path d="M{tileX},{tileY} L{tileX + 66},150 H{leaderEnd}" class="leader" />
      {/if}
    </svg>

    <!-- Readouts -->
    {#if dz}
      <div class="zhero"><span>GPU</span><span>{waking ? 'WAKING' : 'ASLEEP'}</span></div>
      <div class="zpaged"><b>{fmtGiB(dz.pagedOutGiB)} GiB</b><small>&nbsp;paged out</small></div>
      <div class="zline">{sleepLine}</div>
    {:else if liveLlm && llm}
      {#if prefilling && llm.prefill}
        <div class="tag">PREFILL</div>
        <div class="hero tagged" bind:offsetWidth={tagHeroW}><b>{Math.floor(preFrac * 100)}<small>%</small></b></div>
        <div class="stats">
          <div class="sb"><span class="cl">SPEED</span><b>{fmtInt(llm.prefill.tps)}<small>&#8201;tok/s</small></b></div>
          <div class="sb"><span class="cl">ETA</span><b>{fmtEta(llm.prefill.etaS)}</b></div>
        </div>
      {:else}
        {@const ctxText = fmtPct(ctxFrac)}
        <div class="hero" class:long={heroText.length >= 4 && !heroText.includes('.')}><b>{heroText}</b></div>
        <div class="ctx" class:wide={ctxText.length >= 4}><span class="cl">CTX</span><b class:redtxt={ctxFrac >= 0.95}>{ctxText}</b></div>
        <div class="unit">tok/s</div>
      {/if}
    {:else if liveImg && img}
      {#if imgGen}
        <div class="tag">STEP</div>
        <div class="hero tagged" bind:offsetWidth={tagHeroW}><b>{img.step}<small>/{img.steps}</small></b></div>
        <div class="stats">
          <div class="sb"><span class="cl">S/IT</span><b>{img.sPerIt.toFixed(2)}</b></div>
          <div class="sb"><span class="cl">LAST IMAGE</span><b>{lastJob ? `${lastJob.seconds.toFixed(1)}` : '—'}<small>&#8201;s</small></b></div>
        </div>
      {:else}
        <div class="tag">IMAGES</div>
        <div class="hero tagged"><b>{img.imagesThisSession}</b></div>
        <div class="stats">
          <div class="sb"><span class="cl">LAST IMAGE</span><b>{lastJob ? `${lastJob.seconds.toFixed(1)}` : '—'}<small>&#8201;s</small></b></div>
          <div class="sb"><span class="cl">NOW</span><b class="dimb">idle</b></div>
        </div>
      {/if}
    {:else if loading && s}
      <div class="tag amber">STARTUP</div>
      <div class="hero tagged"><b>{Math.floor((s.loading?.fraction ?? 0) * 100)}<small>%</small></b></div>
      {@const stepText = activeStep?.label ?? phaseLabel}
      <div class="note" class:md={stepText.length > 17} class:sm={stepText.length > 20}>{stepText}</div>
      {#if activeStep?.detail}<div class="note n2">{activeStep.detail}</div>{/if}
    {:else if faulted}
      <div class="hero red"><b>FAULT</b></div>
      <div class="fstack">
        <div class="ft">{fault?.title ?? ''}</div>
        <div class="fx redtxt">{exitText}</div>
        {#if fault}<div class="fx ago">{fmtAgo(fault.sinceS)}</div>{/if}
      </div>
    {:else if s && phase === 'stopping'}
      <div class="hero dim"><b>STOP</b></div>
      <div class="note">releasing {fmtGiB(vm.vram.usedGiB)} GiB</div>
    {:else if idle}
      <div class="tier" class:one={labelWords.length < 2}>
        {#each labelWords as wd, i (i)}<span>{wd}</span>{/each}
      </div>
      <div class="avail" class:bad={!ready}><i></i>{availabilityText(slot?.availability ?? 'unsupported')}</div>
    {/if}

    {#if idle && expected !== null}
      <div class="fitlbl" class:redtxt={spare < 0}>{spare >= 0 ? 'FITS' : 'OVER LIMIT'} · {fmtGiB(base + expected)} GiB</div>
      <div class="gib fit" class:redtxt={spare < 0}>{spare >= 0 ? `${fmtGiB(spare)} GiB spare` : `over by ${fmtGiB(-spare)} GiB`}</div>
    {:else}
      {#if dz}<div class="fitlbl amber">RESIDENT</div>{/if}
      <div class="gib" class:amber={lowFree && spillGiB <= 0} class:redtxt={spillGiB > 0} class:sp={spillGiB > 0}>{fmtGiB(vm.vram.usedGiB)} / {fmtGiB(vm.vram.totalGiB)} GiB</div>
      {#if spillGiB > 0}<div class="spilltxt">SPILL {fmtInt(vm.vram.spillMiB)} MiB</div>{/if}
    {/if}

    {#if idle && last}
      <div class="last"><span class="cl">LAST</span>{lastLabel} · {fmtDur(last.uptimeS)}{last.ended === 'fault' ? ' · fault' : ''}</div>
    {/if}
    <div class="tb">KLIF · DWG {dwg} · {titleWord} · REV {rev}</div>

    {#if canLeave}
      <button class="back" onclick={() => actions?.togglePanel?.()} title="Leave panel mode" aria-label="Leave panel mode">
        <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.5 1.5h7v7M1.5 3.5h7v7h-7z" /></svg>
      </button>
    {/if}
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
    font-family: 'Barlow Condensed', 'Iosevka', sans-serif;
    color: #f4faff;
  }
  .top {
    position: absolute;
    left: 34px;
    top: 4px;
    height: 58px;
    right: 20px;
    display: flex;
    align-items: center;
    gap: 28px;
    font-size: 54px;
    font-weight: 700;
    line-height: 1;
    white-space: nowrap;
    letter-spacing: 0.05em;
  }
  .sep {
    width: 2px;
    height: 42px;
    background: #33424f;
    flex: none;
  }
  .st {
    display: flex;
    align-items: center;
    gap: 14px;
    color: #7c8f9e;
    letter-spacing: 0.02em;
  }
  .st .dot {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 0 3px #0d1115, 0 0 0 5px #23313b;
  }
  .st.live {
    color: #5ab6eb;
  }
  .st.amber {
    color: #f2b33a;
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
  .st.fault {
    color: #ff5a36;
  }
  .model {
    font-weight: 700;
    letter-spacing: 0.08em;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  /* Long phase word (LOADING / STOPPING) + long model name: one size down so the name is not cut. */
  .model.fit {
    font-size: 46px;
    letter-spacing: 0.06em;
  }
  .rule {
    position: absolute;
    left: 15px;
    right: 15px;
    top: 64px;
    height: 1px;
    background: #3a4c5b;
  }
  .die {
    position: absolute;
  }
  .lines {
    position: absolute;
    left: 0;
    top: 0;
    overflow: visible;
    pointer-events: none;
  }
  .hl {
    stroke: #7f9fb6;
    stroke-width: 1.5;
  }
  .ah {
    fill: #7f9fb6;
  }
  .frame {
    fill: none;
    stroke: #6f8ca2;
    stroke-width: 1.5;
  }
  .seg {
    fill: #1a2631;
    stroke: #3d5161;
    stroke-width: 1;
  }
  .segfill {
    fill: #5ab6eb;
  }
  .ghost {
    stroke: #8fd0f5;
    stroke-width: 2;
    stroke-dasharray: 8 5;
  }
  .ghost.over {
    stroke: #ff5a36;
  }
  .spill {
    stroke: #ff5a36;
    stroke-width: 2;
  }
  .paged {
    stroke: #8fd0f5;
    stroke-width: 1.5;
    stroke-dasharray: 6 4;
  }
  .hl.amb {
    stroke: #f2b33a;
    stroke-dasharray: 5 4;
  }
  .ah.amb {
    fill: #f2b33a;
  }
  .limit {
    stroke: #ff5a36;
    stroke-width: 2;
    stroke-dasharray: 8 5;
  }
  .pt {
    fill: #5ab6eb;
  }
  .leader {
    fill: none;
    stroke: #5ab6eb;
    stroke-width: 2;
  }
  .leader.red {
    stroke: #ff5a36;
  }
  .hero {
    position: absolute;
    right: 34px;
    top: 52px;
    font-size: 270px;
    font-weight: 600;
    line-height: 1;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }
  .hero b {
    font-weight: 700;
  }
  .hero.long {
    font-size: 220px;
    top: 82px;
  }
  .hero small {
    font-size: 0.5em;
    color: #afc2cf;
  }
  .hero.dim {
    color: #7c8f9e;
    font-size: 200px;
    top: 96px;
  }
  .hero.red {
    color: #ff5a36;
    font-size: 150px;
    top: 70px;
  }
  .hero.tagged {
    top: 64px;
  }
  .tag {
    position: absolute;
    left: 548px;
    top: 76px;
    font-size: 36px;
    font-weight: 600;
    letter-spacing: 0.08em;
    color: #5ab6eb;
    line-height: 1;
  }
  .tag.amber {
    color: #f2b33a;
  }
  /* GPU dormant: the speed gives way to the state. */
  .zhero {
    position: absolute;
    right: 34px;
    top: 66px;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    font-size: 116px;
    font-weight: 700;
    line-height: 0.9;
    letter-spacing: 0.01em;
    color: #f2b33a;
    white-space: nowrap;
  }
  .zpaged {
    position: absolute;
    right: 34px;
    top: 278px;
    line-height: 1;
    white-space: nowrap;
  }
  .zpaged b {
    font-size: 76px;
    font-weight: 600;
  }
  .zpaged small {
    font-size: 40px;
    font-weight: 600;
    color: #afc2cf;
  }
  .zline {
    position: absolute;
    right: 34px;
    top: 362px;
    font-size: 34px;
    font-weight: 600;
    letter-spacing: 0.03em;
    color: #afc2cf;
    line-height: 1;
    white-space: nowrap;
  }
  .unit {
    position: absolute;
    right: 34px;
    top: 290px;
    font-size: 142px;
    font-weight: 600;
    line-height: 1;
  }
  .ctx {
    position: absolute;
    left: 548px;
    top: 338px;
    display: flex;
    flex-direction: column;
    line-height: 1;
  }
  .cl {
    font-size: 34px;
    font-weight: 600;
    color: #5ab6eb;
    letter-spacing: 0.06em;
  }
  .ctx b {
    font-size: 60px;
    font-weight: 600;
    margin-top: 4px;
  }
  /* "100%" would run into the tok/s unit: shift left and step down a size. */
  .ctx.wide {
    left: 516px;
  }
  .ctx.wide b {
    font-size: 52px;
    margin-top: 10px;
  }
  .stats {
    position: absolute;
    left: 548px;
    right: 34px;
    top: 330px;
    display: flex;
    justify-content: space-between;
    gap: 24px;
    line-height: 1;
  }
  .sb {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .sb:last-child {
    align-items: flex-end;
  }
  .sb b {
    font-size: 76px;
    font-weight: 600;
    margin-top: 6px;
    white-space: nowrap;
  }
  .sb b small {
    font-size: 40px;
    color: #afc2cf;
  }
  .sb b.dimb {
    color: #7c8f9e;
  }
  .note {
    position: absolute;
    left: 548px;
    right: 34px;
    top: 330px;
    font-size: 52px;
    font-weight: 600;
    color: #f4faff;
    line-height: 1.05;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Long step names ("Load diffusion weights") step down instead of truncating; still >= 40 px. */
  .note.md {
    font-size: 46px;
    top: 334px;
  }
  .note.sm {
    font-size: 41px;
    top: 338px;
  }
  .note.n2 {
    top: 392px;
    color: #afc2cf;
    font-size: 46px;
  }
  .note.wrap {
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .fstack {
    position: absolute;
    left: 512px;
    right: 34px;
    top: 228px;
    height: 236px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    overflow: hidden;
    font-weight: 600;
    line-height: 1.08;
  }
  .ft {
    font-size: 38px;
    color: #f4faff;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .fx {
    font-size: 42px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fx.ago {
    color: #afc2cf;
    font-size: 38px;
  }
  .redtxt {
    color: #ff5a36 !important;
  }
  .amber {
    color: #f2b33a;
  }
  .tier {
    position: absolute;
    right: 34px;
    top: 82px;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    font-size: 112px;
    font-weight: 700;
    line-height: 0.98;
    letter-spacing: 0.03em;
    white-space: nowrap;
  }
  .tier.one {
    font-size: 150px;
  }
  .avail {
    position: absolute;
    right: 34px;
    top: 318px;
    display: flex;
    align-items: center;
    gap: 16px;
    font-size: 52px;
    font-weight: 600;
    color: #86c3e6;
    line-height: 1;
  }
  .avail i {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: #4fd6a4;
  }
  .avail.bad {
    color: #f2b33a;
  }
  .avail.bad i {
    background: transparent;
    border: 3px solid #f2b33a;
  }
  .fitlbl {
    position: absolute;
    left: 552px;
    right: 22px;
    top: 420px;
    font-size: 34px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: #5ab6eb;
    line-height: 1;
    white-space: nowrap;
  }
  .fitlbl.amber {
    color: #f2b33a;
  }
  .gib {
    position: absolute;
    left: 552px;
    right: 22px;
    top: 476px;
    font-size: 66px;
    font-weight: 600;
    line-height: 1;
    white-space: nowrap;
    letter-spacing: 0.01em;
  }
  .gib.fit {
    top: 462px;
    font-size: 64px;
  }
  .spilltxt {
    position: absolute;
    left: 552px;
    right: 22px;
    top: 446px;
    font-size: 36px;
    font-weight: 600;
    letter-spacing: 0.04em;
    color: #ff5a36;
    line-height: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .gib.sp {
    top: 488px;
    font-size: 60px;
  }
  .last {
    position: absolute;
    left: 36px;
    top: 588px;
    max-width: 418px;
    font-size: 32px;
    font-weight: 600;
    color: #afc2cf;
    line-height: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .last .cl {
    font-size: 32px;
    margin-right: 12px;
  }
  /* Back to window: invisible (and inert) until the panel is hovered or the button is keyboard-focused.
     Sits in the empty strip between the title rule and the hero; the ::before widens the hit area. */
  .back {
    position: absolute;
    right: 34px;
    top: 70px;
    width: 60px;
    height: 30px;
    display: grid;
    place-items: center;
    padding: 0;
    color: #afc2cf;
    background: #0d1115;
    border: 1.5px solid #4e6779;
    cursor: pointer;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.18s ease;
  }
  .back::before {
    content: '';
    position: absolute;
    inset: -10px;
  }
  .back svg {
    width: 22px;
    height: 22px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linejoin: round;
  }
  .mini:hover .back,
  .back:focus-visible {
    opacity: 1;
    pointer-events: auto;
  }
  .back:hover,
  .back:focus-visible {
    color: #f4faff;
    border-color: #5ab6eb;
  }
  .back:focus-visible {
    outline: 1.5px solid #5ab6eb;
    outline-offset: 2px;
  }
  @media (prefers-reduced-motion: reduce) {
    .back {
      transition: none;
    }
  }
  .tb {
    position: absolute;
    right: 14px;
    top: 576px;
    min-width: 484px;
    max-width: 560px;
    padding: 0 16px;
    height: 50px;
    border: 1.5px solid #4e6779;
    display: grid;
    place-items: center;
    font-size: 34px;
    font-weight: 600;
    letter-spacing: 0.03em;
    white-space: nowrap;
    overflow: hidden;
  }
</style>
