<script lang="ts">
  // A System's conversation slots in Options ([systems.<id>] slots): on or off (it changes the launch command, so it
  // applies on the next launch), then auto-save, auto-load and the three slots.
  import type { System } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import SlotsControls from '../slots/SlotsControls.svelte';
  import Section from './Section.svelte';
  import { getTune } from './state.svelte';
  import { attempt, QUIET } from './util';

  interface Props {
    system: System;
  }
  let { system }: Props = $props();
  const t = getTune();

  let busy = $state(false);
  let err = $state('');
  const v = $derived(system.slots);
  const on = $derived(!!v);

  async function toggle() {
    busy = true;
    err = await attempt(() => player.actions.updateSystem(system.id, { slots: !on }, QUIET));
    busy = false;
  }

  const headline = $derived.by(() => {
    if (!v) return 'off';
    const parts = [`slot ${v.active}`];
    if (v.autoSave) parts.push('auto-save');
    if (v.autoLoad) parts.push('auto-load');
    if (v.job) parts.push(v.job.op === 'save' ? 'saving' : 'loading');
    return parts.join(' · ');
  });
</script>

<Section id="tune-slots" title="Slots" open={t.slotsOpen} ontoggle={(o) => (t.slotsOpen = o)}>
  {#snippet summary()}{headline}{/snippet}
  <div class="row">
    <div class="desc">
      <b>Conversation slots</b>
      <span>Save what the server holds to disk and load it back later, so a long conversation is not read again after a restart. On adds <code>--slot-save-path</code> to the command: it applies on the next launch.</span>
    </div>
    <button
      type="button"
      class="switch"
      class:on
      role="switch"
      aria-checked={on}
      aria-label="Conversation slots"
      disabled={busy || !system.editable}
      onclick={() => void toggle()}
    >
      <i></i>{on ? 'on' : 'off'}
    </button>
  </div>
  {#if v}<SlotsControls {system} />{/if}
  {#if err}<p class="err" role="alert">{err}</p>{/if}
</Section>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 2px 0 10px;
  }
  .desc {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  b {
    font: 600 13px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #eee);
  }
  .desc span {
    font: 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .err {
    margin: 0 0 8px;
    font: 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-err, #e5534b);
  }
</style>
