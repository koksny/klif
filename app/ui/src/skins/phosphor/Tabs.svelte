<script lang="ts">
  // Job selector. The three LLM slots are tiers (AGENT HIGH / MEDIUM / LOW); the model behind each
  // tier is swappable, so its current model is printed under the tier name.
  // While a session boots or stops the other tiers are locked (padlock); in a fault the running tier
  // is outlined in the fault colour.
  import type { Actions, Slot, SlotId } from '../../lib/model/types';
  import { AVAIL_TEXT } from './geom';

  let {
    slots,
    selected,
    running,
    actions,
    locked = false,
    tone = 'cyan',
  }: {
    slots: Slot[];
    selected: SlotId;
    running: SlotId | null;
    actions: Actions;
    locked?: boolean;
    tone?: 'cyan' | 'danger';
  } = $props();

  function onKey(e: KeyboardEvent, i: number) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    e.preventDefault();
    const n = slots.length;
    const j = (i + (e.key === 'ArrowRight' ? 1 : n - 1)) % n;
    const el = (e.currentTarget as HTMLElement).parentElement?.children[j] as HTMLElement | undefined;
    el?.focus();
  }
</script>

<div class="tabs" role="tablist" aria-label="Job">
  {#each slots as s, i (s.id)}
    {@const lock = locked && s.id !== selected}
    <button
      role="tab"
      class="tab {tone}"
      class:on={s.id === selected}
      class:lock
      aria-selected={s.id === selected}
      aria-disabled={lock}
      tabindex={s.id === selected ? 0 : -1}
      onclick={() => (lock ? undefined : actions.select(s.id))}
      ondblclick={() => (running || s.availability !== 'ready' ? undefined : actions.launch(s.id))}
      onkeydown={(e) => onKey(e, i)}
      title={lock
        ? `${s.model.name} (locked while a session starts or stops)`
        : running || s.availability !== 'ready'
          ? s.model.name
          : `${s.model.name} (double-click to launch)`}
    >
      <span class="name">
        {#if lock}
          <svg class="padlock" viewBox="0 0 12 14" aria-label="locked"><rect x="1" y="6" width="10" height="7.5" rx="1.2" /><path d="M3.2 6V4.2a2.8 2.8 0 0 1 5.6 0V6" /></svg>
        {:else if s.id === running}<span class="run" aria-label="running"></span>{/if}{s.label}
      </span>
      <span class="model" class:warn={s.availability !== 'ready'}>
        {s.availability === 'ready' ? s.model.name : (AVAIL_TEXT[s.availability] ?? s.availability)}
      </span>
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    align-items: stretch;
    gap: calc(8px * var(--k));
    flex: none;
    height: calc(50px * var(--k));
    padding-bottom: calc(5px * var(--k));
    border-bottom: 1px solid var(--ph-rule);
  }
  .tab {
    flex: 1 1 0;
    min-width: 0;
    display: grid;
    align-content: center;
    justify-items: center;
    gap: calc(2px * var(--k));
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 0 calc(8px * var(--k));
    color: var(--ph-brand);
  }
  .tab:hover:not(.lock) {
    border-color: rgba(23, 79, 92, 0.9);
  }
  .tab.lock {
    cursor: not-allowed;
    color: var(--ph-muted);
  }
  .tab.on {
    border: 1.5px solid var(--ph-cyan);
    background: rgba(18, 48, 58, 0.35);
    box-shadow:
      0 0 10px rgba(127, 227, 255, 0.25),
      inset 0 0 12px rgba(127, 227, 255, 0.12);
    color: var(--ph-cyan);
  }
  .tab.on.danger {
    border-color: var(--ph-danger);
    background: rgba(60, 14, 14, 0.3);
    box-shadow:
      0 0 12px rgba(229, 97, 92, 0.35),
      inset 0 0 12px rgba(229, 97, 92, 0.14);
    color: #ff8f88;
  }
  .name {
    display: flex;
    align-items: center;
    gap: calc(8px * var(--k));
    font-size: calc(18px * var(--k));
    letter-spacing: 0.1em;
    white-space: nowrap;
  }
  .on .name {
    text-shadow: var(--ph-glow);
  }
  .on.danger .name {
    text-shadow: 0 0 6px rgba(229, 97, 92, 0.55);
  }
  .run {
    width: calc(7px * var(--k));
    height: calc(7px * var(--k));
    border-radius: 50%;
    background: var(--ph-cyan);
    box-shadow: 0 0 6px var(--ph-cyan);
  }
  .danger .run {
    background: var(--ph-danger);
    box-shadow: 0 0 6px var(--ph-danger);
  }
  .padlock {
    width: calc(12px * var(--k));
    height: calc(14px * var(--k));
    flex: none;
  }
  .padlock rect,
  .padlock path {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    vector-effect: non-scaling-stroke;
  }
  .padlock rect {
    fill: rgba(79, 152, 180, 0.25);
  }
  .model {
    font-size: max(10.5px, calc(12.5px * var(--k)));
    letter-spacing: 0.04em;
    color: var(--ph-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .on .model {
    color: var(--ph-brand);
  }
  .on.danger .model {
    color: #e9a29d;
  }
  .model.warn {
    color: var(--ph-amber);
  }
</style>
