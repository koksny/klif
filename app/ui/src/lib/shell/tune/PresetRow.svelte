<script lang="ts">
  // The preset row: which preset the tab shows (select: name · quant · backend + badges), Use (make it the
  // System's active preset), and ⋯: New blank preset, Save as new…, Delete…, Open klif.toml, Open logs.
  // Picking a preset here only SHOWS it (Command editor, footer "Use & launch"); Use writes it.
  import { AVAILABILITY_TEXT, joinParts } from '../../model/systems';
  import type { PresetInfo, System, ViewModel } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import Menu from './Menu.svelte';
  import SaveAsForm from './SaveAsForm.svelte';
  import { getTune, presetPool } from './state.svelte';
  import type { MenuItem } from './types';
  import { attempt, defaultAdapter, labelsOf, listNames, QUIET, slugify, uniqueId, usersOf } from './util';

  interface Props {
    vm: ViewModel;
    system: System;
    presets: PresetInfo[];
    viewedId: string;
  }
  let { vm, system, presets, viewedId }: Props = $props();
  const t = getTune();

  const remote = $derived(!!system.node);
  const nodeDown = $derived(remote && vm.nodes.find((n) => n.id === system.node)?.state !== 'online');
  const isNewView = $derived(viewedId.startsWith('new|'));
  const draft = $derived(viewedId ? t.draft(system.node, viewedId) : undefined);
  const info = $derived(isNewView ? undefined : presets.find((p) => p.id === viewedId));
  const missing = $derived(!!viewedId && !isNewView && !info);
  const isActive = $derived(!!viewedId && viewedId === system.preset);
  const active = $derived(presets.find((p) => p.id === system.preset));

  let err = $state('');
  let busy = $state(false);
  let saveAs = $state(false);

  function optionText(p: PresetInfo): string {
    const badges = [
      p.id === system.preset ? 'active' : '',
      p.recommended ? 'recommended' : '',
      p.availability !== 'ready' ? 'invalid' : '',
      t.isDirty(t.draft(system.node, p.id)) ? 'edited' : '',
      p.external ? 'external' : '',
    ].filter(Boolean);
    const main = joinParts([p.name, p.model.quant, p.model.backend]);
    return badges.length ? `${main}  —  ${badges.join(', ')}` : main;
  }

  function pick(id: string) {
    err = '';
    saveAs = false;
    t.view(system, id);
  }

  async function use() {
    if (!viewedId || isNewView) return;
    busy = true;
    err = await attempt(() => player.actions.usePreset(system.id, viewedId, QUIET));
    busy = false;
  }

  function newBlank() {
    const taken = presetPool(vm, system.node).map((p) => p.id);
    const adapter = defaultAdapter(system.kind);
    const key = t.createNew(
      system.node,
      uniqueId(slugify(system.label), taken),
      { adapter, kind: adapter === 'generic' ? system.kind : undefined },
      { selectFor: system.preset ? undefined : system.id },
    );
    t.view(system, key);
    t.commandOpen = true;
  }

  async function remove() {
    // Captured now: `info` and `draft` follow the view model and are gone once the preset is deleted.
    const p = info;
    const d = draft;
    if (!p) return;
    const ok = await t.confirm({
      title: `Delete preset ${p.name}?`,
      detail: `This removes [presets.${p.id}] from ${remote ? "that node's " : ''}klif.toml${t.isDirty(d) ? ' and drops your unsaved edits' : ''}.`,
      confirm: 'Delete preset',
      danger: true,
    });
    if (!ok) return;
    busy = true;
    err = await attempt(() => player.actions.deletePreset(p.id, system.node, QUIET));
    busy = false;
    if (err) return;
    if (d) t.drop(d.key);
    t.view(system, '');
  }


  async function openConfig() {
    err = await attempt(() => player.config.openConfig());
  }
  async function openLogs() {
    err = await attempt(() => player.config.openLogs());
  }

  const users = $derived(info ? usersOf(vm, system.node, info.id) : []);
  const items = $derived.by<MenuItem[]>(() => {
    const out: MenuItem[] = [];
    if (system.editable) {
      out.push({ label: 'New blank preset', onselect: newBlank });
      out.push({
        label: 'Save as new…',
        disabled: !draft || draft.loading || !!draft.loadError,
        hint: 'A copy of the shown preset, with your edits, under a new id.',
        onselect: () => (saveAs = true),
      });
      out.push({
        label: 'Delete…',
        danger: true,
        disabled: !info || users.length > 0,
        hint: users.length ? `Used by ${listNames(labelsOf(vm, users.map((u) => u.id)))}.` : 'Remove it from klif.toml.',
        onselect: () => void remove(),
      });
    }
    if (!remote) {
      out.push({ label: 'Open klif.toml', onselect: () => void openConfig() });
      out.push({ label: 'Open logs', onselect: () => void openLogs() });
    }
    return out;
  });

  const badges = $derived.by(() => {
    const out: { text: string; tone: string; title?: string }[] = [];
    if (info?.recommended) out.push({ text: 'recommended', tone: '' });
    if (info && info.availability !== 'ready') out.push({ text: 'invalid', tone: 'warn', title: info.reason });
    if (t.isDirty(draft)) out.push({ text: 'edited', tone: 'accent' });
    if (info?.external) out.push({ text: 'external', tone: '' });
    return out;
  });
  const why = $derived(
    info && info.availability !== 'ready'
      ? (info.reason ?? `Cannot launch: ${AVAILABILITY_TEXT[info.availability]}.`)
      : viewedId && !isActive && active
        ? `Active: ${active.name}`
        : !viewedId && !presets.length
          ? `No ${system.kind === 'llm' ? 'LLM' : system.kind} presets on ${remote ? 'that node' : 'this machine'} yet.`
          : '',
  );

  const useHint = $derived(
    !system.controllable
      ? 'This node does not allow changing the active preset.'
      : !viewedId || isNewView
        ? 'Apply the new preset first.'
        : isActive
          ? 'Already the active preset.'
          : `Make it ${system.label}'s preset (applies on the next launch).`,
  );
</script>

<div class="presetrow">
  <div class="field">
    <span id="tune-preset-lbl">Preset</span>
    <div class="line">
      <select
        aria-labelledby="tune-preset-lbl"
        value={viewedId}
        onchange={(e) => pick(e.currentTarget.value)}
        disabled={busy}
      >
        {#if !viewedId}<option value="">No preset</option>{/if}
        {#if isNewView && draft}<option value={viewedId}>New preset “{draft.id}” (not applied)</option>{/if}
        {#if missing}<option value={viewedId}>{viewedId} ({nodeDown ? 'node not reachable' : 'not in klif.toml'})</option>{/if}
        {#each presets as p (p.id)}
          <option value={p.id}>{optionText(p)}</option>
        {/each}
      </select>
      <button
        type="button"
        class="use"
        disabled={busy || !system.controllable || !viewedId || isNewView || isActive || missing}
        title={useHint}
        onclick={() => void use()}>Use</button
      >
      {#if items.length}<Menu label="Preset actions" {items} disabled={busy} />{/if}
    </div>
  </div>

  {#if badges.length || why}
    <div class="meta">
      {#each badges as b (b.text)}<span class="badge {b.tone}" title={b.title ?? ''}>{b.text}</span>{/each}
      {#if why}<span class="why">{why}</span>{/if}
    </div>
  {/if}

  {#if !viewedId && system.editable}
    <div class="startblank">
      <button type="button" onclick={newBlank}>New blank preset</button>
    </div>
  {/if}

  {#if saveAs && draft}
    <SaveAsForm {vm} {system} {draft} onclose={() => (saveAs = false)} />
  {/if}
  {#if err}<p class="err" role="alert">{err}</p>{/if}
</div>

<style>
  .presetrow {
    display: grid;
    gap: 8px;
  }
  .line {
    display: flex;
    gap: 6px;
    min-width: 0;
  }
  .line select {
    flex: 1;
  }
  .use {
    flex: none;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding-left: 124px;
    min-height: 0;
  }
  .meta:empty {
    display: none;
  }
  .why {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  .startblank {
    padding-left: 124px;
  }
</style>
