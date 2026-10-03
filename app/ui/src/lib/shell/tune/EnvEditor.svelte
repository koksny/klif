<script lang="ts">
  // Environment of the preset: its own rows (secret values masked; "••••" = keep the stored value), the rows
  // KLIF adds itself (read-only, "set by KLIF", Override copies one into the preset), and env_remove.
  import { tick, untrack } from 'svelte';
  import type { CommandView } from '../../model/types';
  import { isSecretEnv, MASK, REMOTE_SECRET_REFUSAL } from './secret';
  import { stableJson } from './util';

  interface Props {
    /** The draft's env (bound; replaced on every edit). */
    env: Record<string, string>;
    /** The draft's env_remove (bound). */
    envRemove: string[];
    preview: CommandView | null;
    readonly?: boolean;
    remote?: boolean;
  }
  let { env = $bindable(), envRemove = $bindable(), preview, readonly = false, remote = false }: Props = $props();

  interface Row {
    name: string;
    value: string;
  }
  let rows = $state<Row[]>([]);
  let written = '';
  let box = $state<HTMLDivElement | undefined>();
  let removeName = $state('');

  // Rows follow the spec when it changes from outside (another preset, Revert); our own writes are skipped.
  $effect(() => {
    const json = stableJson(env ?? {});
    if (json === written) return;
    untrack(() => {
      rows = Object.entries(env ?? {}).map(([name, value]) => ({ name, value }));
      written = json;
    });
  });

  function commit() {
    const next: Record<string, string> = {};
    for (const r of rows) {
      const n = r.name.trim();
      if (n) next[n] = r.value;
    }
    written = stableJson(next);
    env = next;
  }

  const upper = (s: string) => s.trim().toUpperCase();
  const dupes = $derived.by(() => {
    const seen = new Set<string>();
    const out = new Set<string>();
    for (const r of rows) {
      const n = upper(r.name);
      if (!n) continue;
      if (seen.has(n)) out.add(n);
      seen.add(n);
    }
    return out;
  });
  const managed = $derived((preview?.env ?? []).filter((e) => e.managed && !e.overridden && !rows.some((r) => upper(r.name) === upper(e.name))));
  const overridden = $derived(new Set((preview?.env ?? []).filter((e) => e.managed && e.overridden).map((e) => upper(e.name))));

  async function focusRow(i: number, part: 'name' | 'value') {
    await tick();
    box?.querySelector<HTMLInputElement>(`[data-env="${i}"][data-part="${part}"]`)?.focus();
  }

  function add() {
    rows.push({ name: '', value: '' });
    void focusRow(rows.length - 1, 'name');
  }

  function remove(i: number) {
    rows.splice(i, 1);
    commit();
  }

  function override(name: string, value: string | undefined, secret: boolean) {
    rows.push({ name, value: secret ? '' : (value ?? '') });
    commit();
    void focusRow(rows.length - 1, 'value');
  }

  function addRemove() {
    const n = removeName.trim();
    if (!n) return;
    const list = envRemove ?? [];
    if (!list.some((x) => upper(x) === upper(n))) envRemove = [...list, n];
    removeName = '';
  }

  function dropRemove(n: string) {
    envRemove = (envRemove ?? []).filter((x) => x !== n);
  }
</script>

<div class="field env">
  <span id="tune-env-lbl">Env</span>
  <div class="envbox" bind:this={box} role="group" aria-labelledby="tune-env-lbl">
    {#each rows as r, i (i)}
      {@const secret = isSecretEnv(r.name)}
      {@const n = upper(r.name)}
      <div class="erow">
        <input
          class="ename"
          data-env={i}
          data-part="name"
          value={r.name}
          placeholder="NAME"
          spellcheck="false"
          autocomplete="off"
          aria-label="Variable name"
          {readonly}
          oninput={(e) => {
            r.name = e.currentTarget.value;
            commit();
          }}
        />
        <input
          class="evalue"
          data-env={i}
          data-part="value"
          type={secret ? 'password' : 'text'}
          value={r.value}
          placeholder={secret ? 'secret' : 'value'}
          spellcheck="false"
          autocomplete="off"
          aria-label="Value of {r.name || 'the variable'}"
          {readonly}
          onfocus={(e) => {
            if (secret && r.value === MASK) e.currentTarget.select();
          }}
          oninput={(e) => {
            r.value = e.currentTarget.value;
            commit();
          }}
        />
        {#if !readonly}<button type="button" class="mini" aria-label="Remove {r.name || 'variable'}" title="Remove" onclick={() => remove(i)}>×</button>{/if}
        {#if secret && r.value === MASK}<small class="note">kept</small>{/if}
        {#if overridden.has(n)}<small class="note">overrides KLIF</small>{/if}
        {#if dupes.has(n)}<small class="note warn">duplicate name</small>{/if}
        {#if remote && secret && r.value !== '' && r.value !== MASK}<small class="note warn">{REMOTE_SECRET_REFUSAL}.</small>{/if}
      </div>
    {/each}
    {#each managed as m (m.name)}
      <div class="erow managed">
        <input class="ename" value={m.name} readonly aria-label="Variable set by KLIF" />
        <input class="evalue" value={m.secret ? MASK : (m.value ?? '')} type={m.secret ? 'password' : 'text'} readonly aria-label="Value set by KLIF" />
        {#if !readonly}
          <button type="button" class="mini text" title="Copy it into the preset to change it" onclick={() => override(m.name, m.value, m.secret)}>Override</button>
        {/if}
        <small class="note">set by KLIF</small>
      </div>
    {/each}
    {#if !readonly}<div><button type="button" class="mini wide" onclick={add}>+ env</button></div>{/if}

    <div class="removed">
      <small class="hint">Remove from the inherited environment:</small>
      <div class="chips">
        {#each envRemove ?? [] as n (n)}
          <span class="chip">
            {n}
            {#if !readonly}<button type="button" class="x" aria-label="Keep {n}" title="Keep it" onclick={() => dropRemove(n)}>×</button>{/if}
          </span>
        {:else}
          <small class="hint">nothing</small>
        {/each}
      </div>
      {#if !readonly}
        <div class="addremove">
          <input
            bind:value={removeName}
            placeholder="VARIABLE"
            spellcheck="false"
            autocomplete="off"
            aria-label="Variable to remove"
            onkeydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                addRemove();
              }
            }}
          />
          <button type="button" class="mini text" disabled={!removeName.trim()} onclick={addRemove}>Remove</button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .field.env {
    align-items: start;
  }
  .field.env > span {
    padding-top: 9px;
  }
  .envbox {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .erow {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 6px;
    min-width: 0;
  }
  .ename,
  .evalue {
    min-width: 0;
    font-family: var(--k-font-data, ui-monospace, monospace);
    font-size: 12px;
    padding: 6px 8px;
  }
  .ename {
    flex: 0 1 38%;
  }
  .evalue {
    flex: 1 1 0;
  }
  .managed .ename,
  .managed .evalue {
    color: var(--k-muted, #8a8a8a);
    border-style: dashed;
  }
  .note {
    flex-basis: 100%;
    color: var(--k-muted, #8a8a8a);
    font: 500 11px/1.2 var(--k-font-ui, system-ui, sans-serif);
  }
  .managed .note,
  .erow .note + .note {
    flex-basis: auto;
  }
  .note.warn {
    color: var(--k-warn, #f2a33a);
  }
  .removed {
    display: grid;
    gap: 6px;
    margin-top: 6px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .addremove {
    display: flex;
    gap: 6px;
  }
  .addremove input {
    flex: 1;
    min-width: 0;
    font-family: var(--k-font-data, ui-monospace, monospace);
    font-size: 12px;
    padding: 6px 8px;
  }
</style>
