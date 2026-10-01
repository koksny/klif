<script lang="ts">
  // Idle launcher: the four tiers as selectable rows (the model behind each tier is the subtitle),
  // then the previous session. Launch and Tune live in the bottom bar (where Stop is while live).
  import type { Actions, SlotId, ViewModel } from '../../../lib/model/types';
  import { fmtCtx, fmtInt, fmtTps } from '../../../lib/model/format';
  import { availabilityText, fmtAgo, fmtSpan } from '../util';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  const last = $derived(vm.lastSession ?? null);
  const lastLabel = $derived(last ? (vm.slots.find((x) => x.id === last.slot)?.label ?? last.model.name) : '');
  const lastFacts = $derived.by(() => {
    if (!last) return [];
    const f = [lastLabel, fmtSpan(last.uptimeS)];
    if (last.requests !== undefined) f.push(`${fmtInt(last.requests)} requests`);
    if (last.generatedTokens !== undefined) f.push(`${fmtInt(last.generatedTokens)} tok`);
    if (last.decodeTps !== undefined) f.push(`${fmtTps(last.decodeTps)} tok/s`);
    if (last.images !== undefined) f.push(`${fmtInt(last.images)} images`);
    if (last.secondsPerImage !== undefined) f.push(`${last.secondsPerImage.toFixed(1)} s / image`);
    return f;
  });

  function onKey(e: KeyboardEvent, id: SlotId) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    const n = vm.slots.length;
    const i = vm.slots.findIndex((s) => s.id === id);
    const next = vm.slots[(i + (e.key === 'ArrowDown' ? 1 : n - 1)) % n];
    actions.select(next.id);
    (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`[data-row="${next.id}"]`)?.focus();
  }
</script>

<section class="idle">
  <div class="c-lbl">Select a job</div>
  <div class="rows" role="radiogroup" aria-label="Job">
    {#each vm.slots as slot (slot.id)}
      {@const sel = slot.id === vm.selected}
      {@const why = availabilityText(slot.availability)}
      <button
        class="row"
        class:sel
        class:na={!!why}
        role="radio"
        aria-checked={sel}
        tabindex={sel ? 0 : -1}
        data-row={slot.id}
        title={why ? `${slot.label} cannot be launched: ${why}` : `Double-click to launch ${slot.label}`}
        onclick={() => actions.select(slot.id)}
        ondblclick={() => {
          if (!why) actions.launch(slot.id);
        }}
        onkeydown={(e) => onKey(e, slot.id)}
      >
        <i class="radio" aria-hidden="true"></i>
        <span class="who">
          <span class="tier">{slot.label}</span>
          <span class="sub c-data"
            >{slot.model.name}<span class="sep">·</span>{slot.model.quant}{#if slot.model.ctxTokens}<span class="sep">·</span
              >{fmtCtx(slot.model.ctxTokens)}{/if}{#if slot.model.imageSize}<span class="sep">·</span>{slot.model.imageSize}{/if}</span
          >
        </span>
        <span class="av" class:ok={!why}><i aria-hidden="true"></i>{why ?? 'ready'}</span>
      </button>
    {/each}
  </div>

  <div class="last">
    <span class="k">last session</span>
    {#if last}
      <span class="v c-data" title={last.model.name}>
        {#each lastFacts as f, i (i)}{#if i}<span class="sep">·</span>{/if}<span>{f}</span>{/each}
      </span>
      <span class="end" class:bad={last.ended === 'fault'}
        >{last.ended === 'fault' ? 'ended in a fault' : 'stopped'} {fmtAgo(last.endedAgoS)}</span
      >
    {:else}
      <span class="v none">none yet</span>
    {/if}
  </div>
</section>

<style>
  .idle {
    padding-top: max(8px, calc(var(--u) * 14));
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: max(5px, calc(var(--u) * 8));
    margin-top: max(5px, calc(var(--u) * 8));
  }
  .row {
    display: flex;
    align-items: center;
    gap: max(12px, calc(var(--u) * 22));
    min-height: max(42px, calc(var(--u) * 56));
    padding: max(4px, calc(var(--u) * 6)) max(14px, calc(var(--u) * 22)) max(4px, calc(var(--u) * 6)) max(12px, calc(var(--u) * 18));
    border-radius: 8px;
    border: 1px solid #2a353b;
    background: rgba(29, 37, 42, 0.55);
    text-align: left;
  }
  .row:hover {
    border-color: #3a4850;
    background: rgba(34, 44, 50, 0.7);
  }
  .row.sel {
    border-color: var(--sky);
    background: rgba(90, 182, 235, 0.08);
    box-shadow: inset 0 0 0 1px rgba(90, 182, 235, 0.35);
  }
  .radio {
    flex: none;
    width: max(16px, calc(var(--u) * 22));
    height: max(16px, calc(var(--u) * 22));
    border-radius: 50%;
    box-shadow: inset 0 0 0 1.5px #8fa3ae;
    position: relative;
  }
  .sel .radio {
    box-shadow: inset 0 0 0 2px var(--sky);
  }
  .sel .radio::after {
    content: '';
    position: absolute;
    inset: 26%;
    border-radius: 50%;
    background: var(--sky);
  }
  .who {
    display: flex;
    flex-direction: column;
    gap: max(1px, calc(var(--u) * 3));
    min-width: 0;
    flex: 1 1 auto;
  }
  .tier {
    font-family: var(--f-ui);
    font-weight: 600;
    font-size: max(14px, calc(var(--u) * 19));
    letter-spacing: 0.06em;
    color: var(--foam);
    line-height: 1.15;
  }
  .sub {
    font-size: max(11.5px, calc(var(--u) * 14.5));
    color: #b0c2cb;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: 0.02em;
  }
  .sep {
    margin: 0 0.6em;
    color: var(--muted);
  }
  .av {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 0.6em;
    font-size: max(12px, calc(var(--u) * 15));
    color: var(--amber);
  }
  .av i {
    width: 0.62em;
    height: 0.62em;
    border-radius: 50%;
    background: currentColor;
  }
  .av.ok {
    color: #c3d3db;
  }
  .av.ok i {
    background: var(--sky);
  }
  .na .tier {
    color: #b9c6cc;
  }

  .last {
    display: flex;
    align-items: baseline;
    gap: max(12px, calc(var(--u) * 28));
    margin-top: max(9px, calc(var(--u) * 14));
    padding: max(8px, calc(var(--u) * 12)) 0 max(9px, calc(var(--u) * 13));
    border-top: 1px solid var(--rule);
    font-size: max(12px, calc(var(--u) * 15));
    min-width: 0;
  }
  .last .k {
    flex: none;
    color: var(--muted);
  }
  .last .v {
    color: var(--foam);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    letter-spacing: 0.02em;
  }
  .last .none {
    color: var(--muted);
  }
  .last .end {
    margin-left: auto;
    flex: none;
    color: var(--muted);
    white-space: nowrap;
  }
  .last .end.bad {
    color: var(--amber);
  }
</style>
