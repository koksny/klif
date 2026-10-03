<script lang="ts">
  // Rings, mini panel (960x640, read from ~1 m). The specimen fills the whole panel behind a fixed 960x640 sheet
  // scaled to the window; the sheet is the same in every state (only the contents change):
  //   header      glass bar: KLIF · RINGS · status · clock or version · the act pill (one width)
  //   left        the sphere with three thicker ring gauges (the caption names them)
  //   right       System pills (pick any time; they scroll sideways), the model line, the hero card with two rows
  //   VRAM        a slim glass bar at the bottom: used / total (idle: the selected tier's fit; asleep: resident)
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtClock, fmtGiB, fmtInt, fmtSeconds } from '../../lib/model/format';
  import { EXTERNAL_TITLE, canStop, doLaunch, isPendingLaunch, launchCtl, selectedSystem, shortLabel as tierShort } from '../../lib/model/systems';
  import { strip as scrollStrip } from '../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../lib/shell/SystemTabs/tabs';
  import Specimen from './Specimen.svelte';
  import { extentOf } from './gauges';
  import { heroOf } from './hero';
  import { useSleep } from './sleep.svelte';
  import { fitOf, fmtAgo, modelShort, phaseWord, shapeText } from './text';

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
  const loading = $derived(phase === 'starting' || phase === 'loading');
  const faulted = $derived(phase === 'fault');

  const sleep = useSleep(() => vm);
  const dz = $derived(phase === 'live' ? sleep.info : null);
  const waking = $derived(!!dz && sleep.waking);

  const hero = $derived(heroOf(vm, { kind, sel: slot, dz, waking, full: false }));
  const heroSize = $derived(hero.value.length <= 4 ? 88 : hero.value.length === 5 ? 74 : 64);
  const statusText = $derived(dz ? (waking ? 'GPU waking' : 'GPU asleep') : phaseWord(phase));
  const tone = $derived(faulted ? 'danger' : dz || loading || phase === 'stopping' ? 'warn' : s ? 'live' : 'off');
  const clock = $derived(s ? fmtClock(loading ? (s.loading?.elapsedS ?? s.uptimeS) : s.uptimeS) : '');

  // ---- two rows under the hero ------------------------------------------------------------------------------------
  const fault = $derived(faulted ? (s?.fault ?? null) : null);
  const rows = $derived.by<[string, string][]>(() => {
    if (fault) return [
      ['since', fmtAgo(fault.sinceS)],
      ['exit', fault.exitCode === undefined ? 'not reported' : `${fault.exitCodeHex ?? fault.exitCode}`],
    ];
    if (kind === 'image') {
      const j = img && img.recent.length ? img.recent[img.recent.length - 1] : null;
      return [
        ['last image', j ? `${fmtSeconds(j.seconds)} · ${j.width}×${j.height}` : img ? 'none yet' : '—'],
        ['images', img ? `${fmtInt(img.imagesThisSession)} this session` : '—'],
      ];
    }
    if (kind !== 'llm') {
      return [
        ['in flight', gen ? fmtInt(gen.requestsInFlight ?? 0) : '—'],
        ['requests', gen?.requestsTotal !== undefined ? fmtInt(gen.requestsTotal) : '—'],
      ];
    }
    const total = llm?.context.totalTokens || model?.ctxTokens || 0;
    return [
      ['context', llm ? `${fmtInt(llm.context.usedTokens)} / ${fmtInt(total)}` : total ? `— / ${fmtInt(total)}` : '—'],
      ['model', shapeText(model, 'llm', true)],
    ];
  });

  // ---- VRAM bar -----------------------------------------------------------------------------------------------------
  const total = $derived(Math.max(0.01, vm.vram.totalGiB));
  const fit = $derived(!s ? fitOf(vm.vram, slot) : null);
  const spill = $derived(vm.vram.spillMiB > 0);
  const usedPct = $derived(Math.min(100, (vm.vram.usedGiB / total) * 100));
  const ghost = $derived.by(() => {
    if (dz) return { a: usedPct, b: Math.min(100, ((vm.vram.usedGiB + dz.pagedOutGiB) / total) * 100), over: false };
    if (fit) return { a: Math.min(100, (fit.base / total) * 100), b: Math.min(100, (fit.top / total) * 100), over: fit.spare < 0 };
    return null;
  });
  const vramTone = $derived(spill || ghost?.over ? 'danger' : s && total - vm.vram.usedGiB < vm.vram.warnBelowGiB ? 'warn' : '');
  const vramText = $derived.by(() => {
    if (dz) return { k: 'resident', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)}`, x: `${fmtGiB(dz.pagedOutGiB)} paged out` };
    if (fit) return { k: 'vram', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)}`, x: fit.spare >= 0 ? `fits · ${fmtGiB(fit.spare)} spare` : `over by ${fmtGiB(-fit.spare)}` };
    return { k: 'vram', v: `${fmtGiB(vm.vram.usedGiB)} / ${fmtGiB(vm.vram.totalGiB)}`, x: spill ? `spill ${fmtGiB(vm.vram.spillMiB / 1024)}` : 'GiB' };
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

  // ---- the specimen: left column, as large as the gauges and the caption allow (sheet px) ------------------------------
  const COL = 620; // the right column's left edge
  const geo = $derived.by(() => {
    const top = 82;
    const bottom = 568;
    const cx = 352;
    const e0 = extentOf('mini', 0, 1);
    const e1 = extentOf('mini', 1, 1);
    const rH = (bottom - top - e0.up - e0.down) / (e1.up - e0.up + e1.down - e0.down);
    const rS = (Math.min(cx - 16, COL - 16 - cx) - e0.side) / (e1.side - e0.side);
    const r = Math.min(rH, rS);
    const up = e0.up + (e1.up - e0.up) * r;
    const down = e0.down + (e1.down - e0.down) * r;
    return { cx, cy: top + (bottom - top - up - down) / 2 + up, r };
  });
</script>

<div class="mini" bind:clientWidth={w} bind:clientHeight={h}>
  <Specimen {vm} {kind} sel={slot} {dz} {waking} cx={ox + geo.cx * u} cy={oy + geo.cy * u} r={geo.r * u} variant="mini" k={u} />

  <div class="sheet" style="transform: translate({ox}px, {oy}px) scale({u})">
    <!-- Header -->
    <div class="hdr glass">
      <span class="klif">KLIF</span><span class="sl">RINGS</span>
      <span class="st {tone}"><i class:pulse={waking || loading}></i>{statusText}</span>
      <span class="grow"></span>
      {#if s}<span class="clock">{clock}</span>{:else if vm.host?.appVersion}<span class="clock ver">v{vm.host.appVersion}</span>{/if}
      <button class="act {act.kind}" type="button" onclick={act.run} disabled={act.disabled || !actions} title={act.title}>{act.text}</button>
      {#if canLeave}
        <button class="back" onclick={() => actions?.togglePanel?.()} title="Leave panel mode" aria-label="Leave panel mode">
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M4 2.5h5.5V8M2.5 4H8v5.5H2.5z" /></svg>
        </button>
      {/if}
    </div>

    <!-- System pills -->
    <div class="tiers" use:scrollStrip={vm.selected}>
      {#each tabs as t (t.id)}
        <button
          type="button"
          class="tier glass"
          class:sel={t.id === vm.selected}
          class:na={t.system.availability !== 'ready' && t.system.status !== 'not-set'}
          class:fault={t.status === 'fault'}
          data-sel={t.id === vm.selected}
          title={`${t.label}: ${t.system.model.name || 'no preset'} (${t.status})`}
          onclick={() => void actions?.select(t.id)}
        >
          <TabLabel tab={t} short dotSize={8} />
        </button>
      {/each}
    </div>

    <!-- Model line -->
    <div class="mline" class:dim={!s}>{modelShort(model)}</div>

    <!-- Hero -->
    <div class="hero glass {hero.tone}" class:fault={faulted}>
      <div class="hl">{hero.label}</div>
      <div class="hv" style="--hs:{heroSize}px"><b>{hero.value}</b>{#if hero.unit}<span class="hu">{hero.unit}</span>{/if}</div>
      <div class="hs" class:warn={hero.subTone === 'warn'}>{hero.sub}</div>
      <div class="rows" class:dim={!s}>
        {#each rows as [k, v] (k)}<div class="row"><span class="k">{k}</span><span class="v">{v}</span></div>{/each}
      </div>
    </div>

    <!-- VRAM -->
    <div class="vram glass">
      <span class="k {vramTone}">{vramText.k}</span>
      <span class="track">
        {#if ghost && ghost.b > ghost.a}<span class="ghost" class:over={ghost.over} class:paged={!!dz} style="left:{ghost.a}%;width:{ghost.b - ghost.a}%"></span>{/if}
        <span class="fill {vramTone}" style="width:{usedPct}%"></span>
      </span>
      <span class="v {vramTone}">{vramText.v}<small>{vramText.x}</small></span>
    </div>
  </div>
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: #191a26;
    user-select: none;
  }
  .sheet {
    position: absolute;
    left: 0;
    top: 0;
    width: 960px;
    height: 640px;
    transform-origin: 0 0;
    font-family: 'Barlow', 'Segoe UI', system-ui, sans-serif;
    font-variant-numeric: tabular-nums;
    color: #e3f7ff;
    --ink: #e3f7ff;
    --muted: #8fa9b8;
    --faint: #5d7484;
    --cyan: #39e1ff;
    --mint: #4cff99;
    --warn: #ffb347;
    --danger: #ff5c7a;
    --edge: rgba(120, 220, 255, 0.16);
    --edge2: rgba(120, 220, 255, 0.3);
    --caps: 'Barlow Condensed', 'Barlow', system-ui, sans-serif;
  }
  .glass {
    position: absolute;
    background: rgba(255, 255, 255, 0.045);
    -webkit-backdrop-filter: blur(14px) saturate(120%);
    backdrop-filter: blur(14px) saturate(120%);
    border: 1px solid var(--edge);
    border-radius: 16px;
  }
  .sl,
  .st,
  .act,
  .tier,
  .hl,
  .k {
    font-family: var(--caps);
    font-weight: 600;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  /* header: 14..70 */
  .hdr {
    left: 16px;
    right: 16px;
    top: 14px;
    height: 56px;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 8px 0 24px;
    white-space: nowrap;
  }
  .klif {
    font-size: 26px;
    font-weight: 600;
    letter-spacing: 0.3em;
    line-height: 1;
  }
  .sl {
    padding-left: 14px;
    border-left: 1px solid var(--edge2);
    font-size: 18px;
    font-weight: 500;
    letter-spacing: 0.3em;
    color: var(--muted);
  }
  .st {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    margin-left: 4px;
    font-size: 19px;
    color: var(--muted);
  }
  .st i {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: #4a5a66;
  }
  .st i.pulse {
    animation: pulse 1.2s ease-in-out infinite;
  }
  .st.live {
    color: var(--ink);
  }
  .st.live i {
    background: var(--mint);
    box-shadow: 0 0 10px var(--mint);
  }
  .st.warn,
  .hs.warn,
  .vram .v.warn {
    color: var(--warn);
  }
  .st.danger,
  .vram .v.danger {
    color: var(--danger);
  }
  .st.warn i {
    background: var(--warn);
    box-shadow: 0 0 10px var(--warn);
  }
  .st.danger i {
    background: var(--danger);
    box-shadow: 0 0 10px var(--danger);
  }
  .grow {
    flex: 1 1 auto;
  }
  .clock {
    font-size: 26px;
    font-weight: 300;
    color: var(--ink);
  }
  .clock.ver {
    font-size: 22px;
    color: var(--muted);
  }
  /* Launch / Cancel / Stop / Restart: one width, so nothing in the header moves */
  .act {
    flex: none;
    width: 168px;
    height: 40px;
    border-radius: 999px;
    border: 1.5px solid var(--cyan);
    background: var(--cyan);
    color: #04202a;
    font-size: 19px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
    box-shadow: 0 0 22px rgba(57, 225, 255, 0.35);
  }
  .act.stop {
    background: rgba(57, 225, 255, 0.06);
    color: var(--cyan);
    box-shadow: none;
  }
  /* An external server: a quiet note in the control's place. */
  .act.ext,
  .act.ext:disabled {
    background: transparent;
    border-color: var(--edge);
    color: var(--muted);
    opacity: 1;
    box-shadow: none;
  }
  .act.hot {
    background: var(--danger);
    border-color: var(--danger);
    color: #2a0610;
    box-shadow: 0 0 22px rgba(255, 92, 122, 0.35);
  }
  .act:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .act.go:disabled {
    opacity: 1;
    background: rgba(255, 255, 255, 0.05);
    border-color: var(--edge);
    color: var(--muted);
    box-shadow: none;
  }
  /* leave panel mode: shown while the pointer is over the panel or it has focus */
  .back {
    width: 40px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: 1px solid var(--edge);
    background: rgba(255, 255, 255, 0.04);
    color: var(--muted);
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
    width: 18px;
    height: 18px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
  }

  /* right column: 620..944 */
  .tiers {
    position: absolute;
    left: 620px;
    right: 16px;
    top: 84px;
    height: 42px;
    display: flex;
    gap: 8px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tier {
    flex: 1 0 84px;
    padding: 0 12px;
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border-radius: 999px;
    font-size: 19px;
    color: var(--muted);
    cursor: pointer;
  }
  .tier.sel {
    color: var(--ink);
    border-color: rgba(57, 225, 255, 0.65);
    background: rgba(57, 225, 255, 0.12);
  }
  .tier.na {
    opacity: 0.4;
  }
  .tier.fault {
    border-color: rgba(255, 92, 122, 0.75);
    background: rgba(255, 92, 122, 0.12);
  }
  .mline {
    position: absolute;
    left: 624px;
    right: 18px;
    top: 136px;
    height: 26px;
    line-height: 26px;
    font-size: 19px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* hero: 174..566 */
  .hero {
    left: 620px;
    right: 16px;
    top: 174px;
    height: 392px;
    display: flex;
    flex-direction: column;
    padding: 22px 24px 8px;
  }
  .hero.fault {
    border-color: rgba(255, 92, 122, 0.5);
    background: linear-gradient(180deg, rgba(255, 92, 122, 0.12), rgba(255, 255, 255, 0.03) 60%);
  }
  .hl {
    font-size: 20px;
    color: var(--cyan);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hv {
    margin-top: 14px;
    display: flex;
    align-items: baseline;
    gap: 10px;
    white-space: nowrap;
  }
  .hv b {
    font-weight: 200;
    font-size: var(--hs);
    line-height: 0.95;
    letter-spacing: -0.02em;
    color: var(--ink);
    text-shadow: 0 0 26px rgba(57, 225, 255, 0.25);
  }
  .hu {
    font-size: 24px;
    color: var(--muted);
  }
  .hs {
    margin-top: 12px;
    font-size: 19px;
    line-height: 1.3;
    color: var(--muted);
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .hero.dim .hv b {
    color: #6f8796;
    text-shadow: none;
  }
  .hero.warn .hl {
    color: var(--warn);
  }
  .hero.warn .hv b {
    color: #ffe2b8;
  }
  .hero.danger .hl,
  .hero.danger .hv b {
    color: var(--danger);
    text-shadow: 0 0 26px rgba(255, 92, 122, 0.35);
  }
  .hero.danger .hs {
    color: var(--ink);
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }
  .rows {
    margin-top: auto;
  }
  .row {
    height: 62px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 3px;
    border-top: 1px solid var(--edge);
  }
  .row .k {
    font-size: 15px;
    color: var(--muted);
  }
  .row .v {
    font-size: 23px;
    color: var(--ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rows.dim .v {
    color: #b9cdd8;
  }

  /* VRAM: 580..624 */
  .vram {
    left: 16px;
    right: 16px;
    top: 580px;
    height: 44px;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 0 22px;
    border-radius: 999px;
  }
  .vram .k {
    font-size: 17px;
    color: var(--muted);
    flex: none;
  }
  .vram .k.warn {
    color: var(--warn);
  }
  .vram .k.danger {
    color: var(--danger);
  }
  .track {
    position: relative;
    flex: 1 1 auto;
    height: 4px;
    border-radius: 2px;
    background: rgba(57, 225, 255, 0.16);
  }
  .fill {
    position: absolute;
    left: 0;
    top: -1px;
    bottom: -1px;
    border-radius: 3px;
    background: var(--cyan);
    box-shadow: 0 0 10px rgba(57, 225, 255, 0.55);
  }
  .fill.warn {
    background: var(--warn);
    box-shadow: 0 0 10px rgba(255, 179, 71, 0.5);
  }
  .fill.danger {
    background: var(--danger);
    box-shadow: 0 0 10px rgba(255, 92, 122, 0.5);
  }
  .ghost {
    position: absolute;
    top: -2px;
    bottom: -2px;
    background: repeating-linear-gradient(90deg, rgba(57, 225, 255, 0.6) 0 3px, transparent 3px 7px);
  }
  .ghost.paged {
    background: repeating-linear-gradient(90deg, rgba(255, 179, 71, 0.65) 0 3px, transparent 3px 7px);
  }
  .ghost.over {
    background: repeating-linear-gradient(90deg, rgba(255, 92, 122, 0.75) 0 3px, transparent 3px 7px);
  }
  .vram .v {
    flex: none;
    font-size: 22px;
    color: var(--ink);
    white-space: nowrap;
  }
  .vram .v small {
    margin-left: 10px;
    font-size: 17px;
    color: var(--muted);
  }
  .vram .v.warn small,
  .vram .v.danger small {
    color: inherit;
  }
  @media (prefers-reduced-motion: reduce) {
    .st i.pulse {
      animation: none;
    }
  }
</style>
