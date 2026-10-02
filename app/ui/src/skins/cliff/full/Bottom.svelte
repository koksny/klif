<script lang="ts">
  // Controls: the same buttons in the same places in every phase; a phase only changes what they say and
  // whether they are enabled. [endpoint :port] [API key] ... [primary: Launch / Cancel / Stop / Stopping /
  // Restart after a fault, one fixed width] [Restart, or Dismiss after a fault] [Tune, always] [Endpoint or
  // Web UI] [Console, or Full log after a fault]. The last console line under it.
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { availabilityText, blockView, selectedSlot, sessionKind, sessionSlot } from '../util';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();
  const s = $derived(vm.session);
  const view = $derived(blockView(vm));
  const kind = $derived(sessionKind(vm));
  const sel = $derived(selectedSlot(vm));
  const running = $derived(sessionSlot(vm));
  const why = $derived(sel ? (availabilityText(sel.availability) ?? null) : 'no job slots configured');
  const online = $derived(s?.phase === 'live');
  const busy = $derived(view === 'loading' || view === 'stopping');
  const port = $derived(s?.endpoint.port ?? sel?.recipe?.port);
  const label = $derived(running?.label ?? s?.model.name ?? '');
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const ready = $derived(vm.slots.filter((x) => x.availability === 'ready').length);
  const idleLine = $derived(`${ready === vm.slots.length ? 'ready' : `${ready} of ${vm.slots.length} ready`} · ${vm.slots.length} jobs`);
  const tuneTip = $derived(
    s && s.slot === vm.selected && view !== 'fault' ? 'Change the settings; Restart to apply them' : 'Change what this tier launches',
  );
</script>

{#snippet restartIcon()}
  <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M16.2 10a6.2 6.2 0 1 1-1.9-4.45" class="ln" /><path d="M15.4 2.6v3.6h-3.6" class="ln" /></svg>
{/snippet}

<section class="ctrl c-panel">
  <div class="bar">
    <button
      class="chip ep"
      onclick={() => actions.copyEndpoint()}
      disabled={!online}
      title={online && s ? `Copy http://${s.endpoint.host}:${s.endpoint.port}` : 'Endpoint offline'}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true"
        ><path d="M8.4 11.6a3.3 3.3 0 0 0 4.7 0l2.5-2.5a3.3 3.3 0 0 0-4.7-4.7l-.8.8" class="ln" /><path
          d="M11.6 8.4a3.3 3.3 0 0 0-4.7 0l-2.5 2.5a3.3 3.3 0 0 0 4.7 4.7l.8-.8"
          class="ln"
        /></svg
      >
      <span class="c-data">{port ? `:${port}` : '—'}</span>{#if !online}<span class="q">offline</span>{/if}
    </button>
    <button
      class="chip"
      onclick={() => actions.copyApiKey()}
      disabled={!online || !s?.apiKeySet}
      title={s?.apiKeySet ? 'Copy API key' : 'No API key set'}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true"
        ><circle cx="13" cy="7" r="3.6" class="ln" /><path d="M10.4 9.6 3.5 16.5M5.8 14.2l1.9 1.9M7.6 12.4l1.4 1.4" class="ln" /></svg
      >
      {#if s?.apiKeySet}<span class="dots c-data" aria-label="API key hidden">••••••••</span><span class="cp">copy</span>{:else}<span class="q"
          >no key</span
        >{/if}
    </button>
    <span class="grow"></span>

    <!-- Primary: one place, one width -->
    {#if view === 'idle'}
      <button
        class="btn act primary"
        onclick={() => actions.launch(vm.selected)}
        disabled={!!why}
        title={why ? `${sel?.label ?? 'This tier'} cannot be launched: ${sel?.reason ?? why}` : `Launch ${sel?.label ?? ''}`}
      >
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 3.8v12.4L16 10Z" class="fill" /></svg>
        <span class="al">Launch <b>{sel?.label ?? ''}</b></span>
      </button>
    {:else if view === 'fault'}
      <button class="btn act danger" onclick={() => actions.restart()} title="Launch {label} again">
        {@render restartIcon()}
        <span class="al">Restart <b>{label}</b></span>
      </button>
    {:else}
      <button class="btn act stop" onclick={() => actions.stop()} disabled={view === 'stopping'}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="4" y="4" width="12" height="12" rx="1.5" class="fill" /></svg>
        <span class="al">{view === 'loading' ? 'Cancel' : view === 'stopping' ? 'Stopping' : 'Stop'} <b>{label}</b></span>
      </button>
    {/if}

    {#if view === 'fault'}
      <button class="btn w-rs" onclick={() => actions.dismiss?.()} title="Back to the launcher">
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M16 10H4.5M9 5.2 4.2 10 9 14.8" class="ln" /></svg>
        <span>Dismiss</span>
      </button>
    {:else}
      <button class="btn w-rs" onclick={() => actions.restart()} disabled={!s || busy} title="Stop and launch again with the current settings">
        {@render restartIcon()}
        <span>Restart</span>
      </button>
    {/if}
    <button class="btn tune" onclick={() => actions.openTune(vm.selected)} title={tuneTip}>
      <svg viewBox="0 0 20 20" aria-hidden="true"
        ><path d="M3.5 6h13M3.5 14h13" class="ln" /><circle cx="7.5" cy="6" r="2" class="knob" /><circle cx="12.5" cy="14" r="2" class="knob" /></svg
      >
      <span>Tune</span>
    </button>
    <button class="btn w-ep" onclick={() => actions.openEndpoint()} disabled={!online} title={online ? '' : 'The endpoint is not up'}>
      <svg viewBox="0 0 20 20" aria-hidden="true"
        ><path d="M9 3.5H4.5a1 1 0 0 0-1 1v11a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1V11" class="ln" /><path d="M11.5 3.5h5v5M16.5 3.5 9 11" class="ln" /></svg
      >
      <span>{kind === 'image' ? 'Web UI' : 'Endpoint'}</span>
    </button>
    <button class="btn w-cn" onclick={() => actions.toggleConsole(view === 'fault' ? true : undefined)}>
      {#if view === 'fault'}
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4.5 3.5h11v13h-11z" class="ln" /><path d="M7.5 7.5h5M7.5 10.5h5M7.5 13.5h3" class="ln" /></svg>
        <span>Full log</span>
      {:else}
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M3.5 5.5 8 10l-4.5 4.5M10 15h6.5" class="ln" /></svg>
        <span>Console</span>
      {/if}
    </button>
  </div>

  <button class="console" class:hot={view === 'fault'} onclick={() => actions.toggleConsole(true)} title="Open console">
    <span class="caret c-data">&gt;</span>
    <span class="c-data txt">{lastLine || (view === 'idle' ? idleLine : '')}</span>
  </button>
</section>

<style>
  .ctrl {
    display: grid;
    grid-template-rows: max(42px, calc(var(--u) * 48)) max(26px, calc(var(--u) * 30));
    overflow: hidden;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: max(6px, calc(var(--u) * 8));
    padding: 0 max(8px, calc(var(--u) * 10));
    border-bottom: 1px solid var(--edge);
    min-width: 0;
  }
  .grow {
    flex: 1 1 auto;
  }
  .chip,
  .btn {
    display: inline-flex;
    align-items: center;
    gap: max(6px, calc(var(--u) * 8));
    height: max(30px, calc(var(--u) * 32));
    padding: 0 max(10px, calc(var(--u) * 12));
    border-radius: 6px;
    background: var(--slate);
    border: 1px solid var(--edge);
    font-size: var(--fs-m);
    color: var(--foam);
    white-space: nowrap;
    flex: none;
  }
  .chip:hover:not(:disabled),
  .btn:hover:not(:disabled) {
    background: var(--slate-2);
    border-color: #3a4850;
  }
  .chip {
    color: var(--mist);
  }
  /* Fixed places: a label change (offline, Dismiss, Web UI, Full log) never moves the buttons beside it. */
  .chip.ep {
    width: max(128px, calc(var(--u) * 128));
  }
  .btn.w-rs {
    width: max(100px, calc(var(--u) * 98));
    justify-content: center;
  }
  .btn.w-ep {
    width: max(106px, calc(var(--u) * 106));
    justify-content: center;
  }
  .btn.w-cn {
    width: max(102px, calc(var(--u) * 100));
    justify-content: center;
  }
  .chip:disabled {
    opacity: 0.6;
  }
  .q {
    color: var(--muted);
  }
  .dots {
    letter-spacing: 0.1em;
    color: var(--foam);
  }
  .cp {
    color: var(--sky);
    font-weight: 500;
  }
  /* The primary place: one width for Launch / Cancel / Stop / Restart, so nothing beside it moves. */
  .btn.act {
    width: max(176px, calc(var(--u) * 206));
    justify-content: flex-start;
    font-weight: 500;
  }
  .al {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .btn.act b {
    font-weight: 600;
    letter-spacing: 0.06em;
  }
  .btn.primary {
    background: var(--sky);
    border-color: var(--sky);
    color: var(--basalt);
  }
  .btn.primary:hover:not(:disabled) {
    background: #74c3f0;
    border-color: #74c3f0;
  }
  .btn.primary:disabled {
    opacity: 1;
    background: #26333a;
    border-color: #3a4850;
    color: var(--muted);
  }
  .btn.stop {
    border-color: rgba(232, 100, 90, 0.75);
    color: #f3a49c;
  }
  .btn.stop .fill {
    fill: var(--danger);
  }
  .btn.stop:disabled {
    opacity: 0.6;
  }
  .btn.danger {
    border-color: var(--danger);
    background: rgba(232, 100, 90, 0.14);
    color: #f6b2ab;
  }
  .btn.danger:hover {
    background: rgba(232, 100, 90, 0.22);
    border-color: #f08a80;
  }
  .btn.tune {
    border-color: #2f5a75;
    color: var(--sky);
  }
  svg {
    width: max(14px, calc(var(--u) * 16));
    height: max(14px, calc(var(--u) * 16));
    flex: none;
  }
  .ln {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .fill {
    fill: currentColor;
  }
  .knob {
    fill: var(--slate);
    stroke: currentColor;
    stroke-width: 1.5;
  }
  .console {
    display: flex;
    align-items: center;
    gap: max(8px, calc(var(--u) * 10));
    min-width: 0;
    padding: 0 max(12px, calc(var(--u) * 16));
    text-align: left;
    color: #b9cad3;
  }
  .console:hover .txt {
    color: var(--foam);
  }
  .console.hot {
    color: var(--danger);
  }
  .caret {
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .txt {
    font-size: max(10.5px, calc(var(--u) * 10.75));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
</style>
