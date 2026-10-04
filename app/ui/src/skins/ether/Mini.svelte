<script lang="ts">
  // Ether, mini panel (a small 960x640 status screen, read from ~1 m). The cloud fills the whole window; the HUD
  // is a fixed 960x640 sheet scaled to fit, the same in every state (only the contents change):
  //   header      KLIF · ETHER · status · clock or version · Launch / Cancel / Stop / Restart (one width)
  //   system chips one per System with its status dot (select at any time; scrolls sideways when it overflows)
  //   model line  the running model, or the selected System's
  //   hero        (right) label, one big figure + unit, a sub-line, progress, two facts
  //   VRAM        a hairline with the gradient fill (idle: the selected tier's fit)
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtClock, fmtGiB } from '../../lib/model/format';
  import { EXTERNAL_TITLE, canStop, doLaunch, isPendingLaunch, launchCtl, selectedSystem, shortLabel as tierShort } from '../../lib/model/systems';
  import { strip as scrollStrip } from '../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../lib/shell/SystemTabs/tabs';
  import Cloud from './Cloud.svelte';
  import { factsOf, heroOf, type Ctx } from './hero';
  import { held, useSleep } from './state.svelte';
  import { fitOf, modelCompact } from './text';

  let { vm, actions }: { vm: ViewModel; actions?: Actions } = $props();

  const SW = 960;
  const SH = 640;
  let w = $state(SW);
  let h = $state(SH);
  const u = $derived(Math.max(0.1, Math.min(w / SW, h / SH)));
  const ox = $derived((w - SW * u) / 2);
  const oy = $derived((h - SH * u) / 2);
  // The cloud's centre at x = 358 on the sheet (a 0.34 shift on a bare 960x640 panel), whatever the
  // letterbox: shift is in screen heights from the original composition's 0.9.
  const shift = $derived(0.9 - (ox + 358.4 * u) / Math.max(1, h));

  const s = $derived(vm.session);
  const phase = $derived(s?.phase ?? 'idle');
  const sel = $derived(selectedSystem(vm) ?? undefined);
  const tabs = $derived(tabsFor(vm));
  const kind = $derived(sel?.kind ?? 'llm');
  const model = $derived(s?.model ?? sel?.model);
  const loading = $derived(phase === 'starting' || phase === 'loading');
  const busy = $derived(loading || phase === 'stopping');
  const faulted = $derived(phase === 'fault');
  const sleep = useSleep(() => vm);
  const dz = $derived(phase === 'live' ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);
  const tps = held(() => vm.session?.llm?.decodeTps ?? 0);
  const canLeave = $derived(!!(vm.host?.panel?.available || vm.host?.panel?.active));

  const ctx = $derived<Ctx>({ vm, s, kind, sel, model, dz, waking, tps: tps.current, compact: true });
  const hero = $derived(heroOf(ctx));
  const rows = $derived(factsOf(ctx, 2));

  const statusText = $derived(dz ? (waking ? 'GPU waking' : 'GPU asleep') : s ? phase : 'idle');
  const statusTone = $derived(faulted ? 'red' : dz ? (waking ? 'sleep pulse' : 'sleep') : busy ? 'live pulse' : s ? 'live' : 'off');
  const clock = $derived(s ? fmtClock(loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS) : vm.host?.appVersion ? `v${vm.host.appVersion}` : '');

  // VRAM hairline
  const vtotal = $derived(Math.max(0.01, vm.vram.totalGiB));
  const pct = (gib: number) => (Math.min(vtotal, Math.max(0, gib)) / vtotal) * 100;
  const fit = $derived(!s ? fitOf(vm.vram, sel) : null);
  const spill = $derived(vm.vram.spillMiB / 1024);
  const lowFree = $derived(!!s && vm.vram.totalGiB - vm.vram.usedGiB < vm.vram.warnBelowGiB);
  const vramNote = $derived.by(() => {
    if (fit) return fit.spare >= 0 ? { t: `fits · ${fmtGiB(fit.spare)} spare`, tone: '' } : { t: `over by ${fmtGiB(-fit.spare)}`, tone: 'red' };
    if (spill > 0) return { t: `spill ${fmtGiB(spill)}`, tone: 'red' };
    if (dz) return { t: `${fmtGiB(dz.pagedOutGiB)} paged out`, tone: '' };
    if (lowFree) return { t: `${fmtGiB(vm.vram.totalGiB - vm.vram.usedGiB)} free`, tone: 'warn' };
    return { t: '', tone: '' };
  });

  // The panel's one control, in the header: Launch the selected System (idle; with conflicts it reads
  // "STOP S1 & LAUNCH" and stops them first), Cancel (loading), Stop (live), Restart (fault). Systems are picked
  // on the strip at any time: selecting never stops anything.
  const act = $derived.by(() => {
    const ph = vm.session?.phase;
    const pick = sel;
    const mine = !!pick && pick.controllable && !pick.external;
    // An external server: a quiet note, never Launch / Stop (KLIF only watches it). A launch that waits for
    // other Systems to stop (starting, no session yet): Cancel.
    if (pick?.external) return { kind: 'ext', text: 'External', title: EXTERNAL_TITLE, disabled: true, run: () => {} };
    if (!ph && isPendingLaunch(pick)) {
      return { kind: 'stop', text: 'Cancel', title: pick?.reason ?? 'Cancel the launch', disabled: !canStop(pick), run: () => void actions?.stop(pick?.id) };
    }
    if (!ph) {
      const ctl = launchCtl(vm, pick, { short: true });
      const word = tierShort(pick?.label ?? '');
      return {
        kind: 'go',
        text: ctl.stopOthers ? ctl.text : `Launch ${word}`,
        title: ctl.enabled ? (ctl.stopOthers ? ctl.text : `Launch ${pick?.label ?? ''}`) : `${pick?.label ?? ''} cannot launch: ${ctl.blocked}`,
        disabled: !ctl.enabled,
        run: () => {
          if (actions) doLaunch(actions, pick, ctl);
        },
      };
    }
    if (ph === 'fault') return { kind: 'hot', text: 'Restart', title: 'Restart the System that failed', disabled: !mine, run: () => void actions?.restart(pick?.id) };
    if (ph === 'stopping') return { kind: 'stop', text: 'Stopping', title: 'Stopping', disabled: true, run: () => {} };
    return {
      kind: 'stop',
      text: ph === 'live' ? 'Stop' : 'Cancel',
      title: !canStop(pick) ? 'External server: it runs where it was started' : ph === 'live' ? 'Stop the server' : 'Cancel the launch',
      disabled: !canStop(pick),
      run: () => void actions?.stop(pick?.id),
    };
  });
</script>

<div class="mini" bind:clientWidth={w} bind:clientHeight={h}>
  <Cloud {vm} {shift} />
  <div class="sheet" style="transform: translate({ox}px, {oy}px) scale({u})">
    <div class="scrim"></div>

    <header class="hdr">
      <span class="klif" title="Koksny.com LOCAL INFERENCE FORNICATOR">KLIF</span><span class="theme">ETHER</span>
      <span class="st {statusTone}"><i></i>{statusText}</span>
      <span class="grow"></span>
      {#if canLeave}
        <button class="back" type="button" onclick={() => actions?.togglePanel()} title="Leave panel mode" aria-label="Leave panel mode">
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.5 1.5h7v7M1.5 3.5h7v7h-7z" /></svg>
        </button>
      {/if}
      {#if clock}<span class="clock" class:ver={!s}>{clock}</span>{/if}
      <button class="act {act.kind}" type="button" disabled={act.disabled || !actions} onclick={act.run} title={act.title}>{act.text}</button>
    </header>

    <nav class="tiers" aria-label="Systems" use:scrollStrip={vm.selected}>
      {#each tabs as t (t.id)}
        <button
          type="button"
          class="tier"
          class:sel={t.id === vm.selected}
          class:na={t.system.availability !== 'ready' && t.system.status !== 'not-set'}
          aria-pressed={t.id === vm.selected}
          data-sel={t.id === vm.selected}
          title={`${t.label}: ${t.system.model.name || 'no preset'} (${t.status})`}
          onclick={() => void actions?.select(t.id)}
        >
          <TabLabel tab={t} short dotSize={7} />
        </button>
      {/each}
    </nav>
    <div class="mline">{modelCompact(model)}</div>

    <section class="hero {hero.tone}">
      <div class="hl">{hero.label}</div>
      <div class="hv"><b>{hero.value}</b>{#if hero.unit}<span class="u">{hero.unit}</span>{/if}</div>
      <div class="hs" class:warn={hero.warn}>{hero.sub}</div>
      <div class="hx">
        {#if hero.steps?.length}
          <div class="steps">{#each hero.steps as st (st.id)}<i class={st.state}></i>{/each}</div>
        {/if}
        {#if hero.frac !== null}<div class="prog"><i style="width:{hero.frac * 100}%"></i></div>{/if}
      </div>
      <div class="rows" class:dim={!s}>
        {#each rows as [k, v] (k)}<div class="kv"><span class="k">{k}</span><span class="v">{v}</span></div>{/each}
      </div>
    </section>

    <footer class="vram">
      <span class="k">vram</span>
      <span class="track">
        {#if dz}
          <span class="fill sleep" style="width:{pct(vm.vram.usedGiB)}%"></span>
          <span class="paged" style="left:{pct(vm.vram.usedGiB)}%; width:{pct(vm.vram.usedGiB + dz.pagedOutGiB) - pct(vm.vram.usedGiB)}%"></span>
        {:else}
          <span class="fill" class:red={spill > 0} class:warn={lowFree && spill <= 0} style="width:{pct(vm.vram.usedGiB)}%"></span>
        {/if}
        {#if fit}<span class="ghost" class:over={fit.spare < 0} style="left:{pct(fit.base)}%; width:{pct(fit.top) - pct(fit.base)}%"></span>{/if}
      </span>
      <span class="v">{fmtGiB(vm.vram.usedGiB)} / {fmtGiB(vm.vram.totalGiB)} GiB{#if vramNote.t}<em class={vramNote.tone}>{` · ${vramNote.t}`}</em>{/if}</span>
    </footer>
  </div>
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: #000;
    user-select: none;
  }
  .sheet {
    --text: #ede8f5;
    --dim: rgba(237, 232, 245, 0.5);
    --faint: rgba(237, 232, 245, 0.1);
    --muted: #9a90a8;
    --blue: #7ab0ff; /* UI accents; pink stays only in the background and the blue-to-pink gradient bars */
    --teal: #4fc3d9;
    --warn: #ffb347;
    --danger: #ff5470;
    --grad: linear-gradient(90deg, #4fc3d9, #ff4fd8);
    --hero: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
    --caps: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
    --data: 'Iosevka', ui-monospace, monospace;
    position: absolute;
    left: 0;
    top: 0;
    width: 960px;
    height: 640px;
    transform-origin: 0 0;
    color: var(--text);
    font-family: var(--caps);
    font-variant-numeric: tabular-nums;
  }
  button {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    margin: 0;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 1px solid var(--teal);
    outline-offset: 3px;
  }
  .scrim {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      radial-gradient(ellipse 330px 280px at 790px 330px, rgba(0, 0, 0, 0.5), rgba(0, 0, 0, 0.22) 60%, transparent),
      linear-gradient(90deg, transparent 52%, rgba(0, 0, 0, 0.38) 74%, rgba(0, 0, 0, 0.5)),
      linear-gradient(180deg, rgba(0, 0, 0, 0.74), rgba(0, 0, 0, 0.62) 19%, rgba(0, 0, 0, 0.3) 25%, transparent 32%, transparent 82%, rgba(0, 0, 0, 0.6));
  }
  @keyframes pulse {
    50% {
      opacity: 0.25;
    }
  }
  /* small text over the plasma keeps a soft dark halo (the hero figure keeps its own glow) */
  .hdr,
  .tiers,
  .mline,
  .hl,
  .hs,
  .rows,
  .vram {
    text-shadow:
      0 0 10px rgba(0, 0, 0, 0.9),
      0 0 3px rgba(0, 0, 0, 0.8);
  }

  /* ---- header ---- */
  .hdr {
    position: absolute;
    left: 28px;
    right: 30px;
    top: 16px;
    height: 40px;
    display: flex;
    align-items: center;
    gap: 16px;
    white-space: nowrap;
  }
  .klif {
    font: 600 18px/1 var(--hero);
    letter-spacing: 0.32em;
  }
  .theme {
    font: 300 14px/18px var(--caps);
    letter-spacing: 0.3em;
    color: var(--dim);
    padding-left: 16px;
    border-left: 1px solid var(--faint);
  }
  .st {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin-left: 6px;
    font: 400 15px/1 var(--caps);
    letter-spacing: 0.18em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .st i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
  }
  .st.live {
    color: var(--text);
  }
  .st.live i {
    background: var(--blue);
    box-shadow: 0 0 9px var(--blue);
  }
  .st.sleep i {
    background: rgba(154, 144, 168, 0.4);
  }
  .st.red {
    color: var(--danger);
  }
  .st.red i {
    background: var(--danger);
    box-shadow: 0 0 9px rgba(255, 84, 112, 0.7);
  }
  .st.pulse i {
    animation: pulse 1.4s ease-in-out infinite;
  }
  .grow {
    flex: 1;
  }
  .clock {
    font: 300 17px/1 var(--data);
    color: var(--text);
  }
  .clock.ver {
    color: var(--dim);
  }
  /* Launch / Cancel / Stop / Restart: one width, the gradient hairline as its border */
  .act {
    position: relative;
    flex: none;
    width: 168px;
    height: 38px;
    border-radius: 19px;
    background: rgba(122, 176, 255, 0.1);
    color: var(--text);
    font: 500 14px/1 var(--caps);
    letter-spacing: 0.16em;
    text-transform: uppercase;
    white-space: nowrap;
    overflow: hidden;
  }
  .act::before {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    padding: 1px;
    background: var(--grad);
    -webkit-mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    mask:
      linear-gradient(#000 0 0) content-box,
      linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    pointer-events: none;
  }
  .act.stop {
    background: rgba(0, 0, 0, 0.4);
  }
  /* An external server: a quiet note in the control's place. */
  .act.ext,
  .act.ext:disabled {
    background: transparent;
    border-color: var(--faint);
    color: var(--muted);
    opacity: 1;
    box-shadow: none;
  }
  .act.ext::before {
    background: var(--faint);
  }
  .act.hot {
    background: rgba(255, 84, 112, 0.16);
  }
  .act:disabled {
    cursor: default;
    opacity: 0.4;
  }
  /* Leave panel mode: only while the pointer is over the panel or it has focus */
  .back {
    width: 38px;
    height: 34px;
    display: grid;
    place-items: center;
    color: var(--dim);
    opacity: 0;
    transition: opacity 0.15s ease;
    flex: none;
  }
  .mini:hover .back,
  .back:focus-visible {
    opacity: 1;
  }
  .back:hover {
    color: var(--text);
  }
  .back svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.2;
  }

  /* ---- tier chips + model line ---- */
  .tiers {
    position: absolute;
    left: 28px;
    right: 28px;
    top: 70px;
    display: flex;
    gap: 8px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tier {
    flex: none;
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 14px;
    border-radius: 16px;
    background: rgba(0, 0, 0, 0.3);
    color: var(--dim);
    font: 400 15px/1 var(--caps);
    letter-spacing: 0.12em;
  }
  .tier::after {
    content: '';
    position: absolute;
    left: 14px;
    right: 14px;
    bottom: -5px;
    height: 2px;
    border-radius: 1px;
    background: var(--grad);
    opacity: 0;
  }
  .tier.sel {
    color: var(--text);
  }
  .tier.sel::after {
    opacity: 1;
  }
  .tier.na {
    opacity: 0.4;
  }
  .tier:not(.sel):hover {
    color: var(--text);
  }
  .mline {
    position: absolute;
    left: 30px;
    top: 118px;
    max-width: 560px;
    font: 300 16px/1.3 var(--data);
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ---- hero ---- */
  .hero {
    position: absolute;
    right: 30px;
    top: 178px;
    width: 340px;
    height: 330px;
    display: grid;
    grid-template-rows: 16px 96px 46px 22px auto;
    row-gap: 8px;
    text-align: right;
  }
  .hl {
    font: 400 15px/1 var(--caps);
    letter-spacing: 0.26em;
    color: var(--teal);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hv {
    display: flex;
    justify-content: flex-end;
    align-items: baseline;
    gap: 10px;
    white-space: nowrap;
  }
  .hv b {
    font: 200 92px/1 var(--hero);
    letter-spacing: -0.02em;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 0 40px rgba(122, 176, 255, 0.22);
  }
  .hv .u {
    font: 300 19px/1 var(--data);
    color: var(--muted);
  }
  .hs {
    font: 300 16px/1.4 var(--data);
    color: var(--muted);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .hs.warn {
    color: var(--warn);
  }
  .hx {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 16px;
  }
  .steps {
    display: flex;
    gap: 9px;
    flex: none;
  }
  .steps i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px rgba(237, 232, 245, 0.3);
  }
  .steps i.done {
    background: var(--text);
    box-shadow: none;
  }
  .steps i.active {
    background: var(--blue);
    box-shadow: 0 0 8px var(--blue);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .steps i.failed {
    background: var(--danger);
    box-shadow: none;
  }
  .prog {
    position: relative;
    flex: 1 1 auto;
    height: 1px;
    background: var(--faint);
  }
  .prog i {
    position: absolute;
    left: 0;
    top: -1px;
    height: 3px;
    border-radius: 2px;
    background: var(--grad);
    box-shadow: 0 0 9px rgba(255, 79, 216, 0.5);
    transition: width 0.4s ease-out;
  }
  .rows {
    margin-top: 6px;
    border-top: 1px solid var(--faint);
  }
  .kv {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 16px;
    padding: 11px 0;
    border-bottom: 1px solid var(--faint);
  }
  .kv .k {
    flex: none;
    font: 400 14px/1.2 var(--caps);
    letter-spacing: 0.18em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .kv .v {
    min-width: 0;
    font: 300 17px/1.2 var(--data);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v {
    color: rgba(237, 232, 245, 0.62);
  }
  .hero.dim .hv b {
    opacity: 0.42;
    text-shadow: none;
  }
  .hero.red .hl,
  .hero.red .hv b {
    color: var(--danger);
  }
  .hero.red .hv b {
    text-shadow: 0 0 40px rgba(255, 84, 112, 0.3);
  }
  .hero.red .hs {
    color: rgba(255, 190, 200, 0.85);
  }

  /* ---- VRAM ---- */
  .vram {
    position: absolute;
    left: 28px;
    right: 30px;
    bottom: 22px;
    display: flex;
    align-items: center;
    gap: 18px;
  }
  .vram .k {
    font: 400 14px/1 var(--caps);
    letter-spacing: 0.22em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .track {
    flex: 1;
    height: 1px;
    background: var(--faint);
    position: relative;
  }
  .track .fill {
    position: absolute;
    left: 0;
    top: -1px;
    height: 3px;
    border-radius: 2px;
    background: var(--grad);
    box-shadow: 0 0 10px rgba(255, 79, 216, 0.55);
    transition: width 0.4s ease-out;
  }
  .track .fill.warn {
    background: var(--warn);
    box-shadow: 0 0 10px rgba(255, 179, 71, 0.4);
  }
  .track .fill.red {
    background: var(--danger);
    box-shadow: 0 0 10px rgba(255, 84, 112, 0.5);
  }
  .track .fill.sleep {
    opacity: 0.35;
    box-shadow: none;
  }
  .track .paged {
    position: absolute;
    top: -2px;
    height: 5px;
    background: repeating-linear-gradient(-45deg, rgba(237, 232, 245, 0.32) 0 1px, transparent 1px 4px);
  }
  .track .ghost {
    position: absolute;
    top: -4px;
    height: 9px;
    border: 1px dashed rgba(237, 232, 245, 0.45);
    border-radius: 4px;
  }
  .track .ghost.over {
    border-color: var(--danger);
  }
  .vram .v {
    font: 300 17px/1 var(--data);
    white-space: nowrap;
  }
  .vram .v em {
    font-style: normal;
    color: var(--muted);
  }
  .vram .v em.red {
    color: var(--danger);
  }
  .vram .v em.warn {
    color: var(--warn);
  }

  @media (prefers-reduced-motion: reduce) {
    .st.pulse i,
    .steps i.active {
      animation: none;
    }
  }
</style>
