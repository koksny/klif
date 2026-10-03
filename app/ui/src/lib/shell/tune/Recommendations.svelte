<script lang="ts">
  // Model suggestions for a System's kind (its class first), with the disclaimer and the models_dir notice.
  // Use: the preset that already uses the files, else AdoptRecommendation (a preset from the card, set on the
  // System). Local Systems only (a remote tab hides this section).
  import type { RecommendationInfo, System, ViewModel } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import RecCard from './RecCard.svelte';
  import Section from './Section.svelte';
  import { getTune } from './state.svelte';
  import { attempt, QUIET } from './util';

  interface Props {
    vm: ViewModel;
    system: System;
  }
  let { vm, system }: Props = $props();
  const t = getTune();

  const recs = $derived(sortRecs(vm.recommendations, system));
  let errs = $state<Record<string, string>>({});
  let busy = $state('');

  function sortRecs(all: RecommendationInfo[], s: System): RecommendationInfo[] {
    const same = all.filter((r) => r.kind === s.kind);
    if (s.kind !== 'llm' || !s.class) return same;
    return [...same.filter((r) => r.class === s.class), ...same.filter((r) => r.class !== s.class)];
  }

  async function use(r: RecommendationInfo) {
    busy = r.id;
    errs[r.id] = await attempt(() =>
      r.existing ? player.actions.usePreset(system.id, r.existing, QUIET) : player.actions.adoptRecommendation(r.id, system.id, QUIET),
    );
    busy = '';
    if (!errs[r.id]) t.view(system, r.existing ?? '');
  }

  async function openConfig() {
    errs.__config = await attempt(() => player.config.openConfig());
  }
</script>

<Section id="tune-recs" title="Recommended" open={t.recOpen} ontoggle={(o) => (t.recOpen = o)}>
  {#snippet summary()}{recs.length ? `${recs.length} for this kind` : 'none for this kind'}{/snippet}
  <p class="hint">
    Starting points measured on one machine. Models, drivers and backends change: let your agent benchmark and calibrate. Each
    model keeps its own license. KLIF downloads only from huggingface.co, and only when you ask.
  </p>
  {#if !vm.config.modelsDir}
    <div class="notice">
      <p>Downloads need a models folder: set <code>[paths] models_dir</code> in klif.toml.</p>
      <button type="button" class="mini text" onclick={() => void openConfig()}>Open klif.toml</button>
    </div>
    {#if errs.__config}<p class="err" role="alert">{errs.__config}</p>{/if}
  {:else}
    <p class="hint">Models folder: <code>{vm.config.modelsDir}</code></p>
  {/if}
  {#each recs as r (r.id)}
    <RecCard {vm} rec={r}>
      {#snippet action()}
        <button
          type="button"
          class="mini text"
          disabled={!!busy || !system.editable || (!r.installed && !r.existing)}
          title={r.installed || r.existing ? `Make it ${system.label}'s preset` : 'Download it first.'}
          onclick={() => void use(r)}>{busy === r.id ? 'Using…' : 'Use'}</button
        >
      {/snippet}
    </RecCard>
    {#if errs[r.id]}<p class="err" role="alert">{errs[r.id]}</p>{/if}
  {:else}
    <p class="hint">No recommendations for this kind yet.</p>
  {/each}
</Section>

<style>
  .notice {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--k-warn, #f2a33a);
    border-radius: var(--k-radius, 6px);
  }
  .notice p {
    flex: 1;
    margin: 0;
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  code {
    font: 500 12px/1.2 var(--k-font-data, ui-monospace, monospace);
  }
</style>
