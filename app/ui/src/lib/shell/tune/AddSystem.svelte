<script lang="ts">
  // Add mode ("+" or the onboarding card): kind (and LLM class), label (prefilled), where it lives (only when a
  // node grants "edit"), then a starting point: Recommended (cards for that kind/class), Existing preset, or
  // Blank (the Command editor with an empty preset). AddSystem (+ AdoptRecommendation), then the drawer switches
  // to the new tab.
  import { joinParts, KIND_LABEL } from '../../model/systems';
  import type { LlmClass, PresetInfo, RecommendationInfo, SystemId, SystemKind, ViewModel } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { ui } from '../../state/ui.svelte';
  import RecCard from './RecCard.svelte';
  import { getTune, presetPool } from './state.svelte';
  import { attempt, defaultAdapter, defaultSystemLabel, QUIET, slugify, uniqueId } from './util';

  interface Props {
    vm: ViewModel;
  }
  let { vm }: Props = $props();
  const t = getTune();

  const KINDS: { value: SystemKind; label: string }[] = [
    { value: 'llm', label: 'LLM' },
    { value: 'image', label: 'Image' },
    { value: 'tts', label: 'Speech (TTS)' },
    { value: 'stt', label: 'Transcription (STT)' },
    { value: 'video', label: 'Video' },
  ];
  const CLASSES: { value: LlmClass | 'other'; label: string; sub: string }[] = [
    { value: 'fast', label: 'Fast', sub: 'System 1' },
    { value: 'deep', label: 'Deep', sub: 'System 2' },
    { value: 'max', label: 'Max', sub: 'System 3' },
    { value: 'other', label: 'Other', sub: 'System N' },
  ];
  type Start = 'recommended' | 'existing' | 'blank';

  let kind = $state<SystemKind>('llm');
  let cls = $state<LlmClass | 'other'>('fast');
  let where = $state('');
  let label = $state('');
  let labelTouched = $state(false);
  let startPick = $state<Start | null>(null);
  let recPick = $state('');
  let presetPick = $state('');
  let err = $state('');
  let busy = $state(false);

  const llmClass = $derived<LlmClass | undefined>(kind === 'llm' && cls !== 'other' ? cls : undefined);
  const editNodes = $derived(vm.nodes.filter((n) => n.state === 'online' && n.allow.includes('edit')));
  const remote = $derived(where !== '');
  const prefill = $derived(
    defaultSystemLabel(
      kind,
      llmClass,
      vm.systems.filter((s) => (s.node ?? '') === where).map((s) => s.label),
    ),
  );
  const shownLabel = $derived(labelTouched ? label : prefill);
  const recs = $derived<RecommendationInfo[]>(
    remote
      ? []
      : [
          ...vm.recommendations.filter((r) => r.kind === kind && (!llmClass || r.class === llmClass)),
          ...vm.recommendations.filter((r) => r.kind === kind && llmClass && r.class !== llmClass),
        ],
  );
  const presets = $derived<PresetInfo[]>(presetPool(vm, where || undefined).filter((p) => p.kind === kind));
  const start = $derived<Start>(
    startPick && (startPick !== 'recommended' || recs.length) && (startPick !== 'existing' || presets.length)
      ? startPick
      : recs.length
        ? 'recommended'
        : presets.length
          ? 'existing'
          : 'blank',
  );
  const recId = $derived(recs.some((r) => r.id === recPick) ? recPick : (recs[0]?.id ?? ''));
  const presetId = $derived(presets.some((p) => p.id === presetPick) ? presetPick : (presets[0]?.id ?? ''));
  const pickedRec = $derived(recs.find((r) => r.id === recId));

  function setKind(k: SystemKind) {
    kind = k;
    err = '';
  }

  /** The new System's id: the one that was not there before (it may take a snapshot or two to arrive). */
  async function waitForNew(before: Set<string>, node: string | undefined, k: SystemKind): Promise<SystemId | null> {
    const deadline = performance.now() + 5000;
    while (performance.now() < deadline) {
      const found = player.vm.systems.find((s) => !before.has(s.id) && (s.node ?? '') === (node ?? '') && s.kind === k);
      if (found) return found.id;
      await new Promise((r) => setTimeout(r, 100));
    }
    return null;
  }

  async function add() {
    if (busy) return;
    // Everything the steps need, as chosen now (the form stays live while the steps run).
    const node = where || undefined;
    const k = kind;
    const from = start;
    const rec = recId;
    const typed = label.trim();
    const fallbackLabel = shownLabel;
    const before = new Set(player.vm.systems.map((s) => s.id));
    busy = true;
    err = await attempt(() =>
      player.actions.addSystem({
        kind: k,
        class: llmClass,
        label: labelTouched && typed && typed !== prefill ? typed : undefined,
        preset: from === 'existing' && presetId ? presetId : undefined,
        node,
      }, QUIET),
    );
    if (err) {
      busy = false;
      return;
    }
    const id = await waitForNew(before, node, k);
    if (!id) {
      busy = false;
      err = 'The System was added but has not shown up yet. Pick its tab when it appears.';
      return;
    }
    if (from === 'recommended' && rec) {
      const e = await attempt(() => player.actions.adoptRecommendation(rec, id, QUIET));
      if (e) ui.toast(`Added, but the recommendation was not applied: ${e}`, 6000);
    }
    if (from === 'blank') {
      const sys = player.vm.systems.find((s) => s.id === id);
      const adapter = defaultAdapter(k);
      const key = t.createNew(
        node,
        uniqueId(slugify(sys?.label ?? fallbackLabel), presetPool(player.vm, node).map((p) => p.id)),
        { adapter, kind: adapter === 'generic' ? k : undefined },
        { selectFor: id },
      );
      t.viewed[id] = key;
      t.commandOpen = true;
    }
    busy = false;
    ui.tuneSystem = id;
    ui.tuneAdd = false;
  }

  const canAdd = $derived(
    !busy && shownLabel.trim() !== '' && (start !== 'recommended' || !!recId) && (start !== 'existing' || !!presetId),
  );
  const addHint = $derived(
    start === 'recommended' && pickedRec && !pickedRec.installed && !pickedRec.existing
      ? 'The model is not downloaded yet: download it on its card, here or later in the new tab.'
      : start === 'blank'
        ? 'The new tab opens with the Command editor and an empty preset.'
        : '',
  );
</script>

<div class="scroll add">
  <div class="block">
    <span class="blabel" id="tune-add-kind">Kind</span>
    <div class="seg3 wrap" role="radiogroup" aria-labelledby="tune-add-kind">
      {#each KINDS as k (k.value)}
        <button type="button" role="radio" aria-checked={kind === k.value} class:on={kind === k.value} onclick={() => setKind(k.value)}>{k.label}</button>
      {/each}
    </div>
    {#if kind === 'llm'}
      <div class="seg3 two" role="radiogroup" aria-label="LLM class">
        {#each CLASSES as c (c.value)}
          <button type="button" role="radio" aria-checked={cls === c.value} class:on={cls === c.value} onclick={() => (cls = c.value)}>
            <span>{c.label}</span><small>{c.sub}</small>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <label class="field">
    <span>Label</span>
    <input
      value={shownLabel}
      maxlength="64"
      autocomplete="off"
      oninput={(e) => {
        label = e.currentTarget.value;
        labelTouched = true;
      }}
    />
  </label>

  {#if editNodes.length}
    <label class="field">
      <span>Machine</span>
      <select value={where} onchange={(e) => (where = e.currentTarget.value)}>
        <option value="">This machine</option>
        {#each editNodes as n (n.id)}<option value={n.id}>{n.name} ({n.address})</option>{/each}
      </select>
    </label>
  {/if}

  <div class="block">
    <span class="blabel" id="tune-add-start">Start from</span>
    <div class="seg3" role="radiogroup" aria-labelledby="tune-add-start">
      <button type="button" role="radio" aria-checked={start === 'recommended'} class:on={start === 'recommended'} disabled={!recs.length} title={remote ? 'Recommendations are offered for this machine only.' : recs.length ? '' : 'No recommendations for this kind yet.'} onclick={() => (startPick = 'recommended')}>Recommended</button>
      <button type="button" role="radio" aria-checked={start === 'existing'} class:on={start === 'existing'} disabled={!presets.length} title={presets.length ? '' : `No ${KIND_LABEL[kind]} presets yet.`} onclick={() => (startPick = 'existing')}>Existing preset</button>
      <button type="button" role="radio" aria-checked={start === 'blank'} class:on={start === 'blank'} onclick={() => (startPick = 'blank')}>Blank</button>
    </div>
  </div>

  {#if start === 'recommended'}
    <p class="hint">
      Starting points measured on one machine. Models, drivers and backends change: let your agent benchmark and calibrate. Each
      model keeps its own license. KLIF downloads only from huggingface.co, and only when you ask.
    </p>
    {#if !vm.config.modelsDir}<p class="hint warn">Downloads need <code>[paths] models_dir</code> in klif.toml.</p>{/if}
    <div class="recs" role="radiogroup" aria-label="Recommendation">
      {#each recs as r (r.id)}
        <RecCard {vm} rec={r} selected={r.id === recId}>
          {#snippet action()}
            <label class="pick">
              <input type="radio" name="tune-add-rec" checked={r.id === recId} onchange={() => (recPick = r.id)} />
              {r.class && kind === 'llm' && r.class !== llmClass ? `Use this (${r.class})` : 'Use this'}
            </label>
          {/snippet}
        </RecCard>
      {/each}
    </div>
  {:else if start === 'existing'}
    <label class="field">
      <span>Preset</span>
      <select value={presetId} onchange={(e) => (presetPick = e.currentTarget.value)}>
        {#each presets as p (p.id)}
          <option value={p.id}>{joinParts([p.name, p.model.quant, p.model.backend])}{p.availability !== 'ready' ? '  —  invalid' : ''}</option>
        {/each}
      </select>
    </label>
  {:else}
    <p class="hint">An empty {KIND_LABEL[kind]} preset: you set the program, its arguments and environment yourself.</p>
  {/if}
</div>

<footer class="addfoot">
  {#if addHint}<p>{addHint}</p>{/if}
  {#if err}<p class="err" role="alert">{err}</p>{/if}
  <button type="button" class="primary" disabled={!canAdd} onclick={() => void add()}>
    {busy ? 'Adding…' : `Add ${shownLabel.trim() || 'System'}`}
  </button>
</footer>

<style>
  .block {
    display: grid;
    gap: 8px;
  }
  .blabel {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.04em;
  }
  .seg3.wrap {
    flex-wrap: wrap;
  }
  .seg3.wrap button {
    flex: 1 1 auto;
  }
  .seg3.two button {
    display: grid;
    gap: 4px;
    justify-items: center;
    padding: 8px 6px;
  }
  .seg3.two small {
    font: 500 10px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.06em;
    opacity: 0.75;
  }
  .recs {
    display: grid;
    gap: 8px;
  }
  .pick {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: 500 12px/1 var(--k-font-ui, system-ui, sans-serif);
    cursor: pointer;
  }
  .hint.warn {
    color: var(--k-warn, #f2a33a);
  }
  code {
    font: 500 12px/1.2 var(--k-font-data, ui-monospace, monospace);
  }
  .addfoot {
    padding: 12px 18px 16px;
    border-top: 1px solid var(--k-line, #2e2e2e);
    display: grid;
    gap: 10px;
    flex: none;
  }
  .addfoot p {
    margin: 0;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  .addfoot p.err {
    color: var(--k-danger, #e05a5a);
  }
</style>
