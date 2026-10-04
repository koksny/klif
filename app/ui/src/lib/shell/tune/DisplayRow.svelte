<script lang="ts">
  // This machine's display settings ([ui] in klif.toml): for now whether a broken record is celebrated over the skin.
  import type { ConfigInfo } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { attempt, QUIET } from './util';

  interface Props {
    config: ConfigInfo;
  }
  let { config }: Props = $props();
  let busy = $state(false);
  let err = $state('');

  async function toggle() {
    busy = true;
    err = await attempt(() => player.actions.updateSettings({ recordMoment: !config.recordMoment }, QUIET));
    busy = false;
  }
</script>

<div class="row">
  <div class="text">
    <b>New record moment</b>
    <span>Celebrate a broken record over the skin while models run. Records are kept either way.</span>
  </div>
  <!-- Tune's shared switch (TuneDrawer :global .switch): the same control as External and the params -->
  <button
    type="button"
    class="switch"
    class:on={config.recordMoment}
    role="switch"
    aria-checked={config.recordMoment}
    aria-label="New record moment"
    disabled={busy}
    onclick={() => void toggle()}
  >
    <i></i>{config.recordMoment ? 'on' : 'off'}
  </button>
</div>
{#if err}<p class="err" role="alert">{err}</p>{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 18px;
    border-top: 1px solid var(--k-line, #2e2e2e);
  }
  .text {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  b {
    font: 600 13px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  span {
    font: 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .err {
    margin: 0 18px 8px;
    color: var(--k-danger, #e05a5a);
    font: 12px var(--k-font-ui, system-ui, sans-serif);
  }
</style>
