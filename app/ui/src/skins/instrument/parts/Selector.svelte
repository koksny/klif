<script lang="ts">
  // Rotary job selector: a knob with one detent per slot, printed leader lines to each label.
  // Pointer = the running slot (or the launcher's selection while idle). Full size: interactive radio
  // group (select / launch). Mini: read-only indicator.
  //   layout 'inline' : one line per tier (label + model subtitle), used while a session exists.
  //   layout 'stack'  : the idle launcher, two lines per tier plus an availability column with lamps.
  //   locked          : a launch is in progress: the tiers carry a lock (as in the loading mockup). Clicks
  //                     still reach actions.select so the core can say why it refuses.
  //   tone 'orange'   : the pointed tier faulted.
  import type { Slot, SlotId } from '../../../lib/model/types';
  import { availLabel, shortLabel, slotSubtitle } from '../theme';
  import Icon from './Icon.svelte';

  let {
    slots,
    pointer,
    selected,
    running,
    variant = 'full',
    layout = 'inline',
    locked = false,
    tone = 'cyan',
    onselect,
    onlaunch,
  }: {
    slots: Slot[];
    pointer: SlotId;
    selected: SlotId;
    running: SlotId | null;
    variant?: 'full' | 'mini';
    layout?: 'inline' | 'stack';
    locked?: boolean;
    tone?: 'cyan' | 'orange';
    onselect?: (id: SlotId) => void;
    onlaunch?: (id: SlotId) => void;
  } = $props();

  const uid = $props.id();
  const interactive = $derived(variant === 'full' && !!onselect);
  const stack = $derived(variant === 'full' && layout === 'stack');

  // Geometry in unscaled px (multiplied by --u in CSS).
  const G = $derived(
    variant === 'mini'
      ? { rowH: 37, R: 60, cx: 64, rho: 72, dotX: 150, pad: 4 }
      : stack
        ? { rowH: 46, R: 54, cx: 62, rho: 66, dotX: 172, pad: 4 }
        : { rowH: 27.5, R: 46, cx: 56, rho: 56, dotX: 160, pad: 3 },
  );
  const W = $derived(G.dotX);
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
    if (!interactive) return;
    const dir =
      e.key === 'ArrowDown' || e.key === 'ArrowRight' ? 1 : e.key === 'ArrowUp' || e.key === 'ArrowLeft' ? -1 : 0;
    if (!dir) return;
    e.preventDefault();
    const j = (i + dir + rows.length) % rows.length;
    onselect?.(rows[j].slot.id);
    const list = (e.currentTarget as HTMLElement).closest('.opts')?.querySelectorAll<HTMLButtonElement>('button.opt');
    list?.[j]?.focus();
  }
</script>

<div class="sel {variant}" class:stack class:orange={tone === 'orange'} class:locked style="--w:{W}; --h:{H}; --rowh:{G.rowH}">
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

  {#if interactive}
    <button
      class="knobhit"
      style="--cx:{G.cx}; --cy:{cy}; --r:{G.R + 3}"
      aria-label="Next job"
      onclick={() => {
        const i = rows.findIndex((r) => r.slot.id === selected);
        onselect?.(rows[(i + 1) % rows.length].slot.id);
      }}
    ></button>
  {/if}

  <div class="opts" role={variant === 'full' ? 'radiogroup' : undefined} aria-label="Job">
    {#each rows as r, i (r.slot.id)}
      {@const isRun = r.slot.id === running}
      {@const isSel = r.slot.id === selected}
      {@const isPtr = r.slot.id === pointer}
      {@const ready = r.slot.availability === 'ready'}
      {#if variant === 'full'}
        <div class="row" style="--y:{r.y - G.rowH / 2}; --x:{G.dotX - 7}">
          <button
            class="opt"
            class:ptr={isPtr}
            role="radio"
            aria-checked={isSel}
            tabindex={isSel ? 0 : -1}
            onclick={() => onselect?.(r.slot.id)}
            onkeydown={(e) => key(e, i)}
            title={locked ? 'Stop the running session to change the tier' : undefined}
          >
            <span class="dot" class:on={isPtr} class:sel={isSel && !isPtr}></span>
            {#if locked}<span class="lk"><Icon name="lock" size="calc(15 * var(--u))" /></span>{/if}
            {#if stack}
              <span class="two">
                <span class="name">{r.slot.label}</span>
                <span class="sub">{slotSubtitle(r.slot)}</span>
              </span>
            {:else}
              <span class="name">{r.slot.label}</span>
              <span class="sub" class:bad={!ready}>{ready ? slotSubtitle(r.slot) : availLabel(r.slot.availability)}</span>
            {/if}
          </button>
          {#if !stack && isSel && running && !isRun && ready && onlaunch}
            <button class="launch" onclick={() => onlaunch?.(r.slot.id)}>launch</button>
          {/if}
        </div>
        {#if stack}
          <!-- availability column: lit lamp = ready (cyan on the selected tier), orange = cannot launch -->
          <div class="av" class:bad={!ready} class:on={isPtr && ready} style="--y:{r.y - G.rowH / 2}">
            <span class="lamp"></span>
            <span class="avt">{availLabel(r.slot.availability)}</span>
          </div>
        {/if}
      {:else}
        <div class="row ro" class:ptr={isPtr} style="--y:{r.y - G.rowH / 2}; --x:{G.dotX - 11}">
          <span class="dot" class:on={isPtr}></span>
          <span class="name">{shortLabel(r.slot.label)}</span>
        </div>
      {/if}
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
  }
  .sel.orange {
    --tone: #ff6b2c;
    --tone-glow: rgba(255, 107, 44, 0.3);
    --tone-line: rgba(255, 107, 44, 0.85);
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
  .mini .leader {
    stroke-width: 1.8;
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
  .stack .ptr {
    stroke-width: 4.2;
  }
  .mini .ptr {
    stroke-width: 5.5;
  }
  .mini .ptr-glow {
    stroke-width: 11;
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
  .row {
    position: absolute;
    left: calc(var(--x) * var(--u));
    top: calc(var(--y) * var(--u));
    height: calc(28 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(10 * var(--u));
    white-space: nowrap;
  }
  .stack .row {
    height: calc(var(--rowh) * var(--u));
  }
  .opt {
    height: 100%;
    display: flex;
    align-items: center;
    gap: calc(11 * var(--u));
    background: none;
    border: 0;
    padding: 0 calc(6 * var(--u)) 0 0;
    margin: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
    border-radius: calc(4 * var(--u));
  }
  .locked .opt {
    cursor: default;
  }
  .stack .opt {
    gap: calc(14 * var(--u));
  }
  .dot {
    display: inline-block;
    flex: 0 0 auto;
    width: calc(14 * var(--u));
    height: calc(14 * var(--u));
    border-radius: 50%;
    border: calc(1.6 * var(--u)) solid rgba(237, 230, 214, 0.55);
    background: #141517;
  }
  .stack .dot {
    width: calc(16 * var(--u));
    height: calc(16 * var(--u));
  }
  .dot.sel {
    border-color: var(--cyan);
  }
  .dot.on {
    border-color: #0b0c0d;
    background: radial-gradient(circle at 45% 40%, #bfe6fb 0%, #5ab6eb 50%, #3d9ed4 100%);
    box-shadow: 0 0 calc(9 * var(--u)) rgba(90, 182, 235, 0.65);
  }
  .orange .dot.on {
    background: radial-gradient(circle at 45% 40%, #ffd2bd 0%, #ff6b2c 50%, #d84e14 100%);
    box-shadow: 0 0 calc(9 * var(--u)) rgba(255, 107, 44, 0.65);
  }
  .lk {
    display: flex;
    color: rgba(237, 230, 214, 0.42);
    margin: 0 calc(-2 * var(--u));
  }
  .two {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: calc(4 * var(--u));
  }
  .name {
    font-weight: 500;
    font-size: calc(18.5 * var(--u));
    letter-spacing: 0.06em;
    color: rgba(237, 230, 214, 0.86);
  }
  .stack .name {
    font-size: calc(20 * var(--u));
    line-height: 1;
  }
  .opt.ptr .name {
    color: var(--tone);
  }
  .sub {
    font-weight: 400;
    font-size: calc(15 * var(--u));
    letter-spacing: 0.02em;
    color: var(--muted);
  }
  .stack .sub {
    font-size: calc(16 * var(--u));
    line-height: 1;
    color: rgba(237, 230, 214, 0.62);
  }
  .sub.bad {
    color: rgba(255, 107, 44, 0.85);
  }
  .opt:hover .name {
    color: var(--cream);
  }
  .opt.ptr:hover .name {
    color: #7cc8f5;
  }
  .orange .opt.ptr:hover .name,
  .locked .opt.ptr:hover .name {
    color: var(--tone);
  }
  .locked .opt:not(.ptr):hover .name {
    color: rgba(237, 230, 214, 0.86);
  }
  .launch {
    height: calc(22 * var(--u));
    padding: 0 calc(10 * var(--u));
    border-radius: calc(3 * var(--u));
    border: 1px solid rgba(90, 182, 235, 0.7);
    background: rgba(90, 182, 235, 0.1);
    color: var(--cyan);
    font: 600 calc(14 * var(--u)) / 1 var(--font-label);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    cursor: pointer;
  }
  /* availability column (stack layout) */
  .av {
    position: absolute;
    right: calc(var(--av-right, 44) * var(--u));
    top: calc(var(--y) * var(--u));
    height: calc(var(--rowh) * var(--u));
    width: calc(var(--av-w, 150) * var(--u));
    display: flex;
    align-items: center;
    gap: calc(12 * var(--u));
    font-size: calc(17 * var(--u));
    letter-spacing: 0.03em;
    color: rgba(237, 230, 214, 0.72);
    white-space: nowrap;
  }
  .lamp {
    flex: 0 0 auto;
    width: calc(13 * var(--u));
    height: calc(13 * var(--u));
    border-radius: 50%;
    background: radial-gradient(circle at 50% 42%, #c9c3b5 0%, #8e8a82 70%);
    box-shadow:
      0 0 0 calc(1.6 * var(--u)) #0b0c0d,
      0 0 calc(5 * var(--u)) rgba(237, 230, 214, 0.18);
  }
  .av.on {
    color: var(--cyan);
  }
  .av.on .lamp {
    background: radial-gradient(circle at 50% 42%, #bfe6fb 0%, #5ab6eb 50%, #3d9ed4 100%);
    box-shadow:
      0 0 0 calc(1.6 * var(--u)) #0b0c0d,
      0 0 calc(9 * var(--u)) rgba(90, 182, 235, 0.65);
  }
  .av.bad {
    color: var(--orange);
  }
  .av.bad .lamp {
    background: radial-gradient(circle at 50% 42%, #ffd2bd 0%, #ff6b2c 50%, #d84e14 100%);
    box-shadow:
      0 0 0 calc(1.6 * var(--u)) #0b0c0d,
      0 0 calc(9 * var(--u)) rgba(255, 107, 44, 0.6);
  }
  /* mini: read-only indicator, 30 px minimum type */
  .mini .row {
    height: calc(37 * var(--u));
    gap: calc(14 * var(--u));
  }
  .mini .dot {
    width: calc(20 * var(--u));
    height: calc(20 * var(--u));
    border-width: calc(2.5 * var(--u));
  }
  .mini .name {
    font-weight: 600;
    font-size: calc(31 * var(--u));
    letter-spacing: 0.03em;
    line-height: 1;
  }
  .mini .row.ptr .name {
    color: var(--cream);
  }
</style>
