<script lang="ts">
  // Fault: covers the whole status block (the same outer box, so nothing below moves). What happened
  // (headline, exit code, the tier and model, how long ago), the last log lines, and beside them either the
  // load steps (died while loading) or the VRAM used over the last minutes, which shows the moment the
  // memory was released.
  import type { Session } from '../../../lib/model/types';
  import { fmtAgo } from '../util';

  let {
    session,
    label,
    history,
    totalGiB,
  }: { session: Session; label: string; history: number[]; totalGiB: number } = $props();
  const f = $derived(session.fault);
  const lines = $derived(f ? f.logTail.slice(-4) : []);
  const failed = $derived(f?.steps?.find((x) => x.state === 'failed') ?? null);
  const code = $derived.by(() => {
    if (!f) return null;
    if (f.exitCodeHex && f.exitCode !== undefined) return `${f.exitCodeHex} (${f.exitCode})`;
    if (f.exitCodeHex) return f.exitCodeHex;
    if (f.exitCode !== undefined) return String(f.exitCode);
    return null;
  });

  // VRAM history sparkline: x = the last N seconds (fixed 5-minute scale), y = 0..totalGiB.
  let sw = $state(0);
  let sh = $state(0);
  const spark = $derived.by(() => {
    const n = Math.min(300, history.length);
    if (n < 2 || sw <= 0 || sh <= 0 || totalGiB <= 0) return null;
    const pad = 5;
    const src = history.slice(-n);
    const x1 = sw - pad;
    const xOf = (i: number) => x1 - ((n - 1 - i) / 299) * (sw - 2 * pad);
    const yOf = (v: number) => pad + (1 - Math.max(0, Math.min(1, v / totalGiB))) * (sh - 2 * pad);
    let d = '';
    for (let i = 0; i < n; i++) d += (i ? 'L' : 'M') + xOf(i).toFixed(1) + ' ' + yOf(src[i]).toFixed(1);
    let peak = 0;
    for (const v of src) peak = Math.max(peak, v);
    return { d, ex: x1, ey: yOf(src[n - 1]), peak, now: src[n - 1], base: yOf(0) };
  });
</script>

<section class="fault c-panel" role="alert">
  <div class="head">
    <svg class="warn" viewBox="0 0 48 44" aria-hidden="true"
      ><path d="M24 4 45 40H3Z" /><path class="ex" d="M24 17v11" /><circle cx="24" cy="33.5" r="1.9" /></svg
    >
    <div class="txt">
      <div class="title" title={f?.title}>{f?.title ?? 'The server stopped unexpectedly.'}</div>
      <div class="sub">
        {label} · {session.model.name}{#if code}{' · exit code '}<b>{code}</b>{:else}{' · no exit code reported'}{/if}
      </div>
    </div>
    <div class="ago">{f ? fmtAgo(f.sinceS) : ''}</div>
  </div>

  <div class="grid">
    <div class="log">
      <div class="c-lbl">Last log lines</div>
      <div class="lines c-data">
        {#each lines as line, i (i)}<div title={line}>{line}</div>{/each}
      </div>
    </div>
    <div class="side">
      {#if f?.steps && f.steps.length}
        <div class="c-lbl">Load steps{#if failed}<span class="why">{` · failed at ${failed.label}`}</span>{/if}</div>
        <ol class="steps">
          {#each f.steps as st (st.id)}
            <li class={st.state}>
              <i class="mk" aria-hidden="true">{st.state === 'done' ? '✓' : st.state === 'failed' ? '✕' : '○'}</i><span>{st.label}</span>
            </li>
          {/each}
        </ol>
      {:else}
        <div class="c-lbl">
          VRAM used · last 5 min{#if spark}<span class="q">{` · peak ${spark.peak.toFixed(2)} · now ${spark.now.toFixed(2)} GiB`}</span>{/if}
        </div>
        <div class="spark" bind:clientWidth={sw} bind:clientHeight={sh}>
          {#if spark}
            <svg width={sw} height={sh} viewBox="0 0 {sw} {sh}" aria-hidden="true">
              <line class="axis" x1="0" x2={sw} y1={spark.base} y2={spark.base} />
              <path class="glow" d={spark.d} />
              <path class="line" d={spark.d} />
              <circle class="halo" cx={spark.ex} cy={spark.ey} r="7" />
              <circle class="end" cx={spark.ex} cy={spark.ey} r="4" />
            </svg>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  .fault {
    grid-row: 1 / -1;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    padding: max(9px, calc(var(--u) * 12)) max(12px, calc(var(--u) * 16));
    border-color: rgba(232, 100, 90, 0.8);
    background: linear-gradient(180deg, rgba(48, 22, 20, 0.82), rgba(21, 24, 27, 0.86) 70%);
    overflow: hidden;
  }
  .head {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    column-gap: max(10px, calc(var(--u) * 14));
    align-items: center;
    padding-bottom: max(8px, calc(var(--u) * 10));
    border-bottom: 1px solid rgba(232, 100, 90, 0.45);
  }
  .warn {
    width: max(26px, calc(var(--u) * 32));
    height: max(24px, calc(var(--u) * 29));
  }
  .warn path {
    fill: none;
    stroke: var(--danger);
    stroke-width: 3.2;
    stroke-linejoin: round;
  }
  .warn .ex {
    stroke-linecap: round;
  }
  .warn circle {
    fill: var(--danger);
  }
  .txt {
    min-width: 0;
  }
  .title {
    font-weight: 600;
    font-size: max(13.5px, calc(var(--u) * 15.5));
    line-height: 1.2;
    color: #f2f9fc;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    margin-top: max(2px, calc(var(--u) * 3));
    font-size: var(--fs-s);
    color: var(--mist);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub b {
    font-weight: 500;
    color: var(--foam);
  }
  .ago {
    align-self: start;
    font-size: var(--fs-s);
    color: var(--danger);
    white-space: nowrap;
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr);
    column-gap: max(14px, calc(var(--u) * 24));
    padding-top: max(8px, calc(var(--u) * 10));
    min-height: 0;
  }
  .log,
  .side {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .side {
    padding-left: max(14px, calc(var(--u) * 20));
    border-left: 1px solid rgba(232, 100, 90, 0.3);
  }
  .lines {
    margin-top: max(5px, calc(var(--u) * 6));
    padding-left: max(9px, calc(var(--u) * 12));
    border-left: 2px solid var(--danger);
    font-size: max(10.5px, calc(var(--u) * 10.75));
    line-height: 1.42;
    color: var(--foam);
    overflow: hidden;
  }
  .lines div {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .q {
    color: var(--muted);
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0.01em;
  }
  .why {
    color: var(--danger);
  }
  .spark {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    margin-top: max(4px, calc(var(--u) * 6));
  }
  .spark svg {
    position: absolute;
    inset: 0;
    overflow: visible;
  }
  .axis {
    stroke: var(--rule);
    stroke-width: 1;
  }
  .line {
    fill: none;
    stroke: var(--sky);
    stroke-width: 1.6;
    stroke-linejoin: round;
  }
  .glow {
    fill: none;
    stroke: var(--sky);
    stroke-opacity: 0.16;
    stroke-width: 5;
    stroke-linejoin: round;
  }
  .halo {
    fill: var(--danger);
    fill-opacity: 0.22;
  }
  .end {
    fill: var(--danger);
  }
  .steps {
    list-style: none;
    margin: max(5px, calc(var(--u) * 7)) 0 0;
    padding: 0;
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: repeat(3, auto);
    /* read down the first column, then the second (the order the steps run in) */
    grid-auto-flow: column;
    gap: max(3px, calc(var(--u) * 4)) max(12px, calc(var(--u) * 18));
    font-size: var(--fs-s);
  }
  .steps li {
    display: flex;
    align-items: baseline;
    gap: 0.5em;
    color: var(--muted);
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
  }
  .steps li.done {
    color: var(--mist);
  }
  .steps li.failed {
    color: var(--danger);
    font-weight: 600;
  }
  .steps .mk {
    font-style: normal;
    width: 1em;
    flex: none;
    text-align: center;
  }
  .steps li.done .mk {
    color: var(--sky);
  }
</style>
