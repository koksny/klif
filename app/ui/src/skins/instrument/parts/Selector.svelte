<script lang="ts">
  // Rotary MODE selector: a knob with one detent per tier, printed leader lines to each label, and a
  // state column of lamps on the right. One layout in every phase (idle, loading, live, fault).
  // Pointer = the running tier (or the launcher's selection while idle). Click selects, double-click asks
  // to launch (the caller decides whether that is allowed right now).
  // Per tier the state column reads: ready / cannot launch (why) / the running tier's phase / locked while
  // a session starts or stops. Locked clicks still reach onselect so the core can say why it refuses.
  import type { Slot, SlotId } from '../../../lib/model/types';
  import { availLabel, slotSubtitle } from '../theme';
  import Icon from './Icon.svelte';

  let {
    slots,
    pointer,
    selected,
    running,
    runWord = 'running',
    locked = false,
    tone = 'cyan',
    onselect,
    onlaunch,
  }: {
    slots: Slot[];
    pointer: SlotId;
    selected: SlotId;
    running: SlotId | null;
    /** What the running tier is doing: "starting", "running", "stopping", "fault", "asleep". */
    runWord?: string;
    /** A session is starting or stopping: every other tier is locked. */
    locked?: boolean;
    /** orange = the running tier faulted; amber = its GPU is dormant. */
    tone?: 'cyan' | 'orange' | 'amber';
    onselect?: (id: SlotId) => void;
    onlaunch?: (id: SlotId) => void;
  } = $props();

  const uid = $props.id();

  // Geometry in unscaled px (multiplied by --u in CSS).
  const G = { rowH: 25, R: 41, cx: 50, rho: 52, dotX: 136, pad: 3 };
  const W = G.dotX;
  const n = $derived(Math.max(1, slots.length));
  const H = $derived(G.pad * 2 + n * G.rowH);
  const cy = $derived(H / 2);
  const rows = $derived(
    slots.map((s, i) => {
      const y = G.pad + G.rowH * (i + 0.5);
      const off = y - cy;
      const a = Math.asin(Math.max(-0.97, Math.min(0.97, off / G.rho))); // radians, screen y down
      return { slot: s, y, a, deg: (a * 180) / Math.PI, rx: G.cx + G.rho * Math.cos(a) };
    }),
  );
  const pointerRow = $derived(rows.find((r) => r.slot.id === pointer) ?? rows[0]);
  const knurl = Array.from({ length: 56 }, (_, i) => (i / 56) * Math.PI * 2);

  function key(e: KeyboardEvent, i: number) {
    const dir =
      e.key === 'ArrowDown' || e.key === 'ArrowRight' ? 1 : e.key === 'ArrowUp' || e.key === 'ArrowLeft' ? -1 : 0;
    if (!dir) return;
    e.preventDefault();
    const j = (i + dir + rows.length) % rows.length;
    onselect?.(rows[j].slot.id);
    const list = (e.currentTarget as HTMLElement).closest('.opts')?.querySelectorAll<HTMLButtonElement>('button.opt');
    list?.[j]?.focus();
  }

  /** The state column of one tier: lamp tone and word. */
  function stateOf(slot: Slot): { lamp: 'off' | 'cyan' | 'orange' | 'amber'; word: string; lock: boolean } {
    if (slot.id === running) return { lamp: tone, word: runWord, lock: false };
    if (locked) return { lamp: 'off', word: 'locked', lock: true };
    if (slot.availability !== 'ready') return { lamp: 'orange', word: availLabel(slot.availability), lock: false };
    return { lamp: slot.id === pointer && !running ? 'cyan' : 'off', word: 'ready', lock: false };
  }
</script>

<div class="sel" class:orange={tone === 'orange'} class:amber={tone === 'amber'} class:locked style="--w:{W}; --h:{H}; --rowh:{G.rowH}">
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
    {#each rows as r (r.slot.id)}
      <polyline
        class="leader"
        class:on={r.slot.id === pointer}
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
    <g class="pointer" style="transform-origin:{G.cx}px {cy}px; transform: rotate({pointerRow?.deg ?? 0}deg)">
      <line x1={G.cx + G.R * 0.12} y1={cy} x2={G.cx + G.R * 0.8} y2={cy} class="ptr-glow" />
      <line x1={G.cx + G.R * 0.12} y1={cy} x2={G.cx + G.R * 0.8} y2={cy} class="ptr" />
    </g>
  </svg>

  <button
    class="knobhit"
    style="--cx:{G.cx}; --cy:{cy}; --r:{G.R + 3}"
    aria-label="Next tier"
    onclick={() => {
      const i = rows.findIndex((r) => r.slot.id === selected);
      onselect?.(rows[(i + 1) % rows.length].slot.id);
    }}
  ></button>

  <div class="opts" role="radiogroup" aria-label="Tier">
    {#each rows as r, i (r.slot.id)}
      {@const isSel = r.slot.id === selected}
      {@const isPtr = r.slot.id === pointer}
      {@const st = stateOf(r.slot)}
      {@const ready = r.slot.availability === 'ready'}
      <button
        class="opt"
        class:ptr={isPtr}
        style="--y:{r.y - G.rowH / 2}; --x:{G.dotX - 6}"
        role="radio"
        aria-checked={isSel}
        tabindex={isSel ? 0 : -1}
        onclick={() => onselect?.(r.slot.id)}
        ondblclick={() => onlaunch?.(r.slot.id)}
        onkeydown={(e) => key(e, i)}
        title={st.lock ? `${r.slot.label}: locked while the session ${runWord === 'stopping' ? 'stops' : 'starts'}` : ready ? `${r.slot.label}: ${r.slot.model.name}` : `${r.slot.label}: ${r.slot.reason ?? availLabel(r.slot.availability)}`}
      >
        <span class="dot" class:on={isPtr} class:sel={isSel && !isPtr}></span>
        <span class="name">{r.slot.label}</span>
        <span class="sub">{slotSubtitle(r.slot)}</span>
      </button>
      <!-- state column: lit lamp = this tier runs (or is selected and ready); orange = cannot launch -->
      <div class="av {st.lamp}" class:lock={st.lock} style="--y:{r.y - G.rowH / 2}">
        {#if st.lock}<span class="lk"><Icon name="lock" size="calc(12 * var(--u))" /></span>{:else}<span class="lamp"></span>{/if}
        <span class="avt">{st.word}</span>
      </div>
    {/each}
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
  .locked .opt:not(.ptr) {
    cursor: default;
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
  .locked .opt.ptr:hover .name {
    color: var(--tone);
  }
  .locked .opt:not(.ptr) .name,
  .locked .opt:not(.ptr) .sub {
    color: rgba(237, 230, 214, 0.38);
  }
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
  .lk {
    display: flex;
    width: calc(10 * var(--u));
    justify-content: center;
    color: rgba(237, 230, 214, 0.45);
  }
  .av.lock {
    color: rgba(237, 230, 214, 0.42);
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
</style>
