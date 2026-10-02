<script lang="ts">
  // Tier strip: the same four tabs in every phase. LLM slots are tiers (AGENT HIGH / MEDIUM / LOW); the
  // model behind each tier is the second line. The dot is the tier's state: kelp = ready, amber ring =
  // cannot launch, sky = this tier runs. Click selects; double-click launches when nothing runs (or after
  // a fault). While a session starts or stops the other tiers are locked; after a fault its tier is red.
  import type { Actions, SlotId, ViewModel } from '../../../lib/model/types';
  import { availabilityText, modelShort } from '../util';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();
  const s = $derived(vm.session);
  const busy = $derived(!!s && (s.phase === 'starting' || s.phase === 'loading' || s.phase === 'stopping'));
  const faulted = $derived(s?.phase === 'fault');
  const verb = $derived(s?.phase === 'stopping' ? 'stopping' : 'loading');

  function onKey(e: KeyboardEvent, id: SlotId) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    e.preventDefault();
    const n = vm.slots.length;
    const i = vm.slots.findIndex((x) => x.id === id);
    for (let k = 1; k < n; k++) {
      const next = vm.slots[(i + (e.key === 'ArrowRight' ? k : n - k)) % n];
      const el = (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLButtonElement>(`[data-slot="${next.id}"]`);
      if (el?.disabled) continue;
      actions.select(next.id);
      el?.focus();
      return;
    }
  }
</script>

<div class="tabs" role="tablist" aria-label="Tier">
  {#each vm.slots as slot (slot.id)}
    {@const sel = slot.id === vm.selected}
    {@const why = availabilityText(slot.availability)}
    {@const mine = s?.slot === slot.id}
    {@const running = mine && !faulted}
    {@const locked = busy && !mine}
    {@const failed = mine && faulted}
    <button
      role="tab"
      data-slot={slot.id}
      aria-selected={sel}
      tabindex={sel ? 0 : -1}
      class="tab"
      class:sel
      class:na={!!why}
      class:locked
      class:failed
      disabled={locked}
      title={locked
        ? `${slot.label}: locked while ${s?.model.name ?? 'the session'} is ${verb}`
        : `${slot.label}: ${slot.model.name} · ${slot.model.quant}${running ? ' (running)' : ''}${why ? ` (${slot.reason ?? why})` : ''}`}
      onclick={() => actions.select(slot.id)}
      ondblclick={() => {
        if ((!s || faulted) && !why) actions.launch(slot.id);
      }}
      onkeydown={(e) => onKey(e, slot.id)}
    >
      <span class="lbl">
        {#if locked}
          <svg class="lock" viewBox="0 0 12 14" aria-label="locked"
            ><rect x="1.5" y="6" width="9" height="7" rx="1.2" /><path d="M3.6 6V4.3a2.4 2.4 0 0 1 4.8 0V6" /></svg
          >
        {:else}
          <i class="st" class:run={running} class:bad={!!why} class:pulse={running && busy} aria-hidden="true"></i>
        {/if}
        <span class="tx">{slot.label}</span>
      </span>
      <span class="sub" class:bad={!!why}>{why ? `${slot.model.name} · ${why}` : modelShort(slot.model)}</span>
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--gap);
    height: max(42px, calc(var(--u) * 48));
    min-width: 0;
  }
  .tab {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: max(2px, calc(var(--u) * 3));
    min-width: 0;
    padding: 0 max(9px, calc(var(--u) * 12));
    border-radius: 8px;
    border: 1px solid var(--edge);
    background: var(--panel);
    text-align: left;
    transition:
      border-color 160ms ease-out,
      background 160ms ease-out;
  }
  .tab:hover:not(.sel):not(:disabled) {
    border-color: #3a4850;
    background: rgba(34, 44, 50, 0.8);
  }
  .tab:disabled {
    opacity: 1;
  }
  .tab.sel {
    border-color: var(--sky);
    background: rgba(90, 182, 235, 0.09);
    box-shadow: inset 0 0 0 1px rgba(90, 182, 235, 0.28);
  }
  .tab.failed {
    border-color: var(--danger);
    background: rgba(232, 100, 90, 0.1);
    box-shadow: inset 0 0 0 1px rgba(232, 100, 90, 0.3);
  }
  .lbl {
    display: flex;
    align-items: center;
    gap: max(6px, calc(var(--u) * 7));
    min-width: 0;
    font-family: var(--f-ui);
    font-weight: 600;
    font-size: max(12px, calc(var(--u) * 13));
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--mist);
    white-space: nowrap;
  }
  .tx {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sel .lbl {
    color: var(--foam);
  }
  .failed .lbl {
    color: var(--danger);
  }
  .sub {
    font-size: var(--fs-s);
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .sel .sub {
    color: #a9c3d1;
  }
  .sub.bad {
    color: var(--amber);
  }
  .na .lbl {
    color: #9fb0b8;
  }
  .locked {
    border-color: #222b30;
  }
  .locked .lbl,
  .locked .sub {
    color: #66767e;
  }
  /* Tier state */
  .st {
    width: max(7px, calc(var(--u) * 8));
    height: max(7px, calc(var(--u) * 8));
    flex: none;
    border-radius: 50%;
    background: var(--kelp);
  }
  .st.bad {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--amber);
  }
  .st.run {
    background: var(--sky);
    box-shadow: 0 0 0 max(2px, calc(var(--u) * 2.5)) rgba(90, 182, 235, 0.25);
  }
  .failed .st {
    background: var(--danger);
    box-shadow: none;
  }
  .st.pulse {
    animation: pulse 1s ease-in-out infinite;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }
  .lock {
    width: max(9px, calc(var(--u) * 10));
    height: max(10px, calc(var(--u) * 12));
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
  }
  .lock rect {
    fill: currentColor;
    fill-opacity: 0.25;
  }
</style>
