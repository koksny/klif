<script lang="ts">
  // Idle launcher actions (Launch <TIER>, Tune) and the summary of the previous session.
  import type { Actions, LastSession, Slot } from '../../lib/model/types';
  import { fmtInt, fmtTps } from '../../lib/model/format';
  import { AVAIL_TEXT, fmtAgo, fmtDur } from './geom';

  let {
    slot,
    slots,
    last,
    actions,
  }: { slot: Slot | undefined; slots: Slot[]; last: LastSession | null | undefined; actions: Actions } = $props();

  const ok = $derived(!!slot && slot.availability === 'ready');
  const lastLabel = $derived(last ? (slots.find((x) => x.id === last.slot)?.label ?? last.slot) : '');
  const lastParts = $derived.by(() => {
    if (!last) return [];
    const p: string[] = [fmtDur(last.uptimeS)];
    if (last.requests !== undefined) p.push(`${fmtInt(last.requests)} requests`);
    if (last.generatedTokens !== undefined) p.push(`${fmtInt(last.generatedTokens)} tok`);
    if (last.decodeTps !== undefined) p.push(`${fmtTps(last.decodeTps)} tok/s`);
    if (last.images !== undefined) p.push(`${fmtInt(last.images)} images`);
    if (last.secondsPerImage !== undefined) p.push(`${last.secondsPerImage.toFixed(1)} s/image`);
    return p;
  });
</script>

<div class="go-row">
  <button
    class="launch"
    onclick={() => actions.launch(slot?.id)}
    disabled={!ok}
    title={ok ? `Launch ${slot?.label}` : slot ? `${slot.label}: ${AVAIL_TEXT[slot.availability] ?? slot.availability}` : ''}
  >
    {#if ok || !slot}Launch {slot?.label ?? ''}{:else}{slot.label} · {AVAIL_TEXT[slot.availability] ?? slot.availability}{/if}
  </button>
  <button class="tune" onclick={() => actions.openTune(slot?.id)}>Tune</button>
</div>

<section class="panel last" aria-label="Last session">
  <div class="lhead">
    <span class="lbl">Last session</span>
    {#if last}
      <span class="ended">
        {last.model.name} · <span class:bad={last.ended === 'fault'}>{last.ended === 'fault' ? 'ended in a fault' : 'stopped'}</span> · {fmtAgo(last.endedAgoS)}
      </span>
    {/if}
  </div>
  <div class="lline">
    {#if last}
      <span class="who">{lastLabel}</span>
      {#each lastParts as p}<span class="sep" aria-hidden="true">·</span><span class="part">{p}</span>{/each}
    {:else}
      <span class="mut">no previous session</span>
    {/if}
  </div>
</section>

<style>
  .go-row {
    flex: none;
    display: grid;
    grid-template-columns: minmax(0, 2.45fr) minmax(0, 1fr);
    gap: calc(18px * var(--k));
    height: calc(80px * var(--k));
  }
  .launch,
  .tune {
    border-radius: 6px;
    font-family: var(--ph-display);
    font-stretch: 112%;
    letter-spacing: 0.06em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0 calc(16px * var(--k));
  }
  .launch {
    font-size: calc(33px * var(--k));
    color: #b9f1ff;
    border: 2px solid var(--ph-cyan);
    background: rgba(10, 34, 42, 0.8);
    box-shadow:
      0 0 16px rgba(127, 227, 255, 0.42),
      0 0 40px rgba(90, 182, 235, 0.16),
      inset 0 0 26px rgba(127, 227, 255, 0.16);
    text-shadow:
      0 0 8px rgba(127, 227, 255, 0.7),
      0 0 22px rgba(90, 182, 235, 0.35);
  }
  .launch:hover:not(:disabled) {
    background: rgba(18, 60, 72, 0.85);
    color: var(--ph-hot);
  }
  .launch:disabled {
    opacity: 1;
    color: var(--ph-amber);
    border-color: rgba(232, 176, 74, 0.55);
    box-shadow: none;
    text-shadow: none;
    background: rgba(3, 9, 12, 0.72);
    font-size: calc(26px * var(--k));
  }
  .tune {
    font-size: calc(28px * var(--k));
    color: var(--ph-cyan);
    border: 1px solid #2a7f93;
    background: rgba(3, 9, 12, 0.72);
    text-shadow: var(--ph-glow-soft);
  }
  .tune:hover {
    border-color: var(--ph-cyan);
    background: rgba(18, 48, 58, 0.5);
  }

  .last {
    flex: none;
    height: calc(84px * var(--k));
    display: grid;
    align-content: center;
    gap: calc(10px * var(--k));
    padding: 0 calc(24px * var(--k));
  }
  .lhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: calc(16px * var(--k));
  }
  .ended {
    font-size: calc(16px * var(--k));
    letter-spacing: 0.05em;
    color: var(--ph-muted);
  }
  .ended .bad {
    color: var(--ph-danger);
  }
  .lline {
    display: flex;
    align-items: baseline;
    column-gap: calc(12px * var(--k));
    overflow: hidden;
    white-space: nowrap;
    font-size: calc(22px * var(--k));
    letter-spacing: 0.05em;
    color: var(--ph-ink);
    text-shadow: var(--ph-glow-soft);
  }
  .who {
    color: var(--ph-cyan);
  }
  .sep {
    color: var(--ph-muted);
  }
  .part {
    white-space: nowrap;
  }
</style>
