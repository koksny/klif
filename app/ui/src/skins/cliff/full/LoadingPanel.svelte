<script lang="ts">
  // Starting / loading: where the live hero will be, a dim "not measured yet" placeholder (so the
  // layout does not jump when the session goes live), then the load steps and the overall progress.
  import type { Session, SlotKind } from '../../../lib/model/types';
  import { fmtSeconds } from '../../../lib/model/format';
  import { detailFraction } from '../util';

  let { session, label, kind }: { session: Session; label: string; kind: SlotKind } = $props();
  const lp = $derived(session.loading);
  const frac = $derived(lp ? Math.max(0, Math.min(1, lp.fraction)) : 0);
  const verb = $derived(session.phase === 'starting' ? 'Starting' : 'Loading');
</script>

<section class="ph">
  <div class="c-lbl">{kind === 'image' ? 'Current job' : 'Decode speed'}<span class="q">{' · not running yet'}</span></div>
  <div class="row">
    <span class="dash">—</span><span class="unit">{kind === 'image' ? 's/it' : 'tok/s'}</span>
    <span class="flat" aria-hidden="true"></span>
  </div>
</section>

<div class="c-rule"></div>

<section class="load">
  <div class="grid">
    <div class="lhs">
      <div class="c-lbl">{verb} {label}</div>
      {#if lp && lp.steps.length}
        <ol class="steps">
          {#each lp.steps as st (st.id)}
            {@const f = st.state === 'active' ? detailFraction(st.detail) : null}
            <li class={st.state}>
              <i class="mk" aria-hidden="true"
                >{#if st.state === 'done'}<svg viewBox="0 0 16 16"><path d="M4.2 8.4l2.5 2.4 5-5.4" /></svg>{/if}</i
              >
              <span class="l">{st.label}</span>{#if st.detail}<span class="d c-data">{st.detail}</span>{/if}
              {#if f !== null}
                <span class="sbar" role="meter" aria-label="{st.label} progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(f * 100)}
                  ><i style:transform="scaleX({f})"></i></span
                >
              {/if}
            </li>
          {/each}
        </ol>
      {:else}
        <div class="none">waiting for the process…</div>
      {/if}
    </div>
    <div class="vr" aria-hidden="true"></div>
    <div class="rhs">
      <div class="big" role="meter" aria-label="Overall load progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(frac * 100)}>
        {lp ? Math.round(frac * 100) : '—'}<span class="pct">{lp ? '%' : ''}</span>
      </div>
      <div class="ov">Overall</div>
      <div class="bar" aria-hidden="true"><i style:transform="scaleX({frac})"></i></div>
      {#if lp}<div class="el c-data">{fmtSeconds(lp.elapsedS)} elapsed</div>{/if}
    </div>
  </div>
</section>

<style>
  .ph {
    padding-top: max(7px, calc(var(--u) * 12));
    padding-bottom: max(6px, calc(var(--u) * 10));
  }
  .q {
    color: var(--muted);
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: max(10px, calc(var(--u) * 16));
    margin-top: max(1px, calc(var(--u) * 2));
  }
  .dash {
    font-family: var(--f-disp);
    font-stretch: 100%;
    font-weight: 300;
    font-size: max(34px, calc(var(--u) * 50));
    line-height: 1;
    color: #56666f;
  }
  .unit {
    font-family: var(--f-disp);
    font-weight: 600;
    font-size: max(20px, calc(var(--u) * 30));
    color: #4f6b7c;
  }
  .flat {
    flex: 1 1 auto;
    align-self: center;
    height: 0;
    margin-left: max(8px, calc(var(--u) * 16));
    border-top: 1px dashed #3a474e;
  }

  .load {
    padding-top: max(8px, calc(var(--u) * 13));
    padding-bottom: max(10px, calc(var(--u) * 16));
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1px minmax(0, 0.42fr);
    column-gap: max(18px, calc(var(--u) * 40));
  }
  .vr {
    background: var(--rule);
  }
  .steps {
    list-style: none;
    margin: max(6px, calc(var(--u) * 10)) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: max(5px, calc(var(--u) * 8));
    font-size: max(12.5px, calc(var(--u) * 16.5));
  }
  li {
    position: relative;
    display: grid;
    grid-template-columns: auto auto 1fr;
    align-items: center;
    column-gap: max(10px, calc(var(--u) * 16));
    color: var(--muted);
    min-width: 0;
  }
  /* the thread joining the markers (the markers sit on top of it) */
  .steps {
    position: relative;
  }
  .steps::before {
    content: '';
    position: absolute;
    left: calc(max(16px, calc(var(--u) * 21)) / 2 - 1px);
    top: calc(max(16px, calc(var(--u) * 21)) / 2);
    bottom: calc(max(16px, calc(var(--u) * 21)) / 2);
    width: 2px;
    background: #33424a;
  }
  .mk {
    position: relative;
    background: var(--basalt);
    width: max(16px, calc(var(--u) * 21));
    height: max(16px, calc(var(--u) * 21));
    border-radius: 50%;
    box-shadow: inset 0 0 0 1.5px #5d6e77;
    display: grid;
    place-items: center;
  }
  .mk svg {
    width: 80%;
    height: 80%;
    fill: none;
    stroke: var(--basalt);
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  li.done {
    color: #c3d3db;
  }
  li.done .mk {
    background: var(--sky);
    box-shadow: none;
  }
  li.active {
    color: var(--foam);
  }
  li.active .mk {
    box-shadow:
      inset 0 0 0 2.5px var(--sky),
      0 0 10px rgba(90, 182, 235, 0.45);
  }
  li.failed {
    color: var(--amber);
  }
  .l {
    white-space: nowrap;
  }
  .d {
    font-size: 0.9em;
    color: #9fb2bc;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  li.active .d {
    color: var(--foam);
  }
  .sbar {
    grid-column: 2 / 4;
    position: relative;
    height: max(6px, calc(var(--u) * 8));
    margin: max(3px, calc(var(--u) * 5)) 0 max(2px, calc(var(--u) * 3));
    border-radius: 99px;
    background: var(--s1);
    overflow: hidden;
    max-width: 92%;
  }
  .sbar i,
  .bar i {
    position: absolute;
    inset: 0;
    background: var(--sky);
    border-radius: 99px;
    transform-origin: left;
    transition: transform 400ms ease-out;
  }
  .none {
    margin-top: 10px;
    font-size: max(12.5px, calc(var(--u) * 16));
    color: var(--muted);
  }
  .rhs {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: flex-start;
    min-width: 0;
  }
  .big {
    font-family: var(--f-disp);
    font-stretch: 125%;
    font-weight: 800;
    font-size: max(54px, calc(var(--u) * 92));
    line-height: 0.95;
    letter-spacing: -0.01em;
    color: #f2f9fc;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .pct {
    font-size: 0.62em;
    margin-left: 0.04em;
    color: var(--sky);
  }
  .ov {
    margin-top: max(2px, calc(var(--u) * 4));
    font-family: var(--f-ui);
    font-weight: 500;
    font-size: max(14px, calc(var(--u) * 19));
    color: var(--foam);
  }
  .bar {
    position: relative;
    align-self: stretch;
    height: max(6px, calc(var(--u) * 8));
    margin-top: max(8px, calc(var(--u) * 12));
    border-radius: 99px;
    background: var(--s1);
    overflow: hidden;
  }
  .el {
    margin-top: max(6px, calc(var(--u) * 9));
    font-size: max(12px, calc(var(--u) * 14.5));
    color: #9fb2bc;
  }
</style>
