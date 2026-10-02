<script lang="ts">
  import type { Actions, HostInfo, Session } from '../../lib/model/types';
  import { fmtClock } from '../../lib/model/format';
  import Mark from './Mark.svelte';
  import WinControls from './WinControls.svelte';
  import PanelButton from './PanelButton.svelte';
  import type { GpuSleep } from './geom';

  let {
    session,
    host,
    actions,
    k,
    gpu = null,
  }: { session: Session | null; host: HostInfo | undefined; actions: Actions; k: number; gpu?: GpuSleep | null } = $props();

  const PHASE: Record<string, { word: string; tone: string }> = {
    starting: { word: 'STARTING', tone: 'amber' },
    loading: { word: 'LOADING', tone: 'amber' },
    live: { word: 'LIVE', tone: 'cyan' },
    stopping: { word: 'STOPPING', tone: 'muted' },
    fault: { word: 'FAULT', tone: 'danger' },
  };
  const st = $derived(session ? PHASE[session.phase] : { word: 'IDLE', tone: 'muted' });
  const booting = $derived(session?.phase === 'starting' || session?.phase === 'loading');
  const frameless = $derived(!!host?.frameless);
  /** A small screen exists (or this is a browser): offer panel mode, frameless or not. */
  const panel = $derived(host?.panel?.available ? host.panel : null);
</script>

<header class="hdr" class:fl={frameless} data-tauri-drag-region>
  <div class="mark" data-tauri-drag-region><Mark size={Math.round(70 * k)} /></div>
  <div class="brand" data-tauri-drag-region>
    <div class="title" data-tauri-drag-region>KLIF</div>
    <div class="tag" data-tauri-drag-region>Koksny.com LOCAL INFERENCE FORNICATOR</div>
  </div>
  {#if panel && !frameless}<div class="pbox"><PanelButton {actions} {panel} /></div>{/if}
  <div class="status" data-tauri-drag-region>
    {#if frameless}
      <div class="winrow">
        {#if panel}<PanelButton {actions} {panel} /><span class="vrule" aria-hidden="true"></span>{/if}
        <WinControls {actions} maximized={!!host?.maximized} />
      </div>
    {/if}
    {#if gpu}
      <!-- dormant GPU: ASLEEP (or WAKING while the VRAM is restored) in amber, next to the server's own state -->
      <div class="stline" data-tauri-drag-region>
        <div class="st amber sleepw {gpu.state}" data-tauri-drag-region><span class="dot ring"></span>{gpu.state === 'waking' ? 'WAKING' : 'ASLEEP'}</div>
        <span class="vrule" aria-hidden="true"></span>
        <div class="st {st.tone}" data-tauri-drag-region><span class="dot"></span>{st.word}</div>
      </div>
    {:else}
      <div class="st {st.tone}" data-tauri-drag-region><span class="dot"></span>{st.word}</div>
    {/if}
    {#if session}
      <div class="up" data-tauri-drag-region>
        {#if booting}elapsed {fmtClock(session.loading?.elapsedS ?? session.uptimeS)}{:else}uptime {fmtClock(session.uptimeS)}{/if}
      </div>
    {/if}
  </div>
</header>

<style>
  .hdr {
    display: flex;
    align-items: center;
    gap: calc(22px * var(--k));
    height: calc(76px * var(--k));
    flex: none;
    padding: 0 calc(4px * var(--k));
  }
  .hdr.fl {
    height: calc(90px * var(--k));
    align-items: flex-end;
    padding-bottom: calc(4px * var(--k));
  }
  .mark {
    flex: none;
    border-radius: calc(14px * var(--k));
    box-shadow: 0 0 calc(12px * var(--k)) rgba(90, 182, 235, 0.22);
  }
  .brand {
    min-width: 0;
    flex: 1;
  }
  .title {
    font-family: var(--ph-display);
    font-stretch: 125%;
    font-weight: 500;
    font-size: calc(52px * var(--k));
    line-height: 0.95;
    letter-spacing: 0.06em;
    color: var(--ph-cyan);
    text-shadow:
      0 0 8px rgba(127, 227, 255, 0.55),
      0 0 22px rgba(90, 182, 235, 0.3);
  }
  .tag {
    margin-top: calc(6px * var(--k));
    font-size: calc(16px * var(--k));
    letter-spacing: 0.08em;
    color: var(--ph-brand);
    opacity: 0.9;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    flex: none;
    align-self: stretch;
    text-align: right;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(6px * var(--k));
    align-items: flex-end;
  }
  .fl .status {
    justify-content: flex-start;
    gap: calc(4px * var(--k));
  }
  .winrow {
    display: flex;
    align-items: center;
    gap: calc(10px * var(--k));
    margin: calc(-8px * var(--k)) calc(-12px * var(--k)) calc(4px * var(--k)) 0;
  }
  .vrule {
    width: 1px;
    height: calc(20px * var(--k));
    background: var(--ph-rule);
  }
  /* windowed (not frameless): the button sits in the right cluster, left of the status readout */
  .pbox {
    flex: none;
    align-self: center;
    margin-right: calc(-4px * var(--k));
  }
  .st {
    display: flex;
    align-items: center;
    gap: calc(12px * var(--k));
    font-size: calc(19px * var(--k));
    letter-spacing: 0.12em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
  }
  .dot {
    width: calc(11px * var(--k));
    height: calc(11px * var(--k));
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 8px currentColor;
  }
  .stline {
    display: flex;
    align-items: center;
    gap: calc(16px * var(--k));
  }
  .stline .vrule {
    margin: 0;
  }
  .dot.ring {
    background: transparent;
    border: calc(2.5px * var(--k)) solid currentColor;
    box-shadow: 0 0 8px currentColor;
    animation: ph-breathe 3.2s ease-in-out infinite;
  }
  .sleepw.waking .dot.ring {
    background: currentColor;
    animation: ph-breathe 0.9s ease-in-out infinite;
  }
  @keyframes ph-breathe {
    50% {
      opacity: 0.3;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dot.ring,
    .sleepw.waking .dot.ring {
      animation: none;
    }
  }
  .st.amber {
    color: var(--ph-amber);
    text-shadow: 0 0 6px rgba(232, 176, 74, 0.45);
  }
  .st.danger {
    color: var(--ph-danger);
    text-shadow: 0 0 6px rgba(229, 97, 92, 0.5);
  }
  .st.muted {
    color: var(--ph-muted);
    text-shadow: none;
  }
  .up {
    font-size: calc(16px * var(--k));
    letter-spacing: 0.06em;
    color: var(--ph-cyan);
    opacity: 0.9;
    white-space: nowrap;
  }
</style>
