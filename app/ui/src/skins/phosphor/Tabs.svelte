<script lang="ts">
  // Tier strip: the same four tabs in every phase. The three LLM slots are tiers (SYSTEM 1 / 2 /
  // 3); the model behind each tier is swappable, so its current model is printed under the tier name.
  // State dot: ready (cyan), cannot launch (amber ring, and the reason in amber), running (hot, ringed).
  // Click selects; double-click launches when nothing runs or after a fault. While a session starts or
  // stops the other tiers are locked (padlock); in a fault the failed tier is drawn in the fault colour.
  import type { Actions, Slot, SlotId } from '../../lib/model/types';
  import { availText, modelLine } from './geom';

  let {
    slots,
    selected,
    running,
    actions,
    locked = false,
    fault = false,
    canLaunch = false,
  }: {
    slots: Slot[];
    selected: SlotId;
    /** The session's slot (any phase), or null when idle. */
    running: SlotId | null;
    actions: Actions;
    /** A session is starting or stopping: every other tier is locked. */
    locked?: boolean;
    /** The session in `running` has faulted. */
    fault?: boolean;
    /** Double-click launches (nothing runs, or after a fault). */
    canLaunch?: boolean;
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

<div class="tabs" role="tablist" aria-label="Tier">
  {#each slots as s, i (s.id)}
    {@const lock = locked && s.id !== running}
    {@const na = s.availability !== 'ready'}
    {@const run = s.id === running && !fault}
    {@const bad = fault && s.id === running}
    <button
      role="tab"
      class="tab"
      class:on={s.id === selected}
      class:lock
      class:na
      class:bad
      aria-selected={s.id === selected}
      aria-disabled={lock}
      tabindex={s.id === selected ? 0 : -1}
      onclick={() => (lock ? undefined : actions.select(s.id))}
      ondblclick={() => (canLaunch && !na ? actions.launch(s.id) : undefined)}
      onkeydown={(e) => onKey(e, i)}
      title={lock
        ? `${s.label}: locked while a session starts or stops`
        : na
          ? `${s.label}: ${s.reason ?? availText(s.availability)}`
          : canLaunch
            ? `${s.label}: ${s.model.name} (double-click to launch)`
            : `${s.label}: ${s.model.name}`}
    >
      <span class="name">
        {#if lock}
          <svg class="padlock" viewBox="0 0 12 14" aria-label="locked"><rect x="1" y="6" width="10" height="7.5" rx="1.2" /><path d="M3.2 6V4.2a2.8 2.8 0 0 1 5.6 0V6" /></svg>
        {:else}
          <i class="sd" class:run class:pulse={run && locked} class:ring={na && !run && !bad} class:bad aria-hidden="true"></i>
        {/if}
        {s.label}
      </span>
      <span class="model" class:warn={na}>{na ? `${s.model.name} · ${availText(s.availability)}` : modelLine(s.model).join(' · ')}</span>
    </button>
  {/each}
</div>

<style>
  .tabs {
    flex: none;
    height: calc(50px * var(--k));
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: calc(8px * var(--k));
  }
  .tab {
    min-width: 0;
    display: grid;
    align-content: center;
    justify-items: start;
    gap: calc(4px * var(--k));
    padding: 0 calc(12px * var(--k));
    border: 1px solid var(--ph-rule);
    border-radius: 5px;
    background: rgba(3, 9, 12, 0.72);
    color: var(--ph-brand);
    text-align: left;
  }
  .tab:hover:not(.lock):not(.on) {
    border-color: #2a7f93;
  }
  .tab.lock {
    cursor: not-allowed;
    color: var(--ph-muted);
    border-color: rgba(23, 79, 92, 0.55);
  }
  .tab.on {
    border: 1.5px solid var(--ph-cyan);
    background: rgba(18, 48, 58, 0.42);
    box-shadow:
      0 0 10px rgba(127, 227, 255, 0.25),
      inset 0 0 12px rgba(127, 227, 255, 0.12);
    color: var(--ph-cyan);
  }
  .tab.bad {
    border: 1.5px solid var(--ph-danger);
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
    max-width: 100%;
    font-size: var(--ph-fs-l);
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.12em;
    line-height: 1.1;
    white-space: nowrap;
    overflow: hidden;
  }
  .on .name {
    text-shadow: var(--ph-glow);
  }
  .bad .name {
    text-shadow: 0 0 6px rgba(229, 97, 92, 0.55);
  }
  .na:not(.on) .name {
    color: var(--ph-muted);
  }
  /* state dot: ready = cyan, running = hot with a ring, cannot launch = amber ring, failed = fault colour */
  .sd {
    flex: none;
    width: calc(7px * var(--k));
    height: calc(7px * var(--k));
    min-width: 6px;
    min-height: 6px;
    border-radius: 50%;
    background: var(--ph-cyan);
    box-shadow: 0 0 5px rgba(127, 227, 255, 0.6);
    opacity: 0.85;
  }
  .sd.run {
    background: var(--ph-hot);
    box-shadow:
      0 0 0 calc(2.5px * var(--k)) rgba(127, 227, 255, 0.3),
      0 0 8px var(--ph-cyan);
    opacity: 1;
  }
  .sd.ring {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--ph-amber);
  }
  .sd.bad {
    background: var(--ph-danger);
    box-shadow: 0 0 6px var(--ph-danger);
    opacity: 1;
  }
  .sd.pulse {
    animation: tab-pulse 1.1s ease-in-out infinite;
  }
  @keyframes tab-pulse {
    50% {
      opacity: 0.35;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sd.pulse {
      animation: none;
    }
  }
  .padlock {
    width: calc(10px * var(--k));
    height: calc(12px * var(--k));
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
    font-size: max(10px, calc(10.5px * var(--k)));
    letter-spacing: 0.01em;
    color: var(--ph-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .on .model {
    color: var(--ph-ink);
    opacity: 0.85;
  }
  .bad .model {
    color: #e9a29d;
  }
  .model.warn {
    color: var(--ph-amber);
    opacity: 1;
  }
  .lock .model {
    opacity: 0.7;
  }
</style>
