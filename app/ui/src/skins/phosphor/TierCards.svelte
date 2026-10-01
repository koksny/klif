<script lang="ts">
  // Idle launcher: the four job slots as 2x2 cards. The selected tier glows; a slot whose model is
  // not available says why and cannot be launched. Click selects, double-click launches.
  import type { Actions, Slot, SlotId } from '../../lib/model/types';
  import { fmtGiB } from '../../lib/model/format';
  import { AVAIL_TEXT, modelLine } from './geom';

  let { slots, selected, actions }: { slots: Slot[]; selected: SlotId; actions: Actions } = $props();

  const need = (s: Slot) => (s.expectedVram ?? []).reduce((a, l) => a + l.gib, 0);

  function onKey(e: KeyboardEvent, i: number) {
    const step = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: 2, ArrowUp: -2 }[e.key];
    if (step === undefined) return;
    e.preventDefault();
    const n = slots.length;
    const j = (((i + step) % n) + n) % n;
    const el = (e.currentTarget as HTMLElement).parentElement?.children[j] as HTMLElement | undefined;
    el?.focus();
    actions.select(slots[j].id);
  }
</script>

<div class="cards" role="radiogroup" aria-label="Job">
  {#each slots as s, i (s.id)}
    {@const ok = s.availability === 'ready'}
    {@const g = need(s)}
    <button
      role="radio"
      class="card panel"
      class:on={s.id === selected}
      class:na={!ok}
      aria-checked={s.id === selected}
      tabindex={s.id === selected ? 0 : -1}
      onclick={() => actions.select(s.id)}
      ondblclick={() => (ok ? actions.launch(s.id) : undefined)}
      onkeydown={(e) => onKey(e, i)}
      title={ok ? `${s.model.name} (double-click to launch)` : `${s.model.name}: ${AVAIL_TEXT[s.availability] ?? s.availability}`}
    >
      <span class="tier">{s.label}</span>
      <span class="mline">
        {#each modelLine(s.model) as part, j}
          {#if j > 0}<span class="sep" aria-hidden="true">·</span>{/if}<span class="part">{part}</span>
        {/each}
      </span>
      <span class="av">
        {#if ok}
          <span class="dot" aria-hidden="true"></span><span>ready</span>
        {:else}
          <svg class="tri" viewBox="0 0 14 12" aria-hidden="true"><path d="M7 1L13 11H1Z" /></svg><span>{AVAIL_TEXT[s.availability] ?? s.availability}</span>
        {/if}
        {#if g > 0}<span class="need" title="Expected VRAM if launched with the current recipe">needs {fmtGiB(g)} GiB</span>{/if}
      </span>
    </button>
  {/each}
</div>

<style>
  .cards {
    flex: none;
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-auto-rows: calc(112px * var(--k));
    gap: calc(12px * var(--k)) calc(18px * var(--k));
  }
  .card {
    display: grid;
    align-content: center;
    justify-items: start;
    gap: calc(8px * var(--k));
    padding: 0 calc(24px * var(--k));
    text-align: left;
    color: var(--ph-cyan);
    min-width: 0;
  }
  .card:hover:not(.on) {
    border-color: #2a7f93;
  }
  .card.on {
    border: 2px solid var(--ph-cyan);
    background-color: rgba(10, 34, 42, 0.78);
    box-shadow:
      0 0 14px rgba(127, 227, 255, 0.35),
      0 0 34px rgba(90, 182, 235, 0.14),
      inset 0 0 22px rgba(127, 227, 255, 0.12);
  }
  .tier {
    font-family: var(--ph-display);
    font-stretch: 118%;
    font-weight: 420;
    font-size: calc(33px * var(--k));
    line-height: 1;
    letter-spacing: 0.06em;
    color: var(--ph-cyan);
    text-shadow: 0 0 8px rgba(127, 227, 255, 0.35);
    white-space: nowrap;
  }
  .on .tier {
    color: #b9f1ff;
    text-shadow:
      0 0 8px rgba(127, 227, 255, 0.6),
      0 0 22px rgba(90, 182, 235, 0.3);
  }
  .na .tier {
    color: var(--ph-muted);
    text-shadow: none;
  }
  .mline {
    display: flex;
    flex-wrap: nowrap;
    column-gap: calc(9px * var(--k));
    max-width: 100%;
    overflow: hidden;
    font-size: calc(18px * var(--k));
    letter-spacing: 0.04em;
    color: var(--ph-cyan);
    opacity: 0.92;
    white-space: nowrap;
  }
  .na .mline {
    color: var(--ph-muted);
  }
  .sep {
    color: var(--ph-muted);
  }
  .part {
    white-space: nowrap;
  }
  .av {
    display: flex;
    align-items: center;
    gap: calc(10px * var(--k));
    width: 100%;
    font-size: calc(17px * var(--k));
    letter-spacing: 0.05em;
    color: var(--ph-cyan);
  }
  .dot {
    width: calc(9px * var(--k));
    height: calc(9px * var(--k));
    margin: 0 calc(3px * var(--k));
    border-radius: 50%;
    background: var(--ph-cyan);
    box-shadow: 0 0 6px var(--ph-cyan);
  }
  .tri {
    width: calc(15px * var(--k));
    height: calc(13px * var(--k));
  }
  .tri path {
    fill: rgba(232, 176, 74, 0.2);
    stroke: var(--ph-amber);
    stroke-width: 1.4;
    vector-effect: non-scaling-stroke;
  }
  .na .av {
    color: var(--ph-amber);
  }
  .need {
    margin-left: auto;
    font-size: calc(14px * var(--k));
    color: var(--ph-muted);
    white-space: nowrap;
  }
</style>
