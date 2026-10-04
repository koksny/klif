<script lang="ts">
  // Models for a System's kind: first the one the embedded pool suggests for this System's slot on this machine
  // (with the context and KV type it was sized for), then every model of the kind, one row each, opening to its
  // quant rungs. Use: the preset that already uses the files, else AdoptRecommendation (a preset from the rung, set
  // on the System). Local Systems only (a remote tab hides this section). Every number is an estimate.
  import type { RecommendationInfo, SuggestSlot, System, ViewModel } from '../../model/types';
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

  const recs = $derived(vm.recommendations.filter((r) => r.kind === system.kind));
  const slot = $derived<SuggestSlot>(system.kind === 'llm' ? (system.class ?? 'fast') : (system.kind as SuggestSlot));
  const sugg = $derived(vm.suggestions.find((s) => s.slot === slot));
  const suggRec = $derived(sugg?.rec ? recs.find((r) => r.id === sugg.rec) : undefined);
  const groups = $derived(group(recs));
  const hw = $derived(vm.hardware);
  let openGroup = $state<string | null>(null);
  let errs = $state<Record<string, string>>({});
  let busy = $state('');

  /** One row per model, its rungs in pool order. */
  function group(all: RecommendationInfo[]): { name: string; rungs: RecommendationInfo[] }[] {
    const out: { name: string; rungs: RecommendationInfo[] }[] = [];
    for (const r of all) {
      const g = out.find((x) => x.name === r.name);
      if (g) g.rungs.push(r);
      else out.push({ name: r.name, rungs: [r] });
    }
    return out;
  }

  async function use(r: RecommendationInfo, fit?: { ctx?: number; kv?: string }) {
    busy = r.id;
    errs[r.id] = await attempt(() =>
      r.existing ? player.actions.usePreset(system.id, r.existing, QUIET) : player.actions.adoptRecommendation(r.id, system.id, { ...QUIET, fit }),
    );
    busy = '';
    if (!errs[r.id]) t.view(system, r.existing ?? '');
  }

  async function openConfig() {
    errs.__config = await attempt(() => player.config.openConfig());
  }

  const gb = (v: number) => (v >= 100 ? Math.round(v).toString() : v.toFixed(1));
  const k = (n?: number) => (n ? (n >= 1024 ? `${Math.round(n / 1024)}k` : `${n}`) : '');
  const fitLine = $derived(
    sugg
      ? [
          sugg.ctx ? `ctx ${k(sugg.ctx)}` : '',
          sugg.kv ? `KV ${sugg.kv}` : '',
          `~${gb(sugg.estVramGiB)} of ${gb(sugg.budgetVramGiB)} GB VRAM`,
          sugg.estRamGiB > 0 ? `~${gb(sugg.estRamGiB)} GB RAM` : '',
          sugg.placement ?? '',
        ]
          .filter(Boolean)
          .join(' · ')
      : '',
  );
</script>

<Section id="tune-recs" title="Models" open={t.recOpen} ontoggle={(o) => (t.recOpen = o)}>
  {#snippet summary()}{sugg?.rec ? `suggested: ${sugg.model} ${sugg.quant}` : `${groups.length} models for this kind`}{/snippet}
  {#if hw}
    <p class="mach">
      This machine: <b>{gb(hw.vramPoolGiB)} GB</b> VRAM pool{hw.unified ? ' (unified)' : ''} · <b>{gb(hw.ramTotalGiB)} GB</b> RAM ·
      <b>{hw.tflopsFp32.toFixed(1)}</b> TFLOPS FP32
    </p>
  {/if}
  {#if sugg}
    <div class="sugg">
      <span class="lbl">Suggested for {system.label} on this machine</span>
      {#if suggRec}
        <RecCard {vm} rec={suggRec} selected>
          {#snippet action()}
            <button
              type="button"
              class="mini text"
              disabled={!!busy || !system.editable || (!suggRec.installed && !suggRec.existing)}
              title={suggRec.installed || suggRec.existing ? `Make it the preset of ${system.label}, sized as suggested` : 'Download it first.'}
              onclick={() => void use(suggRec, { ctx: sugg.ctx, kv: sugg.kv })}>{busy === suggRec.id ? 'Using…' : 'Use'}</button
            >
          {/snippet}
        </RecCard>
        <p class="fit">{fitLine}</p>
        {#if sugg.note}<p class="hint">{sugg.note}</p>{/if}
        {#if errs[suggRec.id]}<p class="err" role="alert">{errs[suggRec.id]}</p>{/if}
      {:else}
        <p class="hint">{sugg.note ?? 'Nothing in the model pool fits this machine for this System.'}</p>
      {/if}
    </div>
  {/if}
  <p class="hint">
    Estimates from the model pool of this KLIF version (weights, KV cache and overhead against the memory each System may
    use), not measurements: benchmark and calibrate on your machine (<code>klif-cli bench</code>). Each model keeps its own
    license. KLIF downloads only from huggingface.co, and only when you ask.
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
  {#if groups.length}<span class="lbl">Every model for this kind</span>{/if}
  {#each groups as g (g.name)}
    {@const installed = g.rungs.filter((r) => r.installed).length}
    <button type="button" class="grp" aria-expanded={openGroup === g.name} onclick={() => (openGroup = openGroup === g.name ? null : g.name)}>
      <span class="caret">{openGroup === g.name ? '▾' : '▸'}</span>
      <b>{g.name}</b>
      <span class="facts">{g.rungs.length} {g.rungs.length === 1 ? 'quant' : 'quants'}{installed ? ` · ${installed} installed` : ''} · {g.rungs[0].license}</span>
    </button>
    {#if openGroup === g.name}
      {#each g.rungs as r (r.id)}
        <RecCard {vm} rec={r} selected={r.id === suggRec?.id}>
          {#snippet action()}
            <button
              type="button"
              class="mini text"
              disabled={!!busy || !system.editable || (!r.installed && !r.existing)}
              title={r.installed || r.existing ? `Make it the preset of ${system.label}` : 'Download it first.'}
              onclick={() => void use(r, r.id === sugg?.rec ? { ctx: sugg.ctx, kv: sugg.kv } : undefined)}>{busy === r.id ? 'Using…' : 'Use'}</button
            >
          {/snippet}
        </RecCard>
        {#if errs[r.id]}<p class="err" role="alert">{errs[r.id]}</p>{/if}
      {/each}
    {/if}
  {:else}
    <p class="hint">No models for this kind in the pool yet.</p>
  {/each}
</Section>

<style>
  .mach {
    margin: 0 0 8px;
    font: 500 12px/1.4 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .mach b {
    font: 600 12px var(--k-font-data, ui-monospace, monospace);
    color: var(--k-ink, #eee);
  }
  .sugg {
    display: grid;
    gap: 6px;
    margin-bottom: 10px;
    padding: 10px;
    border: 1px solid color-mix(in srgb, var(--k-accent, #5ab6eb) 45%, var(--k-line, #2e2e2e));
    border-radius: var(--k-radius, 6px);
    background: color-mix(in srgb, var(--k-accent, #5ab6eb) 6%, transparent);
  }
  .lbl {
    display: block;
    margin: 6px 0 4px;
    font: 600 10px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .sugg .lbl {
    margin-top: 0;
    color: var(--k-accent, #5ab6eb);
  }
  .fit {
    margin: 0;
    font: 500 11.5px/1.35 var(--k-font-data, ui-monospace, monospace);
    color: var(--k-ink, #eee);
  }
  .grp {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    padding: 7px 4px;
    border: 0;
    border-bottom: 1px solid var(--k-line, #2e2e2e);
    background: transparent;
    color: var(--k-ink, #eee);
    font: 500 13px var(--k-font-ui, system-ui, sans-serif);
    text-align: left;
    cursor: pointer;
  }
  .grp:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
  }
  .grp .caret {
    width: 10px;
    color: var(--k-muted, #8a8a8a);
  }
  .grp .facts {
    margin-left: auto;
    font: 11.5px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
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
