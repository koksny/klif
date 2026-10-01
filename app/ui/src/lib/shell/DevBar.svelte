<script lang="ts">
  // Developer bar (toggle with the backquote key; never rendered when shot=1).
  import { SKINS } from '../../skins/registry';
  import type { Tier } from '../render/scheduler';
  import { player } from '../state/player.svelte';
  import type { SizeParam } from '../state/params';
  import { ui } from '../state/ui.svelte';
  import { fmtClock } from '../model/format';

  const SPEEDS = [0.5, 1, 2, 4, 10, 30, 60];
  const TIERS: Tier[] = ['off', 'calm', 'ambient', 'live'];

  function setTier(v: string) {
    ui.tierPin = v === 'auto' ? null : (v as Tier);
  }
</script>

<div class="bar" role="toolbar" aria-label="Developer bar">
  <label>
    <span>skin</span>
    <select value={ui.skinId} onchange={(e) => ui.setSkin(e.currentTarget.value as typeof ui.skinId)}>
      {#each SKINS as s (s.id)}
        <option value={s.id}>{s.name}</option>
      {/each}
    </select>
  </label>
  <label>
    <span>size</span>
    <select value={ui.sizeMode} onchange={(e) => ui.setSizeMode(e.currentTarget.value as SizeParam)}>
      <option value="auto">auto ({ui.size})</option>
      <option value="full">full</option>
      <option value="mini">mini</option>
    </select>
  </label>
  <label class="wide">
    <span>scenario</span>
    <select value={player.scenario} onchange={(e) => player.setScenario(e.currentTarget.value)}>
      {#each player.scenarios as s (s.name)}
        <option value={s.name} title={s.blurb}>{s.name}</option>
      {/each}
    </select>
  </label>
  <label>
    <span>speed</span>
    <select value={String(player.speed)} onchange={(e) => (player.speed = Number(e.currentTarget.value))}>
      {#each SPEEDS as v (v)}
        <option value={String(v)}>{v}x</option>
      {/each}
      {#if !SPEEDS.includes(player.speed)}
        <option value={String(player.speed)}>{player.speed}x</option>
      {/if}
    </select>
  </label>
  <label>
    <span>tier</span>
    <select value={ui.tierPin ?? 'auto'} onchange={(e) => setTier(e.currentTarget.value)}>
      <option value="auto">auto</option>
      {#each TIERS as t (t)}
        <option value={t}>{t}</option>
      {/each}
    </select>
  </label>
  <span class="tier" data-tier={ui.tier} title="Tier currently applied">{ui.tier}</span>
  <button type="button" onclick={() => (player.paused = !player.paused)}>{player.paused ? 'resume' : 'pause'}</button>
  {#if player.paused}
    <button type="button" onclick={() => player.stepOnce(0.5)}>+0.5 s</button>
  {/if}
  <span class="clock" title="Simulated time since the scenario started">{fmtClock(player.simT)}</span>
</div>

<style>
  .bar {
    position: absolute;
    top: 8px;
    left: 8px;
    right: 8px;
    z-index: 90;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    padding: 8px 10px;
    background: color-mix(in srgb, var(--k-surface-raised, #1a1a1a) 94%, transparent);
    color: var(--k-ink, #e6e6e6);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.45);
    font: 500 12px/1 var(--k-font-ui, system-ui, sans-serif);
    width: fit-content;
    max-width: calc(100% - 16px);
  }
  label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  label span {
    color: var(--k-muted, #8a8a8a);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    font-size: 10px;
  }
  select,
  button {
    font: 500 12px/1 var(--k-font-data, ui-monospace, monospace);
    color: var(--k-ink, #e6e6e6);
    background: var(--k-surface, #141414);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 6px 8px;
  }
  button {
    cursor: pointer;
  }
  button:hover {
    border-color: var(--k-accent, #5ab6eb);
  }
  select:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
    outline-offset: 1px;
  }
  .wide select {
    min-width: 128px;
  }
  .tier {
    font: 600 11px/1 var(--k-font-data, ui-monospace, monospace);
    padding: 5px 8px;
    border-radius: 999px;
    border: 1px solid var(--k-line, #2e2e2e);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .tier[data-tier='live'] {
    color: var(--k-accent, #5ab6eb);
    border-color: var(--k-accent, #5ab6eb);
  }
  .tier[data-tier='off'],
  .tier[data-tier='calm'] {
    color: var(--k-warn, #f2a33a);
  }
  .clock {
    margin-left: auto;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1 var(--k-font-data, ui-monospace, monospace);
  }
</style>
