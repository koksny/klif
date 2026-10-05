<script lang="ts">
  // This machine's display settings ([ui] in klif.toml): the skin (also F2 and the tray's Skin menu; the window writes
  // its choice to [ui] skin) and whether a broken record is celebrated over the skin. One line each, in the drawer's
  // label | value grid.
  import type { ConfigInfo } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { ui } from '../../state/ui.svelte';
  import { SKINS, skinMeta } from '../../../skins/registry';
  import { attempt, QUIET } from './util';

  interface Props {
    config: ConfigInfo;
  }
  let { config }: Props = $props();
  let busy = $state(false);
  let err = $state('');
  const skin = $derived(skinMeta(ui.skinId));

  async function toggle() {
    busy = true;
    err = await attempt(() => player.actions.updateSettings({ recordMoment: !config.recordMoment }, QUIET));
    busy = false;
  }
</script>

<div class="field">
  <span id="tune-skin-lbl">Skin</span>
  <div class="line">
    <select aria-labelledby="tune-skin-lbl" title={skin.blurb} value={skin.id} onchange={(e) => ui.setSkin(e.currentTarget.value)}>
      {#each SKINS as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
    </select>
    <span class="hint">F2 cycles</span>
  </div>
</div>
<div class="field">
  <span id="tune-rec-lbl">Record moment</span>
  <div class="line">
    <!-- Tune's shared switch (TuneDrawer :global .switch): the same control as External and the params -->
    <button
      type="button"
      class="switch"
      class:on={config.recordMoment}
      role="switch"
      aria-checked={config.recordMoment}
      aria-labelledby="tune-rec-lbl"
      title="Records are kept either way"
      disabled={busy}
      onclick={() => void toggle()}
    >
      <i></i>{config.recordMoment ? 'on' : 'off'}
    </button>
    <span class="hint">celebrate a broken record over the skin</span>
  </div>
</div>
{#if err}<p class="err" role="alert">{err}</p>{/if}

<style>
  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  /* beats the drawer's shared select width (100%) */
  .line select {
    flex: none;
    width: 150px;
  }
  .hint {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .err {
    margin: 0;
    color: var(--k-danger, #e05a5a);
    font: 12px var(--k-font-ui, system-ui, sans-serif);
  }
</style>
