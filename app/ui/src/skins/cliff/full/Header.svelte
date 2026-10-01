<script lang="ts">
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtClock } from '../../../lib/model/format';
  import { statusOf } from '../util';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();
  const st = $derived(statusOf(vm));
  const s = $derived(vm.session);
  const booting = $derived(!!s && (s.phase === 'starting' || s.phase === 'loading'));
  /** While loading the clock is the load's own elapsed time (the mockup's "elapsed"). */
  const clock = $derived(
    s ? (booting && s.loading ? { k: 'elapsed', v: s.loading.elapsedS } : { k: 'uptime', v: s.uptimeS }) : null,
  );
  const frameless = $derived(!!vm.host?.frameless);
  const maximized = $derived(!!vm.host?.maximized);
</script>

<header class="hdr" class:frameless data-tauri-drag-region>
  <img class="mark" src="/koksny-mark.png" alt="" draggable="false" data-tauri-drag-region />
  <div class="brand" data-tauri-drag-region>
    <span class="word" data-tauri-drag-region>KLIF</span>
    <span class="tag" data-tauri-drag-region><span class="dom">Koksny.com</span> LOCAL INFERENCE FORNICATOR</span>
  </div>
  <div class="right" data-tauri-drag-region>
    <span class="status {st.tone}">
      <!-- the dot re-pulses on every telemetry snapshot (vm.now): a real heartbeat -->
      {#key vm.now}<i class="dot"></i>{/key}
      <span class="st">{st.text}</span>
    </span>
    {#if clock}
      <span class="up"><span class="k">{clock.k}</span><span class="v">{fmtClock(clock.v)}</span></span>
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
