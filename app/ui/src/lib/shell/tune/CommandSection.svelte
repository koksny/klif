<script lang="ts">
  // The Command editor of the preset the tab shows. Header: the exact command line + Copy. Body: adapter, kind
  // (generic; image | video for sd.cpp), program, working dir, external endpoint, args, env, the grid (port, host,
  // health, model, mmproj, ctx, GPU, managed, API key), a live preview (commandPreview, debounced 250 ms) with its
  // issues, and
  // Revert · Save as new… · Apply. The draft is local (TuneState), never bound to the 2 Hz view model.
  import { ctxProblem, portProblem, specNumberProblem } from '../../model/presets';
  import { KIND_LABEL } from '../../model/systems';
  import type { AdapterId, GpuMemory, PresetSpec, System, SystemKind, ViewModel } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import { ui } from '../../state/ui.svelte';
  import ArgsEditor from './ArgsEditor.svelte';
  import EnvEditor from './EnvEditor.svelte';
  import SaveAsForm from './SaveAsForm.svelte';
  import { maskClearSecrets } from './secret';
  import Section from './Section.svelte';
  import { getTune, presetPool } from './state.svelte';
  import { cleanSpec, copyText, errorText, labelsOf, listNames, plain, presetIdProblem, usersOf } from './util';

  interface Props {
    vm: ViewModel;
    system: System;
    viewedId: string;
  }
  let { vm, system, viewedId }: Props = $props();
  const t = getTune();

  const ADAPTERS: AdapterId[] = ['llama.cpp', 'sd.cpp', 'vllm', 'openai', 'audiocpp', 'generic'];
  const KINDS: SystemKind[] = ['llm', 'image', 'tts', 'stt', 'video', 'music'];

  /** A loaded draft spec always carries its lists (TuneState makes sure), so the editors can bind them. */
  type EditSpec = PresetSpec & { args: string[]; env: Record<string, string>; env_remove: string[] };

  const draft = $derived(viewedId ? t.draft(system.node, viewedId) : undefined);
  const ready = $derived(!!draft && !draft.loading && !draft.loadError);
  const spec = $derived<EditSpec | undefined>(ready ? (draft!.spec as EditSpec) : undefined);
  const editable = $derived(system.editable);
  const remote = $derived(!!system.node);
  const nodeDown = $derived(remote && vm.nodes.find((n) => n.id === system.node)?.state !== 'online');
  const external = $derived(spec?.endpoint !== undefined);
  const gpus = $derived<GpuMemory[]>(remote ? (vm.nodes.find((n) => n.id === system.node)?.gpus ?? []) : vm.gpus);

  const shown = $derived(
    draft?.preview ?? draft?.base?.command ?? (viewedId && viewedId === system.preset ? system.command : undefined) ?? null,
  );
  const display = $derived(shown ? (shown.external ? `external · ${shown.external}` : shown.display) : '');

  // ---- preview: what the edited spec would run, 250 ms after the last change ----
  // (primitives via $derived: the System object itself is replaced at every 2 Hz snapshot)
  const sysId = $derived(system.id);
  let seq = 0;
  $effect(() => {
    const d = draft;
    if (!d || d.loading || d.loadError || !editable) return;
    // A remote draft's typed secrets never leave this machine: the node gets MASK in their place (SPEC 16.10).
    const clean = cleanSpec(plain(d.spec));
    const sent = remote ? maskClearSecrets(clean) : clean;
    const problem = specNumberProblem(clean);
    if (problem) {
      d.previewError = problem;
      return;
    }
    const sys = sysId;
    const mine = ++seq;
    const timer = setTimeout(() => {
      player.config.commandPreview(sent, sys).then(
        (cv) => {
          if (mine !== seq) return;
          d.preview = cv;
          d.previewError = '';
        },
        (e: unknown) => {
          if (mine !== seq) return;
          d.previewError = errorText(e);
        },
      );
    }, 250);
    return () => clearTimeout(timer);
  });

  // ---- field helpers ----
  function text(field: keyof PresetSpec, v: string) {
    if (!spec) return;
    (spec as unknown as Record<string, unknown>)[field] = v === '' ? undefined : v;
  }
  function num(field: 'port' | 'ctx', el: HTMLInputElement) {
    if (!spec) return;
    const n = el.valueAsNumber;
    spec[field] = Number.isFinite(n) ? Math.round(n) : undefined;
  }

  // External toggle keeps what it hides, so switching back restores it.
  let stash: { command?: string; args: string[]; cwd?: string } | null = null;
  function setExternal(on: boolean) {
    if (!spec) return;
    if (on) {
      stash = { command: spec.command, args: [...(spec.args ?? [])], cwd: spec.cwd };
      spec.command = undefined;
      spec.cwd = undefined;
      spec.args = [];
      spec.endpoint = '';
    } else {
      spec.endpoint = undefined;
      if (stash) {
        spec.command = stash.command;
        spec.cwd = stash.cwd;
        spec.args = stash.args;
      }
      stash = null;
    }
  }

  const healthMode = $derived(spec?.health === undefined ? 'auto' : spec.health.trim().toLowerCase() === 'tcp' ? 'tcp' : 'http');
  function setHealth(mode: string) {
    if (!spec) return;
    spec.health = mode === 'auto' ? undefined : mode === 'tcp' ? 'tcp' : spec.health && healthMode === 'http' ? spec.health : '/health';
  }

  const gpuKnown = $derived(!spec?.gpu || spec.gpu === 'cpu' || gpus.some((g) => g.id === spec.gpu));

  // ---- actions ----
  const dirty = $derived(t.isDirty(draft));
  const needsApply = $derived(t.needsApply(draft));
  const users = $derived(draft && !draft.isNew ? usersOf(vm, system.node, draft.id) : []);
  const shared = $derived(users.length > 1 || (users.length === 1 && users[0].id !== system.id));
  const idProblem = $derived(draft?.isNew ? presetIdProblem(draft.id.trim(), presetPool(vm, system.node).map((p) => p.id)) : '');
  // Out-of-range numbers never reach the core (it could not even read them): a sentence under the field instead.
  const portErr = $derived(spec ? portProblem(spec.port) : '');
  const ctxErr = $derived(spec ? ctxProblem(spec.ctx) : '');
  let saveAs = $state(false);

  async function revert() {
    const d = draft;
    if (!d) return;
    if (dirty) {
      const ok = await t.confirm({ title: 'Revert your edits?', detail: `${t.draftName(d)} goes back to what is stored.`, confirm: 'Revert', danger: true });
      if (!ok) return;
    }
    t.revert(d.key);
  }

  async function reload() {
    const d = draft;
    if (!d) return;
    if (dirty) {
      const ok = await t.confirm({
        title: 'Reload from klif.toml?',
        detail: 'The stored preset changed meanwhile. Reloading drops your edits.',
        confirm: 'Reload',
        danger: true,
      });
      if (!ok) return;
    }
    await t.load(d.key);
  }

  async function apply() {
    if (draft) await t.apply(vm, system, draft.key);
  }

  async function copy() {
    if (!display) return;
    ui.toast((await copyText(display)) ? 'Command copied' : 'Clipboard is not available here.');
  }

  const issues = $derived(draft?.preview?.issues ?? shown?.issues ?? []);
</script>

<Section id="tune-command" title="Command" open={t.commandOpen} ontoggle={(o) => (t.commandOpen = o)}>
  {#snippet summary()}{display || '—'}{/snippet}
  {#snippet actions()}
    <button type="button" class="mini text" disabled={!display} onclick={() => void copy()} title="Copy the command line (secrets stay masked)">Copy</button>
  {/snippet}

  {#if !draft}
    <p class="hint">No preset to show. Pick one above{editable ? ', or start a blank one from the preset ⋯ menu' : ''}.</p>
  {:else if draft.loading}
    <p class="hint">Loading the preset…</p>
  {:else if draft.loadError && nodeDown}
    <p class="hint">The node is not reachable, so its preset cannot be read now.</p>
    <div><button type="button" onclick={() => draft && void t.load(draft.key)}>Try again</button></div>
  {:else if draft.loadError}
    <p class="err" role="alert">{draft.loadError}</p>
    <div><button type="button" onclick={() => draft && void t.load(draft.key)}>Try again</button></div>
  {:else if spec}
    {#if !editable}
      <p class="hint">Read-only: {remote ? 'that node does not grant "edit"' : 'not in klif.toml'}.</p>
    {/if}

    {#if draft.isNew}
      <label class="field">
        <span>Preset id</span>
        <input value={draft.id} spellcheck="false" autocomplete="off" aria-invalid={!!idProblem} oninput={(e) => draft && (draft.id = e.currentTarget.value.trim())} />
      </label>
      {#if idProblem}<p class="err indent">{idProblem}</p>{/if}
      <label class="field">
        <span>Name</span>
        <input value={spec.name ?? ''} autocomplete="off" placeholder={draft.id} oninput={(e) => text('name', e.currentTarget.value)} />
      </label>
    {/if}

    <label class="field">
      <span>Adapter</span>
      <select value={spec.adapter ?? 'llama.cpp'} disabled={!editable} onchange={(e) => spec && (spec.adapter = e.currentTarget.value as AdapterId)}>
        {#each ADAPTERS as a (a)}<option value={a}>{a}</option>{/each}
      </select>
    </label>
    {#if (spec.adapter ?? 'llama.cpp') === 'generic'}
      <label class="field">
        <span>Kind</span>
        <select
          value={spec.kind ?? ''}
          disabled={!editable}
          onchange={(e) => spec && (spec.kind = (e.currentTarget.value || undefined) as SystemKind | undefined)}
        >
          <option value="">— required for generic —</option>
          {#each KINDS as k (k)}<option value={k}>{KIND_LABEL[k]}</option>{/each}
        </select>
      </label>
    {:else if spec.adapter === 'sd.cpp'}
      <!-- sd.cpp serves images, or video with a vid_gen model (MiniMax H3). -->
      <label class="field">
        <span>Kind</span>
        <select
          value={spec.kind === 'video' ? 'video' : ''}
          disabled={!editable}
          onchange={(e) => spec && (spec.kind = (e.currentTarget.value || undefined) as SystemKind | undefined)}
        >
          <option value="">{KIND_LABEL.image} (default)</option>
          <option value="video">{KIND_LABEL.video}</option>
        </select>
      </label>
    {/if}

    <div class="field">
      <span id="tune-ext-lbl">External</span>
      <button
        type="button"
        class="switch"
        role="switch"
        aria-labelledby="tune-ext-lbl"
        aria-checked={external}
        class:on={external}
        disabled={!editable}
        onclick={() => setExternal(!external)}
      >
        <i></i>{external ? 'a server KLIF only watches' : 'off · KLIF starts and stops it'}
      </button>
    </div>

    {#if external}
      <label class="field">
        <span>Endpoint</span>
        <input class="mono" value={spec.endpoint ?? ''} placeholder="http://192.0.2.20:11434" spellcheck="false" autocomplete="off" readonly={!editable} oninput={(e) => spec && (spec.endpoint = e.currentTarget.value)} />
      </label>
    {:else}
      <label class="field">
        <span>Program</span>
        <input class="mono" value={spec.command ?? ''} placeholder="D:\llama.cpp\llama-server.exe" spellcheck="false" autocomplete="off" readonly={!editable} oninput={(e) => text('command', e.currentTarget.value)} />
      </label>
      <label class="field">
        <span>Working dir</span>
        <input class="mono" value={spec.cwd ?? ''} placeholder="folder of the program" spellcheck="false" autocomplete="off" readonly={!editable} oninput={(e) => text('cwd', e.currentTarget.value)} />
      </label>
      <ArgsEditor bind:args={spec.args} readonly={!editable} />
      <EnvEditor bind:env={spec.env} bind:envRemove={spec.env_remove} preview={draft.preview ?? shown} readonly={!editable} {remote} />
    {/if}

    <div class="grid">
      {#if !external}
        <label class="cell">
          <span>Port</span>
          <input type="number" min="1" max="65535" value={spec.port ?? ''} placeholder={shown?.port ? String(shown.port) : 'adapter default'} readonly={!editable} aria-invalid={!!portErr} oninput={(e) => num('port', e.currentTarget)} />
          {#if portErr}<small class="err">{portErr}</small>{/if}
        </label>
        <label class="cell">
          <span>Host</span>
          <input class="mono" value={spec.host ?? ''} placeholder={shown?.host || '127.0.0.1'} spellcheck="false" readonly={!editable} oninput={(e) => text('host', e.currentTarget.value)} />
        </label>
      {/if}
      <div class="cell">
        <span id="tune-health-lbl">Health</span>
        <div class="pair">
          <select aria-labelledby="tune-health-lbl" value={healthMode} disabled={!editable} onchange={(e) => setHealth(e.currentTarget.value)}>
            <option value="auto">Auto</option>
            <option value="http">HTTP</option>
            <option value="tcp">TCP</option>
          </select>
          {#if healthMode === 'http'}
            <input class="mono" aria-label="Health path" value={spec.health ?? ''} spellcheck="false" readonly={!editable} oninput={(e) => spec && (spec.health = e.currentTarget.value)} />
          {/if}
        </div>
      </div>
      <label class="cell wide">
        <span>Model</span>
        <input class="mono" value={spec.model ?? ''} placeholder="D:\models\model.gguf" spellcheck="false" readonly={!editable} oninput={(e) => text('model', e.currentTarget.value)} />
      </label>
      {#if !external}
        <label class="cell wide">
          <span>MMProj</span>
          <input class="mono" value={spec.mmproj ?? ''} placeholder="none" spellcheck="false" readonly={!editable} oninput={(e) => text('mmproj', e.currentTarget.value)} />
        </label>
      {/if}
      <label class="cell">
        <span>Ctx</span>
        <input type="number" min="0" step="1024" value={spec.ctx ?? ''} placeholder="—" readonly={!editable} aria-invalid={!!ctxErr} oninput={(e) => num('ctx', e.currentTarget)} />
        {#if ctxErr}<small class="err">{ctxErr}</small>{/if}
      </label>
      <label class="cell">
        <span>GPU</span>
        <select value={spec.gpu ?? ''} disabled={!editable} onchange={(e) => text('gpu', e.currentTarget.value)}>
          <option value="">Default</option>
          {#each gpus as g (g.id)}<option value={g.id}>{g.name || g.id}</option>{/each}
          <option value="cpu">CPU</option>
          {#if !gpuKnown}<option value={spec.gpu}>{spec.gpu}</option>{/if}
        </select>
      </label>
      {#if !external}
        <div class="cell">
          <span id="tune-managed-lbl">Managed</span>
          <button
            type="button"
            class="switch"
            role="switch"
            aria-labelledby="tune-managed-lbl"
            aria-checked={spec.managed !== false}
            class:on={spec.managed !== false}
            disabled={!editable}
            title="KLIF adds the adapter's telemetry env"
            onclick={() => spec && (spec.managed = spec.managed === false)}><i></i>{spec.managed !== false ? 'on' : 'off'}</button
          >
        </div>
        <div class="cell">
          <span id="tune-apikey-lbl">API key</span>
          <button
            type="button"
            class="switch"
            role="switch"
            aria-labelledby="tune-apikey-lbl"
            aria-checked={spec.api_key !== false}
            class:on={spec.api_key !== false}
            disabled={!editable}
            title="KLIF passes its API key in the adapter's key variable"
            onclick={() => spec && (spec.api_key = spec.api_key === false)}><i></i>{spec.api_key !== false ? 'on' : 'off'}</button
          >
        </div>
      {/if}
    </div>

    <div class="preview" aria-live="polite">
      <span class="plabel">Runs</span>
      {#if shown}
        <pre class="cmdline">{shown.external ? `external server · ${shown.external}` : shown.display}</pre>
      {:else}
        <pre class="cmdline muted">—</pre>
      {/if}
      {#if draft.previewError}<p class="err">Preview failed: {draft.previewError}</p>{/if}
      {#if issues.length}
        <ul class="issues">
          {#each issues as is, i (i)}
            <li class={is.level}><b>{is.level === 'error' ? 'Error' : 'Warning'}</b>{is.field ? ` · ${is.field}` : ''}: {is.text}</li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if editable}
      {#if shared && needsApply}
        <p class="hint">Used by {listNames(labelsOf(vm, users.map((u) => u.id)))}: Apply changes them all.</p>
      {/if}
      {#if draft.applyError}
        <div class="applyerr" role="alert">
          <p class="err">{draft.applyError}</p>
          {#if draft.stale}<button type="button" onclick={() => void reload()}>Reload</button>{/if}
        </div>
      {/if}
      {#if saveAs}
        <SaveAsForm {vm} {system} {draft} onclose={() => (saveAs = false)} />
      {/if}
      <div class="cmdactions">
        <button type="button" disabled={!dirty || draft.saving} onclick={() => void revert()}>Revert</button>
        {#if !draft.isNew}<button type="button" disabled={draft.saving} onclick={() => (saveAs = !saveAs)}>Save as new…</button>{/if}
        <span class="grow"></span>
        <button type="button" class="primary" disabled={!needsApply || draft.saving || !!idProblem || !!portErr || !!ctxErr} onclick={() => void apply()}>
          {draft.saving ? 'Applying…' : 'Apply'}
        </button>
      </div>
    {/if}
  {/if}
</Section>

<style>
  .mono {
    font-family: var(--k-font-data, ui-monospace, monospace);
    font-size: 12px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 10px 12px;
    padding: 4px 0;
  }
  .cell {
    display: grid;
    gap: 5px;
    align-content: start;
    min-width: 0;
  }
  .cell.wide {
    grid-column: 1 / -1;
  }
  .cell > span {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.04em;
  }
  .pair {
    display: flex;
    gap: 6px;
    min-width: 0;
  }
  .pair select {
    flex: 0 0 auto;
    width: auto;
  }
  .pair input {
    flex: 1;
    min-width: 0;
  }
  .preview {
    display: grid;
    gap: 6px;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 10px 12px;
    background: var(--k-surface, #141414);
  }
  .plabel {
    font: 600 11px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .cmdline {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    font: 500 12px/1.5 var(--k-font-data, ui-monospace, monospace);
    color: var(--k-ink, #e6e6e6);
    max-height: 180px;
    overflow: auto;
  }
  .cmdline.muted {
    color: var(--k-muted, #8a8a8a);
  }
  .issues {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  .issues li.error {
    color: var(--k-danger, #e05a5a);
  }
  .issues li.warn {
    color: var(--k-warn, #f2a33a);
  }
  .issues b {
    font-weight: 600;
  }
  .applyerr {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .applyerr .err {
    flex: 1;
  }
  .cmdactions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
</style>
