<script lang="ts">
  // The "new record" moment: every record a request broke (decode, prefill, TTFT... of one model file) celebrated
  // together over the skin, in the full window and on the 960x640 panel: the main one large and counting up from
  // the old best, the others below it. Hidden 5 s after the last record of the turn arrived, whatever the animation
  // does; never blocks input; reduced motion gets a still card. A background effect per skin (data-skin).
  import { onDestroy } from 'svelte';
  import type { RecordEntry, RecordEvent, RecordMetric } from '../model/types';
  import { player } from '../state/player.svelte';
  import { ui } from '../state/ui.svelte';
  import { backendLabel, deltaText, fmtValue, METRICS, metricMeta } from './records/metrics';

  const SHOW_MS = 5000;
  /** The exit animation: rows leave in reverse order, then the card. */
  const FADE_MS = 650;
  const COUNT_MS = 1200;
  /** Events of one model file this close together are one turn (one request breaks several records at once). */
  const TURN_S = 3;

  interface Row {
    metric: RecordMetric;
    ev: RecordEvent;
  }

  const seen = new Set<string>();
  let primed = false;
  let key = $state<string | null>(null);
  let rows = $state<Row[]>([]);
  let turn = $state(0);
  let fading = $state(false);
  let progress = $state(1);
  let raf = 0;
  let fadeTimer: ReturnType<typeof setTimeout> | 0 = 0;
  let hideTimer: ReturnType<typeof setTimeout> | 0 = 0;
  const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const order = (m: RecordMetric) => METRICS.findIndex((x) => x.id === m);
  const idOf = (ev: RecordEvent) => `${ev.key}|${ev.metric}|${ev.at}`;

  $effect(() => {
    const events = player.vm.recordEvents ?? [];
    if (!primed) {
      for (const ev of events) seen.add(idOf(ev));
      primed = player.ready;
      return;
    }
    const fresh = events.filter((ev) => ev.metric && !seen.has(idOf(ev)));
    if (!fresh.length) return;
    for (const ev of fresh) seen.add(idOf(ev));
    // Off in [ui] record_moment: the records still count, nothing is celebrated.
    if (player.vm.config.recordMoment === false) return;
    show(fresh);
  });

  function show(fresh: RecordEvent[]) {
    const newest = fresh[fresh.length - 1];
    const current = rows;
    const sameTurn = key === newest.key && current.length > 0 && current.some((r) => Math.abs(r.ev.at - newest.at) <= TURN_S);
    const add = fresh.filter((ev) => ev.key === newest.key && Math.abs(ev.at - newest.at) <= TURN_S);
    const next = new Map<RecordMetric, Row>(sameTurn ? current.map((r) => [r.metric, r] as [RecordMetric, Row]) : []);
    for (const ev of add) next.set(ev.metric!, { metric: ev.metric!, ev });
    key = newest.key;
    rows = [...next.values()].sort((a, b) => order(a.metric) - order(b.metric));
    if (!sameTurn) {
      turn += 1;
      countUp();
    }
    fading = false;
    if (fadeTimer) clearTimeout(fadeTimer);
    if (hideTimer) clearTimeout(hideTimer);
    fadeTimer = setTimeout(() => (fading = true), SHOW_MS - FADE_MS);
    hideTimer = setTimeout(() => {
      key = null;
      rows = [];
      fading = false;
    }, SHOW_MS);
  }

  function countUp() {
    cancelAnimationFrame(raf);
    if (reduced) {
      progress = 1;
      return;
    }
    const t0 = performance.now();
    progress = 0;
    const step = (t: number) => {
      const p = Math.min(1, (t - t0) / COUNT_MS);
      progress = 1 - Math.pow(1 - p, 3);
      if (p < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
  }

  onDestroy(() => {
    cancelAnimationFrame(raf);
    if (fadeTimer) clearTimeout(fadeTimer);
    if (hideTimer) clearTimeout(hideTimer);
  });

  /** The value shown while counting: from the old best to the new one. */
  function shownValue(r: Row): number {
    const from = r.ev.old ?? r.ev.new;
    return from + (r.ev.new - from) * progress;
  }

  // Decode's moment scrambles the digits before they settle.
  const GLYPHS = '0123456789';
  function scramble(text: string, p: number): string {
    if (p >= 1) return text;
    return [...text].map((c, i) => (/[0-9]/.test(c) && i / text.length > p ? GLYPHS[(Math.random() * 10) | 0] : c)).join('');
  }

  const entry = $derived<RecordEntry | undefined>(key ? player.vm.records.find((e) => e.key === key) : undefined);
  const hero = $derived(rows[0]);
  const rest = $derived(rows.slice(1));
  const first = $derived(rows.length > 0 && rows.every((r) => r.ev.old === undefined));
  const kicker = $derived(first ? (rows.length > 1 ? 'First records' : 'First record') : rows.length > 1 ? `${rows.length} new records` : 'New record');
  const heroText = $derived(hero ? fmtValue(hero.metric, shownValue(hero)) : '');
  /** The count-up reached the new best: the number pulses once. */
  const landed = $derived(progress >= 1);
</script>

{#if hero}
  {#key turn}
    <div class="moment" data-skin={ui.skinId} data-size={ui.size} class:reduced class:fading aria-live="polite" role="status">
      <div class="fx" aria-hidden="true">
        {#if ui.skinId === 'cliff' || ui.skinId === 'rings'}
          <svg viewBox="-100 -100 200 200" preserveAspectRatio="xMidYMid slice">
            {#each [0, 1, 2, 3, 4, 5] as i}<ellipse class="ring" style="animation-delay:{i * 0.18}s" rx={18 + i * 4} ry={11 + i * 2.6} />{/each}
          </svg>
        {:else if ui.skinId === 'silicon'}
          {#each [0, 1, 2, 3, 4, 5, 6] as i}<i class="trace" style="top:{18 + i * 10}%; animation-delay:{i * 0.07}s"></i>{/each}
        {:else if ui.skinId === 'instrument'}
          <div class="dial"><i class="needle"></i></div>
        {:else if ui.skinId === 'spirit'}
          <span class="rec">● REC</span><i class="corner tl"></i><i class="corner tr"></i><i class="corner bl"></i><i class="corner br"></i>
        {:else}
          <i class="bloom"></i><i class="bloom b2"></i>
        {/if}
      </div>
      <div class="card" class:multi={rows.length > 1} style="--n:{rows.length}">
        <span class="kicker">{kicker}</span>
        <span class="metric">{metricMeta(hero.metric).long}</span>
        <div class="value" class:landed>
          <b>{ui.skinId === 'decode' && !reduced ? scramble(heroText, progress) : heroText}</b><span>{metricMeta(hero.metric).unit}</span>
        </div>
        {#if hero.ev.old !== undefined}<div class="delta">{deltaText(hero.metric, hero.ev)}</div>{/if}
        {#if rest.length}
          <div class="more">
            {#each rest as r, i (r.metric)}
              <div class="row" style="--i:{i}">
                <span class="rl">{metricMeta(r.metric).label}</span>
                <b>{fmtValue(r.metric, shownValue(r))}</b><small>{metricMeta(r.metric).unit}</small>
                <em>{r.ev.old !== undefined ? deltaText(r.metric, r.ev) : 'first record'}</em>
              </div>
            {/each}
          </div>
        {/if}
        {#if entry}
          <div class="model">
            {entry.model.name}{entry.model.quant ? ` · ${entry.model.quant}` : ''} · {backendLabel(entry.backend)}{entry.node ? ` · ${entry.machine}` : ''}
          </div>
        {/if}
      </div>
    </div>
  {/key}
{/if}

<style>
  .moment {
    position: absolute;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    pointer-events: none;
    overflow: hidden;
    background: radial-gradient(60% 50% at 50% 50%, color-mix(in srgb, var(--k-bg, #000) 70%, transparent), transparent 100%);
    animation: moment-in 0.35s ease backwards;
    transition: opacity 0.4s ease 0.25s;
    --rec: var(--k-record, #f2a33a);
  }
  @keyframes moment-in {
    from {
      opacity: 0;
    }
  }
  .moment.fading {
    opacity: 0;
  }
  .more {
    display: grid;
    gap: 4px;
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid color-mix(in srgb, var(--rec) 35%, transparent);
    min-width: 360px;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: baseline;
    column-gap: 8px;
    text-align: left;
  }
  .row .rl {
    font: 600 11px var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .row b {
    font: 500 26px/1.1 var(--k-font-data, monospace);
    color: color-mix(in srgb, var(--rec) 35%, var(--k-ink, #fff));
    font-variant-numeric: tabular-nums;
  }
  .row small {
    font: 500 12px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-accent, #5ab6eb);
  }
  .row em {
    grid-column: 1 / 4;
    justify-self: end;
    font: 600 12px var(--k-font-data, monospace);
    font-style: normal;
    color: var(--rec);
  }
  [data-size='mini'] .row b {
    font-size: 34px;
  }
  [data-size='mini'] .row .rl,
  [data-size='mini'] .row em {
    font-size: 15px;
  }
  .card {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 2px;
    padding: 22px 44px 24px;
    border: 1px solid color-mix(in srgb, var(--rec) 70%, transparent);
    border-radius: var(--k-radius, 8px);
    background: color-mix(in srgb, var(--k-bg, #0f1316) 88%, transparent);
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--rec) 25%, transparent),
      0 20px 80px color-mix(in srgb, var(--rec) 22%, transparent);
    text-align: center;
    overflow: hidden;
    /* opens from a vertical seam in the middle, out of a blur */
    animation: card-in 0.6s cubic-bezier(0.2, 0.9, 0.25, 1) both;
  }
  @keyframes card-in {
    from {
      opacity: 0;
      transform: scale(0.94);
      filter: blur(8px);
      clip-path: inset(0 49% 0 49% round var(--k-radius, 8px));
    }
    45% {
      opacity: 1;
    }
    to {
      opacity: 1;
      transform: none;
      filter: none;
      clip-path: inset(-80px round var(--k-radius, 8px));
    }
  }
  /* one light sweep along the card once it is open */
  .card::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: linear-gradient(105deg, transparent 38%, color-mix(in srgb, var(--rec) 28%, transparent) 50%, transparent 62%);
    transform: translateX(-130%);
    animation: shine 1.1s ease-in-out 0.4s both;
  }
  @keyframes shine {
    to {
      transform: translateX(130%);
    }
  }
  .kicker {
    animation: drop 0.55s cubic-bezier(0.3, 1.6, 0.5, 1) 0.18s both;
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-16px) scale(0.85);
    }
  }
  .metric,
  .value {
    animation: rise 0.5s cubic-bezier(0.2, 0.9, 0.3, 1) 0.24s both;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(10px);
    }
  }
  /* the number pulses once when the count-up reaches the new best */
  .value.landed b {
    animation: land 0.7s ease-out both;
  }
  @keyframes land {
    35% {
      transform: scale(1.06);
      text-shadow: 0 0 70px color-mix(in srgb, var(--rec) 85%, transparent);
    }
  }
  .delta {
    animation: rise 0.45s ease 1.2s both;
  }
  .more {
    animation: line 0.5s ease 0.45s both;
  }
  @keyframes line {
    from {
      border-top-color: transparent;
    }
  }
  /* several records: the rows arrive one after another */
  .row {
    animation: row-in 0.5s cubic-bezier(0.2, 0.9, 0.3, 1) calc(0.55s + var(--i, 0) * 0.15s) both;
  }
  .row em {
    animation: rise 0.4s ease calc(1.25s + var(--i, 0) * 0.15s) both;
  }
  @keyframes row-in {
    from {
      opacity: 0;
      transform: translateY(16px);
      filter: blur(4px);
    }
  }
  .model {
    animation: rise 0.45s ease calc(0.5s + var(--n, 1) * 0.12s) both;
  }
  /* exit: the rows leave in reverse order, then the card lifts away into a blur */
  .fading .row {
    animation: row-out 0.24s ease-in calc((var(--n, 1) - 2 - var(--i, 0)) * 0.06s) both;
  }
  @keyframes row-out {
    to {
      opacity: 0;
      transform: translateY(-8px);
    }
  }
  .fading .card {
    animation: card-out 0.42s cubic-bezier(0.5, 0, 0.75, 0) calc(var(--n, 1) * 0.05s) both;
  }
  @keyframes card-out {
    from {
      opacity: 1;
      transform: none;
      filter: none;
      clip-path: inset(-80px round var(--k-radius, 8px));
    }
    to {
      opacity: 0;
      transform: translateY(-14px) scale(0.95);
      filter: blur(6px);
      clip-path: inset(-80px round var(--k-radius, 8px));
    }
  }
  .kicker {
    display: inline-block;
    padding: 3px 12px;
    border-radius: 3px;
    background: var(--rec);
    color: var(--k-bg, #0f1316);
    font: 800 12px/1.3 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.22em;
    text-transform: uppercase;
  }
  .metric {
    margin-top: 10px;
    font: 600 11px var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.18em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .value {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .value b {
    font: 500 96px/1 var(--k-font-data, monospace);
    letter-spacing: -0.02em;
    color: color-mix(in srgb, var(--rec) 45%, var(--k-ink, #fff));
    text-shadow: 0 0 40px color-mix(in srgb, var(--rec) 55%, transparent);
    font-variant-numeric: tabular-nums;
  }
  .value span {
    font: 500 20px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-accent, #5ab6eb);
  }
  .delta {
    font: 600 15px var(--k-font-data, monospace);
    color: var(--rec);
  }
  .model {
    margin-top: 6px;
    max-width: 720px;
    font: 500 15px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* the 960x640 panel is read from a metre away: bigger, the whole screen */
  [data-size='mini'] .card {
    padding: 26px 40px 30px;
  }
  [data-size='mini'] .value b {
    font-size: 150px;
  }
  [data-size='mini'] .kicker {
    font-size: 18px;
  }
  [data-size='mini'] .metric,
  [data-size='mini'] .delta,
  [data-size='mini'] .model {
    font-size: 20px;
  }

  /* ---------------------------------------------------------------- per-skin backgrounds */
  .fx {
    position: absolute;
    inset: 0;
  }
  .fx svg {
    width: 100%;
    height: 100%;
  }
  .ring {
    fill: none;
    stroke: var(--rec);
    stroke-width: 0.35;
    opacity: 0;
    transform-box: fill-box;
    transform-origin: center;
    animation: ripple 2.6s ease-out infinite;
  }
  [data-skin='rings'] .ring {
    stroke: var(--k-accent, #39e1ff);
    stroke-width: 0.6;
    filter: drop-shadow(0 0 2px var(--k-accent, #39e1ff));
  }
  @keyframes ripple {
    0% {
      opacity: 0.9;
      transform: scale(0.6);
    }
    100% {
      opacity: 0;
      transform: scale(2.6);
    }
  }
  .trace {
    position: absolute;
    left: -40%;
    width: 40%;
    height: 2px;
    background: linear-gradient(90deg, transparent, var(--k-accent, #5ab6eb), var(--rec));
    animation: trace 1.1s ease-in infinite;
  }
  @keyframes trace {
    to {
      left: 110%;
    }
  }
  .dial {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 520px;
    height: 520px;
    margin: -260px 0 0 -260px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--k-ink, #fff) 10%, transparent);
    border-top-color: var(--rec);
  }
  .needle {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 4px;
    height: 250px;
    margin-left: -2px;
    transform-origin: 50% 0;
    background: linear-gradient(var(--rec), transparent);
    animation: swing 1.3s cubic-bezier(0.3, 1.5, 0.5, 1) both;
  }
  @keyframes swing {
    from {
      transform: rotate(150deg);
    }
    to {
      transform: rotate(200deg);
    }
  }
  .bloom {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 70%;
    height: 70%;
    margin: -35% 0 0 -35%;
    border-radius: 50%;
    background: radial-gradient(circle, color-mix(in srgb, var(--rec) 40%, transparent), transparent 65%);
    filter: blur(20px);
    animation: bloom 2.2s ease-in-out infinite alternate;
  }
  .bloom.b2 {
    background: radial-gradient(circle, color-mix(in srgb, var(--k-accent, #5ab6eb) 35%, transparent), transparent 65%);
    animation-delay: 0.6s;
    margin: -25% 0 0 -45%;
  }
  @keyframes bloom {
    from {
      transform: scale(0.8);
      opacity: 0.6;
    }
    to {
      transform: scale(1.15);
      opacity: 1;
    }
  }
  [data-skin='phosphor'] .value b,
  [data-skin='loom'] .value b {
    animation: flicker 0.12s steps(2) 6;
  }
  @keyframes flicker {
    50% {
      opacity: 0.55;
    }
  }
  [data-skin='loom'] .card,
  [data-skin='decode'] .card,
  [data-skin='spirit'] .card {
    border-radius: 0;
  }
  .rec {
    position: absolute;
    left: 6%;
    top: 7%;
    font: 700 22px var(--k-font-data, monospace);
    letter-spacing: 0.1em;
    color: var(--rec);
    animation: blink 0.8s steps(2) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
  .corner {
    position: absolute;
    width: 46px;
    height: 46px;
    border: 3px solid var(--k-ink, #fff);
    opacity: 0.8;
  }
  .tl {
    left: 4%;
    top: 5%;
    border-right: 0;
    border-bottom: 0;
  }
  .tr {
    right: 4%;
    top: 5%;
    border-left: 0;
    border-bottom: 0;
  }
  .bl {
    left: 4%;
    bottom: 5%;
    border-right: 0;
    border-top: 0;
  }
  .br {
    right: 4%;
    bottom: 5%;
    border-left: 0;
    border-top: 0;
  }
  .reduced,
  .reduced .card,
  .reduced * {
    animation-duration: 0s !important;
    animation-iteration-count: 1 !important;
  }
  .reduced {
    animation: none;
    transition: none;
  }
</style>
