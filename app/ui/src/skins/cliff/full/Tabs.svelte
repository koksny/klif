<script lang="ts">
  // System strip: one tab per System in vm.systems (no fixed four), the same in every phase. The label
  // carries the status dot (online / busy / starting / offline / fault / unreachable; not set and invalid are a
  // muted label) and, for a System on another node, a muted node suffix; the model behind it is the second
  // line. Click selects (never stops anything); double-click launches a System that is ready and has nothing
  // in its way. The strip scrolls sideways when it overflows; the last tab, +, adds a System.
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { canLaunch } from '../../../lib/model/systems';
  import { strip } from '../../../lib/shell/SystemTabs/scroll';
  import TabLabel from '../../../lib/shell/SystemTabs/TabLabel.svelte';
  import { tabsFor } from '../../../lib/shell/SystemTabs/tabs';
  import { availabilityText, modelShort } from '../util';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();
  const tabs = $derived(tabsFor(vm));

  function onKey(e: KeyboardEvent, id: string) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    e.preventDefault();
    const n = vm.systems.length;
    const i = vm.systems.findIndex((x) => x.id === id);
    const next = vm.systems[(i + (e.key === 'ArrowRight' ? 1 : n - 1)) % n];
    if (!next) return;
    void actions.select(next.id);
    (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLButtonElement>(`[data-system="${CSS.escape(next.id)}"]`)?.focus();
  }

  /** The second line of a tab: the model, or why the System cannot start. */
  function sub(t: (typeof tabs)[number]): string {
    const s = t.system;
    if (s.status === 'not-set') return 'no preset';
    if (s.status === 'unreachable') return `${t.nodeName ?? 'node'} unreachable`;
    const why = availabilityText(s.availability);
    if (why) return s.model.name ? `${s.model.name} · ${why}` : why;
    return s.external && s.status === 'offline' ? `${modelShort(s.model)} · not answering` : modelShort(s.model);
  }
</script>

<div class="strip">
  <div class="tabs" role="tablist" aria-label="Systems" use:strip={vm.selected}>
    {#each tabs as t (t.id)}
      {@const s = t.system}
      {@const sel = t.id === vm.selected}
      {@const bad = s.status === 'invalid' || s.status === 'unreachable' || (s.availability !== 'ready' && s.status !== 'not-set')}
      <button
        role="tab"
        data-system={t.id}
        aria-selected={sel}
        tabindex={sel ? 0 : -1}
        class="tab"
        class:sel
        class:na={bad}
        class:failed={s.status === 'fault'}
        title={`${t.label}${t.nodeName ? ` on ${t.nodeName}` : ''}: ${s.model.name || 'no preset'}${s.model.quant ? ` · ${s.model.quant}` : ''} (${s.status}${s.reason ? `: ${s.reason}` : ''})`}
        onclick={() => actions.select(t.id)}
        ondblclick={() => {
          if (canLaunch(s) && s.conflicts.length === 0) void actions.launch(s.id).catch(() => {});
        }}
        onkeydown={(e) => onKey(e, t.id)}
      >
        <span class="lbl"><TabLabel tab={t} /></span>
        <span class="sub" class:bad>{sub(t)}</span>
      </button>
    {/each}
  </div>
  <button class="tab add" type="button" title="Add a System" aria-label="Add a System" onclick={() => actions.openTune(undefined, { add: true })}>
    <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M6 1.5v9M1.5 6h9" /></svg>
  </button>
</div>

<style>
  .strip {
    display: flex;
    gap: var(--gap);
    height: max(42px, calc(var(--u) * 48));
    min-width: 0;
  }
  .tabs {
    display: flex;
    flex: 1 1 auto;
    gap: var(--gap);
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  .tabs::-webkit-scrollbar {
    display: none;
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
  .tabs .tab {
    flex: 1 0 max(150px, calc(var(--u) * 168));
  }
  .tab.add {
    flex: none;
    width: max(40px, calc(var(--u) * 44));
    align-items: center;
    padding: 0;
    color: var(--mist);
  }
  .add svg {
    width: max(12px, calc(var(--u) * 13));
    height: max(12px, calc(var(--u) * 13));
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
  }
  .tab:hover:not(.sel) {
    border-color: #3a4850;
    background: rgba(34, 44, 50, 0.8);
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
    min-width: 0;
    font-family: var(--f-ui);
    font-weight: 600;
    font-size: max(12px, calc(var(--u) * 13));
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--mist);
    white-space: nowrap;
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
</style>
