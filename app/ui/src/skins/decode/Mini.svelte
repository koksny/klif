<script lang="ts">
  // Decode, mini panel (960x640, read from 1 m). A fixed 960x640 sheet scaled to fit, the same in every state
  // (only the contents change):
  //   header      KLIF · status · clock or version · Launch / Cancel / Stop / Restart (one width)
  //   system strip  one tab per System with its status dot, scrolls sideways when it overflows
  //   model line  the running model, or the selected System's
  //   field (L)   the Decode terminal itself, larger glyphs, no VRAM margin
  //   hero (R)    label, one big figure that decodes on change, a status line; rows (R): two fixed facts per
  //               kind (LLM: context, prefill; image: last image, images)
  //   VRAM strip  a memory bar at true scale, used / total (idle: the selected tier's fit; dormant: resident)
  // A fault covers the hero and the rows. Smallest text 24 px, values 34 px and up.
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtClock, fmtCtx, fmtGiB, fmtInt, fmtPct, fmtTps } from '../../lib/model/format';
  import { EXTERNAL_TITLE, KIND_LABEL, canStop, doLaunch, idleState, isPendingLaunch, launchCtl, selectedSystem, systemLabel } from '../../lib/model/systems';
  import { strip as scrollStrip } from '../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../lib/shell/SystemTabs/tabs';
  import Field from './Field.svelte';
  import Lock from './Lock.svelte';
  import { useSleep } from './sleep.svelte';
  import { baselineOf, fmtAgo, fmtDur, fmtEta, modelShort, phaseWord, tierWord } from './text';

  let { vm, actions }: { vm: ViewModel; actions?: Actions } = $props();

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
  const slot = $derived(selectedSystem(vm) ?? undefined);
  const tabs = $derived(tabsFor(vm));
  const kind = $derived(slot?.kind ?? 'llm');
  const model = $derived(s?.model ?? slot?.model);
  const llm = $derived(s?.llm ?? null);
  const img = $derived(s?.image ?? null);
  const gen = $derived(s?.generic ?? null);
  const idle = $derived(!s);
  const loading = $derived(phase === 'starting' || phase === 'loading');
  const faulted = $derived(phase === 'fault');
  const idleWhy = $derived(idleState(slot));

  const sleep = useSleep(() => vm);
  const dz = $derived(phase === 'live' ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);

  const prefilling = $derived(!!llm?.prefill && llm.activity === 'prefill');
  const ctxTotal = $derived(llm?.context.totalTokens || model?.ctxTokens || 0);
  const ctxFrac = $derived(llm && llm.context.totalTokens > 0 ? llm.context.usedTokens / llm.context.totalTokens : 0);
  const lastJob = $derived(img && img.recent.length ? img.recent[img.recent.length - 1] : null);

  const tone = $derived(faulted ? 'red' : dz || loading ? 'amber' : s ? 'live' : 'off');
  const statusLabel = $derived(dz ? (waking ? 'GPU WAKING' : 'GPU ASLEEP') : phaseWord(phase));
  const clock = $derived(s ? fmtClock(loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS) : '');

  // ---- hero ------------------------------------------------------------------------------------------------
  type Hero = { label: string; tone: '' | 'amber' | 'red'; fig: string; unit: string; dim: boolean; line: string; lineTone: '' | 'amber' };
  const hero = $derived.by<Hero>(() => {
    const base = { tone: '' as const, dim: false, lineTone: '' as const };
    if (!s) {
      const ls = vm.lastSession;
      const blocked = idleWhy.warn || slot?.status === 'not-set';
      const line = blocked
        ? (slot?.reason ?? idleWhy.text).toLowerCase()
        : ls
          ? `last ${tierWord(systemLabel(vm, ls.system) || ls.model.name)}${ls.ended === 'fault' ? ' · fault' : ''} · ${fmtAgo(ls.endedAgoS)}`
          : 'nothing running';
      return { ...base, label: 'STANDBY', fig: '—', unit: kind === 'image' ? 'steps' : kind === 'llm' ? 'tok/s' : 'requests', dim: true, line, lineTone: idleWhy.warn ? 'amber' : '' };
    }
    if (dz) {
      if (waking) return { ...base, label: 'GPU WAKING', tone: 'amber', fig: String(Math.round(dz.restoredFrac * 100)), unit: '%', line: `${fmtGiB(dz.pagedOutGiB)} GiB still in system ram` };
      return {
        ...base,
        label: dz.powerState ? `GPU ASLEEP · ${dz.powerState}` : 'GPU ASLEEP',
        tone: 'amber',
        fig: llm ? fmtTps(llm.decodeTps) : '—',
        unit: llm ? 'tok/s' : '',
        dim: true,
        line: `${fmtGiB(dz.pagedOutGiB)} GiB paged out · ${fmtDur(dz.sinceS)}`,
      };
    }
    if (loading) {
      const st = s.loading?.steps.find((x) => x.state === 'active');
      return { ...base, label: 'LOAD', tone: 'amber', fig: String(Math.floor((s.loading?.fraction ?? 0) * 100)), unit: '%', line: st ? `${st.label}${st.detail ? ` · ${st.detail}` : ''}`.toLowerCase() : phase };
    }
    if (phase === 'stopping') return { ...base, label: 'STOPPING', fig: '—', unit: '', dim: true, line: `releasing ${fmtGiB(vm.vram.usedGiB)} GiB` };
    if (llm) {
      if (prefilling && llm.prefill) {
        const p = llm.prefill;
        return { ...base, label: 'PREFILL', fig: String(p.tokens > 0 ? Math.floor((p.doneTokens / p.tokens) * 100) : 0), unit: '%', line: `${fmtInt(p.doneTokens)} / ${fmtInt(p.tokens)} tok` };
      }
      return { ...base, label: 'DECODE', fig: fmtTps(llm.decodeTps), unit: 'tok/s', dim: llm.activity !== 'decode', line: `${fmtInt(llm.generatedTokens)} tok generated${llm.activity === 'idle' ? ' · idle' : ''}` };
    }
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
      if (img.activity === 'generating' && img.steps > 0) return { ...base, label: `DENOISE${img.edit ? ' · EDIT' : ''}`, fig: String(img.step), unit: `/ ${img.steps}`, line: `${img.sPerIt.toFixed(2)} s/it · ${img.width}x${img.height}` };
      return { ...base, label: 'DENOISE', fig: '—', unit: `/ ${img.steps}`, dim: true, line: 'waiting for the next job' };
    }
    return { ...base, label: 'LIVE', fig: '—', unit: '', dim: true, line: 'waiting for data' };
  });

  // ---- rows: two fixed facts per kind ---------------------------------------------------------------------------
  type Row = { k: string; v: string; sub: string; red?: boolean };
  const rows = $derived.by<Row[]>(() => {
    if (kind === 'image') {
      return [
        { k: 'LAST IMAGE', v: lastJob ? `${lastJob.seconds.toFixed(1)} s` : '—', sub: lastJob ? `${lastJob.width}x${lastJob.height}${lastJob.edit ? ' · edit' : ''}` : '' },
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
      prefilling && pf ? { k: 'PREFILL', v: fmtInt(pf.tps), sub: `tok/s · ${fmtEta(pf.etaS)} left` } : { k: 'PREFILL', v: pf ? fmtInt(pf.tps) : '—', sub: pf ? 'tok/s' : '' },
    ];
  });

  // ---- fault -----------------------------------------------------------------------------------------------------
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const exitText = $derived(fault ? (fault.exitCode === undefined ? 'no exit code' : `exit ${fault.exitCodeHex ?? fault.exitCode}`) : '');

  // ---- VRAM strip: a memory bar at true scale (layers, paged out, idle fit preview) ------------------------------
  const CELLS = 30;
  const LAYER_CSS: Record<string, string> = {
    weights: '#2a6bf2',
    kv: '#5cc8ff',
    buffers: '#9ee0ff',
    draft: '#8c99ff',
    projector: '#73b3f2',
    other: '#33507a',
  };
  const total = $derived(Math.max(0.01, vm.vram.totalGiB));
  const spillGiB = $derived(vm.vram.spillMiB / 1024);
  const lowFree = $derived(!!s && vm.vram.totalGiB - vm.vram.usedGiB < vm.vram.warnBelowGiB);
  const base = $derived(Math.max(baselineOf(vm.vram), vm.vram.usedGiB));
  const expected = $derived(idle && slot?.expectedVram?.length && !slot.external ? slot.expectedVram.reduce((a, l) => a + l.gib, 0) : null);
  const spare = $derived(expected !== null ? vm.vram.totalGiB - base - expected : 0);
  type Seg = { a: number; b: number; cls: string; color?: string };
  const segs = $derived.by<Seg[]>(() => {
    const out: Seg[] = [];
    let at = 0;
    if (dz) {
      out.push({ a: 0, b: vm.vram.usedGiB, cls: 'res' });
      out.push({ a: vm.vram.usedGiB, b: vm.vram.usedGiB + dz.pagedOutGiB, cls: 'paged' });
      return out;
    }
    const layers = [...vm.vram.layers].sort((a, b) => (a.id === 'other' ? -1 : b.id === 'other' ? 1 : 0));
    for (const l of layers) {
      out.push({ a: at, b: at + l.gib, cls: spillGiB > 0 ? 'spill' : 'used', color: LAYER_CSS[l.id] });
      at += l.gib;
    }
    if (expected !== null) out.push({ a: base, b: base + expected, cls: spare < 0 ? 'ghost over' : 'ghost' });
    return out;
  });
  const cells = $derived.by(() => {
    const per = total / CELLS;
    return Array.from({ length: CELLS }, (_, i) => {
      const c0 = i * per;
      const c1 = c0 + per;
      const parts = segs
        .filter((sg) => sg.b > c0 && sg.a < c1)
        .map((sg) => ({ cls: sg.cls, color: sg.color, l: ((Math.max(sg.a, c0) - c0) / per) * 100, w: ((Math.min(sg.b, c1) - Math.max(sg.a, c0)) / per) * 100 }));
      return { parts, free: parts.every((p) => p.cls.startsWith('ghost')) };
    });
  });
  const vramText = $derived.by(() => {
    if (expected !== null) return spare >= 0 ? { k: 'FITS', v: `${fmtGiB(spare)} GiB spare`, tone: '' } : { k: 'OVER', v: `by ${fmtGiB(-spare)} GiB`, tone: 'red' };
    if (spillGiB > 0) return { k: 'SPILL', v: `${fmtGiB(spillGiB)} GiB`, tone: 'red' };
    if (dz) return { k: 'RESIDENT', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)}`, tone: 'amber' };
    return { k: 'VRAM', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)}`, tone: lowFree ? 'amber' : '' };
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
    if (pick?.external) return { kind: 'ext', text: 'EXTERNAL SERVER', title: EXTERNAL_TITLE, disabled: true, run: () => {} };
    if (!ph && isPendingLaunch(pick)) {
      return { kind: 'stop', text: '■ CANCEL', title: pick?.reason ?? 'Cancel the launch', disabled: !canStop(pick), run: () => void actions?.stop(pick?.id) };
    }
    if (!ph) {
      const ctl = launchCtl(vm, pick, { short: true });
      const word = tierWord(pick?.label ?? '');
      return {
        kind: 'go',
        text: ctl.stopOthers ? `▶ ${ctl.text.toUpperCase()}` : `▶ LAUNCH ${word.toUpperCase()}`,
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
      <span class="st {tone}"><i class:pulse={waking || loading}></i>{statusLabel}</span>
      <span class="grow"></span>
      {#if s}<span class="clock">{clock}</span>{:else if vm.host?.appVersion}<span class="clock ver">v{vm.host.appVersion}</span>{/if}
      <button class="act {act.kind}" type="button" onclick={act.run} disabled={act.disabled || !actions} title={act.title}>{act.text}</button>
      {#if canLeave}
        <button class="back" onclick={() => actions?.togglePanel?.()} title="Leave panel mode" aria-label="Leave panel mode">
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.5 1.5h7v7M1.5 3.5h7v7h-7z" /></svg>
        </button>
      {/if}
    </div>

    <!-- System strip -->
    <div class="tiers" use:scrollStrip={vm.selected}>
      {#each tabs as t, i (t.id)}
        {@const sel = t.id === vm.selected}
        {@const na = t.system.status === 'invalid' || t.system.status === 'unreachable'}
        <button type="button" class="tier" class:sel class:na class:fault={t.status === 'fault'} data-sel={sel} onclick={() => void actions?.select(t.id)}>
          <span class="tk">[{i + 1}]</span><TabLabel tab={t} short dotSize={12} />
        </button>
      {/each}
    </div>

    <!-- Model line -->
    <div class="mline panel" class:dim={idle}><span class="pr">&gt;</span><span class="mt">{modelShort(model)}</span></div>

    <!-- The terminal -->
    <div class="fbox panel">
      <Field {vm} fontPx={22} pxScale={u} compact maxFps={30} label="Context window as a terminal: noise is free context, dim text is context in use" />
      <div class="scan"></div>
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
        <div class="fig {hero.tone}" class:dim={hero.dim}><b><Lock value={hero.fig} /></b>{#if hero.unit}<span class="unit">{hero.unit}</span>{/if}</div>
        <div class="line {hero.lineTone}">{hero.line}</div>
      </div>

      <!-- Rows -->
      <div class="rows panel" class:dim={idle}>
        {#each rows as r (r.k)}
          <div class="row">
            <span class="k">{r.k}</span>
            <span class="v"><b class:red={r.red}><Lock value={r.v} ms={300} /></b>{#if r.sub}<small>{r.sub}</small>{/if}</span>
          </div>
        {/each}
      </div>
    {/if}

    <!-- VRAM strip -->
    <div class="vram panel">
      <span class="k {vramText.tone}">{vramText.k}</span>
      <div class="bar" aria-hidden="true">
        {#each cells as c, i (i)}
          <i class="c" class:free={c.free}>
            {#each c.parts as p, j (j)}<b class={p.cls} style="left:{p.l}%;width:{p.w}%;{p.color ? `--c:${p.color}` : ''}"></b>{/each}
          </i>
        {/each}
        <span class="limit"></span>
      </div>
      <span class="v {vramText.tone}">{vramText.v}</span>
    </div>
  </div>
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: #010409;
    user-select: none;
  }
  .sheet {
    position: absolute;
    left: 0;
    top: 0;
    width: 960px;
    height: 640px;
    transform-origin: 0 0;
    font-family: 'Iosevka', 'JetBrains Mono', Consolas, monospace;
    font-variant-numeric: tabular-nums;
    color: #cfe3f5;
    --rule: #0b2545;
    --rule2: #12345e;
    --abyss: #030b16;
    --stream: #2e8bff;
    --arc: #5cc8ff;
    --ice: #e9f7ff;
    --muted: #4d6f93;
    --amber: #ffb547;
    --red: #ff4d6d;
  }
  .panel {
    position: absolute;
    background: var(--abyss);
    border: 1px solid var(--rule);
    box-sizing: border-box;
  }
  .red {
    color: var(--red);
  }
  .amber,
  .amb {
    color: var(--amber);
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  /* header: 0..62 */
  .hdr {
    position: absolute;
    left: 24px;
    right: 24px;
    top: 0;
    height: 62px;
    display: flex;
    align-items: center;
    gap: 12px;
    white-space: nowrap;
    border-bottom: 1px solid var(--rule2);
  }
  .klif {
    font-size: 40px;
    font-weight: 700;
    letter-spacing: 0.16em;
    line-height: 1;
    color: var(--ice);
  }
  .sep {
    width: 1px;
    height: 32px;
    background: var(--rule2);
    flex: none;
  }
  .st {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 32px;
    font-weight: 500;
    letter-spacing: 0.12em;
    color: var(--muted);
  }
  .st i {
    width: 16px;
    height: 16px;
    background: currentColor;
    box-shadow: 0 0 12px currentColor;
  }
  .st i.pulse {
    animation: blink 1s steps(1) infinite;
  }
  .st.live {
    color: var(--arc);
  }
  .st.amber {
    color: var(--amber);
  }
  .st.red {
    color: var(--red);
  }
  .grow {
    flex: 1 1 auto;
  }
  .clock {
    font-size: 34px;
    font-weight: 300;
    letter-spacing: 0.04em;
    color: var(--ice);
  }
  .clock.ver {
    font-size: 28px;
    color: var(--muted);
  }
  /* Launch / Cancel / Stop / Restart: one width, so nothing in the header moves */
  .act {
    flex: none;
    width: 292px;
    height: 44px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0 14px;
    border: 1.5px solid var(--stream);
    background: var(--stream);
    color: #010915;
    font-family: inherit;
    font-size: 24px;
    font-weight: 700;
    letter-spacing: 0.08em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .act.stop {
    background: transparent;
    border-color: var(--ice);
    color: var(--ice);
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
    color: #12030a;
  }
  .act:disabled {
    cursor: default;
    opacity: 0.45;
  }
  .act.go:disabled {
    opacity: 1;
    background: #0a1a2e;
    border-color: var(--rule2);
    color: var(--muted);
    font-weight: 500;
  }
  /* leave panel mode: shown while the pointer is over the panel or it has focus */
  .back {
    width: 44px;
    height: 34px;
    display: grid;
    place-items: center;
    border: 1px solid var(--rule2);
    background: #020812;
    color: #7fa6cc;
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

  /* tier strip: 72..128 */
  .tiers {
    position: absolute;
    left: 24px;
    right: 24px;
    top: 72px;
    height: 56px;
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: minmax(150px, 1fr);
    gap: 10px;
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
    background: var(--abyss);
    font-family: inherit;
    font-size: 28px;
    font-weight: 500;
    letter-spacing: 0.08em;
    color: #9fbfdd;
    white-space: nowrap;
    overflow: hidden;
    text-align: left;
    cursor: pointer;
  }
  .tier .tk {
    color: var(--muted);
    font-size: 24px;
    letter-spacing: 0;
  }
  .tier.sel {
    border-color: var(--stream);
    background: linear-gradient(180deg, #06203f, #041428);
    box-shadow: 0 0 18px rgba(46, 139, 255, 0.18) inset;
    color: var(--ice);
  }
  .tier.na {
    color: var(--muted);
  }
  .tier.fault {
    border-color: var(--red);
    background: linear-gradient(180deg, #2a0812, #16040a);
    box-shadow: none;
  }

  /* model line: 138..178 */
  .mline {
    left: 24px;
    right: 24px;
    top: 138px;
    height: 40px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    font-size: 26px;
    color: #cfe3f5;
    white-space: nowrap;
    overflow: hidden;
  }
  .mline.dim {
    color: #8fb3d6;
  }
  .mline .pr {
    color: var(--stream);
  }
  .mline .mt {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* field: 188..550, left */
  .fbox {
    left: 24px;
    top: 188px;
    width: 412px;
    height: 362px;
    background: #01040a;
    overflow: hidden;
  }
  .scan {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      repeating-linear-gradient(0deg, rgba(0, 0, 0, 0.22) 0 1px, transparent 1px 3px),
      radial-gradient(ellipse at 50% 50%, transparent 60%, rgba(0, 2, 6, 0.6) 100%);
    mix-blend-mode: multiply;
  }

  /* hero: 188..398, right */
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
    letter-spacing: 0.18em;
    color: var(--arc);
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
    font-size: 112px;
    font-weight: 300;
    color: var(--ice);
    text-shadow:
      0 0 18px rgba(92, 200, 255, 0.45),
      0 0 46px rgba(46, 139, 255, 0.28);
  }
  .fig.amber b {
    color: #ffd9a1;
    text-shadow: 0 0 24px rgba(255, 181, 71, 0.35);
  }
  .fig.dim b {
    color: #29486b;
    text-shadow: none;
  }
  .fig.amber.dim b {
    color: #a3814f;
  }
  .unit {
    font-size: 36px;
    color: #7fa6cc;
    letter-spacing: 0.04em;
  }
  .fig.dim .unit {
    color: #29486b;
  }
  .fig.amber .unit {
    color: #a3814f;
  }
  .line {
    height: 46px;
    line-height: 46px;
    border-top: 1px solid var(--rule);
    margin: 0 -20px;
    padding: 0 20px;
    font-size: 26px;
    color: #8fb3d6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .line.amber {
    color: var(--amber);
  }

  /* rows: 408..550, right */
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
    border-top: 1px solid var(--rule);
  }
  .row .k {
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: 20px;
    border-right: 1px solid var(--rule);
    font-size: 24px;
    letter-spacing: 0.1em;
    color: var(--stream);
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
    font-weight: 500;
    color: var(--ice);
  }
  .row .v b.red {
    color: var(--red);
  }
  .row .v small {
    font-size: 26px;
    color: #8fb3d6;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v b {
    color: #3d5f85;
  }

  /* fault: the hero and the rows together */
  .fpanel {
    left: 448px;
    right: 24px;
    top: 188px;
    height: 362px;
    padding: 16px 20px;
    border-color: rgba(255, 77, 109, 0.85);
    background: linear-gradient(180deg, #1c0610, #10040a 60%, #030b16);
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    row-gap: 12px;
  }
  .ftitle {
    font-size: 36px;
    font-weight: 500;
    line-height: 1.2;
    color: var(--ice);
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

  /* VRAM strip: 560..620 */
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
    width: 136px;
    flex: none;
    font-size: 24px;
    letter-spacing: 0.1em;
    color: var(--stream);
  }
  .vram .k.red,
  .vram .v.red {
    color: var(--red);
  }
  .vram .k.amber,
  .vram .v.amber {
    color: var(--amber);
  }
  .bar {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    height: 30px;
    display: flex;
    gap: 3px;
    padding-right: 8px;
  }
  .c {
    position: relative;
    flex: 1 1 0;
    min-width: 0;
    background: #06121f;
  }
  .c.free::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 50%;
    width: 4px;
    height: 4px;
    margin: -2px 0 0 -2px;
    background: #1d3d66;
  }
  .c b {
    position: absolute;
    top: 0;
    bottom: 0;
  }
  .c b.used {
    background: var(--c, var(--arc));
  }
  .c b.spill {
    background: var(--red);
  }
  .c b.res {
    background: #a3814f;
  }
  .c b.paged {
    background: repeating-linear-gradient(-45deg, rgba(255, 181, 71, 0.75) 0 2px, transparent 2px 6px);
  }
  .c b.ghost {
    top: 3px;
    bottom: 3px;
    background: repeating-linear-gradient(-45deg, rgba(92, 200, 255, 0.7) 0 2px, transparent 2px 6px);
  }
  .c b.ghost.over {
    background: repeating-linear-gradient(-45deg, rgba(255, 77, 109, 0.8) 0 2px, transparent 2px 6px);
  }
  .limit {
    position: absolute;
    right: 2px;
    top: -4px;
    bottom: -4px;
    border-left: 2px dashed var(--red);
  }
  .vram .v {
    flex: none;
    font-size: 34px;
    font-weight: 500;
    color: var(--ice);
    white-space: nowrap;
  }
  @media (prefers-reduced-motion: reduce) {
    .st i.pulse {
      animation: none;
    }
  }
</style>
