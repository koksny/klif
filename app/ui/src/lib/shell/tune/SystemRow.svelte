<script lang="ts">
  // Top of a System tab: its label (inline rename), status + reason, the node it lives on, and the ⋯ menu
  // (move, exclusive GPU, remove). Remote Systems are read-only unless their node grants "edit".
  import { tick } from 'svelte';
  import { isHeld, STATUS_TEXT } from '../../model/systems';
  import type { NodeView, System, ViewModel } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { ui } from '../../state/ui.svelte';
  import StatusDot from '../SystemTabs/StatusDot.svelte';
  import Menu from './Menu.svelte';
  import { getTune } from './state.svelte';
  import type { MenuItem } from './types';
  import { attempt, QUIET } from './util';

  interface Props {
    vm: ViewModel;
    system: System;
    node: NodeView | undefined;
  }
  let { vm, system, node }: Props = $props();
  const t = getTune();

  const ghost = $derived(!system.node && !system.editable);
  const siblings = $derived(vm.systems.filter((s) => (s.node ?? '') === (system.node ?? '') && s.editable));
  const index = $derived(siblings.findIndex((s) => s.id === system.id));
  const held = $derived(isHeld(system));
  const NODE_STATE: Record<NodeView['state'], string> = {
    connecting: 'connecting',
    online: 'online',
    offline: 'offline',
    unauthorized: 'not authorized',
    incompatible: 'incompatible version',
  };

  let renaming = $state(false);
  let draftLabel = $state('');
  let input = $state<HTMLInputElement | undefined>();
  let err = $state('');
  let busy = $state(false);

  async function startRename() {
    if (!system.editable) return;
    draftLabel = system.label;
    renaming = true;
    err = '';
    await tick();
    input?.select();
  }

  async function commitRename() {
    if (!renaming) return;
    renaming = false;
    const label = draftLabel.trim();
    if (!label || label === system.label) return;
    busy = true;
    err = await attempt(() => player.actions.updateSystem(system.id, { label }, QUIET));
    busy = false;
  }

  function onRenameKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void commitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      renaming = false;
    }
  }

  async function update(patch: { moveTo?: number; exclusive?: boolean }) {
    busy = true;
    err = await attempt(() => player.actions.updateSystem(system.id, patch, QUIET));
    busy = false;
  }

  async function remove() {
    const id = system.id;
    const ok = await t.confirm({
      title: `Remove ${system.label}?`,
      detail: `This removes [systems.${system.id.split('/').pop()}] from ${system.node ? `${node?.name ?? system.node}'s ` : ''}klif.toml. Its presets stay.`,
      confirm: 'Remove System',
      danger: true,
    });
    if (!ok) return;
    busy = true;
    err = await attempt(() => player.actions.removeSystem(id, QUIET));
    busy = false;
    if (!err && ui.tuneSystem === id) ui.tuneSystem = null;
  }

  const items = $derived<MenuItem[]>([
    { label: 'Move left', disabled: index <= 0, onselect: () => update({ moveTo: index - 1 }) },
    { label: 'Move right', disabled: index < 0 || index >= siblings.length - 1, onselect: () => update({ moveTo: index + 1 }) },
    {
      label: 'Exclusive GPU',
      checked: system.exclusive,
      hint: 'Needs the whole GPU: launching it asks to stop every other System on that GPU, and the other way round.',
      onselect: () => update({ exclusive: !system.exclusive }),
    },
    {
      label: 'Remove System…',
      danger: true,
      disabled: held,
      hint: held ? 'Stop it first.' : 'Remove it from klif.toml.',
      onselect: () => void remove(),
    },
  ]);

  const readOnlyHint = $derived(
    ghost
      ? 'Not in klif.toml: only Stop and Dismiss are offered.'
      : system.node && !system.editable
        ? `Read-only: ${node?.name ?? system.node} does not grant "edit".`
        : '',
  );
</script>

<div class="sysrow">
  <div class="name">
    {#if renaming}
      <input
        class="rename"
        bind:this={input}
        bind:value={draftLabel}
        aria-label="System label"
        maxlength="64"
        onkeydown={onRenameKey}
        onblur={() => void commitRename()}
      />
    {:else if system.editable}
      <button type="button" class="label" title="Rename" aria-label="Rename {system.label}" onclick={startRename}>{system.label}</button>
    {:else}
      <span class="label ro">{system.label}</span>
    {/if}
    <Menu label="System actions" {items} disabled={!system.editable || busy} />
  </div>
  <div class="status">
    <StatusDot status={system.status} reason={system.reason} />
    <span class="st">{STATUS_TEXT[system.status]}</span>
    {#if system.reason}<span class="reason">{system.reason}</span>{/if}
  </div>
  {#if system.node}
    <div class="where">
      on <b>{node?.name ?? system.node}</b>{node ? ` · ${NODE_STATE[node.state]}` : ''}{node?.latencyMs !== undefined ? ` · ${Math.round(node.latencyMs)} ms` : ''}
    </div>
  {/if}
  {#if readOnlyHint}<p class="hint">{readOnlyHint}</p>{/if}
  {#if err}<p class="err" role="alert">{err}</p>{/if}
</div>

<style>
  .sysrow {
    display: grid;
    gap: 6px;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .label {
    flex: 1;
    min-width: 0;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 600 16px/1.2 var(--k-font-display, var(--k-font-ui, system-ui));
    letter-spacing: 0.03em;
    border-color: transparent;
    padding: 6px 6px 6px 0;
  }
  button.label:hover:not(:disabled) {
    border-color: transparent;
    text-decoration: underline dotted;
    text-underline-offset: 4px;
  }
  .label.ro {
    padding: 6px 0;
  }
  .rename {
    flex: 1;
    min-width: 0;
    font: 600 15px/1.2 var(--k-font-ui, system-ui, sans-serif);
  }
  .status {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 0 8px;
    font: 500 12px/1.4 var(--k-font-ui, system-ui, sans-serif);
  }
  .status :global(.dot) {
    margin-right: 0;
    align-self: center;
  }
  .st {
    color: var(--k-ink, #e6e6e6);
  }
  .reason,
  .where {
    color: var(--k-muted, #8a8a8a);
  }
  .where {
    font: 500 12px/1.3 var(--k-font-ui, system-ui, sans-serif);
  }
  .where b {
    color: var(--k-ink, #e6e6e6);
    font-weight: 600;
  }
</style>
