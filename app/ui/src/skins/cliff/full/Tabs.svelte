<script lang="ts">
  // Job selector. LLM slots are tiers (AGENT HIGH / MEDIUM / LOW); the model behind each tier is
  // the subtitle. Click selects; double-click launches when nothing is running.
  import type { Actions, SlotId, ViewModel } from '../../../lib/model/types';
  import { availabilityText } from '../util';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();
  /** While a session boots the other tiers are locked (select is refused until it is stopped). */
  const booting = $derived(!!vm.session && (vm.session.phase === 'starting' || vm.session.phase === 'loading'));
  const faulted = $derived(vm.session?.phase === 'fault');

  function onKey(e: KeyboardEvent, id: SlotId) {
    const i = vm.slots.findIndex((s) => s.id === id);
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const n = vm.slots.length;
      const next = vm.slots[(i + (e.key === 'ArrowRight' ? 1 : n - 1)) % n];
      actions.select(next.id);
      const el = (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`[data-slot="${next.id}"]`);
      el?.focus();
    }
  }
</script>

<div class="tabs" role="tablist" aria-label="Job">
  {#each vm.slots as slot (slot.id)}
    {@const sel = slot.id === vm.selected}
    {@const running = vm.session?.slot === slot.id}
    {@const why = availabilityText(slot.availability)}
    {@const locked = booting && !running}
    <button
      role="tab"
      data-slot={slot.id}
      aria-selected={sel}
      tabindex={sel ? 0 : -1}
      class="tab"
      class:sel
      class:na={!!why}
      class:locked
      class:hot={running && faulted}
      title="{slot.label}: {slot.model.name} · {slot.model.quant}{running ? ' (running)' : ''}{locked ? ' (locked while loading)' : ''}{why ? ` (${why})` : ''}"
      onclick={() => actions.select(slot.id)}
      ondblclick={() => {
        if (!vm.session && !why) actions.launch(slot.id);
      }}
      onkeydown={(e) => onKey(e, slot.id)}
    >
      <span class="lbl"
        >{#if locked}<svg class="lock" viewBox="0 0 12 14" aria-label="locked"
            ><rect x="1.5" y="6" width="9" height="7" rx="1.2" /><path d="M3.6 6V4.3a2.4 2.4 0 0 1 4.8 0V6" /></svg
          >{/if}{slot.label}{#if running}<i class="run" aria-label="running"></i>{/if}</span
      >
      <span class="sub">{slot.model.name}{#if why}{` · ${why}`}{/if}</span>
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    gap: max(6px, calc(var(--u) * 14));
    margin-top: max(3px, calc(var(--u) * 5));
    border-bottom: 1px solid var(--rule);
    min-width: 0;
  }
  .tab {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: max(1px, calc(var(--u) * 2));
    padding: max(4px, calc(var(--u) * 5)) max(10px, calc(var(--u) * 18)) max(6px, calc(var(--u) * 8));
    min-width: 0;
    border-radius: 6px 6px 0 0;
    text-align: left;
  }
  .tab:hover {
    background: rgba(220, 239, 248, 0.035);
  }
  .tab::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: -1px;
    height: 3px;
    border-radius: 2px;
    background: var(--sky);
    transform: scaleX(0);
    transition: transform 200ms ease-out;
  }
  .tab.sel::after {
    transform: scaleX(1);
  }
  .lbl {
    display: inline-flex;
    align-items: center;
    gap: 0.5em;
    font-family: var(--f-ui);
    font-weight: 500;
    font-size: max(12px, calc(var(--u) * 15.5));
    letter-spacing: 0.06em;
    color: #c3d3db;
    white-space: nowrap;
  }
  .sel .lbl {
    color: var(--sky);
    font-weight: 600;
  }
  .sub {
    font-family: var(--f-data);
    font-size: max(10.5px, calc(var(--u) * 11.5));
    color: var(--muted);
    letter-spacing: 0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .na .sub {
    color: var(--amber);
  }
  .locked .lbl,
  .locked .sub {
    color: #7d8f99;
  }
  .lock {
    width: 0.78em;
    height: 0.9em;
    margin-right: 0.1em;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
  }
  .lock rect {
    fill: currentColor;
    fill-opacity: 0.25;
  }
  .hot::after {
    background: var(--amber);
  }
  .hot.sel .lbl {
    color: var(--amber);
  }
  .hot .run {
    background: var(--amber);
  }
  .run {
    width: 0.45em;
    height: 0.45em;
    border-radius: 50%;
    background: var(--sky);
  }
</style>
