<script lang="ts">
  // Starting / loading: the hero is the startup progress (overall %, the active step, the overall bar, the
  // VRAM filled so far and the elapsed time); the six startup steps fill the two detail rows, three each,
  // in the order they run.
  import type { Session } from '../../../lib/model/types';
  import { fmtClock, fmtGiB } from '../../../lib/model/format';
  import { detailFraction } from '../util';

  let { session, usedGiB, totalGiB }: { session: Session; usedGiB: number; totalGiB: number } = $props();
  const lp = $derived(session.loading);
  const frac = $derived(lp ? Math.max(0, Math.min(1, lp.fraction)) : 0);
  const steps = $derived(lp?.steps ?? []);
  const active = $derived(steps.find((x) => x.state === 'active') ?? null);
  const activeFrac = $derived(active ? detailFraction(active.detail) : null);
</script>

<!-- Hero -->
<section class="c-hero c-panel">
  <div class="c-hl amb">
    <span class="c-lbl">{session.phase === 'starting' ? 'Starting' : 'Startup'}</span>
    <div class="c-fig" class:dim={!lp}><b>{lp ? Math.floor(frac * 100) : '—'}</b><span class="u">%</span></div>
  </div>
  <div class="c-hr">
    <div class="c-hline">
      <span><b>{active?.label ?? (session.phase === 'starting' ? 'Starting the process' : 'Waiting for the process')}</b>{active?.detail ? ` · ${active.detail}` : ''}</span>
      <span class="end">elapsed <b>{fmtClock(lp?.elapsedS ?? session.uptimeS)}</b></span>
    </div>
    <span class="c-bar tall" role="meter" aria-label="Overall startup progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(frac * 100)}>
      <i style:transform="scaleX({frac})"></i>
    </span>
    <div class="c-hline sm">
      <span>VRAM so far {fmtGiB(usedGiB)} / {fmtGiB(totalGiB)} GiB</span>
      {#if activeFrac !== null}<span class="end">{active?.label.toLowerCase()} {Math.round(activeFrac * 100)}%</span>{/if}
    </div>
  </div>
</section>

<!-- Rows: the six startup steps -->
<section class="c-rows c-panel steps">
  {#each [steps.slice(0, 3), steps.slice(3, 6)] as line, li (li)}
    <div class="srow">
      {#each line as st (st.id)}
        <div class="sc {st.state}">
          <i class="mk" aria-hidden="true"
            >{#if st.state === 'done'}<svg viewBox="0 0 16 16"><path d="M4.2 8.4l2.5 2.4 5-5.4" /></svg>{/if}</i
          >
          <span class="l">{st.label}</span>{#if st.detail}<span class="d">{st.detail}</span>{/if}
        </div>
      {:else}
        <div class="sc pending"><span class="l">{li ? '' : 'waiting for the process…'}</span></div>
      {/each}
    </div>
  {/each}
</section>

<style>
  .srow {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    min-height: 0;
  }
  .srow + .srow {
    border-top: 1px solid var(--edge);
  }
  .sc {
    display: flex;
    align-items: center;
    gap: max(8px, calc(var(--u) * 10));
    padding: 0 max(12px, calc(var(--u) * 16));
    min-width: 0;
    font-size: var(--fs-m);
    color: var(--muted);
    white-space: nowrap;
    border-left: 1px solid var(--edge);
  }
  .sc:first-child {
    border-left: 0;
  }
  .sc.done {
    color: var(--mist);
  }
  .sc.active {
    color: var(--foam);
  }
  .sc.failed {
    color: var(--danger);
  }
  .l {
    flex: none;
  }
  .active .l {
    font-weight: 500;
  }
  .d {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--fs-s);
    color: #8a9ca5;
  }
  .active .d {
    color: var(--mist);
  }
  .mk {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: max(13px, calc(var(--u) * 15));
    height: max(13px, calc(var(--u) * 15));
    border-radius: 50%;
    box-shadow: inset 0 0 0 1.5px #56666f;
  }
  .mk svg {
    width: 82%;
    height: 82%;
    fill: none;
    stroke: var(--basalt);
    stroke-width: 2.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .done .mk {
    background: var(--sky);
    box-shadow: none;
  }
  .active .mk {
    box-shadow:
      inset 0 0 0 2px var(--sky),
      0 0 8px rgba(90, 182, 235, 0.45);
  }
  .active .mk::after {
    content: '';
    position: absolute;
    inset: 30%;
    border-radius: 50%;
    background: var(--sky);
    animation: pulse 1.6s ease-in-out infinite;
  }
  .failed .mk {
    box-shadow: inset 0 0 0 1.5px var(--danger);
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }
</style>
