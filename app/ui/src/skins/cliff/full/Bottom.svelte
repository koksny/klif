<script lang="ts">
  // Action bar + the last console line. The bar changes with the state; it stays in one place so the
  // primary action is always where the eye already is (Launch / Cancel / Stop / Restart).
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { availabilityText, selectedSlot, sessionKind, sessionSlot, viewState } from '../util';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();
  const s = $derived(vm.session);
  const view = $derived(viewState(vm));
  const kind = $derived(sessionKind(vm));
  const sel = $derived(selectedSlot(vm));
  const running = $derived(sessionSlot(vm));
  const why = $derived(sel ? availabilityText(sel.availability) : 'no job slots configured');
  const ready = $derived(vm.slots.filter((x) => x.availability === 'ready').length);
  const lastLine = $derived(vm.console.length ? vm.console[vm.console.length - 1] : '');
  const idleLine = $derived(`${ready === vm.slots.length ? 'ready' : `${ready} of ${vm.slots.length} ready`} · ${vm.slots.length} jobs`);
</script>

{#snippet portChip()}
  {#if s}
    <button class="chip ep c-data" onclick={() => actions.copyEndpoint()} title="Copy endpoint http://{s.endpoint.host}:{s.endpoint.port}">
      :{s.endpoint.port}
    </button>
  {/if}
{/snippet}

{#snippet keyChip()}
  {#if s}
    <button
      class="chip key"
      onclick={() => actions.copyApiKey()}
      disabled={!s.apiKeySet}
      title={s.apiKeySet ? 'Copy API key' : 'No API key set'}
    >
      {#if s.apiKeySet}<span class="dots" aria-label="API key hidden">••••••••</span><span class="cp">copy</span>{:else}<span class="nokey">no key</span>{/if}
    </button>
  {/if}
{/snippet}

{#snippet consoleBtn()}
  <button class="btn" onclick={() => actions.toggleConsole()}>
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M3.5 5.5 8 10l-4.5 4.5M10 15h6.5" class="ln" /></svg>
    <span>Console</span>
  </button>
{/snippet}

{#snippet openBtn(label: string, enabled: boolean)}
  <button class="btn" onclick={() => actions.openEndpoint()} disabled={!enabled} title={enabled ? '' : 'The endpoint is not up yet'}>
    <svg viewBox="0 0 20 20" aria-hidden="true"
      ><path d="M9 3.5H4.5a1 1 0 0 0-1 1v11a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1V11" class="ln" /><path
        d="M11.5 3.5h5v5M16.5 3.5 9 11"
        class="ln"
      /></svg
    >
    <span>{label}</span>
  </button>
{/snippet}

{#snippet restartIcon()}
  <svg viewBox="0 0 20 20" aria-hidden="true"
    ><path d="M16.2 10a6.2 6.2 0 1 1-1.9-4.45" class="ln" /><path d="M15.4 2.6v3.6h-3.6" class="ln" /></svg
  >
{/snippet}

<div class="bar">
  {#if view === 'idle'}
    <button
      class="btn primary"
      onclick={() => actions.launch(vm.selected)}
      disabled={!!why}
      title={why ? `${sel?.label ?? 'This job'} cannot be launched: ${why}` : `Launch ${sel?.label ?? ''}`}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 3.8v12.4L16 10Z" class="fill" /></svg>
      <span>Launch {sel?.label ?? ''}</span>{#if why}<span class="why">· {why}</span>{/if}
    </button>
    <button class="btn" onclick={() => actions.openTune(vm.selected)}>
      <svg viewBox="0 0 20 20" aria-hidden="true"
        ><path d="M3.5 6h13M3.5 14h13" class="ln" /><circle cx="7.5" cy="6" r="2" class="knob" /><circle
          cx="12.5"
          cy="14"
          r="2"
          class="knob"
        /></svg
      >
      <span>Tune</span>
    </button>
    {@render consoleBtn()}
  {:else if view === 'fault' && s}
    <!-- the server is gone: the key is moot, the port says which endpoint went down -->
    {@render portChip()}
    <span class="sep" aria-hidden="true"></span>
    <button class="btn hot" onclick={() => actions.restart()}>
      {@render restartIcon()}
      <span>Restart {running?.label ?? ''}</span>
    </button>
    <button class="btn" onclick={() => actions.toggleConsole(true)}>
      <svg viewBox="0 0 20 20" aria-hidden="true"
        ><path d="M4.5 3.5h11v13h-11z" class="ln" /><path d="M7.5 7.5h5M7.5 10.5h5M7.5 13.5h3" class="ln" /></svg
      >
      <span>Show full log</span>
    </button>
    <button class="btn" onclick={() => actions.dismiss?.()}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M16 10H4.5M9 5.2 4.2 10 9 14.8" class="ln" /></svg>
      <span>Back to launcher</span>
    </button>
  {:else if view === 'loading' && s}
    {@render portChip()}
    {#if kind === 'llm'}{@render keyChip()}{/if}
    <span class="sep" aria-hidden="true"></span>
    <button class="btn" onclick={() => actions.stop()} disabled={s.phase === 'stopping'}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3" y="3" width="14" height="14" rx="1.5" class="cancel" /></svg>
      <span>Cancel</span>
    </button>
    <button class="btn" disabled title="Available once the session is live">
      {@render restartIcon()}
      <span>Restart</span>
    </button>
    {@render openBtn(kind === 'image' ? 'Open web UI' : 'Open endpoint', false)}
    {@render consoleBtn()}
  {:else if s && kind === 'image'}
    {@render portChip()}
    {@render openBtn('Open web UI', s.phase === 'live')}
    <button class="btn" onclick={() => actions.stop()} disabled={s.phase === 'stopping'}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3" y="3" width="14" height="14" rx="1.5" class="stop" /></svg>
      <span>Stop</span>
    </button>
    <button class="btn" onclick={() => actions.restart()} disabled={s.phase === 'stopping'}>
      {@render restartIcon()}
      <span>Restart</span>
    </button>
    {@render consoleBtn()}
  {:else if s}
    {@render portChip()}
    {@render keyChip()}
    <span class="sep" aria-hidden="true"></span>
    <button class="btn" onclick={() => actions.stop()} disabled={s.phase === 'stopping'}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3" y="3" width="14" height="14" rx="1.5" class="stop" /></svg>
      <span>Stop</span>
    </button>
    <button class="btn" onclick={() => actions.restart()} disabled={s.phase === 'stopping'}>
      {@render restartIcon()}
      <span>Restart</span>
    </button>
    {@render openBtn('Open endpoint', true)}
    {@render consoleBtn()}
  {/if}
</div>

<button class="console" class:hot={view === 'fault'} onclick={() => actions.toggleConsole()} title="Open console">
  <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M7.5 5 12.5 10l-5 5" class="ln" /></svg>
  <span class="c-data txt">{lastLine || (view === 'idle' ? idleLine : '')}</span>
</button>

<style>
  .bar {
    display: flex;
    align-items: stretch;
    gap: max(8px, calc(var(--u) * 14));
    height: max(40px, calc(var(--u) * 50));
    margin-top: max(10px, calc(var(--u) * 18));
  }
  .chip,
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: max(8px, calc(var(--u) * 14));
    padding: 0 max(10px, calc(var(--u) * 18));
    border-radius: 8px;
    background: var(--slate);
    border: 1px solid #2a353b;
    font-size: max(13px, calc(var(--u) * 17));
    color: var(--foam);
    white-space: nowrap;
  }
  .chip:hover:not(:disabled),
  .btn:hover:not(:disabled) {
    background: var(--slate-2);
    border-color: #36434a;
  }
  .ep {
    font-size: max(13px, calc(var(--u) * 17.5));
    letter-spacing: 0.04em;
    min-width: max(70px, calc(var(--u) * 92));
  }
  .key {
    gap: max(8px, calc(var(--u) * 12));
  }
  .dots {
    letter-spacing: 0.12em;
    font-size: 1.05em;
  }
  .cp {
    color: var(--sky);
    font-weight: 500;
  }
  .nokey {
    color: var(--muted);
  }
  .sep {
    width: 1px;
    margin: 8px max(2px, calc(var(--u) * 10));
    background: var(--rule);
    flex: none;
  }
  .btn {
    flex: 1 1 auto;
  }
  .btn.primary {
    flex: 2 1 auto;
    background: var(--sky);
    border-color: var(--sky);
    color: var(--basalt);
    font-weight: 600;
  }
  .btn.primary:hover:not(:disabled) {
    background: #74c3f0;
    border-color: #74c3f0;
  }
  .btn.primary:disabled {
    opacity: 1;
    background: #26333a;
    border-color: #3a4850;
    color: #8fa3ae;
  }
  .why {
    color: var(--amber);
    font-weight: 500;
  }
  .btn.hot {
    flex: 1.4 1 auto;
    background: rgba(242, 163, 58, 0.12);
    border-color: var(--amber);
    color: var(--amber);
    font-weight: 600;
  }
  .btn.hot:hover {
    background: rgba(242, 163, 58, 0.2);
    border-color: #f6b65a;
  }
  svg {
    width: max(16px, calc(var(--u) * 22));
    height: max(16px, calc(var(--u) * 22));
    flex: none;
  }
  .stop {
    fill: var(--danger);
  }
  .cancel {
    fill: var(--amber);
  }
  .ln {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.7;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .hot .ln {
    stroke-width: 2;
  }
  .fill {
    fill: currentColor;
  }
  .knob {
    fill: var(--slate);
    stroke: currentColor;
    stroke-width: 1.6;
  }
  .console {
    display: flex;
    align-items: center;
    gap: max(10px, calc(var(--u) * 16));
    width: 100%;
    height: max(34px, calc(var(--u) * 44));
    margin-top: max(10px, calc(var(--u) * 20));
    margin-bottom: max(10px, calc(var(--u) * 26));
    padding: 0 max(10px, calc(var(--u) * 14));
    border-radius: 8px;
    border: 1px solid #2a353b;
    background: rgba(15, 19, 22, 0.6);
    text-align: left;
    color: #c3d3db;
  }
  .console:hover {
    border-color: #36434a;
  }
  .console.hot {
    color: var(--amber);
  }
  .console svg {
    width: max(14px, calc(var(--u) * 18));
    height: max(14px, calc(var(--u) * 18));
    color: #9cb0ba;
  }
  .console.hot svg {
    color: var(--amber);
  }
  .txt {
    font-size: max(12px, calc(var(--u) * 15.5));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
</style>
