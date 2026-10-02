<script lang="ts">
  // One compact row: mark, wordmark and tagline; then the session's status (and the GPU's while it is
  // dormant), uptime / elapsed or the version when idle, panel mode, and the window controls when frameless.
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtClock } from '../../../lib/model/format';
  import { gpuDetail, gpuStatus, type GpuView } from '../power';
  import { statusOf } from '../util';

  let { vm, actions, gpu = null }: { vm: ViewModel; actions: Actions; gpu?: GpuView | null } = $props();
  const st = $derived(statusOf(vm));
  const s = $derived(vm.session);
  const booting = $derived(!!s && (s.phase === 'starting' || s.phase === 'loading'));
  /** While loading the clock is the load's own elapsed time. */
  const clock = $derived(
    s ? (booting && s.loading ? { k: 'elapsed', v: s.loading.elapsedS } : { k: 'uptime', v: s.uptimeS }) : null,
  );
  const frameless = $derived(!!vm.host?.frameless);
  const maximized = $derived(!!vm.host?.maximized);
  /** Panel mode (the read-only mini layout on the small screen): offered whenever the host can do it. */
  const panel = $derived(vm.host?.panel);
  const panelTip = $derived(`Panel mode: show on the small screen${panel?.target ? ` (${panel.target})` : ''}`);
</script>

<header class="hdr" class:frameless data-tauri-drag-region>
  <img class="mark" src="/koksny-mark.png" alt="" draggable="false" data-tauri-drag-region />
  <div class="brand" data-tauri-drag-region>
    <span class="word" data-tauri-drag-region>KLIF</span>
    <span class="tag" data-tauri-drag-region>Koksny.com LOCAL INFERENCE FORNICATOR</span>
  </div>
  <div class="right" data-tauri-drag-region>
    <span class="status {st.tone}">
      <!-- the dot re-pulses on every telemetry snapshot (vm.now): a real heartbeat -->
      {#key vm.now}<i class="dot"></i>{/key}
      <span>{st.text}</span>
    </span>
    {#if gpu}
      <!-- the inference GPU is powered down (or coming back): amber, beside the session's status -->
      <span class="gpu" title={gpuDetail(gpu)}>
        <svg class="gi" viewBox="0 0 20 20" aria-hidden="true">
          {#if gpu.phase === 'waking'}
            <!-- waking: the disc fills in as the VRAM comes back (area follows the resident fraction) -->
            <circle class="ring" cx="10" cy="10" r="7.6" />
            <circle cx="10" cy="10" r={(7.6 * Math.sqrt(gpu.frac)).toFixed(2)} />
          {:else}
            <path d="M12.6 2.6A7.6 7.6 0 1 0 17.4 12.6 6 6 0 0 1 12.6 2.6Z" />
          {/if}
        </svg>
        <span>{gpuStatus(gpu)}</span>
      </span>
    {/if}
    <span class="vr" aria-hidden="true"></span>
    {#if clock}
      <span class="up"><span class="k">{clock.k}</span><span class="v">{fmtClock(clock.v)}</span></span>
    {:else if vm.host?.appVersion}
      <span class="up"><span class="k">v{vm.host.appVersion}</span></span>
    {/if}
    {#if panel?.available}
      <button class="pb" class:on={panel.active} title={panelTip} aria-label={panelTip} aria-pressed={panel.active} onclick={() => actions.togglePanel?.()}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2.5" y="3.5" width="15" height="10" rx="1.6" /><path d="M7.5 17h5M10 13.5V17" /></svg>
        <span class="pl">Panel</span>
      </button>
    {/if}
    {#if frameless}
      <span class="wc" role="group" aria-label="Window">
        <button class="wb" title="Minimize" aria-label="Minimize" onclick={() => actions.minimize?.()}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 8.5h9" /></svg>
        </button>
        <button
          class="wb"
          title={maximized ? 'Restore' : 'Maximize'}
          aria-label={maximized ? 'Restore' : 'Maximize'}
          onclick={() => actions.toggleMaximize?.()}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            {#if maximized}<path d="M5.5 3.5h7v7M3.5 5.5h7v7h-7z" />{:else}<path d="M3.5 3.5h9v9h-9z" />{/if}
          </svg>
        </button>
        <button class="wb close" title="Close" aria-label="Close" onclick={() => actions.closeWindow?.()}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" /></svg>
        </button>
      </span>
    {/if}
  </div>
</header>

<style>
  .hdr {
    display: flex;
    align-items: center;
    gap: max(10px, calc(var(--u) * 12));
    height: max(44px, calc(var(--u) * 52));
    min-width: 0;
    border-bottom: 1px solid var(--rule);
  }
  .mark {
    width: max(28px, calc(var(--u) * 34));
    height: max(28px, calc(var(--u) * 34));
    flex: none;
    border-radius: 22%;
    box-shadow: 0 0 0 1px rgba(220, 239, 248, 0.06);
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: max(10px, calc(var(--u) * 14));
    min-width: 0;
  }
  .word {
    font-family: var(--f-disp);
    font-stretch: 125%;
    font-weight: 700;
    font-size: max(22px, calc(var(--u) * 26));
    line-height: 1;
    letter-spacing: 0.02em;
    color: var(--foam);
  }
  .tag {
    font-family: var(--f-ui);
    font-weight: 500;
    font-size: var(--fs-xs);
    letter-spacing: 0.08em;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .right {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: max(10px, calc(var(--u) * 14));
    flex: none;
    white-space: nowrap;
  }
  .status,
  .gpu {
    display: inline-flex;
    align-items: center;
    gap: max(7px, calc(var(--u) * 8));
    font-family: var(--f-ui);
    font-weight: 600;
    font-size: var(--fs-s);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--sky);
  }
  .dot {
    width: max(8px, calc(var(--u) * 9));
    height: max(8px, calc(var(--u) * 9));
    border-radius: 50%;
    background: var(--sky);
    animation: beat 700ms ease-out 1;
  }
  @keyframes beat {
    0% {
      opacity: 0.45;
    }
    100% {
      opacity: 1;
    }
  }
  /* starting / loading: amber */
  .busy {
    color: var(--amber);
  }
  .busy .dot {
    background: var(--amber);
  }
  .idle,
  .stop {
    color: var(--muted);
  }
  .idle .dot,
  .stop .dot {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--muted);
  }
  .fault {
    color: var(--danger);
  }
  .fault .dot {
    background: var(--danger);
  }
  /* GPU asleep / waking: amber, a crescent (or a filling disc) for a mark */
  .gpu {
    color: var(--amber);
  }
  .gi {
    width: max(12px, calc(var(--u) * 14));
    height: max(12px, calc(var(--u) * 14));
    flex: none;
    fill: var(--amber);
  }
  .gi .ring {
    fill: none;
    stroke: var(--amber);
    stroke-width: 1.6;
  }
  .vr {
    width: 1px;
    height: max(14px, calc(var(--u) * 18));
    background: var(--s1);
  }
  .up {
    display: inline-flex;
    align-items: baseline;
    gap: 0.5em;
    font-size: var(--fs-m);
  }
  .up .k {
    color: var(--muted);
  }
  .up .v {
    font-weight: 500;
    color: var(--foam);
  }

  /* panel mode: the same slate chip as the action bar */
  .pb {
    display: inline-flex;
    align-items: center;
    gap: max(6px, calc(var(--u) * 7));
    height: max(26px, calc(var(--u) * 28));
    padding: 0 max(9px, calc(var(--u) * 10)) 0 max(7px, calc(var(--u) * 8));
    border-radius: 6px;
    border: 1px solid var(--edge);
    background: var(--slate);
    color: var(--mist);
    font-family: var(--f-ui);
    font-weight: 600;
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }
  .pb:hover {
    background: var(--slate-2);
    border-color: #36434a;
    color: var(--foam);
  }
  .pb.on {
    border-color: var(--sky);
    color: var(--sky);
  }
  .pb svg {
    width: max(13px, calc(var(--u) * 15));
    height: max(13px, calc(var(--u) * 15));
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  /* too narrow for the word: the glyph and its tooltip carry it */
  @container (max-width: 760px) {
    .pl {
      display: none;
    }
  }

  /* window chrome (frameless host only): quiet line icons, close turns red on hover */
  .wc {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding-left: max(6px, calc(var(--u) * 8));
    border-left: 1px solid var(--rule);
  }
  .wb {
    display: inline-grid;
    place-items: center;
    width: max(28px, calc(var(--u) * 30));
    height: max(26px, calc(var(--u) * 28));
    border-radius: 6px;
    color: var(--muted);
  }
  .wb:hover {
    background: rgba(220, 239, 248, 0.07);
    color: var(--foam);
  }
  .wb.close:hover {
    background: rgba(232, 100, 90, 0.85);
    color: #fff;
  }
  .wb svg {
    width: max(12px, calc(var(--u) * 13));
    height: max(12px, calc(var(--u) * 13));
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
