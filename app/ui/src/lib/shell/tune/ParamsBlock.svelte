<script lang="ts">
  // The active preset's params with this System's selection (SetParam). Up to four choices: a segmented
  // control; more: a select. Changes apply on the next launch.
  import type { ParamView, System } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { attempt, QUIET } from './util';

  interface Props {
    system: System;
    /** The tab shows a different preset than the active one (the params belong to the active one). */
    otherViewed: boolean;
  }
  let { system, otherViewed }: Props = $props();

  // A clicked value shows at once; the view model confirms it at its next snapshot.
  let pending = $state<Record<string, string>>({});
  let errs = $state<Record<string, string>>({});

  function current(p: ParamView): string {
    const want = pending[p.name];
    return want !== undefined && want !== p.value ? want : p.value;
  }

  $effect(() => {
    // Drop pending values the view model now reports (or that went stale).
    for (const p of system.params) if (pending[p.name] === p.value) delete pending[p.name];
  });

  async function set(p: ParamView, value: string) {
    if (value === current(p)) return;
    pending[p.name] = value;
    errs[p.name] = '';
    const err = await attempt(() => player.actions.setParam(system.id, p.name, value, QUIET));
    if (err) {
      delete pending[p.name];
      errs[p.name] = err;
    }
  }
</script>

<div class="params" role="group" aria-label="Params">
  {#each system.params as p (p.name)}
    {@const id = `tune-param-${p.name}`}
    <div class="field">
      <span id={id}>{p.label || p.name}</span>
      {#if p.choices.length <= 4}
        <div class="seg3" role="radiogroup" aria-labelledby={id}>
          {#each p.choices as c (c.value)}
            <button
              type="button"
              role="radio"
              aria-checked={current(p) === c.value}
              class:on={current(p) === c.value}
              disabled={!system.controllable}
              onclick={() => void set(p, c.value)}>{c.label || c.value}</button
            >
          {/each}
        </div>
      {:else}
        <select aria-labelledby={id} value={current(p)} disabled={!system.controllable} onchange={(e) => void set(p, e.currentTarget.value)}>
          {#each p.choices as c (c.value)}
            <option value={c.value}>{c.label || c.value}</option>
          {/each}
        </select>
      {/if}
    </div>
    {#if errs[p.name]}<p class="err indent" role="alert">{errs[p.name]}</p>{/if}
  {/each}
  <p class="hint indent">
    {otherViewed ? 'Params of the active preset. ' : ''}Changes apply on the next launch.
  </p>
</div>

<style>
  .params {
    display: grid;
    gap: 10px;
  }
</style>
