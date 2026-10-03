<script lang="ts">
  // System strip: one tab per System in vm.systems (no fixed four), the same in every phase. The name carries
  // the status dot (online / busy / starting / offline / fault / unreachable; not set and invalid are a muted
  // name) and, for a System on another node, a muted node suffix; the model behind it is printed under the
  // name. Click selects (never stops anything); double-click launches a System that is ready and has nothing in
  // its way. The strip scrolls sideways when it overflows; the last tab, +, adds a System.
  import type { Actions, ViewModel } from '../../lib/model/types';
  import { canLaunch } from '../../lib/model/systems';
  import { strip } from '../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../lib/shell/SystemTabs/tabs';
  import { availText, modelLine } from './geom';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();
  const tabs = $derived(tabsFor(vm));

  function onKey(e: KeyboardEvent, i: number) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    e.preventDefault();
    const n = tabs.length;
    const j = (i + (e.key === 'ArrowRight' ? 1 : n - 1)) % n;
    const el = (e.currentTarget as HTMLElement).parentElement?.children[j] as HTMLElement | undefined;
    el?.focus();
  }
</script>

<div class="strip">
  <div class="tabs" role="tablist" aria-label="Systems" use:strip={vm.selected}>
    {#each tabs as t, i (t.id)}
      {@const s = t.system}
      {@const on = t.id === vm.selected}
      {@const na = s.availability !== 'ready' && s.status !== 'not-set'}
      {@const bad = s.status === 'fault'}
      <button
        role="tab"
        class="tab"
        class:on
        class:na
        class:bad
        aria-selected={on}
        tabindex={on ? 0 : -1}
        onclick={() => actions.select(t.id)}
        ondblclick={() => {
          if (canLaunch(s) && s.conflicts.length === 0) void actions.launch(s.id).catch(() => {});
        }}
        onkeydown={(e) => onKey(e, i)}
        title={`${t.label}${t.nodeName ? ` on ${t.nodeName}` : ''}: ${s.model.name || 'no preset'} (${s.status}${s.reason ? `: ${s.reason}` : ''})`}
      >
        <span class="name"><TabLabel tab={t} /></span>
        <span class="model" class:warn={na}>{
          s.status === 'not-set'
            ? 'no preset'
            : s.status === 'unreachable'
              ? `${t.nodeName ?? 'node'} unreachable`
              : na
                ? `${s.model.name} · ${availText(s.availability)}`
                : modelLine(s.model).join(' · ')
        }</span>
      </button>
    {/each}
  </div>
  <button class="tab add" type="button" title="Add a System" aria-label="Add a System" onclick={() => actions.openTune(undefined, { add: true })}>
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M6 1.5v9M1.5 6h9" /></svg>
  </button>
</div>

<style>
  .strip {
    flex: none;
    height: calc(50px * var(--k));
    display: flex;
    gap: calc(8px * var(--k));
    min-width: 0;
  }
  .tabs {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    gap: calc(8px * var(--k));
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tabs::-webkit-scrollbar {
    display: none;
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
  .tabs .tab {
    flex: 1 0 calc(150px * var(--k));
  }
  .tab.add {
    flex: none;
    width: calc(42px * var(--k));
    justify-items: center;
    padding: 0;
    color: var(--ph-cyan);
  }
  .add svg {
    width: calc(13px * var(--k));
    height: calc(13px * var(--k));
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }
  .tab:hover:not(.on) {
    border-color: #2a7f93;
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
    max-width: 100%;
    min-width: 0;
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
</style>
