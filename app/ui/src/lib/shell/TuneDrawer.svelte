<script lang="ts">
  // Side sheet to tune the selected slot: model, quant, backend, context, KV type, prompt cache, port,
  // vision, mode. Changes update the mock slot's ModelRef immediately; a running session keeps the
  // settings it started with until it is restarted. Themed purely by the --k-* variables.
  import { fly, fade } from 'svelte/transition';
  import { BACKENDS, catalogByKind, catalogFor, fitLayers, modelFromCatalog, PROMPT_CACHE_MIB } from '../mock/catalog';
  import { fmtCtx } from '../model/format';
  import type { ModelRef, Slot, SlotId } from '../model/types';
  import { player } from '../state/player.svelte';
  import { ui } from '../state/ui.svelte';

  const slot = $derived<Slot>(player.vm.slots.find((s) => s.id === ui.tuneSlot) ?? player.vm.slots[0]);
  const m = $derived(slot.model);
  const cat = $derived(catalogFor(m.name));
  const isLlm = $derived(slot.kind === 'llm');
  const extras = $derived(player.extras[slot.id]);
  const session = $derived(player.vm.session);
  const running = $derived(session !== null && session.phase !== 'fault');
  const runningThis = $derived(running && session?.slot === slot.id);

  function patch(p: Partial<ModelRef>) {
    const next: ModelRef = { ...m, ...p };
    // Quant drives the on-disk weights size.
    if (p.quant !== undefined) {
      const q = cat?.quants.find((x) => x.quant === p.quant);
      if (q) next.weightsGiB = q.weightsGiB;
    }
    player.applyModel(slot.id, next);
  }

  function pickModel(name: string) {
    const c = catalogFor(name);
    if (c) player.applyModel(slot.id, modelFromCatalog(c, m));
  }

  function pickSpec(v: string) {
    patch({ specMode: v === 'off' ? undefined : v });
  }

  function pickMode(v: string) {
    patch({ mode: v === 'auto' ? undefined : v });
  }

  function setPort(raw: string) {
    const n = Math.round(Number(raw));
    if (Number.isFinite(n)) player.setExtras(slot.id, { port: Math.min(65535, Math.max(1024, n)) });
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

  const otherRunning = $derived(running && !runningThis);
  const slotsList = $derived(player.vm.slots);
  function pickSlot(id: SlotId) {
    ui.tuneSlot = id;
  }
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

    <label class="field">
      <span>Model</span>
      <select value={m.name} onchange={(e) => pickModel(e.currentTarget.value)}>
        {#each catalogByKind(slot.kind) as c (c.name)}
          <option value={c.name}>{c.name}</option>
        {/each}
      </select>
    </label>

    <label class="field">
      <span>Quant</span>
      <select value={m.quant} onchange={(e) => patch({ quant: e.currentTarget.value })}>
        {#each cat?.quants ?? [] as q (q.quant)}
          <option value={q.quant}>{q.quant} · {q.weightsGiB.toFixed(1)} GiB</option>
        {/each}
      </select>
    </label>

    <div class="field">
      <span id="lbl-backend">Backend</span>
      <div class="seg3" role="radiogroup" aria-labelledby="lbl-backend">
        {#each BACKENDS as b (b)}
          <button type="button" role="radio" aria-checked={m.backend === b} class:on={m.backend === b} onclick={() => patch({ backend: b })}>{b}</button>
        {/each}
      </div>
    </div>

    {#if isLlm}
      <label class="field">
        <span>Context</span>
        <select value={m.ctxTokens} onchange={(e) => patch({ ctxTokens: Number(e.currentTarget.value) })}>
          {#each cat?.ctxOptions ?? [] as c (c)}
            <option value={c}>{fmtCtx(c)} tokens</option>
          {/each}
        </select>
      </label>

      <div class="field">
        <span id="lbl-kv">KV cache</span>
        <div class="seg3" role="radiogroup" aria-labelledby="lbl-kv">
          {#each ['q4_0', 'q8_0'] as k (k)}
            <button type="button" role="radio" aria-checked={m.kvType === k} class:on={m.kvType === k} onclick={() => patch({ kvType: k })}>{k}</button>
          {/each}
        </div>
      </div>

      <label class="field">
        <span>Speculative</span>
        <select value={m.specMode ?? 'off'} onchange={(e) => pickSpec(e.currentTarget.value)}>
          {#each cat?.specModes ?? ['off'] as s (s)}
            <option value={s}>{s}</option>
          {/each}
        </select>
      </label>

      <label class="field">
        <span>Mode</span>
        <select value={m.mode ?? 'auto'} onchange={(e) => pickMode(e.currentTarget.value)}>
          <option value="auto">auto</option>
          {#each cat?.modes ?? [] as md (md)}
            <option value={md}>{md}</option>
          {/each}
        </select>
      </label>

      {#if cat?.hasVision}
        <div class="field">
          <span id="lbl-vision">Vision</span>
          <button
            type="button"
            class="switch"
            role="switch"
            aria-checked={!!m.vision}
            aria-labelledby="lbl-vision"
            class:on={!!m.vision}
            onclick={() => patch({ vision: !m.vision })}
          >
            <i></i>{m.vision ? 'projector loaded' : 'off'}
          </button>
        </div>
      {/if}

      <label class="field">
        <span>Prompt cache</span>
        <select value={extras.promptCacheMiB} onchange={(e) => player.setExtras(slot.id, { promptCacheMiB: Number(e.currentTarget.value) })}>
          {#each PROMPT_CACHE_MIB as c (c)}
            <option value={c}>{c === 0 ? 'off' : `${c / 1024} GiB in RAM`}</option>
          {/each}
        </select>
      </label>
    {:else}
      <label class="field">
        <span>Image size</span>
        <select value={m.imageSize} onchange={(e) => patch({ imageSize: e.currentTarget.value })}>
          {#each cat?.imageSizes ?? [] as s (s)}
            <option value={s}>{s}</option>
          {/each}
        </select>
      </label>
    {/if}

    <label class="field">
      <span>Port</span>
      <input type="number" inputmode="numeric" min="1024" max="65535" value={extras.port} onchange={(e) => setPort(e.currentTarget.value)} />
    </label>
  </div>

  <footer>
    {#if runningThis}
      <p>This session keeps its current settings until it restarts.</p>
      <button type="button" class="primary" onclick={() => player.actions.restart()}>Restart to apply</button>
    {:else if otherRunning}
      <p>Another slot is running. Stop it to launch this one.</p>
      <button type="button" class="primary" disabled>Launch</button>
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
    font: 500 12px/1.2 var(--k-font-data, ui-monospace, monospace);
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
  select,
  input[type='number'] {
    width: 100%;
    min-width: 0;
    font: 500 13px/1.2 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
    background: var(--k-surface, #141414);
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 8px 10px;
  }
  input[type='number'] {
    font-family: var(--k-font-data, ui-monospace, monospace);
  }
  select:focus-visible,
  input:focus-visible,
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
    font-family: var(--k-font-data, ui-monospace, monospace);
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
    font: 500 12px/1 var(--k-font-data, ui-monospace, monospace);
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
  .fitnote {
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
