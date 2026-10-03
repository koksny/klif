<script lang="ts">
  // Rotary MODE selector: a knob with one detent per System (a window of four at a time when there are more:
  // it follows the selected System; the chevrons scroll it without selecting anything and carry one small lamp
  // per System out of view, so a fault or a running System there still shows), printed leader lines to each
  // label, and a state column of lamps on the right. One layout in every phase. The pointer is the selected
  // System.
  // Click selects (never stops anything), double-click launches a System that is ready and has nothing in its
  // way. A row carries the shared status dot, the label (ellipsis) and, for a remote System, its node.
  import type { SystemId, ViewModel } from '../../../lib/model/types';
  import { blockedText, canLaunch, STATUS_TEXT } from '../../../lib/model/systems';
  import TabLabel from '../../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor, type SystemTab } from '../../../lib/shell/SystemTabs/tabs';
  import { availLabel, slotSubtitle } from '../theme';
  import Icon from './Icon.svelte';

  let {
    vm,
    pointer,
    tone = 'cyan',
    sleepWord = null,
    onselect,
    onlaunch,
  }: {
    vm: ViewModel;
    pointer: SystemId;
    /** orange = the pointer System faulted; amber = its GPU is dormant. */
    tone?: 'cyan' | 'orange' | 'amber';
    /** The pointer System's GPU is dormant: its state word reads "asleep" / "waking". */
    sleepWord?: string | null;
    onselect?: (id: SystemId) => void;
    onlaunch?: (id: SystemId) => void;
  } = $props();

  const uid = $props.id();
  const MAXV = 4;

  // Geometry in unscaled px (multiplied by --u in CSS).
  const G = { rowH: 25, R: 41, cx: 50, rho: 52, dotX: 136, pad: 3 };
  const W = G.dotX;
  const all = $derived(tabsFor(vm));
  const selIdx = $derived(Math.max(0, all.findIndex((t) => t.id === pointer)));
  // A number, so the 2 Hz snapshot (a new list every tick) does not reset a window the chevrons moved.
  const count = $derived(all.length);
  const clampStart = (i: number) => Math.max(0, Math.min(count - MAXV, i));
  /** First System in view: follows the pointer; the chevrons move it (until the pointer or the count changes). */
  let start = $derived(clampStart(selIdx - Math.floor(MAXV / 2)));
  const shown = $derived(all.slice(start, start + MAXV));
  const hiddenAbove = $derived(all.slice(0, start));
  const hiddenBelow = $derived(all.slice(start + shown.length));
  const above = $derived(hiddenAbove.length);
  const below = $derived(hiddenBelow.length);
  const ptrShown = $derived(shown.some((t) => t.id === pointer));
  const n = $derived(Math.max(1, shown.length));
  const H = $derived(G.pad * 2 + n * G.rowH);
  const cy = $derived(H / 2);
  const rows = $derived(
    shown.map((t, i) => {
      const y = G.pad + G.rowH * (i + 0.5);
      const off = y - cy;
      const a = Math.asin(Math.max(-0.97, Math.min(0.97, off / G.rho))); // radians, screen y down
      return { tab: t, y, a, deg: (a * 180) / Math.PI, rx: G.cx + G.rho * Math.cos(a) };
    }),
  );
  const pointerRow = $derived(rows.find((r) => r.tab.id === pointer));
  // The pointer System scrolled out of view: the knob points past the first / last detent, toward it.
  const pointerDeg = $derived(pointerRow ? pointerRow.deg : ((selIdx < start ? -1 : 1) * Math.asin(0.97) * 180) / Math.PI);
  const knurl = Array.from({ length: 56 }, (_, i) => (i / 56) * Math.PI * 2);

  function step(dir: 1 | -1, from: number) {
    const j = (from + dir + all.length) % all.length;
    onselect?.(all[j].id);
    return j;
  }

  function key(e: KeyboardEvent, id: SystemId) {
    const dir = e.key === 'ArrowDown' || e.key === 'ArrowRight' ? 1 : e.key === 'ArrowUp' || e.key === 'ArrowLeft' ? -1 : 0;
    if (!dir) return;
    e.preventDefault();
    const i = all.findIndex((t) => t.id === id);
    const j = step(dir, i);
    const el = (e.currentTarget as HTMLElement).closest('.opts');
    queueMicrotask(() => el?.querySelector<HTMLButtonElement>(`button.opt[data-system="${CSS.escape(all[j].id)}"]`)?.focus());
  }

  /** The state column of one System: lamp tone and word. */
  function stateOf(t: SystemTab): { lamp: 'off' | 'cyan' | 'orange' | 'amber'; word: string } {
    const s = t.system;
    const isPtr = t.id === pointer;
    switch (s.status) {
      case 'starting':
        return { lamp: isPtr ? tone : 'cyan', word: 'starting' };
      case 'stopping':
        return { lamp: isPtr ? tone : 'cyan', word: 'stopping' };
      case 'online':
      case 'busy':
        return { lamp: isPtr ? tone : 'cyan', word: isPtr && sleepWord ? sleepWord : 'running' };
      case 'fault':
        return { lamp: 'orange', word: 'fault' };
      case 'not-set':
        return { lamp: 'off', word: 'not set' };
      case 'unreachable':
        return { lamp: 'orange', word: 'unreachable' };
      case 'invalid':
        return { lamp: 'orange', word: s.availability === 'ready' ? 'invalid' : availLabel(s.availability) };
      default:
        if (s.external) return { lamp: 'orange', word: 'not answering' };
        if (s.availability !== 'ready') return { lamp: 'orange', word: availLabel(s.availability) };
        return { lamp: isPtr ? 'cyan' : 'off', word: 'ready' };
    }
  }

  const LAMP_RANK = { off: 0, amber: 1, cyan: 2, orange: 3 } as const;
  /** The lamps of Systems out of view: one each, or one in the worst tone when there are many. */
  function hiddenLamps(list: SystemTab[]): ('off' | 'cyan' | 'orange' | 'amber')[] {
    const lamps = list.map((t) => stateOf(t).lamp);
    if (lamps.length <= 5) return lamps;
    return [lamps.reduce((a, b) => (LAMP_RANK[b] > LAMP_RANK[a] ? b : a), 'off' as const)];
  }
  const hiddenText = (list: SystemTab[]) => list.map((t) => `${t.label}${t.nodeName ? ` on ${t.nodeName}` : ''}: ${stateOf(t).word}`).join('; ');

  const titleOf = (t: SystemTab) => {
    const s = t.system;
    const where = t.nodeName ? ` on ${t.nodeName}` : '';
    const why = canLaunch(s) ? '' : blockedText(s);
    return `${t.label}${where}: ${s.model.name || 'no preset'} (${STATUS_TEXT[s.status].toLowerCase()}${why ? `: ${why}` : ''})`;
  };
</script>

<div class="sel" class:orange={tone === 'orange'} class:amber={tone === 'amber'} style="--w:{W}; --h:{H}; --rowh:{G.rowH}">
  <svg class="geo" viewBox="0 0 {W} {H}" aria-hidden="true">
    <defs>
      <radialGradient id="{uid}-cap" cx="45%" cy="38%" r="70%">
        <stop offset="0" stop-color="#34353a" />
        <stop offset="0.7" stop-color="#1c1d20" />
        <stop offset="1" stop-color="#141517" />
      </radialGradient>
      <linearGradient id="{uid}-rim" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#3a3b3f" />
        <stop offset="1" stop-color="#101112" />
      </linearGradient>
    </defs>
    <!-- printed leader lines from each detent to its label -->
    {#each rows as r (r.tab.id)}
      <polyline
        class="leader"
        class:on={r.tab.id === pointer}
        points="{G.cx + (G.R + 4) * Math.cos(r.a)},{cy + (G.R + 4) * Math.sin(r.a)} {r.rx},{r.y} {G.dotX - 4},{r.y}"
      />
    {/each}
    <!-- knob -->
    <circle cx={G.cx} {cy} r={G.R + 3} fill="#0a0b0c" />
    <circle cx={G.cx} {cy} r={G.R} fill="url(#{uid}-rim)" />
    {#each knurl as t, i (i)}
      <line
        class="knurl"
        x1={G.cx + (G.R - 0.5) * Math.cos(t)}
        y1={cy + (G.R - 0.5) * Math.sin(t)}
        x2={G.cx + G.R * 0.88 * Math.cos(t)}
        y2={cy + G.R * 0.88 * Math.sin(t)}
      />
    {/each}
    <circle cx={G.cx} {cy} r={G.R * 0.84} fill="url(#{uid}-cap)" stroke="#0c0d0e" stroke-width="1" />
    <g class="pointer" style="transform-origin:{G.cx}px {cy}px; transform: rotate({pointerDeg}deg)">
      <line x1={G.cx + G.R * 0.12} y1={cy} x2={G.cx + G.R * 0.8} y2={cy} class="ptr-glow" />
      <line x1={G.cx + G.R * 0.12} y1={cy} x2={G.cx + G.R * 0.8} y2={cy} class="ptr" />
    </g>
  </svg>

  <button class="knobhit" style="--cx:{G.cx}; --cy:{cy}; --r:{G.R + 3}" aria-label="Next System" onclick={() => step(1, selIdx)}></button>

  <div class="opts" role="radiogroup" aria-label="Systems">
    {#each rows as r, i (r.tab.id)}
      {@const isPtr = r.tab.id === pointer}
      {@const st = stateOf(r.tab)}
      <button
        class="opt"
        class:ptr={isPtr}
        style="--y:{r.y - G.rowH / 2}; --x:{G.dotX - 6}"
        role="radio"
        data-system={r.tab.id}
        aria-checked={isPtr}
        tabindex={isPtr || (!ptrShown && i === 0) ? 0 : -1}
        onclick={() => onselect?.(r.tab.id)}
        ondblclick={() => onlaunch?.(r.tab.id)}
        onkeydown={(e) => key(e, r.tab.id)}
        title={titleOf(r.tab)}
      >
        <span class="dot" class:on={isPtr}></span>
        <span class="name"><TabLabel tab={r.tab} /></span>
        <span class="sub">{slotSubtitle(r.tab.system)}</span>
      </button>
      <!-- state column: lit lamp = this System runs (or is selected and ready); orange = cannot launch -->
      <div class="av {st.lamp}" style="--y:{r.y - G.rowH / 2}">
        <span class="lamp"></span>
        <span class="avt">{st.word}</span>
      </div>
    {/each}
    <!-- Systems out of view: the chevrons scroll the window (they never select) and show each one's lamp -->
    {#if above > 0}
      <button class="more up" type="button" aria-label="Show {above} more above: {hiddenText(hiddenAbove)}" title={hiddenText(hiddenAbove)} onclick={() => (start = clampStart(start - 1))}>
        <Icon name="up" size="calc(10 * var(--u))" />{above}{#each hiddenLamps(hiddenAbove) as l, k (k)}<span class="ml {l}"></span>{/each}
      </button>
    {/if}
    {#if below > 0}
      <button class="more down" type="button" aria-label="Show {below} more below: {hiddenText(hiddenBelow)}" title={hiddenText(hiddenBelow)} onclick={() => (start = clampStart(start + 1))}>
        <Icon name="down" size="calc(10 * var(--u))" />{below}{#each hiddenLamps(hiddenBelow) as l, k (k)}<span class="ml {l}"></span>{/each}
      </button>
    {/if}
  </div>
</div>

<style>
  .sel {
    position: relative;
    height: calc(var(--h) * var(--u));
    min-width: calc(var(--w) * var(--u));
    --tone: #5ab6eb;
    --tone-glow: rgba(90, 182, 235, 0.28);
    --tone-line: rgba(90, 182, 235, 0.85);
    --av-w: 118;
  }
  .sel.orange {
    --tone: #ff6b2c;
    --tone-glow: rgba(255, 107, 44, 0.3);
    --tone-line: rgba(255, 107, 44, 0.85);
  }
  .sel.amber {
    --tone: #ffb02e;
    --tone-glow: rgba(255, 176, 46, 0.28);
    --tone-line: rgba(255, 176, 46, 0.8);
  }
  .geo {
    position: absolute;
    left: 0;
    top: 0;
    width: calc(var(--w) * var(--u));
    height: calc(var(--h) * var(--u));
    overflow: visible;
  }
  .leader {
    fill: none;
    stroke: rgba(237, 230, 214, 0.3);
    stroke-width: 1.1;
  }
  .leader.on {
    stroke: var(--tone-line);
  }
  .knurl {
    stroke: rgba(255, 255, 255, 0.06);
    stroke-width: 1.2;
  }
  .pointer {
    transition: transform 400ms cubic-bezier(0.3, 0.8, 0.3, 1);
  }
  .ptr {
    stroke: var(--tone);
    stroke-width: 3.6;
    stroke-linecap: round;
  }
  .ptr-glow {
    stroke: var(--tone-glow);
    stroke-width: 8;
    stroke-linecap: round;
  }
  .knobhit {
    position: absolute;
    left: calc((var(--cx) - var(--r)) * var(--u));
    top: calc((var(--cy) - var(--r)) * var(--u));
    width: calc(var(--r) * 2 * var(--u));
    height: calc(var(--r) * 2 * var(--u));
    border-radius: 50%;
    background: transparent;
    border: 0;
    padding: 0;
    cursor: pointer;
  }
  .opts {
    position: absolute;
    inset: 0;
  }
  /* One row per tier: lamp dot on the leader, the tier name in a fixed column, the model beside it. */
  .opt {
    position: absolute;
    left: calc(var(--x) * var(--u));
    right: calc((var(--av-w) + 12) * var(--u));
    top: calc(var(--y) * var(--u));
    height: calc(var(--rowh) * var(--u));
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    background: none;
    border: 0;
    padding: 0 calc(6 * var(--u)) 0 0;
    margin: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
    border-radius: calc(3 * var(--u));
    white-space: nowrap;
    min-width: 0;
    text-align: left;
  }
  .dot {
    display: inline-block;
    flex: 0 0 auto;
    width: calc(12 * var(--u));
    height: calc(12 * var(--u));
    border-radius: 50%;
    border: calc(1.5 * var(--u)) solid rgba(237, 230, 214, 0.55);
    background: #141517;
  }
  .dot.sel {
    border-color: var(--cyan);
  }
  .dot.on {
    border-color: #0b0c0d;
    background: radial-gradient(circle at 45% 40%, #bfe6fb 0%, #5ab6eb 50%, #3d9ed4 100%);
    box-shadow: 0 0 calc(8 * var(--u)) rgba(90, 182, 235, 0.65);
  }
  .orange .dot.on {
    background: radial-gradient(circle at 45% 40%, #ffd2bd 0%, #ff6b2c 50%, #d84e14 100%);
    box-shadow: 0 0 calc(8 * var(--u)) rgba(255, 107, 44, 0.65);
  }
  .amber .dot.on {
    background: radial-gradient(circle at 45% 40%, #fff0c8 0%, #ffb02e 50%, #d98a0a 100%);
    box-shadow: 0 0 calc(8 * var(--u)) rgba(255, 176, 46, 0.6);
  }
  .name {
    flex: 0 0 calc(104 * var(--u));
    min-width: 0;
    overflow: hidden;
    font-family: var(--font-label);
    font-weight: 600;
    font-size: var(--fs-tier);
    letter-spacing: 0.07em;
    line-height: 1;
    color: rgba(237, 230, 214, 0.86);
  }
  .opt.ptr .name {
    color: var(--tone);
  }
  .sub {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--font-text);
    font-weight: 400;
    font-size: var(--fs-body);
    line-height: 1.2;
    color: rgba(237, 230, 214, 0.6);
  }
  .opt.ptr .sub {
    color: rgba(237, 230, 214, 0.82);
  }
  .opt:hover .name {
    color: var(--cream);
  }
  .opt.ptr:hover .name {
    color: #7cc8f5;
  }
  .orange .opt.ptr:hover .name,
  .amber .opt.ptr:hover .name,
  /* state column */
  .av {
    position: absolute;
    right: calc(14 * var(--u));
    top: calc(var(--y) * var(--u));
    height: calc(var(--rowh) * var(--u));
    width: calc(var(--av-w) * var(--u));
    display: flex;
    align-items: center;
    gap: calc(9 * var(--u));
    font-family: var(--font-text);
    font-size: var(--fs-small);
    letter-spacing: 0.01em;
    color: rgba(237, 230, 214, 0.6);
    white-space: nowrap;
    overflow: hidden;
    pointer-events: none;
  }
  .avt {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lamp {
    flex: 0 0 auto;
    width: calc(10 * var(--u));
    height: calc(10 * var(--u));
    border-radius: 50%;
    background: radial-gradient(circle at 50% 42%, #6b6862 0%, #45433f 70%);
    box-shadow: 0 0 0 calc(1.4 * var(--u)) #0b0c0d;
  }
  .av.cyan {
    color: var(--cyan);
  }
  .av.cyan .lamp {
    background: radial-gradient(circle at 50% 42%, #bfe6fb 0%, #5ab6eb 50%, #3d9ed4 100%);
    box-shadow:
      0 0 0 calc(1.4 * var(--u)) #0b0c0d,
      0 0 calc(8 * var(--u)) rgba(90, 182, 235, 0.65);
  }
  .av.orange {
    color: var(--orange);
  }
  .av.orange .lamp {
    background: radial-gradient(circle at 50% 42%, #ffd2bd 0%, #ff6b2c 50%, #d84e14 100%);
    box-shadow:
      0 0 0 calc(1.4 * var(--u)) #0b0c0d,
      0 0 calc(8 * var(--u)) rgba(255, 107, 44, 0.6);
  }
  .av.amber {
    color: var(--amber);
  }
  .av.amber .lamp {
    background: radial-gradient(circle at 50% 42%, #fff0c8 0%, #ffb02e 50%, #d98a0a 100%);
    box-shadow:
      0 0 0 calc(1.4 * var(--u)) #0b0c0d,
      0 0 calc(8 * var(--u)) rgba(255, 176, 46, 0.6);
  }
  /* more Systems out of view above / below the four shown */
  .more {
    position: absolute;
    left: calc(var(--x) * var(--u));
    display: flex;
    align-items: center;
    gap: calc(3 * var(--u));
    padding: 0 calc(4 * var(--u));
    background: none;
    border: 0;
    margin: 0;
    height: calc(10 * var(--u));
    font-family: var(--font-text);
    font-size: calc(10 * var(--u));
    color: rgba(237, 230, 214, 0.5);
    cursor: pointer;
  }
  .more:hover {
    color: var(--cream);
  }
  .more.up {
    top: calc(-8 * var(--u));
  }
  /* one small lamp per System out of view (same tones as the state column) */
  .ml {
    flex: none;
    width: calc(6 * var(--u));
    height: calc(6 * var(--u));
    border-radius: 50%;
    background: #45433f;
  }
  .ml:first-of-type {
    margin-left: calc(3 * var(--u));
  }
  .ml.cyan {
    background: #5ab6eb;
    box-shadow: 0 0 calc(5 * var(--u)) rgba(90, 182, 235, 0.65);
  }
  .ml.orange {
    background: #ff6b2c;
    box-shadow: 0 0 calc(5 * var(--u)) rgba(255, 107, 44, 0.6);
  }
  .ml.amber {
    background: #ffb02e;
    box-shadow: 0 0 calc(5 * var(--u)) rgba(255, 176, 46, 0.6);
  }
  .more.down {
    bottom: calc(-8 * var(--u));
  }
</style>
