<script lang="ts">
  // The "new record" moment: when vm.recordEvents gains an event, a few seconds of celebration over the skin
  // (full window and the 960x640 panel), the number counting up from the old best. Never blocks input; reduced
  // motion gets a still banner. One structure, a background effect per skin (data-skin), themed by --k-*.
  import { onDestroy } from 'svelte';
  import type { RecordEntry, RecordEvent, RecordMetric } from '../model/types';
  import { player } from '../state/player.svelte';
  import { ui } from '../state/ui.svelte';
  import { backendLabel, deltaText, fmtValue, metricMeta } from './records/metrics';

  const SHOW_MS = 4600;
  const COUNT_MS = 1200;

  interface Shown {
    id: string;
    ev: RecordEvent;
    metric: RecordMetric;
    entry: RecordEntry | undefined;
  }

  // Events already in the view model when the shell starts are history, not news.
  const seen = new Set<string>();
  let primed = false;
  let current = $state<Shown | null>(null);
  let shown = $state(0);
  let raf = 0;
  let timer: ReturnType<typeof setTimeout> | 0 = 0;
  const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  const idOf = (ev: RecordEvent) => `${ev.key}|${ev.metric}|${ev.at}`;

  $effect(() => {
    const events = player.vm.recordEvents ?? [];
    if (!primed) {
      for (const ev of events) seen.add(idOf(ev));
      primed = player.ready;
      return;
    }
    const fresh = events.filter((ev) => ev.metric && !seen.has(idOf(ev)));
    for (const ev of fresh) seen.add(idOf(ev));
    const ev = fresh[fresh.length - 1];
    if (ev && ev.metric) play({ id: idOf(ev), ev, metric: ev.metric, entry: player.vm.records.find((e) => e.key === ev.key) });
  });

  function play(s: Shown) {
    cancelAnimationFrame(raf);
    if (timer) clearTimeout(timer);
    current = s;
    const from = s.ev.old ?? 0;
    const to = s.ev.new;
    if (reduced || from === to) {
      shown = to;
    } else {
      const t0 = performance.now();
      const step = (t: number) => {
        const p = Math.min(1, (t - t0) / COUNT_MS);
        const e = 1 - Math.pow(1 - p, 3);
        shown = from + (to - from) * e;
        if (p < 1) raf = requestAnimationFrame(step);
      };
      shown = from;
      raf = requestAnimationFrame(step);
    }
    timer = setTimeout(() => (current = null), SHOW_MS);
  }

  onDestroy(() => {
    cancelAnimationFrame(raf);
    if (timer) clearTimeout(timer);
  });

  // Decode's moment scrambles the digits before they settle.
  const GLYPHS = '0123456789';
  function scramble(text: string, p: number): string {
    if (p >= 1) return text;
    return [...text].map((c, i) => (/[0-9]/.test(c) && i / text.length > p ? GLYPHS[(Math.random() * 10) | 0] : c)).join('');
  }

  const m = $derived(current ? metricMeta(current.metric) : null);
  const valueText = $derived(current ? fmtValue(current.metric, shown) : '');
  const progress = $derived(current ? (current.ev.old === undefined ? 1 : Math.min(1, Math.abs(shown - (current.ev.old ?? 0)) / Math.max(1e-9, Math.abs(current.ev.new - (current.ev.old ?? 0))))) : 1);
</script>

{#if current && m}
  {#key current.id}
    <div class="moment" data-skin={ui.skinId} data-size={ui.size} class:reduced aria-live="polite" role="status">
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
      <div class="card">
        <span class="kicker">{current.ev.old === undefined ? 'First record' : 'New record'}</span>
        <span class="metric">{m.long}</span>
        <div class="value">
          <b>{ui.skinId === 'decode' && !reduced ? scramble(valueText, progress) : valueText}</b><span>{m.unit}</span>
        </div>
        {#if current.ev.old !== undefined}<div class="delta">{deltaText(current.metric, current.ev)}</div>{/if}
        {#if current.entry}
          <div class="model">
            {current.entry.model.name}{current.entry.model.quant ? ` · ${current.entry.model.quant}` : ''} · {backendLabel(current.entry.backend)}{current.entry.node ? ` · ${current.entry.machine}` : ''}
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
    animation: moment 4.6s ease both;
    --rec: var(--k-record, #f2a33a);
  }
  @keyframes moment {
    0% {
      opacity: 0;
    }
    6% {
      opacity: 1;
    }
    86% {
      opacity: 1;
    }
    100% {
      opacity: 0;
    }
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
    animation: pop 0.55s cubic-bezier(0.2, 1.4, 0.4, 1) both;
  }
  @keyframes pop {
    from {
      transform: scale(0.82) translateY(12px);
    }
    to {
      transform: none;
    }
  }
  .kicker {
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
  }
</style>
