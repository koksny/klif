<script lang="ts">
  // "Save as new…": copies the shown preset, with the edits, under a new id. The edits leave the original draft
  // (they now live in the copy) and the tab shows the copy.
  import { onMount, tick } from 'svelte';
  import { specNumberProblem } from '../../model/presets';
  import type { System, ViewModel } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { clearSecrets, REMOTE_SECRET_REFUSAL } from './secret';
  import { getTune, presetPool, type Draft } from './state.svelte';
  import { attempt, cleanSpec, listNames, plain, presetIdProblem, QUIET, slugify, uniqueId } from './util';

  interface Props {
    vm: ViewModel;
    system: System;
    draft: Draft;
    onclose: () => void;
  }
  let { vm, system, draft, onclose }: Props = $props();
  const t = getTune();

  const taken = $derived(presetPool(vm, system.node).map((p) => p.id));
  // Prefilled once (never bound to the 2 Hz view model).
  const initialId = () => uniqueId(slugify(draft.id), taken);
  const initialName = () => `${draft.base?.info.name ?? draft.id} (copy)`;
  let id = $state(initialId());
  let name = $state(initialName());
  let err = $state('');
  let busy = $state(false);
  let first = $state<HTMLInputElement | undefined>();
  let touched = $state(false);

  onMount(() => {
    void tick().then(() => first?.select());
  });

  const problem = $derived(presetIdProblem(id.trim(), taken));

  async function save() {
    touched = true;
    if (problem) return;
    if (draft.loading) {
      err = 'The preset is still loading.';
      return;
    }
    const spec = cleanSpec(plain({ ...draft.spec, name: name.trim() || undefined }));
    const bad = specNumberProblem(spec);
    if (bad) {
      err = bad;
      return;
    }
    if (system.node) {
      const clear = clearSecrets(spec);
      if (clear.length) {
        err = `${REMOTE_SECRET_REFUSAL} (${listNames(clear)}).`;
        return;
      }
    }
    busy = true;
    const newId = id.trim();
    err = await attempt(() =>
      player.actions.savePreset(newId, spec, { node: system.node, secretsFrom: draft.isNew ? draft.secretsFrom : draft.id, ...QUIET }),
    );
    busy = false;
    if (err) return;
    if (draft.isNew) t.drop(draft.key);
    else t.revert(draft.key);
    t.view(system, newId);
    t.ensure(system.node, newId);
    onclose();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void save();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }
</script>

<div class="saveas" role="group" aria-label="Save as a new preset">
  <label class="field">
    <span>New preset id</span>
    <input bind:this={first} bind:value={id} spellcheck="false" autocomplete="off" onkeydown={onKey} oninput={() => (touched = true)} aria-invalid={touched && !!problem} />
  </label>
  <label class="field">
    <span>Name</span>
    <input bind:value={name} autocomplete="off" onkeydown={onKey} />
  </label>
  {#if touched && problem}<p class="err" role="alert">{problem}</p>{/if}
  {#if err}<p class="err" role="alert">{err}</p>{/if}
  <div class="btns">
    <button type="button" onclick={onclose}>Cancel</button>
    <button type="button" class="primary" disabled={busy || !!problem} onclick={() => void save()}>Save copy</button>
  </div>
</div>

<style>
  .saveas {
    display: grid;
    gap: 10px;
    padding: 12px;
    border: 1px dashed var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
  }
  .btns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
