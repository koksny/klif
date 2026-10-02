<script lang="ts">
  // Compact header, one row: mark + KLIF + tagline on the left; on the right the server phase (and the GPU's
  // sleep state while dormant), uptime / elapsed or the app version when idle, the panel-mode button, and the
  // window controls when the host is frameless. The whole bar is the window's drag region.
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
  <div class="mark" data-tauri-drag-region><Mark size={Math.round(38 * k)} /></div>
  <div class="brand" data-tauri-drag-region>
    <div class="title" data-tauri-drag-region>KLIF</div>
    <div class="tag" data-tauri-drag-region>Koksny.com LOCAL INFERENCE FORNICATOR</div>
  </div>
  <div class="status" data-tauri-drag-region>
    {#if gpu}
      <!-- dormant GPU: ASLEEP (or WAKING while the VRAM is restored) in amber, next to the server's own state -->
      <span class="st amber sleepw {gpu.state}" data-tauri-drag-region><span class="dot ring"></span>{gpu.state === 'waking' ? 'WAKING' : 'ASLEEP'}</span>
      <span class="vrule" aria-hidden="true"></span>
    {/if}
    <span class="st {st.tone}" data-tauri-drag-region><span class="dot" class:pulse={booting}></span>{st.word}</span>
    {#if session}
      <span class="vrule" aria-hidden="true"></span>
      <span class="up" data-tauri-drag-region
        >{booting ? 'elapsed' : 'uptime'} <b>{fmtClock(booting ? (session.loading?.elapsedS ?? session.uptimeS) : session.uptimeS)}</b></span
      >
    {:else if host?.appVersion}
      <span class="vrule" aria-hidden="true"></span>
      <span class="up ver" data-tauri-drag-region>v{host.appVersion}</span>
    {/if}
    {#if panel}
      <span class="vrule" aria-hidden="true"></span>
      <PanelButton {actions} {panel} />
    {/if}
    {#if frameless}
      <span class="vrule" aria-hidden="true"></span>
      <WinControls {actions} maximized={!!host?.maximized} />
    {/if}
  </div>
</header>

<style>
  .hdr {
    display: flex;
    align-items: center;
    gap: calc(14px * var(--k));
    height: calc(56px * var(--k));
    flex: none;
    padding: 0 calc(2px * var(--k));
    border-bottom: 1px solid var(--ph-rule);
  }
  .mark {
    flex: none;
    border-radius: calc(8px * var(--k));
    box-shadow: 0 0 calc(10px * var(--k)) rgba(90, 182, 235, 0.22);
  }
  .brand {
    min-width: 0;
    flex: 1;
  }
  .title {
    font-family: var(--ph-display);
    font-stretch: 125%;
    font-weight: 500;
    font-size: calc(25px * var(--k));
    line-height: 1;
    letter-spacing: 0.08em;
    color: var(--ph-cyan);
    text-shadow:
      0 0 8px rgba(127, 227, 255, 0.55),
      0 0 22px rgba(90, 182, 235, 0.3);
  }
  .tag {
    margin-top: calc(4px * var(--k));
    font-size: var(--ph-fs-xs);
    font-stretch: 87.5%;
    letter-spacing: 0.1em;
    color: var(--ph-brand);
    opacity: 0.9;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    flex: none;
    display: flex;
    align-items: center;
    gap: calc(12px * var(--k));
    white-space: nowrap;
  }
  .vrule {
    width: 1px;
    height: calc(18px * var(--k));
    background: var(--ph-rule);
  }
  .st {
    display: flex;
    align-items: center;
    gap: calc(9px * var(--k));
    font-size: var(--ph-fs-m);
    font-weight: 500;
    font-stretch: 87.5%;
    letter-spacing: 0.14em;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
  }
  .dot {
    width: calc(8px * var(--k));
    height: calc(8px * var(--k));
    min-width: 7px;
    min-height: 7px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 8px currentColor;
  }
  .dot.pulse {
    animation: ph-breathe 1.1s ease-in-out infinite;
  }
  .dot.ring {
    background: transparent;
    border: calc(2px * var(--k)) solid currentColor;
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
    .dot.pulse,
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
    font-size: var(--ph-fs-m);
    color: var(--ph-brand);
  }
  .up b {
    font-weight: 400;
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow-soft);
  }
  .up.ver {
    color: var(--ph-muted);
  }
</style>
