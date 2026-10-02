<script lang="ts">
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtClock } from '../../../lib/model/format';
  import { gpuDetail, gpuStatus, type GpuView } from '../power';
  import { statusOf } from '../util';

  let { vm, actions, gpu = null }: { vm: ViewModel; actions: Actions; gpu?: GpuView | null } = $props();
  const st = $derived(statusOf(vm));
  const s = $derived(vm.session);
  const booting = $derived(!!s && (s.phase === 'starting' || s.phase === 'loading'));
  /** While loading the clock is the load's own elapsed time (the mockup's "elapsed"). */
  const clock = $derived(
    s ? (booting && s.loading ? { k: 'elapsed', v: s.loading.elapsedS } : { k: 'uptime', v: s.uptimeS }) : null,
  );
  const frameless = $derived(!!vm.host?.frameless);
  const maximized = $derived(!!vm.host?.maximized);
  /** Panel mode (the read-only mini layout on the small screen): offered whenever the host can do it. */
  const panel = $derived(vm.host?.panel);
  const panelTip = $derived(`Panel mode: show on the small screen${panel?.target ? ` (${panel.target})` : ''}`);
</script>

<header class="hdr" class:frameless class:has-panel={!!panel?.available} data-tauri-drag-region>
  <img class="mark" src="/koksny-mark.png" alt="" draggable="false" data-tauri-drag-region />
  <div class="brand" data-tauri-drag-region>
    <span class="word" data-tauri-drag-region>KLIF</span>
    <span class="tag" data-tauri-drag-region><span class="dom">Koksny.com</span> LOCAL INFERENCE FORNICATOR</span>
  </div>
  <div class="right" data-tauri-drag-region>
    <span class="stw">
      <span class="status {st.tone}">
        <!-- the dot re-pulses on every telemetry snapshot (vm.now): a real heartbeat -->
        {#key vm.now}<i class="dot"></i>{/key}
        <span class="st">{st.text}</span>
      </span>
      {#if gpu}
        <!-- the inference GPU is powered down (or coming back): amber, next to the session's status -->
        <span class="gpu {gpu.phase}" title={gpuDetail(gpu)}>
          <svg class="gi" viewBox="0 0 20 20" aria-hidden="true">
            {#if gpu.phase === 'waking'}
              <!-- waking: the disc fills in as the VRAM comes back (area follows the resident fraction) -->
              <circle class="ring" cx="10" cy="10" r="7.6" />
              <circle class="fill" cx="10" cy="10" r={(7.6 * Math.sqrt(gpu.frac)).toFixed(2)} />
            {:else}
              <path d="M12.6 2.6A7.6 7.6 0 1 0 17.4 12.6 6 6 0 0 1 12.6 2.6Z" />
            {/if}
          </svg>
          <span class="st">{gpuStatus(gpu)}</span>
        </span>
      {/if}
    </span>
    {#if clock}
      <span class="up"><span class="k">{clock.k}</span><span class="v">{fmtClock(clock.v)}</span></span>
    {/if}
    {#if panel?.available}
      <button class="pb" title={panelTip} aria-label={panelTip} onclick={() => actions.togglePanel?.()}>
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
    gap: max(10px, calc(var(--u) * 16));
    padding-top: max(7px, calc(var(--u) * 9));
    min-width: 0;
  }
  .mark {
    width: max(40px, calc(var(--u) * 58));
    height: max(40px, calc(var(--u) * 58));
    flex: none;
    border-radius: 22%;
    box-shadow: 0 0 0 1px rgba(220, 239, 248, 0.06);
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: max(10px, calc(var(--u) * 18));
    min-width: 0;
  }
  .word {
    font-family: var(--f-disp);
    font-stretch: 125%;
    font-weight: 800;
    font-size: max(36px, calc(var(--u) * 56));
    line-height: 1;
    letter-spacing: -0.005em;
    color: var(--foam);
  }
  .tag {
    font-family: var(--f-ui);
    font-weight: 500;
    font-size: max(11px, calc(var(--u) * 14.5));
    letter-spacing: 0.04em;
    color: #b4c6cf;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .dom {
    font-weight: 400;
    letter-spacing: 0.01em;
    margin-right: 0.25em;
  }
  .right {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: max(18px, calc(var(--u) * 36));
    flex: none;
    align-self: flex-start;
    padding-top: max(6px, calc(var(--u) * 14));
  }
  /* the panel chip eats into the tagline's room: tighter spacing in the right cluster, and (like with
     window controls) the tagline drops under the wordmark rather than being cut off */
  .has-panel .brand {
    flex-wrap: wrap;
    row-gap: 0;
  }
  .has-panel .right {
    gap: max(14px, calc(var(--u) * 22));
  }
  /* with window controls a narrow window cannot hold the tagline beside the wordmark: it goes under it */
  .frameless .brand {
    flex-wrap: wrap;
    row-gap: 0;
  }
  .frameless .right {
    gap: max(12px, calc(var(--u) * 18));
    padding-top: max(4px, calc(var(--u) * 6));
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: max(8px, calc(var(--u) * 11));
    font-family: var(--f-disp);
    font-stretch: 112%;
    font-weight: 600;
    font-size: max(15px, calc(var(--u) * 20.5));
    letter-spacing: 0.07em;
    color: var(--sky);
  }
  .dot {
    width: max(12px, calc(var(--u) * 16));
    height: max(12px, calc(var(--u) * 16));
    border-radius: 50%;
    background: var(--sky);
    animation: beat 700ms ease-out 1;
  }
  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
  @keyframes beat {
    0% {
      opacity: 0.45;
    }
    100% {
      opacity: 1;
    }
  }
  /* starting / loading: amber, like the approved loading mockup */
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
    box-shadow: inset 0 0 0 2px var(--muted);
  }
  .fault {
    color: var(--amber);
  }
  .fault .dot {
    background: var(--amber);
  }
  /* GPU asleep / waking: the status's type in amber, a crescent (or a filling disc) for a mark. It hangs
     under the status row (out of the flow) so the wordmark and the tagline keep their room. */
  .stw {
    position: relative;
    display: inline-flex;
  }
  .gpu {
    position: absolute;
    left: 0;
    top: 100%;
    margin-top: calc(var(--u) * 3);
    display: inline-flex;
    align-items: center;
    gap: max(7px, calc(var(--u) * 9));
    font-family: var(--f-disp);
    font-stretch: 112%;
    font-weight: 600;
    font-size: max(15px, calc(var(--u) * 20.5));
    letter-spacing: 0.07em;
    color: var(--amber);
    white-space: nowrap;
  }
  .gi {
    width: max(14px, calc(var(--u) * 19));
    height: max(14px, calc(var(--u) * 19));
    flex: none;
    fill: var(--amber);
  }
  .gi .ring {
    fill: none;
    stroke: var(--amber);
    stroke-width: 1.6;
  }
  .up {
    display: inline-flex;
    align-items: baseline;
    gap: 0.5em;
    font-size: max(13px, calc(var(--u) * 16.5));
    white-space: nowrap;
  }
  .up .k {
    font-family: var(--f-ui);
    color: #c3d3db;
  }
  .up .v {
    font-family: var(--f-data);
    font-weight: 500;
    letter-spacing: 0.02em;
    color: var(--foam);
  }

  /* panel mode: the same slate chip as the action bar, scaled down to sit beside the status */
  .pb {
    display: inline-flex;
    align-items: center;
    gap: max(6px, calc(var(--u) * 9));
    align-self: center;
    height: max(28px, calc(var(--u) * 34));
    padding: 0 max(10px, calc(var(--u) * 14)) 0 max(8px, calc(var(--u) * 11));
    border-radius: 8px;
    border: 1px solid #2a353b;
    background: var(--slate);
    color: #c3d3db;
    font-family: var(--f-ui);
    font-weight: 500;
    font-size: max(12px, calc(var(--u) * 15.5));
    letter-spacing: 0.06em;
    white-space: nowrap;
  }
  .pb:hover {
    background: var(--slate-2);
    border-color: #36434a;
    color: var(--foam);
  }
  .pb svg {
    width: max(15px, calc(var(--u) * 20));
    height: max(15px, calc(var(--u) * 20));
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.7;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  /* too narrow for the word: the glyph and its tooltip carry it */
  @container (max-width: 900px) {
    .pl {
      display: none;
    }
    .pb {
      padding: 0 max(8px, calc(var(--u) * 11));
    }
  }

  /* window chrome (frameless host only): quiet line icons, close turns red on hover */
  .wc {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding-left: max(8px, calc(var(--u) * 10));
    border-left: 1px solid var(--rule);
    align-self: center;
  }
  .wb {
    display: inline-grid;
    place-items: center;
    width: max(28px, calc(var(--u) * 32));
    height: max(26px, calc(var(--u) * 30));
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
    width: max(13px, calc(var(--u) * 15));
    height: max(13px, calc(var(--u) * 15));
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
