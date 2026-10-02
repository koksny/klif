<script lang="ts">
  // Controls bar: the same buttons in the same places in every phase; a phase only changes what they say
  // and whether they are enabled.
  //   [:port] [API key] ............ [PRIMARY] [Restart | Dismiss] [Tune] [Endpoint | Web UI] [Console | Full log]
  // PRIMARY has one fixed width: Launch <TIER> (idle) / Cancel (loading) / Stop (live) / Stopping (disabled) /
  // Restart <TIER> after a fault. Tune is always enabled: on a running tier the drawer offers "Restart to apply".
  import type { Actions, Session, Slot } from '../../lib/model/types';
  import { availText } from './geom';

  let {
    view,
    session,
    selected,
    running,
    actions,
    port,
  }: {
    view: 'idle' | 'loading' | 'live' | 'stopping' | 'fault';
    session: Session | null;
    /** The selected tier (Launch and Tune act on it). */
    selected: Slot | undefined;
    /** The session's tier. */
    running: Slot | undefined;
    actions: Actions;
    /** The session's port, or (idle) the selected tier's recipe port. */
    port: number | undefined;
  } = $props();

  const online = $derived(view === 'live');
  const image = $derived((running ?? selected)?.kind === 'image');
  const ready = $derived(selected?.availability === 'ready');
  const ep = $derived(session ? `http://${session.endpoint.host}:${session.endpoint.port}` : '');
  const tuneTip = $derived(
    session && session.slot === selected?.id && view !== 'fault' ? 'Change the settings; Restart to apply them' : 'Change what this tier launches',
  );
</script>

<div class="ctl">
  <button class="chip" onclick={() => actions.copyEndpoint()} disabled={!online} title={online ? `Copy ${ep}` : 'Endpoint offline'}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6.6 9.4a2.7 2.7 0 0 0 3.8 0l2.2-2.2a2.7 2.7 0 0 0-3.8-3.8l-.7.7" /><path d="M9.4 6.6a2.7 2.7 0 0 0-3.8 0L3.4 8.8a2.7 2.7 0 0 0 3.8 3.8l.7-.7" /></svg>
    <span class="mono">{port ? `:${port}` : '—'}</span>{#if !online}<span class="note">offline</span>{/if}
  </button>
  <button class="chip key" onclick={() => actions.copyApiKey()} disabled={!online || !session?.apiKeySet} title={session?.apiKeySet ? 'Copy API key' : 'No API key set'}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="5.2" cy="10.8" r="2.7" /><path d="M7.2 8.8L13 3M10.6 5.4l1.8 1.8M12.2 3.8l1.4 1.4" /></svg>
    {#if session?.apiKeySet}<span class="dots mono" aria-hidden="true">•••••••</span><span class="note">copy</span>{:else}<span class="note">no key</span>{/if}
  </button>
  <span class="spacer"></span>

  <!-- the primary place: Launch / Cancel / Stop / Stopping / Restart after a fault -->
  {#if view === 'idle'}
    <button
      class="btn act go"
      onclick={() => actions.launch(selected?.id)}
      disabled={!ready}
      title={ready ? `Launch ${selected?.label ?? ''}` : `${selected?.label ?? ''}: ${selected?.reason ?? availText(selected?.availability ?? 'unsupported')}`}
    >
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 3L13 8L4.5 13Z" class="solid" /></svg><span class="al">Launch <b>{selected?.label ?? ''}</b></span>
    </button>
  {:else if view === 'fault'}
    <button class="btn act fix" onclick={() => actions.restart()} title="Launch {running?.label ?? ''} again">
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13.2 8a5.2 5.2 0 1 1-1.5-3.7" /><path d="M12.2 1.8v3h-3" /></svg><span class="al">Restart <b>{running?.label ?? ''}</b></span>
    </button>
  {:else}
    <button class="btn act stop" onclick={() => actions.stop()} disabled={view === 'stopping'}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="4" y="4" width="8" height="8" class="solid" /></svg><span class="al"
        >{view === 'loading' ? 'Cancel' : view === 'stopping' ? 'Stopping' : 'Stop'} <b>{running?.label ?? ''}</b></span
      >
    </button>
  {/if}

  {#if view === 'fault'}
    <button class="btn w2" onclick={() => actions.dismiss?.()} title="Leave the fault and go back to the launcher">
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13 8H3.5" /><path d="M7 4.2L3.2 8L7 11.8" /></svg>Dismiss
    </button>
  {:else}
    <button class="btn w2" onclick={() => actions.restart()} disabled={!session || view !== 'live'} title="Stop and launch again with the current settings">
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M13.2 8a5.2 5.2 0 1 1-1.5-3.7" /><path d="M12.2 1.8v3h-3" /></svg>Restart
    </button>
  {/if}
  <button class="btn tune w3" onclick={() => actions.openTune(selected?.id)} title={tuneTip}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2 4h12M2 8h12M2 12h12" /><circle cx="10" cy="4" r="1.6" class="knob" /><circle cx="5.5" cy="8" r="1.6" class="knob" /><circle cx="9.5" cy="12" r="1.6" class="knob" /></svg>Tune
  </button>
  <button class="btn w4" onclick={() => actions.openEndpoint()} disabled={!online} title={image ? 'Open the image server web UI' : 'Open the endpoint'}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M9.5 2.5h4v4" /><path d="M13.5 2.5L7.5 8.5" /><path d="M12 9.5v3.2a.8.8 0 0 1-.8.8H3.3a.8.8 0 0 1-.8-.8V4.8a.8.8 0 0 1 .8-.8h3.2" /></svg>{image ? 'Web UI' : 'Endpoint'}
  </button>
  <button class="btn w5" onclick={() => actions.toggleConsole(view === 'fault' ? true : undefined)}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 4.5L6.5 8L3 11.5" /><path d="M8.5 12h4.5" /></svg>{view === 'fault' ? 'Full log' : 'Console'}
  </button>
</div>

<style>
  .ctl {
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(8px * var(--k));
    height: calc(46px * var(--k));
    padding: 0 calc(10px * var(--k));
    font-size: var(--ph-fs-m);
  }
  .spacer {
    flex: 1 1 0;
    min-width: calc(4px * var(--k));
  }
  .chip,
  .btn {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: calc(8px * var(--k));
    height: calc(32px * var(--k));
    white-space: nowrap;
    color: var(--ph-cyan);
    border: 1px solid #2a7f93;
    border-radius: 5px;
    background: rgba(3, 9, 12, 0.7);
  }
  .chip {
    border-radius: 999px;
    padding: 0 calc(14px * var(--k)) 0 calc(12px * var(--k));
    border-color: #1f6577;
    color: var(--ph-ink);
  }
  .chip:disabled {
    opacity: 0.55;
  }
  .chip .mono {
    font-size: 1.04em;
  }
  .dots {
    letter-spacing: 0.12em;
  }
  .note {
    color: var(--ph-muted);
  }
  .btn {
    padding: 0 calc(13px * var(--k));
    text-shadow: var(--ph-glow-soft);
  }
  .chip:hover:not(:disabled),
  .btn:hover:not(:disabled) {
    border-color: var(--ph-cyan);
    background: rgba(18, 48, 58, 0.5);
  }
  svg {
    width: calc(14px * var(--k));
    height: calc(14px * var(--k));
    min-width: 12px;
    min-height: 12px;
    flex: none;
    overflow: visible;
  }
  svg path,
  svg rect,
  svg circle {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
  svg .solid {
    fill: currentColor;
    stroke: none;
  }
  svg .knob {
    fill: var(--ph-glass);
  }
  /* every place to the right of the spacer has one width, whatever its label says in this phase */
  .btn.w2 {
    width: max(80px, calc(100px * var(--k)));
    justify-content: center;
  }
  .btn.w3 {
    width: max(66px, calc(84px * var(--k)));
    justify-content: center;
  }
  .btn.w4 {
    width: max(90px, calc(108px * var(--k)));
    justify-content: center;
  }
  .btn.w5 {
    width: max(82px, calc(100px * var(--k)));
    justify-content: center;
  }
  /* the primary place: one width for every phase, so nothing beside it moves */
  .btn.act {
    width: max(176px, calc(206px * var(--k)));
    justify-content: flex-start;
    padding: 0 calc(14px * var(--k));
  }
  .al {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .btn.act b {
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.08em;
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
    border-color: var(--ph-hot);
  }
  .btn.go:disabled {
    opacity: 1;
    color: var(--ph-muted);
    background: rgba(3, 9, 12, 0.7);
    border-color: var(--ph-rule);
    box-shadow: none;
  }
  .btn.stop {
    color: #ee6f65;
    border-color: var(--ph-danger);
    background: rgba(60, 14, 14, 0.25);
    box-shadow: 0 0 10px rgba(229, 97, 92, 0.22);
    text-shadow: none;
  }
  .btn.stop:hover:not(:disabled) {
    border-color: #ff8a80;
    background: rgba(90, 20, 20, 0.35);
  }
  .btn.fix {
    color: #ff8f88;
    border: 1.5px solid var(--ph-danger);
    background: rgba(60, 14, 14, 0.32);
    box-shadow:
      0 0 12px rgba(229, 97, 92, 0.35),
      inset 0 0 10px rgba(229, 97, 92, 0.12);
    text-shadow: 0 0 6px rgba(229, 97, 92, 0.5);
  }
  .btn.fix:hover {
    border-color: #ff8a80;
    background: rgba(90, 20, 20, 0.42);
  }
  .btn.tune {
    border-color: var(--ph-brand);
    color: var(--ph-cyan);
  }
</style>
