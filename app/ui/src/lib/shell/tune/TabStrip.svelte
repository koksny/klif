<script lang="ts">
  // The drawer's System tabs: exactly vm.systems (tabsFor) with the shared StatusDot, a muted node suffix for
  // remote Systems, then "+" (add mode). Scrolls sideways when the tabs do not fit; arrow keys move between tabs.
  import { tick } from 'svelte';
  import type { SystemId, ViewModel } from '../../model/types';
  import StatusDot from '../SystemTabs/StatusDot.svelte';
  import { tabsFor } from '../SystemTabs/tabs';

  interface Props {
    vm: ViewModel;
    current: SystemId | null;
    adding: boolean;
    onpick: (id: SystemId) => void;
    onadd: () => void;
  }
  let { vm, current, adding, onpick, onadd }: Props = $props();

  const tabs = $derived(tabsFor(vm));
  let strip = $state<HTMLDivElement | undefined>();

  // Keep the active tab in view.
  $effect(() => {
    const id = adding ? '+' : current;
    if (!strip || !id) return;
    void tick().then(() => {
      const el = strip?.querySelector<HTMLElement>(`[data-tab="${CSS.escape(id)}"]`);
      if (!strip || !el) return;
      // Scroll the strip only (scrollIntoView would also scroll the shell's clipped containers).
      const sr = strip.getBoundingClientRect();
      const er = el.getBoundingClientRect();
      if (er.left < sr.left + 18) strip.scrollLeft -= sr.left + 18 - er.left;
      else if (er.right > sr.right - 18) strip.scrollLeft += er.right - (sr.right - 18);
    });
  });

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft' && e.key !== 'Home' && e.key !== 'End') return;
    const all = strip ? [...strip.querySelectorAll<HTMLButtonElement>('[role="tab"]')] : [];
    const i = all.indexOf(document.activeElement as HTMLButtonElement);
    if (i < 0) return;
    e.preventDefault();
    const n = e.key === 'Home' ? 0 : e.key === 'End' ? all.length - 1 : (i + (e.key === 'ArrowRight' ? 1 : -1) + all.length) % all.length;
    all[n].focus();
    all[n].click();
  }

  // A vertical wheel scrolls the strip sideways when it overflows (not passive: it takes the wheel).
  $effect(() => {
    const el = strip;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (el.scrollWidth <= el.clientWidth || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  });

  function title(t: (typeof tabs)[number]): string {
    const where = t.nodeName ? ` on ${t.nodeName}` : '';
    return `${t.label}${where}${t.reason ? `: ${t.reason}` : ''}`;
  }
</script>

<div class="tabs" role="tablist" aria-label="Systems" tabindex="-1" bind:this={strip} onkeydown={onKey}>
  {#each tabs as tab (tab.id)}
    {@const on = !adding && tab.id === current}
    <button
      type="button"
      role="tab"
      data-tab={tab.id}
      aria-selected={on}
      tabindex={on ? 0 : -1}
      class:on
      title={title(tab)}
      onclick={() => onpick(tab.id)}
    >
      <StatusDot status={tab.status} reason={tab.reason} />
      <span class="lbl" class:muted={tab.status === 'not-set' || tab.status === 'invalid'}>{tab.label}</span>
      {#if tab.nodeName}<span class="node">&nbsp;· {tab.nodeName}</span>{/if}
      {#if tab.status === 'invalid'}<span class="bang" aria-hidden="true">!</span>{/if}
    </button>
  {/each}
  <button
    type="button"
    role="tab"
    data-tab="+"
    class="add"
    class:on={adding}
    aria-selected={adding}
    tabindex={adding || tabs.length === 0 ? 0 : -1}
    aria-label="Add System"
    title="Add System"
    onclick={onadd}>+</button
  >
</div>

<style>
  .tabs {
    display: flex;
    gap: 6px;
    padding: 0 18px 10px;
    border-bottom: 1px solid var(--k-line, #2e2e2e);
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
    scrollbar-color: var(--k-line, #2e2e2e) transparent;
    flex: none;
  }
  .tabs button {
    flex: none;
    min-width: 0;
    max-width: 220px;
    display: inline-flex;
    align-items: center;
    padding: 7px 10px;
    font: 600 11px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.1em;
    white-space: nowrap;
  }
  .tabs button.on {
    background: var(--k-accent, #5ab6eb);
    border-color: var(--k-accent, #5ab6eb);
    color: var(--k-accent-ink, #0d1117);
  }
  /* On the accent fill the dot and the muted parts take the ink colour so they stay visible. */
  .tabs button.on :global(.dot) {
    --k-accent: var(--k-accent-ink, #0d1117);
    --k-muted: var(--k-accent-ink, #0d1117);
  }
  .lbl {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lbl.muted,
  .node {
    color: var(--k-muted, #8a8a8a);
  }
  .on .lbl.muted,
  .on .node {
    color: inherit;
    opacity: 0.7;
  }
  .node {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: 0.04em;
  }
  .bang {
    margin-left: 3px;
    color: var(--k-warn, #f2a33a);
    font-weight: 700;
  }
  .add {
    flex: none;
    justify-content: center;
    min-width: 34px;
    font-size: 15px;
  }
</style>
