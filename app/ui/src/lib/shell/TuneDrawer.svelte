<script lang="ts">
  // Side sheet to tune what a slot launches. Data-driven: it shows slot.recipe with the choices in
  // slot.options (both from the core's catalog) and sends every change as actions.setRecipe(slot, patch).
  // A running session keeps the recipe it started with until it is restarted. Themed by the --k-* variables.
  import { fly, fade } from 'svelte/transition';
  import { fmtCtx } from '../model/format';
  import type { Availability, Recipe, RecipeChoice, Slot, SlotId } from '../model/types';
  import { player } from '../state/player.svelte';
  import { ui } from '../state/ui.svelte';
  import { fitLayers } from './fit';

  const slot = $derived<Slot>(player.vm.slots.find((s) => s.id === ui.tuneSlot) ?? player.vm.slots[0]);
  const m = $derived(slot.model);
  const recipe = $derived(slot.recipe);
  const opts = $derived(slot.options);
  const isLlm = $derived(slot.kind === 'llm');
  const session = $derived(player.vm.session);
  const running = $derived(session !== null && session.phase !== 'fault');
  const runningThis = $derived(running && session?.slot === slot.id);
  const otherRunning = $derived(running && !runningThis);
  const ready = $derived(slot.availability === 'ready');

  const AVAIL_TEXT: Record<Availability, string> = {
    ready: 'ready',
    unsupported: 'not available',
    'script-missing': 'starter missing',
    'model-missing': 'model missing',
    'build-required': 'build required',
    busy: 'port busy',
  };

  function set(patch: Partial<Recipe>) {
    player.actions.setRecipe(slot.id, patch);
  }

  /** Option text with the reason it cannot launch, when it cannot. */
  function choiceText<T>(c: RecipeChoice<T>): string {
    return c.availability === 'ready' ? c.label : `${c.label} (${AVAIL_TEXT[c.availability]})`;
  }

  function cacheText(mib: number): string {
    return mib === 0 ? 'off' : `${mib >= 1024 ? `${mib / 1024} GiB` : `${mib} MiB`} in RAM`;
  }

  // ---- "will it fit" preview: baseline (already in use) + the slot's expected layers, after the loader's trim ----
  const total = $derived(player.vm.vram.totalGiB);
  const baseline = $derived(player.vm.vram.baselineGiB);
  const fitted = $derived(fitLayers(slot.expectedVram ?? [], baseline, total));
  const layers = $derived(baseline > 0 ? [{ id: 'other' as const, label: 'other (in use)', gib: baseline }, ...fitted.layers] : fitted.layers);
  const need = $derived(layers.reduce((a, l) => a + l.gib, 0));
  const free = $derived(total - need);
  const warn = $derived(player.vm.vram.warnBelowGiB);
  const fit = $derived(fitted.overGiB > 0 ? 'over' : free < warn ? 'tight' : 'ok');
  const trimNote = $derived(fitted.trimmedGiB > 0 ? ` The loader trims the compute buffers by ${fitted.trimmedGiB.toFixed(2)} GiB to fit.` : '');
  const fitText = $derived(
    fit === 'over'
      ? `Over by ${fitted.overGiB.toFixed(2)} GiB: the overflow spills to shared system memory.`
      : fit === 'tight'
        ? `Fits with ${free.toFixed(2)} GiB to spare. Very little headroom.${trimNote}`
        : `Fits with ${free.toFixed(2)} GiB to spare.`,
  );
  const shades = [100, 74, 52, 34, 22];

  function close() {
    ui.closeTune();
  }

  const slotsList = $derived(player.vm.slots);
  function pickSlot(id: SlotId) {
    ui.tuneSlot = id;
  }

  // The current values may be missing from the offered lists (e.g. a capped cache): show them anyway.
  // No list offered = not tunable (e.g. an image starter that pins its own port).
  function withCurrent(v: number[] | undefined, cur: number | undefined): number[] {
    if (!v?.length) return [];
    return cur !== undefined && !v.includes(cur) ? [...v, cur].sort((a, b) => a - b) : v;
  }
  const cacheValues = $derived(withCurrent(opts?.promptCacheMiB, recipe?.promptCacheMiB));
  const portValues = $derived(withCurrent(opts?.ports, recipe?.port));

  // Krea on the fast starter: Precision (default low) with the hint of the selected level.
  const precision = $derived(recipe?.precision ?? 'low');
  // The levels pick the edit LoRA and the reference size, so they only matter with Edit on.
  const precisionHint = $derived.by(() => {
    const hint = opts?.precisions?.find((p) => p.value === precision)?.hint ?? '';
    return hint && opts?.editToggle && !recipe?.edit ? `${hint} · used when Edit is on` : hint;
  });
</script>

<div class="scrim" role="presentation" onclick={close} transition:fade|global={{ duration: 160 }}></div>
<div class="sheet" role="dialog" aria-label="Tune {slot.label}" transition:fly|global={{ x: 400, duration: 220, opacity: 1 }}>
  <header>
    <div class="titles">
      <h2>Tune</h2>
      <div class="slotname">{slot.label}</div>
      <div class="slotmodel">{m.name}{m.quant ? ` · ${m.quant}` : ''}</div>
    </div>
    <button type="button" class="ghost" aria-label="Close tune drawer" onclick={close}>Close</button>
  </header>

  <div class="tabs" role="tablist" aria-label="Slot">
    {#each slotsList as s (s.id)}
      <button type="button" role="tab" aria-selected={s.id === slot.id} class:on={s.id === slot.id} onclick={() => pickSlot(s.id)}>
        {s.label.replace('AGENT ', '')}
      </button>
    {/each}
  </div>

  <div class="scroll">
    <section class="fit" data-fit={fit}>
      <div class="fitbar" role="img" aria-label="Expected VRAM use">
        {#each layers as l, i (l.id + i)}
          <span class="seg" style="flex: {l.gib} 0 0; --shade: {shades[i % shades.length]}%" title="{l.label} {l.gib.toFixed(2)} GiB"></span>
        {/each}
        <span class="seg free" style="flex: {Math.max(0, free)} 0 0"></span>
      </div>
      <ul class="legend">
        {#each layers as l, i (l.id + i)}
          <li><i style="--shade: {shades[i % shades.length]}%"></i>{l.label}<b>{l.gib.toFixed(2)}</b></li>
        {/each}
        <li class="sum">of {total.toFixed(2)} GiB<b>{need.toFixed(2)}</b></li>
      </ul>
      <p class="fitnote">{fitText}</p>
    </section>

    {#if !recipe || !opts}
      <p class="note">The core offers no launch settings for this slot.</p>
    {:else}
      <label class="field">
        <span>Model</span>
        <select value={recipe.cardId} onchange={(e) => set({ cardId: e.currentTarget.value })}>
          {#each opts.cards as c (c.value)}
            <option value={c.value} title={c.reason ?? ''}>{choiceText(c)}</option>
          {/each}
        </select>
      </label>

      <div class="field">
        <span id="lbl-backend">Backend</span>
        <div class="seg3" role="radiogroup" aria-labelledby="lbl-backend">
          {#each opts.backends as b (b.value)}
            <button
              type="button"
              role="radio"
              aria-checked={recipe.backend === b.value}
              class:on={recipe.backend === b.value}
              class:na={b.availability !== 'ready'}
              title={b.reason ?? (b.availability === 'ready' ? '' : AVAIL_TEXT[b.availability])}
              onclick={() => set({ backend: b.value })}>{b.label}</button
            >
          {/each}
        </div>
      </div>

      <label class="field">
        <span>Hardware</span>
        <select value={recipe.hardware} onchange={(e) => set({ hardware: e.currentTarget.value })}>
          {#each opts.hardware as h (h.value)}
            <option value={h.value} title={h.reason ?? ''}>{choiceText(h)}</option>
          {/each}
        </select>
      </label>

      {#if isLlm}
        {#if opts.contexts?.length}
          <label class="field">
            <span>Context</span>
            <select value={recipe.ctxTokens} onchange={(e) => set({ ctxTokens: Number(e.currentTarget.value) })}>
              {#each opts.contexts as c (c.value)}
                <option value={c.value}>{choiceText(c)}</option>
              {/each}
              {#if recipe.ctxTokens !== undefined && !opts.contexts.some((c) => c.value === recipe.ctxTokens)}
                <option value={recipe.ctxTokens}>{fmtCtx(recipe.ctxTokens)} tokens</option>
              {/if}
            </select>
          </label>
        {/if}

        {#if opts.kvTypes?.length}
          <div class="field">
            <span id="lbl-kv">KV cache</span>
            <div class="seg3" role="radiogroup" aria-labelledby="lbl-kv">
              {#each opts.kvTypes as k (k)}
                <button type="button" role="radio" aria-checked={recipe.kvType === k} class:on={recipe.kvType === k} onclick={() => set({ kvType: k })}>{k}</button>
              {/each}
            </div>
          </div>
        {/if}

        {#if opts.modes?.length}
          <label class="field">
            <span>Mode</span>
            <select value={recipe.mode ?? opts.modes[0]} onchange={(e) => set({ mode: e.currentTarget.value })}>
              {#each opts.modes as md (md)}
                <option value={md}>{md}</option>
              {/each}
            </select>
          </label>
        {/if}

        {#if opts.vision}
          <div class="field">
            <span id="lbl-vision">Vision</span>
            <button
              type="button"
              class="switch"
              role="switch"
              aria-checked={!!recipe.vision}
              aria-labelledby="lbl-vision"
              class:on={!!recipe.vision}
              onclick={() => set({ vision: !recipe.vision })}
            >
              <i></i>{recipe.vision ? 'projector loaded' : 'off'}
            </button>
          </div>
        {/if}

        {#if cacheValues.length}
          <label class="field">
            <span>Prompt cache</span>
            <select value={recipe.promptCacheMiB} onchange={(e) => set({ promptCacheMiB: Number(e.currentTarget.value) })}>
              {#each cacheValues as c (c)}
                <option value={c}>{cacheText(c)}</option>
              {/each}
            </select>
          </label>
        {/if}

        {#if m.specMode}
          <div class="field">
            <span>Speculative</span>
            <span class="ro">{m.specMode} <small>(set by the starter)</small></span>
          </div>
        {/if}
      {:else}
        {#if opts.imageSizes?.length}
          <label class="field">
            <span>Image size</span>
            <select value={recipe.imageSize} onchange={(e) => set({ imageSize: e.currentTarget.value })}>
              {#each opts.imageSizes as s (s.value)}
                <option value={s.value}>{choiceText(s)}</option>
              {/each}
            </select>
          </label>
        {/if}

        {#if opts.editToggle}
          <div class="field">
            <span id="lbl-edit">Edit</span>
            <button
              type="button"
              class="switch"
              role="switch"
              aria-checked={!!recipe.edit}
              aria-labelledby="lbl-edit"
              class:on={!!recipe.edit}
              onclick={() => set({ edit: !recipe.edit })}
            >
              <i></i>{recipe.edit ? 'identity edit · LoRA on' : 'off · plain generation'}
            </button>
          </div>
        {/if}

        {#if opts.precisions?.length}
          <div class="field">
            <span id="lbl-precision">Precision</span>
            <div class="stack">
              <div class="seg3" role="radiogroup" aria-labelledby="lbl-precision">
                {#each opts.precisions as p (p.value)}
                  <button
                    type="button"
                    role="radio"
                    aria-checked={precision === p.value}
                    class:on={precision === p.value}
                    class:na={p.availability !== 'ready'}
                    title={p.hint}
                    onclick={() => set({ precision: p.value })}>{p.label}</button
                  >
                {/each}
              </div>
              {#if precisionHint}<small class="hint">{precisionHint}</small>{/if}
            </div>
          </div>
        {/if}
      {/if}

      {#if portValues.length}
        <label class="field">
          <span>Port</span>
          <select value={recipe.port} onchange={(e) => set({ port: Number(e.currentTarget.value) })}>
            {#each portValues as p (p)}
              <option value={p}>{p}</option>
            {/each}
          </select>
        </label>
      {:else if recipe.port !== undefined}
        <div class="field">
          <span>Port</span>
          <span class="ro">{recipe.port} <small>(fixed by the starter)</small></span>
        </div>
      {/if}
    {/if}
  </div>

  <footer>
    {#if runningThis}
      <p>This session keeps its current settings until it restarts.</p>
      <button type="button" class="primary" disabled={!ready} onclick={() => player.actions.restart()}>Restart to apply</button>
    {:else if otherRunning}
      <p>Another slot is running. Stop it to launch this one.</p>
      <button type="button" class="primary" disabled>Launch</button>
    {:else if !ready}
      <p class="blocked">Cannot launch: {slot.reason ?? AVAIL_TEXT[slot.availability]}</p>
      <button type="button" class="primary" disabled>Launch {slot.label.toLowerCase()}</button>
    {:else}
      <p>Applies on the next launch.</p>
      <button type="button" class="primary" onclick={() => player.actions.launch(slot.id)}>Launch {slot.label.toLowerCase()}</button>
    {/if}
  </footer>
</div>

<style>
  .scrim {
    position: absolute;
    inset: 0;
    z-index: 55;
    background: color-mix(in srgb, var(--k-bg, #0d1117) 55%, transparent);
  }
  .sheet {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(92%, 390px);
    z-index: 56;
    display: flex;
    flex-direction: column;
    background: var(--k-surface-raised, #1a1a1a);
    color: var(--k-ink, #e6e6e6);
    border-left: 1px solid var(--k-line, #2e2e2e);
    box-shadow: -14px 0 40px rgba(0, 0, 0, 0.4);
    font-family: var(--k-font-ui, system-ui, sans-serif);
    font-variant-numeric: tabular-nums;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 18px 10px;
  }
  h2 {
    margin: 0 0 4px;
    font: 600 11px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .slotname {
    font: 600 20px/1.1 var(--k-font-display, var(--k-font-ui, system-ui));
    letter-spacing: 0.04em;
  }
  .slotmodel {
    margin-top: 6px;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-ui, system-ui, sans-serif);
  }
  .tabs {
    display: flex;
    gap: 6px;
    padding: 0 18px 10px;
    border-bottom: 1px solid var(--k-line, #2e2e2e);
  }
  .tabs button {
    flex: 1;
    padding: 7px 4px;
    font: 600 11px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.1em;
  }
  .scroll {
    flex: 1;
    overflow: auto;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    overscroll-behavior: contain;
  }
  .field {
    display: grid;
    grid-template-columns: 112px 1fr;
    align-items: center;
    gap: 12px;
  }
  .field > span {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.04em;
  }
  .field > span.ro {
    color: var(--k-ink, #e6e6e6);
    font: 500 13px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0;
  }
  .ro small {
    color: var(--k-muted, #8a8a8a);
    font: 500 11px/1.2 var(--k-font-ui, system-ui, sans-serif);
  }
  select {
    width: 100%;
    min-width: 0;
    font: 500 13px/1.2 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
    background: var(--k-surface, #141414);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 8px 10px;
  }
  select:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--k-accent, #5ab6eb);
    outline-offset: 1px;
  }
  button {
    font: 500 12px/1 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
    background: transparent;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 8px 12px;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    border-color: var(--k-accent, #5ab6eb);
  }
  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .tabs button.on,
  .seg3 button.on {
    background: var(--k-accent, #5ab6eb);
    border-color: var(--k-accent, #5ab6eb);
    color: var(--k-accent-ink, #0d1117);
  }
  .seg3 {
    display: flex;
    gap: 6px;
  }
  .seg3 button {
    flex: 1;
    font-family: var(--k-font-ui, system-ui, sans-serif);
  }
  .seg3 button.na:not(.on) {
    border-style: dashed;
    color: var(--k-muted, #8a8a8a);
  }
  .seg3 button.na.on {
    background: color-mix(in srgb, var(--k-warn, #f2a33a) 70%, var(--k-surface, #141414));
    border-color: var(--k-warn, #f2a33a);
  }
  .stack {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .stack .hint {
    color: var(--k-muted, #8a8a8a);
    font: 500 11px/1.3 var(--k-font-ui, system-ui, sans-serif);
  }
  .switch {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    width: fit-content;
  }
  .switch i {
    width: 30px;
    height: 16px;
    border-radius: 999px;
    border: 1px solid var(--k-line, #2e2e2e);
    background: var(--k-surface, #141414);
    position: relative;
  }
  .switch i::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--k-muted, #8a8a8a);
    transform: translateX(0);
    transition: transform 140ms;
  }
  .switch.on i {
    border-color: var(--k-accent, #5ab6eb);
  }
  .switch.on i::after {
    background: var(--k-accent, #5ab6eb);
    transform: translateX(14px);
  }

  .fit {
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 12px;
    background: var(--k-surface, #141414);
  }
  .fitbar {
    display: flex;
    height: 14px;
    border-radius: 3px;
    overflow: hidden;
    gap: 2px;
  }
  .seg {
    background: color-mix(in srgb, var(--k-accent, #5ab6eb) var(--shade, 100%), var(--k-surface, #141414));
    min-width: 0;
  }
  .seg.free {
    background: transparent;
    outline: 1px dashed var(--k-line, #2e2e2e);
    outline-offset: -1px;
  }
  [data-fit='over'] .seg:not(.free) {
    background: color-mix(in srgb, var(--k-danger, #e05a5a) var(--shade, 100%), var(--k-surface, #141414));
  }
  [data-fit='tight'] .fitnote {
    color: var(--k-warn, #f2a33a);
  }
  [data-fit='over'] .fitnote {
    color: var(--k-danger, #e05a5a);
  }
  .legend {
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
    display: grid;
    gap: 4px;
    font: 500 12px/1.3 var(--k-font-ui, system-ui, sans-serif);
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--k-muted, #8a8a8a);
  }
  .legend li b {
    margin-left: auto;
    font: 500 12px/1 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
  }
  .legend i {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--k-accent, #5ab6eb) var(--shade, 100%), var(--k-surface, #141414));
  }
  .legend .sum {
    border-top: 1px solid var(--k-line, #2e2e2e);
    padding-top: 6px;
    margin-top: 2px;
  }
  .fitnote,
  .note {
    margin: 10px 0 0;
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  footer {
    padding: 12px 18px 16px;
    border-top: 1px solid var(--k-line, #2e2e2e);
    display: grid;
    gap: 10px;
  }
  footer p {
    margin: 0;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  footer p.blocked {
    color: var(--k-warn, #f2a33a);
  }
  .primary {
    background: var(--k-accent, #5ab6eb);
    color: var(--k-accent-ink, #0d1117);
    border-color: transparent;
    font-weight: 600;
    padding: 11px 14px;
    font-size: 13px;
  }
  .primary:hover:not(:disabled) {
    filter: brightness(1.08);
    border-color: transparent;
  }
</style>
