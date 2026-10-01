<script lang="ts">
  // Bottom control row. The set follows the state:
  //   llm live   : endpoint, API key | Stop, Restart, Open endpoint, Console
  //   image live : endpoint | Open web UI | Stop, Restart, Console
  //   loading    : endpoint (, key) | Cancel, Restart, Open endpoint (not up yet), Console
  //   fault      : endpoint (, key) | Console   (Restart / Show full log / Back are in the fault panel)
  // The idle launcher has its own big Launch row, so this component is not drawn there.
  import type { Actions, Session, Slot } from '../../lib/model/types';

  let {
    session,
    slot,
    actions,
    mode = 'llm',
  }: { session: Session | null; slot: Slot | undefined; actions: Actions; mode?: string } = $props();

  const ep = $derived(session ? `${session.endpoint.host}:${session.endpoint.port}` : '');
  const busy = $derived(session?.phase === 'stopping');
  const image = $derived(slot?.kind === 'image');
</script>

<div class="ctl">
  {#if session}
    <button class="chip" onclick={() => actions.copyEndpoint()} title="Copy endpoint {ep}" aria-label="Copy endpoint {ep}">
      :{session.endpoint.port}
    </button>
    {#if image}
      {#if mode === 'image'}
        <button class="btn web" onclick={() => actions.openEndpoint()}>Open web UI</button>
      {/if}
    {:else}
      <button
        class="chip key"
        onclick={() => actions.copyApiKey()}
        disabled={!session.apiKeySet}
        title={session.apiKeySet ? 'Copy API key' : 'No API key set'}
        aria-label={session.apiKeySet ? 'Copy API key' : 'No API key set'}
      >
        {#if session.apiKeySet}<span class="dots" aria-hidden="true">•••••••••</span><span>copy</span>{:else}<span>no key</span>{/if}
      </button>
    {/if}
  {/if}
  <span class="spacer"></span>
  {#if session && mode === 'fault'}
    <!-- the ways out of a fault live in the fault panel -->
  {:else if session}
    <button class="btn stop" onclick={() => actions.stop()} disabled={busy}>{mode === 'loading' ? 'Cancel' : 'Stop'}</button>
    <button class="btn" onclick={() => actions.restart()} disabled={busy}>Restart</button>
    {#if !image}
      <button class="btn" onclick={() => actions.openEndpoint()} disabled={session.phase !== 'live'}>Open endpoint</button>
    {/if}
  {:else}
    <button class="btn go" onclick={() => actions.launch(slot?.id)} disabled={!slot || slot.availability !== 'ready'}>
      Launch {slot?.label ?? ''}
    </button>
    <button class="btn" onclick={() => actions.openTune(slot?.id)}>Tune</button>
  {/if}
  <button class="btn" onclick={() => actions.toggleConsole()}>Console</button>
</div>

<style>
  .ctl {
    display: flex;
    align-items: stretch;
    gap: calc(12px * var(--k));
    flex: none;
    height: calc(48px * var(--k));
    font-size: calc(18px * var(--k));
    letter-spacing: 0.04em;
  }
  .spacer {
    flex: 1 1 0;
    min-width: calc(4px * var(--k));
  }
  .chip,
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: calc(14px * var(--k));
    white-space: nowrap;
    color: var(--ph-cyan);
    border: 1px solid var(--ph-rule);
    background: rgba(3, 9, 12, 0.7);
  }
  .chip {
    border-radius: 999px;
    padding: 0 calc(26px * var(--k));
    border-color: #1f6577;
  }
  .chip.key {
    padding: 0 calc(28px * var(--k));
  }
  .dots {
    letter-spacing: 0.28em;
  }
  .btn {
    border-radius: 5px;
    padding: 0 calc(26px * var(--k));
    border-color: #2a7f93;
    text-shadow: var(--ph-glow-soft);
  }
  .btn.web {
    padding: 0 calc(40px * var(--k));
  }
  .chip:hover:not(:disabled),
  .btn:hover:not(:disabled) {
    border-color: var(--ph-cyan);
    background: rgba(18, 48, 58, 0.5);
  }
  .btn.stop {
    color: #ee6f65;
    border-color: var(--ph-danger);
    background: rgba(60, 14, 14, 0.25);
    box-shadow: 0 0 10px rgba(229, 97, 92, 0.22);
    padding: 0 calc(28px * var(--k));
  }
  .btn.stop:hover:not(:disabled) {
    border-color: #ff8a80;
    background: rgba(90, 20, 20, 0.35);
  }
  .btn.go {
    color: var(--ph-glass);
    background: var(--ph-cyan);
    border-color: var(--ph-cyan);
    text-shadow: none;
    box-shadow: 0 0 14px rgba(127, 227, 255, 0.35);
  }
  .btn.go:hover:not(:disabled) {
    background: var(--ph-hot);
  }
</style>
