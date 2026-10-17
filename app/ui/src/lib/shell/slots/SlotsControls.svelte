<script lang="ts">
  // A System's conversation slots in Tune's Slots section (the skins have the slot keys): auto-save and auto-load, and
  // the three slots. Clicking a slot makes it the active one (auto-save writes to it, auto-load reads it); its pencil
  // names it; Save, Load and Clear act on it. Replacing or clearing a saved slot asks first, in place.
  import { tick } from 'svelte';
  import type { SlotEntryView, System } from '../../model/types';
  import { player } from '../../state/player.svelte';

  interface Props {
    system: System;
  }
  let { system }: Props = $props();

  const QUIET = { quiet: true };
  let busy = $state(false);
  let err = $state('');
  /** A slot whose Save or Clear waits for a second click. */
  let ask = $state<{ n: number; what: 'save' | 'clear' } | null>(null);
  /** The slot whose name is being edited, and the draft. */
  let naming = $state<number | null>(null);
  let draft = $state('');
  let nameInput = $state<HTMLInputElement>();

  const v = $derived(system.slots);
  const running = $derived(system.status === 'online' || system.status === 'busy');
  const job = $derived(v?.job);
  const title = (e: SlotEntryView | undefined, n: number) => e?.name ?? `Slot ${n}`;
  const activeTitle = $derived(v ? title(v.entries.find((e) => e.n === v.active), v.active) : '');

  async function run(f: () => Promise<void>) {
    busy = true;
    err = '';
    ask = null;
    try {
      await f();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    }
    busy = false;
  }

  const setAuto = (key: 'slotsAutoSave' | 'slotsAutoLoad', on: boolean) => run(() => player.actions.updateSystem(system.id, { [key]: on }, QUIET));
  function save(n: number, saved: boolean) {
    if (saved && (ask?.n !== n || ask.what !== 'save')) {
      ask = { n, what: 'save' };
      return;
    }
    void run(() => player.actions.saveSlot(system.id, n, QUIET));
  }
  const load = (n: number) => run(() => player.actions.loadSlot(system.id, n, QUIET));
  function clear(n: number) {
    if (ask?.n !== n || ask.what !== 'clear') {
      ask = { n, what: 'clear' };
      return;
    }
    void run(() => player.actions.clearSlot(system.id, n, QUIET));
  }

  const canPick = $derived(!busy && !job && system.controllable);
  function pick(n: number) {
    if (!canPick || naming !== null || n === v?.active) return;
    void run(() => player.actions.useSlot(system.id, n, QUIET));
  }
  function onRowKey(e: KeyboardEvent, n: number) {
    if (e.target !== e.currentTarget) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      pick(n);
    }
  }

  async function startNaming(e: SlotEntryView) {
    ask = null;
    naming = e.n;
    draft = e.name ?? '';
    await tick();
    nameInput?.focus();
    nameInput?.select();
  }
  function commitName() {
    const n = naming;
    if (n === null) return;
    naming = null;
    const old = v?.entries.find((x) => x.n === n)?.name ?? '';
    if (draft.trim() === old) return;
    void run(() => player.actions.renameSlot(system.id, n, draft.trim(), QUIET));
  }
  function onNameKey(e: KeyboardEvent) {
    // Keys typed here stay here (Escape must not close the panel, Enter must not pick the row).
    e.stopPropagation();
    if (e.key === 'Enter') {
      e.preventDefault();
      commitName();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      naming = null;
    }
  }

  const when = (s: number) =>
    new Date(s * 1000).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  const size = (b?: number) => (b ? `${(b / 2 ** 30).toFixed(1)} GiB` : '');
  const tokens = (n?: number) => (n === undefined ? '' : `${n.toLocaleString('en-US').replace(/,/g, ' ')} tokens`);
</script>

{#if v}
  <div class="autos">
    <label title="Save the active slot when KLIF stops {system.label} (Stop, Restart, a launch that stops it, quitting)">
      <input type="checkbox" checked={v.autoSave} disabled={busy || !system.editable} onchange={(e) => void setAuto('slotsAutoSave', e.currentTarget.checked)} />
      Auto-save to <b>{activeTitle}</b> on stop
    </label>
    <label title="Load the active slot when KLIF has started {system.label}">
      <input type="checkbox" checked={v.autoLoad} disabled={busy || !system.editable} onchange={(e) => void setAuto('slotsAutoLoad', e.currentTarget.checked)} />
      Auto-load <b>{activeTitle}</b> on start
    </label>
  </div>
  <p class="hint">Click a slot to make it the active one: auto-save writes to it and auto-load reads it. Save and Load also make their slot active.</p>
  {#if !v.supported}
    <p class="hint">Its preset's server cannot save slots; llama.cpp and Strata presets can.</p>
  {:else if !running}
    <p class="hint">Save and Load work while {system.label} runs. A change to slots on or off applies on the next launch.</p>
  {/if}
  <div class="slots" role="radiogroup" aria-label="Active slot of {system.label}">
    {#each v.entries as e (e.n)}
      {@const mine = job?.slot === e.n}
      {@const asking = ask?.n === e.n ? ask.what : null}
      {@const active = e.n === v.active}
      <div
        class="slot"
        class:active
        class:pickable={canPick && !active}
        role="radio"
        aria-checked={active}
        tabindex={active ? 0 : -1}
        title={active ? 'Active: auto-save writes here, auto-load reads this' : `Make ${title(e, e.n)} the active slot`}
        onclick={() => pick(e.n)}
        onkeydown={(ev) => onRowKey(ev, e.n)}
      >
        <span class="n">{e.n}</span>
        <span class="what">
          <span class="head">
            {#if naming === e.n}
              <input
                class="name-edit"
                bind:this={nameInput}
                bind:value={draft}
                maxlength="40"
                placeholder="Slot {e.n}"
                aria-label="Name of slot {e.n}"
                onkeydown={onNameKey}
                onblur={commitName}
                onclick={(ev) => ev.stopPropagation()}
              />
            {:else}
              <b class="name">{title(e, e.n)}</b>
              <button
                type="button"
                class="pen"
                aria-label="Rename slot {e.n}"
                title="Rename"
                disabled={busy || !system.controllable}
                onclick={(ev) => {
                  ev.stopPropagation();
                  void startNaming(e);
                }}
                ><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 13l.6-2.6L10.8 3.2l2 2-7.2 7.2L3 13zM9.6 4.4l2 2" /></svg></button
              >
            {/if}
            {#if active}<span class="tag">active</span>{/if}
          </span>
          {#if mine}
            <span class="info strong">{job?.op === 'save' ? 'Saving…' : 'Loading…'}</span>
          {:else if asking === 'save'}
            <span class="info strong">Replace what it holds? Save again.</span>
          {:else if asking === 'clear'}
            <span class="info strong">Delete its files? Clear again.</span>
          {:else if e.savedAt}
            <span class="info">{[tokens(e.tokens), size(e.bytes), e.model, when(e.savedAt)].filter(Boolean).join(' · ')}</span>
            {#if !e.compatible}<span class="info warn">saved with another command</span>{/if}
          {:else}
            <span class="info">empty</span>
          {/if}
        </span>
        <!-- Its buttons act on this slot; a click on them does not pick the row. -->
        <span class="btns" role="presentation" onclick={(ev) => ev.stopPropagation()} onkeydown={(ev) => ev.stopPropagation()}>
          <button
            type="button"
            class="mini text"
            class:primary={asking === 'save'}
            disabled={busy || !!job || !running || !v.supported || !system.controllable}
            onclick={() => save(e.n, !!e.savedAt)}>Save</button
          >
          <button
            type="button"
            class="mini text"
            disabled={busy || !!job || !running || !e.savedAt || !e.compatible || !system.controllable}
            onclick={() => void load(e.n)}>Load</button
          >
          <button
            type="button"
            class="mini text"
            class:danger={asking === 'clear'}
            aria-label="Clear slot {e.n}"
            disabled={busy || !!job || !e.savedAt || !system.editable}
            onclick={() => clear(e.n)}>Clear</button
          >
        </span>
      </div>
    {/each}
  </div>
  {#if v.notice}<p class={v.notice.ok ? 'note' : 'err'} role={v.notice.ok ? 'status' : 'alert'}>{v.notice.text}</p>{/if}
{/if}
{#if err}<p class="err" role="alert">{err}</p>{/if}

<style>
  .autos {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
    margin: 0 0 8px;
    font: 500 12px/1.4 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  .autos label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }
  .autos b {
    font-weight: 700;
  }
  .hint,
  .info,
  .note {
    font: 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .hint,
  .note {
    margin: 0 0 8px;
  }
  .slots {
    margin: 0 0 8px;
    display: grid;
    gap: 6px;
  }
  .slot {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: 6px;
    outline: none;
  }
  .slot.pickable {
    cursor: pointer;
  }
  .slot.pickable:hover {
    border-color: color-mix(in srgb, var(--k-accent, #5ab6eb) 55%, var(--k-line, #2e2e2e));
  }
  .slot:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
    outline-offset: 1px;
  }
  .slot.active {
    border-color: var(--k-accent, #5ab6eb);
    background: color-mix(in srgb, var(--k-accent, #5ab6eb) 10%, transparent);
  }
  .n {
    flex: none;
    width: 18px;
    text-align: center;
    font: 600 13px var(--k-font-mono, ui-monospace, monospace);
    color: var(--k-muted, #8a8a8a);
  }
  .active .n {
    color: var(--k-accent, #5ab6eb);
  }
  .what {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 600 13px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  .name-edit {
    flex: 1;
    min-width: 0;
    max-width: 240px;
    padding: 3px 6px;
    font: 600 13px var(--k-font-ui, system-ui, sans-serif);
  }
  /* The pencil: plain, not the drawer's boxed button. */
  .slot .pen {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    background: none;
    color: var(--k-muted, #8a8a8a);
    opacity: 0.7;
  }
  .slot .pen:hover:not(:disabled) {
    color: var(--k-ink, #eee);
    opacity: 1;
  }
  .pen svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linejoin: round;
  }
  .tag {
    flex: none;
    padding: 1px 6px;
    border-radius: 999px;
    font: 600 10px/1.5 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--k-accent-ink, #0d1117);
    background: var(--k-accent, #5ab6eb);
  }
  .info {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .info.strong {
    color: var(--k-ink, #eee);
    font-weight: 600;
  }
  .info.warn {
    color: var(--k-warn, #f2a33a);
  }
  .btns {
    flex: none;
    display: flex;
    gap: 4px;
  }
  .danger {
    color: var(--k-err, #e5534b);
  }
  .err {
    margin: 0 0 8px;
    font: 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-err, #e5534b);
  }
</style>
