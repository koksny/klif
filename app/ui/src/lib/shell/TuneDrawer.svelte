<script lang="ts">
  // Tune: a side sheet over the skin to configure what each System runs. Tabs = vm.systems (+ add mode); a tab
  // shows the System row, its preset (with a transparent, editable command), params, VRAM fit, bench, model
  // suggestions, the API key and the remote nodes. Preset edits are local drafts (never bound to the 2 Hz view
  // model) applied with SavePreset + baseHash; closing with unapplied drafts asks first (ui.tuneCanClose).
  // Themed by the --k-* variables of the active skin; the parts live in ./tune/.
  import { onMount } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { joinParts, nodeName } from '../model/systems';
  import type { SystemId } from '../model/types';
  import { player } from '../state/player.svelte';
  import { ui } from '../state/ui.svelte';
  import AddSystem from './tune/AddSystem.svelte';
  import ConfirmDialog from './tune/ConfirmDialog.svelte';
  import { setTune, TuneState } from './tune/state.svelte';
  import SystemView from './tune/SystemView.svelte';
  import TabStrip from './tune/TabStrip.svelte';

  const t = setTune(new TuneState());
  const vm = $derived(player.vm);
  const system = $derived(vm.systems.find((s) => s.id === (ui.tuneSystem ?? vm.selected)) ?? vm.systems[0] ?? null);
  const adding = $derived(ui.tuneAdd || !system);
  const wide = $derived(!adding && t.commandOpen);
  const where = $derived(system ? nodeName(vm, system) : undefined);

  let sheet = $state<HTMLDivElement | undefined>();
  let closeBtn = $state<HTMLButtonElement | undefined>();

  onMount(() => {
    const guard = () => t.canClose();
    ui.tuneCanClose = guard;
    // Start keyboard users inside the sheet.
    if (!sheet?.contains(document.activeElement)) closeBtn?.focus();
    return () => {
      if (ui.tuneCanClose === guard) ui.tuneCanClose = null;
    };
  });

  function close() {
    ui.closeTune();
  }

  function pick(id: SystemId) {
    ui.tuneSystem = id;
    ui.tuneAdd = false;
  }

  function add() {
    ui.tuneAdd = true;
  }

  function isTyping(el: EventTarget | null): boolean {
    const e = el as HTMLElement | null;
    return !!e && (e.tagName === 'INPUT' || e.tagName === 'TEXTAREA' || e.tagName === 'SELECT' || e.isContentEditable);
  }

  // While typing in the sheet, keys belong to the field: global shortcuts (skin switch, panel, dev bar) never see
  // them. Escape still reaches the shell, which asks the drawer before closing.
  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Escape' && isTyping(e.target)) e.stopPropagation();
  }
</script>

<div class="scrim" role="presentation" onclick={close} transition:fade|global={{ duration: 160 }}></div>
<div
  class="sheet tune-sheet"
  class:wide
  role="dialog"
  aria-modal="true"
  aria-label={adding ? 'Add System' : `Tune ${system?.label ?? ''}`}
  tabindex="-1"
  bind:this={sheet}
  onkeydown={onKey}
  transition:fly|global={{ x: 400, duration: 220, opacity: 1 }}
>
  <header>
    <div class="titles">
      <h2>{adding ? 'Add System' : 'Tune'}</h2>
      {#if adding}
        <div class="sysname">New System</div>
        <div class="sysmodel">{vm.systems.length ? 'Another server KLIF manages, with its own tab.' : 'No Systems yet. Add the first one.'}</div>
      {:else if system}
        <div class="sysname">{system.label}{#if where}<span class="where">{` · ${where}`}</span>{/if}</div>
        <div class="sysmodel">{joinParts([system.model.name, system.model.quant, system.model.backend]) || 'No preset'}</div>
      {/if}
    </div>
    <button type="button" class="ghost" bind:this={closeBtn} aria-label="Close the Tune drawer" onclick={close}>Close</button>
  </header>

  <TabStrip {vm} current={system?.id ?? null} {adding} onpick={pick} onadd={add} />

  {#if adding}
    <AddSystem {vm} />
  {:else if system}
    {#key system.id}
      <SystemView {vm} {system} />
    {/key}
  {/if}

  <ConfirmDialog />
</div>

<style>
  .scrim {
    position: absolute;
    inset: 0;
    z-index: 55;
    background: color-mix(in srgb, var(--k-bg, #0d1117) 55%, transparent);
  }
  .sheet {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(96%, 560px);
    z-index: 56;
    display: flex;
    flex-direction: column;
    background: var(--k-surface-raised, #1a1a1a);
    color: var(--k-ink, #e6e6e6);
    border-left: 1px solid var(--k-line, #2e2e2e);
    box-shadow: -14px 0 40px rgba(0, 0, 0, 0.4);
    font-family: var(--k-font-ui, system-ui, sans-serif);
    font-variant-numeric: tabular-nums;
    transition: width 180ms ease;
    outline: none;
  }
  .sheet.wide {
    width: min(96%, 720px);
  }
  @media (prefers-reduced-motion: reduce) {
    .sheet {
      transition: none;
    }
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 18px 10px;
    flex: none;
  }
  .titles {
    min-width: 0;
  }
  h2 {
    margin: 0 0 4px;
    font: 600 11px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .sysname {
    font: 600 20px/1.1 var(--k-font-display, var(--k-font-ui, system-ui));
    letter-spacing: 0.04em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sysname .where {
    color: var(--k-muted, #8a8a8a);
    font-size: 15px;
  }
  .sysmodel {
    margin-top: 6px;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-ui, system-ui, sans-serif);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ---- the drawer's shared vocabulary, used by every part in ./tune/ ---- */
  :global(.tune-sheet .scroll) {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    overscroll-behavior: contain;
  }
  :global(.tune-sheet .field) {
    display: grid;
    grid-template-columns: 112px minmax(0, 1fr);
    align-items: center;
    gap: 12px;
  }
  :global(.tune-sheet .field > span:first-child) {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.04em;
  }
  :global(.tune-sheet .ro) {
    color: var(--k-ink, #e6e6e6);
    font: 500 13px/1.2 var(--k-font-ui, system-ui, sans-serif);
    min-width: 0;
  }
  :global(.tune-sheet .ro small),
  :global(.tune-sheet .hint) {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.4 var(--k-font-ui, system-ui, sans-serif);
  }
  :global(.tune-sheet p.hint) {
    margin: 0;
  }
  :global(.tune-sheet .indent) {
    padding-left: 124px;
  }
  :global(.tune-sheet .err) {
    margin: 0;
    color: var(--k-danger, #e05a5a);
    font: 500 12px/1.4 var(--k-font-ui, system-ui, sans-serif);
  }
  :global(.tune-sheet .stack) {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  :global(.tune-sheet .grow) {
    flex: 1;
  }
  :global(.tune-sheet select),
  :global(.tune-sheet input:where(:not([type='radio'], [type='checkbox']))),
  :global(.tune-sheet textarea) {
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    font: 500 13px/1.2 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
    background: var(--k-surface, #141414);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 8px 10px;
  }
  :global(.tune-sheet input[readonly]),
  :global(.tune-sheet textarea[readonly]) {
    color: var(--k-muted, #8a8a8a);
    border-style: dashed;
  }
  :global(.tune-sheet input[aria-invalid='true']) {
    border-color: var(--k-danger, #e05a5a);
  }
  :global(.tune-sheet input::placeholder),
  :global(.tune-sheet textarea::placeholder) {
    color: var(--k-muted, #8a8a8a);
    opacity: 0.7;
  }
  :global(.tune-sheet select:focus-visible),
  :global(.tune-sheet input:focus-visible),
  :global(.tune-sheet textarea:focus-visible),
  :global(.tune-sheet button:focus-visible) {
    outline: 2px solid var(--k-accent, #5ab6eb);
    outline-offset: 1px;
  }
  :global(.tune-sheet input[type='radio']) {
    accent-color: var(--k-accent, #5ab6eb);
  }
  :global(.tune-sheet button) {
    font: 500 12px/1 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
    background: transparent;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 8px 12px;
    cursor: pointer;
  }
  :global(.tune-sheet button:hover:not(:disabled)) {
    border-color: var(--k-accent, #5ab6eb);
  }
  :global(.tune-sheet button:disabled) {
    opacity: 0.45;
    cursor: not-allowed;
  }
  :global(.tune-sheet button.mini) {
    padding: 5px 8px;
    min-width: 26px;
    font-size: 12px;
  }
  :global(.tune-sheet button.mini.text) {
    padding: 6px 10px;
  }
  :global(.tune-sheet button.mini.wide) {
    padding: 6px 12px;
  }
  :global(.tune-sheet .seg3) {
    display: flex;
    gap: 6px;
    min-width: 0;
  }
  :global(.tune-sheet .seg3 button) {
    flex: 1;
    min-width: 0;
    font-family: var(--k-font-ui, system-ui, sans-serif);
  }
  :global(.tune-sheet .seg3 button.on),
  :global(.tune-sheet .primary) {
    background: var(--k-accent, #5ab6eb);
    border-color: var(--k-accent, #5ab6eb);
    color: var(--k-accent-ink, #0d1117);
  }
  :global(.tune-sheet .primary) {
    border-color: transparent;
    font-weight: 600;
    padding: 11px 14px;
    font-size: 13px;
  }
  :global(.tune-sheet .primary.mini) {
    padding: 6px 10px;
    font-size: 12px;
  }
  :global(.tune-sheet .primary:hover:not(:disabled)) {
    filter: brightness(1.08);
    border-color: transparent;
  }
  /* A disabled primary (Apply with nothing to apply, a blocked Launch) must not keep the accent fill: it would
     still read as the live action. */
  :global(.tune-sheet .primary:disabled) {
    background: transparent;
    border-color: var(--k-line, #2e2e2e);
    color: var(--k-muted, #8a8a8a);
    opacity: 1;
  }
  :global(.tune-sheet .switch) {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    width: fit-content;
  }
  :global(.tune-sheet .switch i) {
    width: 30px;
    height: 16px;
    border-radius: 999px;
    border: 1px solid var(--k-line, #2e2e2e);
    background: var(--k-surface, #141414);
    position: relative;
    flex: none;
  }
  :global(.tune-sheet .switch i::after) {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--k-muted, #8a8a8a);
    transform: translateX(0);
    transition: transform 140ms;
  }
  :global(.tune-sheet .switch.on i) {
    border-color: var(--k-accent, #5ab6eb);
  }
  :global(.tune-sheet .switch.on i::after) {
    background: var(--k-accent, #5ab6eb);
    transform: translateX(14px);
  }
  :global(.tune-sheet .switch.small) {
    padding: 5px 8px;
    gap: 8px;
  }
  :global(.tune-sheet .badge) {
    display: inline-block;
    padding: 3px 7px;
    border-radius: 999px;
    border: 1px solid var(--k-line, #2e2e2e);
    color: var(--k-muted, #8a8a8a);
    font: 600 10px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  :global(.tune-sheet .badge.accent) {
    color: var(--k-accent, #5ab6eb);
    border-color: var(--k-accent, #5ab6eb);
  }
  :global(.tune-sheet .badge.warn) {
    color: var(--k-warn, #f2a33a);
    border-color: var(--k-warn, #f2a33a);
  }
  :global(.tune-sheet .chip) {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 4px 3px 8px;
    border-radius: 999px;
    border: 1px solid var(--k-line, #2e2e2e);
    font: 500 12px/1 var(--k-font-data, ui-monospace, monospace);
  }
  :global(.tune-sheet .chip .x) {
    border: none;
    padding: 2px 5px;
    line-height: 1;
  }
</style>
