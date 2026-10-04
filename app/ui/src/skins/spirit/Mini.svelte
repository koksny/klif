<script lang="ts">
  // Spirit, mini panel (960x640, read from ~1 m): the whole panel is the viewfinder. The smoke is full-bleed,
  // its centre shifted left so the hero reads on the right; a fixed 960x640 OSD sheet scaled to fit sits on
  // top, the same in every state (only the contents change):
  //   corners     the viewfinder brackets at the panel edges (red in a fault)
  //   top         REC state · timecode
  //   header      KLIF · SPIRIT · Launch / Cancel / Stop / Restart (one fixed width)
  //   systems     one chip per System with its status dot (select any time; scrolls sideways)
  //   model       the running model, or the selected System's, and its shape
  //   hero        (right) label, one big figure + unit, a status line
  //   bottom      the settings line, the battery (VRAM), the exposure meter
  // Smallest text 20 px, values 26 px and up.
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { fmtTps } from '../../lib/model/format';
  import { systemLabel } from '../../lib/model/systems';
  import { strip as scrollStrip } from '../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../lib/shell/SystemTabs/tabs';
  import Battery from './Battery.svelte';
  import Ic from './Ic.svelte';
  import Meter from './Meter.svelte';
  import Smoke from './Smoke.svelte';
  import Timecode from './Timecode.svelte';
  import { useOsd } from './osd.svelte';
  import { fmtAgo, modelShort, short } from './text';

  let { vm, actions }: { vm: ViewModel; actions?: Actions } = $props();

  const SW = 960;
  const SH = 640;
  /** The smoke's centre, this far left of the panel's centre (sheet px). */
  const SHIFT = 122;
  let w = $state(SW);
  let h = $state(SH);
  const u = $derived(Math.max(0.1, Math.min(w / SW, h / SH)));
  const ox = $derived((w - SW * u) / 2);
  const oy = $derived((h - SH * u) / 2);

  const o = useOsd(
    () => vm,
    (id, stopOthers) => void actions?.launch(id, stopOthers ? { stopOthers: true } : undefined).catch(() => {}),
    (id) => void actions?.stop(id).catch(() => {}),
    (id) => void actions?.restart(id).catch(() => {}),
  );
  const tabs = $derived(tabsFor(vm));
  const s = $derived(o.s);
  const faulted = $derived(o.faulted);
  const canLeave = $derived(!!(vm.host?.panel?.available || vm.host?.panel?.active));
  let ready = $state(false);

  const settings = $derived(o.settings.filter((x) => !x.model));
  // The panel's status line is shorter than the window's: idle names the last session in one line.
  const sub = $derived.by(() => {
    const ls = vm.lastSession;
    if (s || !o.selReady || !ls) return o.hero.sub;
    const tier = systemLabel(vm, ls.system) || undefined;
    const speed = ls.decodeTps !== undefined ? ` · ${fmtTps(ls.decodeTps)} TOK/S` : ls.secondsPerImage !== undefined ? ` · ${ls.secondsPerImage.toFixed(1)} S/IMG` : '';
    return `LAST ${tier ? short(tier) : ls.model.name.toUpperCase()}${speed} · ${ls.ended === 'fault' ? 'FAULT' : 'STOPPED'} ${fmtAgo(ls.endedAgoS)}`;
  });
</script>

<div class="mini" bind:clientWidth={w} bind:clientHeight={h}>
  <Smoke {vm} shift={SHIFT * u} bind:ready />
  <div class="sheet" class:err={faulted} style="transform: translate({ox}px, {oy}px) scale({u})">
    <i class="cn tl"></i><i class="cn tr"></i><i class="cn bl"></i><i class="cn br2"></i>
    {#if !o.loading}<i class="cross"></i>{/if}

    <!-- Top: REC state and the timecode -->
    <div class="recst {o.rec.kind}" role="status">{#if o.rec.glyph}<span class="g"><Ic kind={o.rec.glyph} /></span>{/if}{o.rec.text}</div>
    <div class="ptag" class:init={!ready}>{#if !ready}<Ic kind="ring" />{/if}1 048 576 PARTICLES</div>
    <div class="tc" class:idle={!s} class:red={faulted}><Timecode t={o.uptime} running={o.rec.kind === 'rec'} /></div>

    <!-- Header line -->
    <div class="hdr">
      <span class="wm">KLIF</span><span class="vbar"></span><span class="sk">SPIRIT</span>
      <span class="grow"></span>
      <button class="act {o.act.kind}" type="button" onclick={o.act.run} disabled={o.act.disabled || !actions} title={o.act.title}>
        <span class="g" class:red={o.act.red}><Ic kind={o.act.glyph} /></span>{o.act.kind === 'go' ? o.act.text : o.act.kind === 'ext' ? 'EXTERNAL' : o.act.text.replace(/ \S+$/, '')}
      </button>
      {#if canLeave}
        <button class="back" type="button" onclick={() => actions?.togglePanel()} title="Leave panel mode" aria-label="Leave panel mode">
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3.5 1.5h7v7M1.5 3.5h7v7h-7z" /></svg>
        </button>
      {/if}
    </div>

    <!-- Systems -->
    <div class="tiers" role="tablist" aria-label="Systems" use:scrollStrip={vm.selected}>
      {#each tabs as t (t.id)}
        <button
          type="button"
          class="tier"
          class:sel={t.id === vm.selected}
          class:na={t.system.availability !== 'ready' && t.system.status !== 'not-set'}
          class:flt={t.status === 'fault'}
          role="tab"
          aria-selected={t.id === vm.selected}
          data-sel={t.id === vm.selected}
          title={`${t.label}: ${t.system.model.name || 'no preset'} (${t.status})`}
          onclick={() => void actions?.select(t.id)}
        >
          <TabLabel tab={t} short dotSize={9} />
        </button>
      {/each}
    </div>

    <!-- Model line and shape -->
    <div class="mline" class:dim={!s}>{modelShort(o.model)}</div>
    <div class="shape">{o.shape}</div>

    <!-- Hero -->
    <div class="hero {o.hero.tone}">
      <div class="hl {o.hero.labelTone}">{o.hero.label}</div>
      <div class="hv"><b>{o.hero.value}</b>{#if o.hero.unit}<span class="un">{o.hero.unit}</span>{/if}</div>
      <div class="hs {o.hero.subTone}">{sub}</div>
      {#if faulted && o.exitText}<div class="hx">{o.exitText}</div>{/if}
    </div>

    {#if o.loading}
      {@const k = s?.loading?.fraction ?? 0}
      <div class="af" class:lock={k >= 0.999} style="--k:{k}">
        <i class="c a"></i><i class="c b"></i><i class="c c2"></i><i class="c d"></i>
        <div class="afl">{o.afLine[0]}{#if o.afLine[1]}<span>{o.afLine[1]}</span>{/if}</div>
      </div>
    {/if}

    <!-- Bottom: the settings line; the exposure meter and the battery under it -->
    <div class="set" class:dim={!s || !!o.dz}>
      {#each settings as it, i (i)}
        <span class="it">{#if it.k}<em>{it.k}</em>{/if}{it.v}{#if it.u}<em class="un">{it.u}</em>{/if}</span>
      {/each}
    </div>
    <div class="mtr"><Meter m={o.meter} /></div>
    <div class="bat"><Battery b={o.battery} compact /></div>
  </div>
</div>

<style>
  .mini {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: #080808;
    user-select: none;
  }
  .sheet {
    --bg: #080808;
    --ink: #d9f3ff;
    --muted: #6f8c99;
    --osd: rgba(217, 243, 255, 0.7);
    --cyan: #2bc8ff;
    --warn: #ffb347;
    --red: #ff3b3b;
    --rule2: rgba(217, 243, 255, 0.32);
    position: absolute;
    left: 0;
    top: 0;
    width: 960px;
    height: 640px;
    transform-origin: 0 0;
    font-family: 'IBM Plex Mono', Consolas, monospace;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.08em;
    color: var(--ink);
    text-shadow:
      0 0 3px rgba(0, 0, 0, 0.95),
      0 1px 7px rgba(0, 0, 0, 0.8);
    pointer-events: none;
  }
  .sheet button {
    pointer-events: auto;
  }
  button {
    font: inherit;
    letter-spacing: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    text-shadow: inherit;
    cursor: pointer;
  }
  button:focus-visible {
    outline: 2px solid var(--cyan);
    outline-offset: 3px;
  }
  i,
  em {
    font-style: normal;
  }

  /* Viewfinder corners at the panel edges */
  .cn {
    position: absolute;
    width: 42px;
    height: 42px;
    border: 0 solid var(--osd);
    transition: border-color 0.4s ease;
  }
  .cn.tl {
    left: 14px;
    top: 14px;
    border-left-width: 2px;
    border-top-width: 2px;
  }
  .cn.tr {
    right: 14px;
    top: 14px;
    border-right-width: 2px;
    border-top-width: 2px;
  }
  .cn.bl {
    left: 14px;
    bottom: 14px;
    border-left-width: 2px;
    border-bottom-width: 2px;
  }
  .cn.br2 {
    right: 14px;
    bottom: 14px;
    border-right-width: 2px;
    border-bottom-width: 2px;
  }
  .err .cn {
    border-color: var(--red);
  }
  .cross {
    position: absolute;
    left: 358px;
    top: 320px;
    width: 20px;
    height: 20px;
    transform: translate(-50%, -50%);
    opacity: 0.4;
    background:
      linear-gradient(var(--ink), var(--ink)) center / 100% 1.5px no-repeat,
      linear-gradient(var(--ink), var(--ink)) center / 1.5px 100% no-repeat;
  }

  /* Top row: 34..70 */
  .recst {
    position: absolute;
    left: 44px;
    top: 36px;
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 30px;
    font-weight: 500;
    letter-spacing: 0.14em;
    line-height: 1;
  }
  .recst.stby,
  .recst.pause,
  .recst.stop {
    color: var(--muted);
  }
  .recst .g {
    display: inline-flex;
    font-size: 0.92em;
    filter: drop-shadow(0 0 3px rgba(0, 0, 0, 0.9));
  }
  .recst.rec .g {
    color: var(--red);
    filter: drop-shadow(0 0 6px rgba(255, 59, 59, 0.55));
    animation: rec 2s ease-in-out infinite;
  }
  .recst.load .g,
  .recst.wake .g {
    color: var(--cyan);
  }
  .recst.err {
    color: var(--red);
  }
  @keyframes rec {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.18;
    }
  }
  .tc {
    position: absolute;
    right: 44px;
    top: 32px;
    font-size: 36px;
    font-weight: 300;
    letter-spacing: 0.04em;
    line-height: 1;
  }
  .tc.idle {
    color: var(--muted);
  }
  .tc.red {
    color: var(--red);
  }

  /* Header line: 84..128 */
  .hdr {
    position: absolute;
    left: 44px;
    right: 44px;
    top: 84px;
    height: 44px;
    display: flex;
    align-items: center;
    gap: 16px;
    white-space: nowrap;
  }
  .wm {
    font-size: 26px;
    font-weight: 500;
    letter-spacing: 0.32em;
  }
  .vbar {
    width: 2px;
    height: 24px;
    background: var(--rule2);
  }
  .sk {
    font-size: 21px;
    letter-spacing: 0.3em;
    color: var(--cyan);
  }
  .grow {
    flex: 1 1 auto;
  }
  /* Launch / Cancel / Stop / Restart: one width, bracketed, so nothing in the header moves. */
  .act {
    position: relative;
    flex: none;
    width: 168px;
    height: 44px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 0 14px;
    font-size: 19px;
    font-weight: 500;
    letter-spacing: 0.07em;
    white-space: nowrap;
    overflow: hidden;
  }
  .act::before,
  .act::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    width: 9px;
    border: 2px solid var(--ink);
  }
  .act::before {
    left: 0;
    border-right: 0;
  }
  .act::after {
    right: 0;
    border-left: 0;
  }
  .act .g {
    display: inline-flex;
    color: var(--osd);
  }
  .act .g.red {
    color: var(--red);
  }
  .act.stop .g.red {
    color: var(--warn);
  }
  .act.hot {
    color: var(--red);
  }
  .act.hot::before,
  .act.hot::after {
    border-color: var(--red);
  }
  .act.hot .g {
    color: var(--red);
  }
  .act:disabled {
    cursor: default;
    color: var(--muted);
  }
  .act:disabled::before,
  .act:disabled::after {
    border-color: var(--rule2);
  }
  /* Leave panel mode: only while the pointer is over the panel or it has focus. */
  .back {
    width: 44px;
    height: 40px;
    display: grid;
    place-items: center;
    flex: none;
    color: var(--osd);
    opacity: 0;
    transition: opacity 0.15s ease;
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

  /* Tiers: 140..180 */
  .tiers {
    position: absolute;
    left: 38px;
    right: 38px;
    top: 140px;
    display: flex;
    gap: 8px;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tier {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 9px;
    height: 40px;
    padding: 0 12px;
    font-size: 27px;
    font-weight: 500;
    letter-spacing: 0.1em;
    white-space: nowrap;
  }
  .tier.sel {
    background: var(--ink);
    color: var(--bg);
    text-shadow: none;
  }
  .tier.na {
    opacity: 0.4;
  }
  .tier.flt {
    color: var(--red);
  }
  .tier.sel.flt {
    background: var(--red);
    color: #140202;
  }
  /* Model line and shape: 192..250 */
  .mline {
    position: absolute;
    left: 44px;
    top: 192px;
    max-width: 640px;
    font-size: 22px;
    letter-spacing: 0.06em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mline.dim {
    color: var(--osd);
  }
  .shape {
    position: absolute;
    left: 44px;
    top: 224px;
    font-size: 20px;
    letter-spacing: 0.1em;
    color: var(--osd);
    white-space: nowrap;
  }

  /* Hero: right, 262..480 */
  .hero {
    position: absolute;
    right: 44px;
    top: 262px;
    width: 440px;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    text-align: right;
  }
  /* Readability over bright smoke: a soft local darkening behind the readout (camera OSDs do the same). */
  .hero {
    isolation: isolate;
  }
  .hero::before {
    content: '';
    position: absolute;
    inset: -36px -28px -28px -90px;
    z-index: -1;
    background: radial-gradient(ellipse at 68% 50%, rgba(0, 0, 0, 0.6), rgba(0, 0, 0, 0.32) 45%, transparent 72%);
    pointer-events: none;
  }
  .hl {
    font-size: 24px;
    font-weight: 500;
    letter-spacing: 0.18em;
    color: var(--cyan);
    white-space: nowrap;
  }
  .hl.dim {
    color: var(--muted);
  }
  .hl.warn {
    color: var(--warn);
  }
  .hl.red {
    color: var(--red);
  }
  .hv {
    display: flex;
    align-items: baseline;
    gap: 12px;
    white-space: nowrap;
  }
  .hv b {
    font-size: 84px;
    font-weight: 200;
    line-height: 1.02;
    letter-spacing: -0.02em;
  }
  .hv .un {
    font-size: 26px;
    font-weight: 300;
    color: var(--osd);
  }
  .hs {
    font-size: 22px;
    line-height: 1.3;
    color: var(--osd);
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .hs.warn {
    color: var(--warn);
  }
  .hs.red {
    color: #ff8a8a;
  }
  .hx {
    margin-top: 6px;
    font-size: 20px;
    color: var(--red);
  }
  .hero.dim .hv b {
    opacity: 0.5;
  }
  .hero.red .hv b {
    color: var(--red);
  }

  /* AF box at the smoke's centre */
  .af {
    position: absolute;
    left: 358px;
    top: 320px;
    width: 160px;
    height: 104px;
    transform: translate(-50%, -50%);
    --sx: calc((1 - var(--k)) * 70px);
    --sy: calc((1 - var(--k)) * 46px);
  }
  .af .c {
    position: absolute;
    width: 22px;
    height: 22px;
    border: 0 solid var(--ink);
    transition:
      transform 0.9s cubic-bezier(0.25, 0.8, 0.3, 1),
      border-color 0.3s ease;
  }
  .af .a {
    left: 0;
    top: 0;
    border-left-width: 2px;
    border-top-width: 2px;
    transform: translate(calc(-1 * var(--sx)), calc(-1 * var(--sy)));
  }
  .af .b {
    right: 0;
    top: 0;
    border-right-width: 2px;
    border-top-width: 2px;
    transform: translate(var(--sx), calc(-1 * var(--sy)));
  }
  .af .c2 {
    left: 0;
    bottom: 0;
    border-left-width: 2px;
    border-bottom-width: 2px;
    transform: translate(calc(-1 * var(--sx)), var(--sy));
  }
  .af .d {
    right: 0;
    bottom: 0;
    border-right-width: 2px;
    border-bottom-width: 2px;
    transform: translate(var(--sx), var(--sy));
  }
  .af.lock .c {
    border-color: var(--cyan);
  }
  .afl {
    position: absolute;
    left: 50%;
    top: calc(100% + 64px);
    transform: translateX(-50%);
    display: flex;
    gap: 12px;
    font-size: 22px;
    font-weight: 500;
    letter-spacing: 0.12em;
    white-space: nowrap;
  }
  .afl span {
    color: var(--osd);
    font-weight: 400;
  }

  /* Bottom: settings line 506..542; meter (left) and battery (right) 556..600 */
  .set {
    position: absolute;
    left: 44px;
    right: 44px;
    top: 506px;
    display: flex;
    gap: 30px;
    font-size: 27px;
    white-space: nowrap;
    overflow: hidden;
  }
  .set .it em {
    color: var(--osd);
    margin-right: 0.45em;
  }
  .set .it em.un {
    margin: 0 0 0 0.4em;
  }
  .set.dim .it {
    color: var(--osd);
  }
  .bat {
    position: absolute;
    right: 44px;
    top: 562px;
    font-size: 26px;
  }
  .mtr {
    position: absolute;
    left: 44px;
    top: 556px;
    width: 470px;
    font-size: 19px;
  }
  /* the camera's format tag, top centre */
  .ptag {
    position: absolute;
    left: 50%;
    top: 42px;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 18px;
    letter-spacing: 0.12em;
    color: var(--muted);
    white-space: nowrap;
  }
  .ptag.init {
    animation: rec 2s ease-in-out infinite;
  }
  @media (prefers-reduced-motion: reduce) {
    .recst.rec .g,
    .ptag.init {
      animation: none;
    }
  }
</style>
