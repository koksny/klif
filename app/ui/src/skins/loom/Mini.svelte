<script lang="ts">
  // Loom, mini panel (960x640, read from 1 m). A fixed 960x640 sheet scaled to fit, the same in every state
  // (only the contents change):
  //   header      KLIF · status · clock or version · Launch / Cancel / Stop / Restart (one width)
  //   system strip  one tab per System with its status dot, scrolls sideways when it overflows
  //   model line  the running model, or the selected System's
  //   visual      (left, 55% of the width) the same 3D scene as the full window on a slow orbit, framed so the
  //               tower fills it top to bottom; two corner captions (the shape; the KV cache or the latents)
  //   hero        (right) label, one figure + unit, a status line; rows: two fixed facts per kind
  //               (LLM: context, prefill; image: last image, images)
  //   VRAM strip  8 segments at true scale: VRAM / FITS (idle) / RESIDENT (dormant) / SPILL
  // A fault covers the hero and the rows. Archivo Expanded caps for labels and the figure, JetBrains Mono for data.
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtCtx, fmtGiB, fmtInt, fmtPct, fmtTps } from '../../lib/model/format';
  import { EXTERNAL_TITLE, KIND_LABEL, canStop, doLaunch, idleState, isPendingLaunch, launchCtl, selectedSystem, systemLabel } from '../../lib/model/systems';
  import { strip as scrollStrip } from '../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../lib/shell/SystemTabs/tabs';
  import Stage from './Stage.svelte';
  import Lock from './Lock.svelte';
  import Chip from './Chip.svelte';
  import { shapeOf } from './shape';
  import { held, useSleep } from './state.svelte';
  import { fitOf, fmtAgo, fmtDur, fmtEta, shortTier } from './text';

  let { vm, actions }: { vm: ViewModel; actions?: Actions } = $props();

  const SW = 960;
  const SH = 640;
  let w = $state(SW);
  let h = $state(SH);
  const u = $derived(Math.max(0.1, Math.min(w / SW, h / SH)));
  const ox = $derived((w - SW * u) / 2);
  const oy = $derived((h - SH * u) / 2);

  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const slot = $derived(selectedSystem(vm) ?? undefined);
  const tabs = $derived(tabsFor(vm));
  const kind = $derived(slot?.kind ?? 'llm');
  const model = $derived(s?.model ?? slot?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);
  const shape = $derived(shapeOf(vm));
  const loading = $derived(phase === 'starting' || phase === 'loading');
  const faulted = $derived(phase === 'fault');
  const busy = $derived(loading || phase === 'stopping');
  const sleep = useSleep(() => vm);
  const dz = $derived(phase === 'live' ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);
  const canLeave = $derived(!!(vm.host?.panel?.available || vm.host?.panel?.active));

  const tps = held(() => llm?.decodeTps ?? 0);
  const prefilling = $derived(phase === 'live' && !!llm?.prefill && llm.activity === 'prefill');
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || 0);
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0);

  const statusTone = $derived(faulted ? 'red' : dz || loading ? 'amb' : s ? 'live' : 'off');
  const statusText = $derived(dz ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : s ? phase.toUpperCase() : 'IDLE');
  const clock = $derived.by(() => {
    if (!s) return '';
    const t = Math.max(0, Math.floor(loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS));
    return [Math.floor(t / 3600), Math.floor((t % 3600) / 60), t % 60].map((v) => String(v).padStart(2, '0')).join(':');
  });

  /** name · quant · context or size · mode */
  const modelLine = $derived.by(() => {
    if (!model) return '';
    const parts = [model.name, model.quant];
    if (model.ctxTokens) parts.push(fmtCtx(model.ctxTokens));
    else if (model.imageSize) parts.push(model.imageSize);
    if (model.mode) parts.push(model.mode);
    return parts.filter(Boolean).join(' · ').toLowerCase();
  });

  // The visual's corner captions: the shape (top left) and what the light ends in (bottom right).
  const shapeCap = $derived.by(() => {
    if (shape.kind === 'dit') return shape.known ? `${shape.layers} blocks · dit` : 'dit';
    if (!shape.known) return 'shape unknown';
    if (shape.kind === 'moe') return `${shape.layers} layers · ${shape.active}${shape.shared ? `+${shape.shared}` : ''} / ${shape.experts}`;
    return `${shape.layers} layers · dense`;
  });
  const sideCap = $derived.by(() => {
    if (shape.kind === 'dit') return img?.activity === 'generating' && img.steps > 0 ? `latents · step ${img.step} / ${img.steps}` : 'latents';
    return llm && !faulted ? `kv cache · ${fmtPct(ctxFrac)}` : 'kv cache';
  });

  // ---- hero ------------------------------------------------------------------------------------------------
  type Hero = { label: string; tone: '' | 'amb' | 'red'; fig: string; unit: string; dim: boolean; line: string; lineTone: '' | 'amb' };
  const idleWhy = $derived(idleState(slot));
  const hero = $derived.by<Hero>(() => {
    const base = { tone: '' as const, dim: false, lineTone: '' as const };
    if (!s) {
      const last = vm.lastSession;
      const lastText = last
        ? `last ${shortTier(systemLabel(vm, last.system) || last.model.name).toLowerCase()} · ${last.ended === 'fault' ? 'fault' : 'stopped'} ${fmtAgo(last.endedAgoS)}`
        : 'nothing running';
      return {
        ...base,
        label: 'STANDBY',
        fig: '—',
        unit: kind === 'image' ? 'steps' : kind === 'llm' ? 'tok/s' : 'requests',
        dim: true,
        line: idleWhy.warn || slot?.status === 'not-set' ? (slot?.reason ?? idleWhy.text).toLowerCase() : lastText,
        lineTone: idleWhy.warn ? 'amb' : '',
      };
    }
    if (dz) {
      if (waking) return { ...base, label: 'GPU WAKING', tone: 'amb', fig: String(Math.round(dz.restoredFrac * 100)), unit: '%', line: `${fmtGiB(dz.pagedOutGiB)} GiB still in system ram` };
      return {
        ...base,
        label: `GPU ASLEEP${dz.powerState ? ` · ${dz.powerState}` : ''}`,
        tone: 'amb',
        fig: llm ? fmtTps(tps.current) : '—',
        unit: llm ? 'tok/s' : '',
        dim: true,
        line: `${fmtGiB(dz.pagedOutGiB)} GiB paged out · ${fmtDur(dz.sinceS)}`,
      };
    }
    if (loading) {
      const st = s.loading?.steps.find((x) => x.state === 'active');
      return { ...base, label: 'LOAD', tone: 'amb', fig: String(Math.floor((s.loading?.fraction ?? 0) * 100)), unit: '%', line: st ? `${st.label.toLowerCase()}${st.detail ? ` · ${st.detail}` : ''}` : 'starting' };
    }
    if (phase === 'stopping') return { ...base, label: 'STOPPING', fig: '—', unit: '', dim: true, line: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB` };
    if (prefilling && llm?.prefill) {
      const p = llm.prefill;
      return { ...base, label: 'PREFILL', fig: String(p.tokens > 0 ? Math.floor((p.doneTokens / p.tokens) * 100) : 0), unit: '%', line: `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok` };
    }
    if (llm) return { ...base, label: 'DECODE', fig: fmtTps(tps.current), unit: 'tok/s', dim: llm.activity !== 'decode', line: `${fmtInt(llm.generatedTokens)} tok generated${llm.activity === 'idle' ? ' · idle' : ''}` };
    if (gen) {
      const inFlight = gen.requestsInFlight ?? 0;
      return {
        ...base,
        label: KIND_LABEL[kind].toUpperCase(),
        fig: gen.requestsTotal !== undefined ? fmtInt(gen.requestsTotal) : '—',
        unit: 'requests',
        dim: inFlight === 0,
        line: inFlight > 0 ? `${inFlight} in flight` : gen.lastActivityS !== undefined ? `last activity ${fmtDur(gen.lastActivityS)} ago` : 'waiting for a request',
      };
    }
    if (img) {
      if (img.activity === 'generating' && img.steps > 0)
        return { ...base, label: `DIFFUSION${img.edit ? ' · EDIT' : ''}`, fig: String(img.step), unit: `/ ${img.steps}`, line: `${img.sPerIt.toFixed(2)} s/it · ${img.width}x${img.height}` };
      return { ...base, label: 'DIFFUSION', fig: '—', unit: '', dim: true, line: 'waiting for the next job' };
    }
    return { ...base, label: kind === 'image' ? 'DIFFUSION' : 'DECODE', fig: '—', unit: '', dim: true, line: 'waiting for data' };
  });

  // ---- rows: two fixed facts per kind ------------------------------------------------------------------------
  type Row = { k: string; v: string; sub: string; red?: boolean };
  const rows = $derived.by<Row[]>(() => {
    if (kind === 'image') {
      const j = img && img.recent.length ? img.recent[img.recent.length - 1] : null;
      return [
        { k: 'LAST IMAGE', v: j ? `${j.seconds.toFixed(1)} s` : '—', sub: j ? `${j.width}x${j.height}${j.edit ? ' · edit' : ''}` : '' },
        { k: 'IMAGES', v: img ? fmtInt(img.imagesThisSession) : '—', sub: img ? 'this session' : '' },
      ];
    }
    if (kind !== 'llm') {
      return [
        { k: 'IN FLIGHT', v: gen ? fmtInt(gen.requestsInFlight ?? 0) : '—', sub: '' },
        { k: 'LAST REQ', v: gen?.lastActivityS !== undefined ? fmtDur(gen.lastActivityS) : '—', sub: gen?.lastActivityS !== undefined ? 'ago' : '' },
      ];
    }
    const pf = llm?.prefill;
    return [
      { k: 'CONTEXT', v: llm ? fmtPct(ctxFrac) : '—', sub: ctxTotal ? `of ${fmtCtx(ctxTotal)}` : '', red: ctxFrac >= 0.95 },
      prefilling && pf ? { k: 'PREFILL', v: fmtInt(pf.tps), sub: `tok/s · ${fmtEta(pf.etaS)}` } : { k: 'PREFILL', v: pf ? fmtInt(pf.tps) : '—', sub: pf ? 'tok/s' : '' },
    ];
  });

  // ---- fault -------------------------------------------------------------------------------------------------
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const exitText = $derived(fault ? (fault.exitCode === undefined ? 'no exit code' : `exit ${fault.exitCodeHex ?? fault.exitCode}`) : '');

  // ---- VRAM strip: 8 segments at true scale --------------------------------------------------------------------
  const GW = 600;
  const GH = 16;
  const total = $derived(Math.max(0.01, vm.vram.totalGiB));
  const fx = (gib: number) => (Math.min(total, Math.max(0, gib)) / total) * GW;
  const usedC = $derived(Math.min(total, Math.max(0, vm.vram.usedGiB)));
  const segW = GW / 8;
  const pagedEnd = $derived(dz ? Math.min(total, usedC + dz.pagedOutGiB) : usedC);
  const segs = $derived(
    Array.from({ length: 8 }, (_, i) => ({
      x: i * segW,
      f: Math.min(1, Math.max(0, usedC / (total / 8) - i)),
      p: dz ? Math.min(1, Math.max(0, pagedEnd / (total / 8) - i)) : 0,
    })),
  );
  const spillGiB = $derived(vm.vram.spillMiB / 1024);
  const fit = $derived(!s ? fitOf(vm.vram, slot) : null);
  const ghost = $derived(fit ? { x0: fx(fit.base), x1: fx(fit.top), over: fit.spare < 0 } : null);
  const vramText = $derived.by(() => {
    if (fit) return fit.spare >= 0 ? { k: 'FITS', v: `${fmtGiB(fit.spare)} GiB spare`, tone: '' } : { k: 'OVER', v: `by ${fmtGiB(-fit.spare)} GiB`, tone: 'red' };
    if (spillGiB > 0) return { k: 'SPILL', v: `${fmtGiB(spillGiB)} GiB`, tone: 'red' };
    if (dz) return { k: 'RESIDENT', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)}`, tone: 'amb' };
    return { k: 'VRAM', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)}`, tone: '' };
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
    if (pick?.external) return { kind: 'ext', text: 'EXTERNAL', title: EXTERNAL_TITLE, disabled: true, run: () => {} };
    if (!ph && isPendingLaunch(pick)) {
      return { kind: 'stop', text: '■ CANCEL', title: pick?.reason ?? 'Cancel the launch', disabled: !canStop(pick), run: () => void actions?.stop(pick?.id) };
    }
    if (!ph) {
      const ctl = launchCtl(vm, pick, { short: true });
      const word = shortTier(pick?.label ?? '');
      return {
        kind: 'go',
        text: ctl.stopOthers ? `▶ ${ctl.text.toUpperCase()}` : `▶ LAUNCH ${word}`,
        title: ctl.enabled ? (ctl.stopOthers ? ctl.text : `Launch ${pick?.label ?? ''}`) : `${pick?.label ?? ''} cannot launch: ${ctl.blocked}`,
        disabled: !ctl.enabled,
        run: () => {
          if (actions) doLaunch(actions, pick, ctl);
        },
      };
    }
    if (ph === 'fault') return { kind: 'hot', text: '↻ RESTART', title: 'Restart the System that failed', disabled: !mine, run: () => void actions?.restart(pick?.id) };
    if (ph === 'stopping') return { kind: 'stop', text: '■ STOPPING', title: 'Stopping', disabled: true, run: () => {} };
    return {
      kind: 'stop',
      text: ph === 'live' ? '■ STOP' : '■ CANCEL',
      title: !canStop(pick) ? 'External server: it runs where it was started' : ph === 'live' ? 'Stop the server' : 'Cancel the launch',
      disabled: !canStop(pick),
      run: () => void actions?.stop(pick?.id),
    };
  });
</script>

<div class="mini" bind:clientWidth={w} bind:clientHeight={h}>
  <div class="sheet" style="transform: translate({ox}px, {oy}px) scale({u})">
    <!-- Header -->
    <div class="hdr">
      <span class="klif">KLIF</span>
      <span class="sep"></span>
      <span class="st {statusTone}"><i></i>{statusText}</span>
      <span class="grow"></span>
      {#if s}<span class="clock">{clock}</span>{:else if vm.host?.appVersion}<span class="clock ver">v{vm.host.appVersion}</span>{/if}
      <button class="act {act.kind}" type="button" onclick={act.run} disabled={act.disabled || !actions} title={act.title}>{act.text}</button>
      {#if canLeave}
        <button class="back" type="button" onclick={() => actions?.togglePanel()} title="Leave panel mode" aria-label="Leave panel mode">
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.5 1.5h7v7M1.5 3.5h7v7h-7z" /></svg>
        </button>
      {/if}
    </div>

    <!-- Tier strip -->
    <div class="tiers" use:scrollStrip={vm.selected}>
      {#each tabs as t (t.id)}
        <button type="button" class="tier" class:sel={t.id === vm.selected} class:na={t.system.status === 'invalid' || t.system.status === 'unreachable'} class:fault={t.status === 'fault'} data-sel={t.id === vm.selected} onclick={() => void actions?.select(t.id)}>
          <TabLabel tab={t} short dotSize={7} />
        </button>
      {/each}
    </div>

    <!-- Model line -->
    <div class="mline panel" class:dim={!s}><span class="pr">&gt;</span><span class="mt">{modelLine}</span></div>

    <!-- Visual: the 3D scene on a slow orbit -->
    <div class="scene panel">
      <Stage {vm} {shape} variant="mini" pxScale={u} />
      <div class="crt"></div>
      <div class="cap l" class:unk={!shape.known}><Chip text={shapeCap} /></div>
      <div class="cap r"><Chip text={sideCap} /></div>
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
        <div class="fig" class:dim={hero.dim}><b><Lock value={hero.fig} /></b>{#if hero.unit}<span class="unit">{hero.unit}</span>{/if}</div>
        <div class="line {hero.lineTone}">{hero.line}</div>
      </div>

      <!-- Rows -->
      <div class="rows panel" class:dim={!s}>
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
      <svg class="gauge" viewBox="-2 -2 {GW + 4} {GH + 4}" width={GW + 4} height={GH + 4} aria-hidden="true">
        <defs>
          <pattern id="loom-hatch" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <line x1="0" y1="0" x2="0" y2="6" stroke="rgba(255,176,0,0.55)" stroke-width="1.6" />
          </pattern>
        </defs>
        {#each segs as sg, i (i)}
          <rect x={sg.x + 1.5} y="0" width={segW - 3} height={GH} class="seg" />
          {#if sg.f > 0}<rect x={sg.x + 1.5} y="0" width={(segW - 3) * sg.f} height={GH} class="segfill" class:red={spillGiB > 0} class:sleep={!!dz} />{/if}
          {#if sg.p - sg.f > 0.004}<rect x={sg.x + 1.5 + (segW - 3) * sg.f} y="0" width={(segW - 3) * (sg.p - sg.f)} height={GH} fill="url(#loom-hatch)" class="paged" />{/if}
        {/each}
        {#if ghost}
          <rect x={ghost.x0} y="2" width={Math.max(0, ghost.x1 - ghost.x0)} height={GH - 4} fill="url(#loom-hatch)" class="ghost" class:over={ghost.over} />
        {/if}
        <line x1={GW} x2={GW} y1="-2" y2={GH + 2} class="limit" />
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
    background: #050302;
    user-select: none;
  }
  .sheet {
    --panel: #0c0703;
    --rule: #2e1a06;
    --rule2: #4a2a0a;
    --amber: #ffb000;
    --hot: #ffd98a;
    --white: #fff4de;
    --ember: #ff6a1a;
    --muted: #8a5a1c;
    --red: #ff3b30;
    /* Archivo Expanded (width axis at 125%) for labels and the figure; JetBrains Mono for data. */
    --disp: 'Archivo Variable', 'Archivo', sans-serif;
    --mono: 'JetBrains Mono Variable', Consolas, monospace;
    position: absolute;
    left: 0;
    top: 0;
    width: 960px;
    height: 640px;
    transform-origin: 0 0;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    color: #e8a83c;
    text-shadow: 0 0 6px rgba(255, 150, 0, 0.3);
  }
  .panel {
    position: absolute;
    background: var(--panel);
    border: 1px solid var(--rule);
    box-sizing: border-box;
  }
  .red {
    color: var(--red);
  }
  .amb {
    color: var(--ember);
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    text-shadow: inherit;
  }
  .klif,
  .st,
  .act,
  .tier,
  .lbl,
  .fig,
  .row .k,
  .vram .k {
    font-family: var(--disp);
    font-stretch: 125%;
  }
  .st,
  .act,
  .tier,
  .lbl,
  .row .k,
  .vram .k {
    font-weight: 600;
    text-transform: uppercase;
  }

  /* Header: 0..46 */
  .hdr {
    position: absolute;
    left: 16px;
    right: 16px;
    top: 0;
    height: 46px;
    display: flex;
    align-items: center;
    gap: 16px;
    white-space: nowrap;
    border-bottom: 1px solid var(--rule2);
  }
  .klif {
    font-size: 20px;
    font-weight: 600;
    letter-spacing: 0.28em;
    line-height: 1;
    color: var(--white);
  }
  .sep {
    width: 1px;
    height: 20px;
    background: var(--rule2);
    flex: none;
  }
  .st {
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: 13px;
    letter-spacing: 0.18em;
    color: var(--muted);
  }
  .st i {
    width: 8px;
    height: 8px;
    background: currentColor;
    box-shadow: 0 0 8px currentColor;
  }
  .st.live {
    color: var(--amber);
  }
  .st.amb {
    color: var(--ember);
  }
  .st.red {
    color: var(--red);
  }
  .grow {
    flex: 1 1 auto;
  }
  .clock {
    font-size: 15px;
    letter-spacing: 0.04em;
    color: var(--white);
  }
  .clock.ver {
    font-size: 13px;
    font-weight: 300;
    color: var(--muted);
  }
  /* Launch / Cancel / Stop / Restart: one width, so nothing in the header moves. */
  .act {
    flex: none;
    width: 184px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--amber);
    background: var(--amber);
    color: #1a0d00;
    text-shadow: none;
    font-size: 13px;
    letter-spacing: 0.14em;
    white-space: nowrap;
    overflow: hidden;
    cursor: pointer;
  }
  .act.stop {
    background: transparent;
    border-color: var(--ember);
    color: var(--ember);
  }
  /* An external server: a quiet note in the control's place. */
  .act.ext,
  .act.ext:disabled {
    background: transparent;
    border-color: var(--rule2);
    color: var(--muted);
    opacity: 1;
    box-shadow: none;
  }
  .act.hot {
    background: var(--red);
    border-color: var(--red);
    color: #1a0300;
  }
  .act:disabled {
    cursor: default;
    opacity: 0.45;
  }
  .act.go:disabled {
    opacity: 1;
    background: #1a0e03;
    border-color: var(--rule2);
    color: var(--muted);
  }
  /* Leave panel mode: only while the pointer is over the panel or it has focus. */
  .back {
    width: 34px;
    height: 28px;
    display: grid;
    place-items: center;
    border: 1px solid var(--rule2);
    color: #d7962e;
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
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
  }

  /* Tier strip: 54..88 */
  .tiers {
    position: absolute;
    left: 16px;
    right: 16px;
    top: 54px;
    height: 34px;
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: minmax(110px, 1fr);
    gap: 8px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tier {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 14px;
    border: 1px solid var(--rule);
    background: var(--panel);
    font-size: 13px;
    letter-spacing: 0.18em;
    color: var(--hot);
    white-space: nowrap;
    overflow: hidden;
    text-align: left;
    cursor: pointer;
  }
  .tier:not(.sel):hover {
    border-color: var(--rule2);
  }
  .tier.sel {
    border-color: var(--amber);
    background: linear-gradient(180deg, #2a1604, #160b02);
    color: var(--white);
  }
  .tier.na {
    color: var(--muted);
  }
  .tier.fault {
    border-color: var(--red);
    background: linear-gradient(180deg, #2a0a04, #160402);
  }

  /* Model line: 96..124 */
  .mline {
    left: 16px;
    right: 16px;
    top: 96px;
    height: 28px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px;
    font-size: 13px;
    font-weight: 300;
    color: var(--hot);
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim .mt {
    color: #c08a3c;
  }
  .mt {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pr {
    color: var(--ember);
  }

  /* Visual: 132..584, left, 55% of the width */
  .scene {
    left: 16px;
    top: 132px;
    width: 528px;
    height: 452px;
    overflow: hidden;
    background: #050302;
  }
  .crt {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      repeating-linear-gradient(0deg, rgba(0, 0, 0, 0.25) 0 1px, transparent 1px 3px),
      radial-gradient(ellipse at 50% 50%, transparent 60%, rgba(0, 0, 0, 0.7) 100%);
  }
  .cap {
    position: absolute;
    padding: 2px 7px 2px 6px;
    border-left: 1px solid rgba(255, 176, 0, 0.75);
    background: rgba(5, 3, 2, 0.5);
    font-size: 11px;
    color: var(--hot);
    white-space: nowrap;
    pointer-events: none;
  }
  .cap.l {
    left: 12px;
    top: 12px;
  }
  .cap.r {
    right: 12px;
    bottom: 12px;
    color: var(--white);
  }
  .cap.unk {
    color: #c08a3c;
    border-left-style: dashed;
  }

  /* Hero: 132..392, right */
  .hero {
    left: 556px;
    right: 16px;
    top: 132px;
    height: 260px;
    padding: 16px 18px 0;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
  }
  .lbl {
    font-size: 12.5px;
    letter-spacing: 0.2em;
    color: var(--amber);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lbl.amb {
    color: var(--ember);
  }
  .lbl.red {
    color: var(--red);
  }
  .fig {
    display: flex;
    align-items: baseline;
    gap: 12px;
    align-self: center;
    white-space: nowrap;
    overflow: hidden;
    line-height: 1;
  }
  .fig b {
    font-weight: 300;
    font-size: 62px;
    letter-spacing: -0.01em;
    font-variant-numeric: tabular-nums;
    color: var(--white);
    text-shadow:
      0 0 12px rgba(255, 176, 0, 0.55),
      0 0 32px rgba(255, 106, 26, 0.3);
  }
  .unit {
    font-size: 18px;
    font-weight: 400;
    letter-spacing: 0.06em;
    color: var(--amber);
  }
  .fig.dim b {
    color: #6b4210;
    text-shadow: none;
  }
  .fig.dim .unit {
    color: #8a5a1c;
  }
  .line {
    height: 40px;
    display: flex;
    align-items: center;
    border-top: 1px solid var(--rule);
    margin: 0 -18px;
    padding: 0 18px;
    font-size: 13px;
    font-weight: 300;
    color: var(--hot);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .line.amb {
    color: var(--ember);
  }

  /* Rows: 400..584, right */
  .rows {
    left: 556px;
    right: 16px;
    top: 400px;
    height: 184px;
    display: grid;
    grid-template-rows: repeat(2, minmax(0, 1fr));
  }
  .row {
    display: grid;
    grid-template-columns: 136px minmax(0, 1fr);
    align-items: center;
    min-height: 0;
  }
  .row + .row {
    border-top: 1px solid var(--rule);
  }
  .row .k {
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: 18px;
    border-right: 1px solid var(--rule);
    font-size: 12.5px;
    letter-spacing: 0.18em;
    color: var(--ember);
    white-space: nowrap;
  }
  .row .v {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 0 16px;
    white-space: nowrap;
    overflow: hidden;
  }
  .row .v b {
    font-size: 17px;
    font-weight: 400;
    color: var(--white);
  }
  .row .v b.red {
    color: var(--red);
  }
  .row .v small {
    font-size: 13px;
    font-weight: 300;
    color: var(--hot);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v b {
    color: #6b4210;
  }

  /* Fault: the hero and the rows together */
  .fpanel {
    left: 556px;
    right: 16px;
    top: 132px;
    height: 452px;
    padding: 16px 18px;
    border-color: rgba(255, 59, 48, 0.8);
    background: linear-gradient(180deg, #1a0604, var(--panel) 70%);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    row-gap: 14px;
  }
  .ftitle {
    font-family: var(--disp);
    font-stretch: 112%;
    font-size: 17px;
    font-weight: 400;
    line-height: 1.4;
    color: var(--white);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 9;
    line-clamp: 9;
    -webkit-box-orient: vertical;
  }
  .fexit {
    font-size: 13px;
    color: var(--red);
  }

  /* VRAM strip: 592..628 */
  .vram {
    left: 16px;
    right: 16px;
    top: 592px;
    height: 36px;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 14px;
  }
  .vram .k {
    width: 112px;
    flex: none;
    font-size: 12.5px;
    letter-spacing: 0.16em;
    color: var(--ember);
  }
  .vram .k.red,
  .vram .v.red {
    color: var(--red);
  }
  .vram .k.amb,
  .vram .v.amb {
    color: var(--amber);
  }
  .gauge {
    flex: none;
    overflow: visible;
  }
  .seg {
    fill: #140b04;
    stroke: var(--rule2);
    stroke-width: 1;
  }
  .segfill {
    fill: var(--amber);
    filter: drop-shadow(0 0 3px rgba(255, 176, 0, 0.55));
  }
  .segfill.sleep {
    fill: #8a5a1c;
    filter: none;
  }
  .segfill.red {
    fill: var(--red);
  }
  .ghost {
    stroke: var(--hot);
    stroke-width: 1.4;
    stroke-dasharray: 6 4;
  }
  .ghost.over {
    stroke: var(--red);
  }
  .paged {
    stroke: var(--amber);
    stroke-width: 1.2;
    stroke-dasharray: 5 3;
  }
  .limit {
    stroke: var(--red);
    stroke-width: 1.5;
    stroke-dasharray: 4 3;
  }
  .vram .v {
    margin-left: auto;
    font-size: 15px;
    color: var(--white);
    white-space: nowrap;
  }
</style>
