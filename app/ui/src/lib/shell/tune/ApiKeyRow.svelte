<script lang="ts">
  // KLIF's API key (this machine): where it comes from, set or not, Set… (native, never echoed back) and Clear.
  import type { ApiKeyInfo } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { tick } from 'svelte';
  import { getTune } from './state.svelte';
  import { attempt } from './util';

  interface Props {
    info: ApiKeyInfo;
  }
  let { info }: Props = $props();
  const t = getTune();

  const source = $derived(
    info.source === 'file' ? 'api-key.txt' : info.source.startsWith('env:') ? `%${info.source.slice(4)}%` : info.source === 'none' ? 'off' : info.source,
  );
  let editing = $state(false);
  let value = $state('');
  let err = $state('');
  let busy = $state(false);
  let input = $state<HTMLInputElement | undefined>();

  async function startSet() {
    editing = true;
    value = '';
    err = '';
    await tick();
    input?.focus();
  }

  function cancel() {
    editing = false;
    value = '';
  }

  async function save() {
    const key = value.trim();
    if (!key) return;
    busy = true;
    err = await attempt(() => player.config.setApiKey(key));
    busy = false;
    value = '';
    if (!err) editing = false;
  }

  async function clear() {
    const ok = await t.confirm({
      title: 'Clear the API key?',
      detail: 'Servers launched from now on run without it; clients that send it keep working only where the server ignores keys.',
      confirm: 'Clear key',
      danger: true,
    });
    if (!ok) return;
    busy = true;
    err = await attempt(() => player.config.setApiKey(null));
    busy = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void save();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      cancel();
    }
  }
</script>

<div class="field apikey">
  <span>API key</span>
  <div class="stack">
    <div class="line">
      <span class="ro">
        {info.source === 'none' ? 'Off' : info.set ? 'Set' : 'Not set'}
        <small>· {info.source === 'none' ? '[security] api_key = "none"' : `from ${source}`}</small>
      </span>
      <span class="grow"></span>
      {#if info.source !== 'none' && !editing}
        <button type="button" class="mini text" disabled={busy} onclick={() => void startSet()}>Set…</button>
        <button type="button" class="mini text" disabled={busy || !info.set} onclick={() => void clear()}>Clear</button>
      {/if}
    </div>
    {#if editing}
      <div class="line">
        <input
          bind:this={input}
          bind:value
          type="password"
          autocomplete="off"
          spellcheck="false"
          aria-label="New API key"
          placeholder="new key"
          onkeydown={onKey}
        />
        <button type="button" class="mini text" onclick={cancel}>Cancel</button>
        <button type="button" class="mini text primary" disabled={busy || !value.trim()} onclick={() => void save()}>Save</button>
      </div>
    {/if}
    {#if err}<p class="err" role="alert">{err}</p>{/if}
  </div>
</div>

<style>
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .line input {
    flex: 1;
    min-width: 0;
  }
  .ro small {
    margin-left: 4px;
  }
</style>
