<script lang="ts">
  // Fault: what happened (headline, exit code, how long ago), the last log lines, and beside them
  // either the load steps (died while loading) or the VRAM used over the last minutes, which shows the
  // moment the memory was released.
  import type { Session } from '../../../lib/model/types';
  import { fmtAgo } from '../util';

  let {
    session,
    history,
    totalGiB,
  }: { session: Session; history: number[]; totalGiB: number } = $props();
  const f = $derived(session.fault);
  const lines = $derived(f ? f.logTail.slice(-4) : []);
  const failed = $derived(f?.steps?.find((x) => x.state === 'failed') ?? null);
  const code = $derived.by(() => {
    if (!f) return null;
    if (f.exitCodeHex && f.exitCode !== undefined) return { main: f.exitCodeHex, alt: String(f.exitCode) };
    if (f.exitCodeHex) return { main: f.exitCodeHex, alt: '' };
    if (f.exitCode !== undefined) return { main: String(f.exitCode), alt: '' };
    return null;
  });

  // VRAM history sparkline: x = the last N seconds (fixed 5-minute scale), y = 0..totalGiB.
  let sw = $state(0);
  let sh = $state(0);
  const spark = $derived.by(() => {
    const n = Math.min(300, history.length);
    if (n < 2 || sw <= 0 || sh <= 0 || totalGiB <= 0) return null;
    const pad = 6;
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

<section class="fault">
  <div class="head">
    <svg class="warn" viewBox="0 0 48 44" aria-hidden="true"
      ><path d="M24 4 45 40H3Z" /><path class="ex" d="M24 17v11" /><circle cx="24" cy="33.5" r="1.9" /></svg
    >
    <div class="txt">
      <h2 class="title">{f?.title ?? 'The server stopped unexpectedly.'}</h2>
      <div class="sub">
        {#if code}<span>exit code <span class="c-data v">{code.main}</span>{#if code.alt}<span class="c-data alt">{` (${code.alt})`}</span>{/if}</span
          ><span class="sep">·</span>{/if}<span>{f ? fmtAgo(f.sinceS) : ''}</span>
      </div>
    </div>
  </div>

  <div class="grid">
    <div class="log">
      <div class="c-lbl">Last log lines</div>
      <pre class="lines c-data">{lines.join('\n')}</pre>
    </div>
    <div class="side">
      {#if f?.steps && f.steps.length}
        <div class="c-lbl">Load steps</div>
        <ol class="steps" style:grid-template-rows="repeat({Math.ceil(f.steps.length / 2)}, auto)">
          {#each f.steps as st (st.id)}
            <li class={st.state}>
              <i class="mk" aria-hidden="true">{st.state === 'done' ? '✓' : st.state === 'failed' ? '✕' : '○'}</i><span class="l"
                >{st.label}</span
              >
            </li>
          {/each}
        </ol>
        {#if failed}
          <div class="fwhy c-data">failed at {failed.label}{#if failed.detail}{`: ${failed.detail}`}{/if}</div>
        {/if}
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
              <circle class="halo" cx={spark.ex} cy={spark.ey} r="8" />
              <circle class="end" cx={spark.ex} cy={spark.ey} r="4.5" />
            </svg>
          {/if}
        </div>
        {#if spark}<div class="scale c-data">scale 0–{totalGiB.toFixed(2)} GiB</div>{/if}
      {/if}
    </div>
  </div>
</section>

<style>
  .fault {
    padding-top: max(10px, calc(var(--u) * 18));
    padding-bottom: max(10px, calc(var(--u) * 16));
  }
  .head {
    display: flex;
    align-items: flex-start;
    gap: max(12px, calc(var(--u) * 20));
  }
  .warn {
    flex: none;
    width: max(36px, calc(var(--u) * 54));
    height: max(33px, calc(var(--u) * 50));
    margin-top: max(1px, calc(var(--u) * 2));
  }
  .warn path {
    fill: none;
    stroke: var(--amber);
    stroke-width: 3.2;
    stroke-linejoin: round;
  }
  .warn .ex {
    stroke-linecap: round;
  }
  .warn circle {
    fill: var(--amber);
  }
  .txt {
    min-width: 0;
  }
  .title {
    margin: 0;
    font-family: var(--f-disp);
    font-stretch: 112%;
    font-weight: 800;
    font-size: max(24px, calc(var(--u) * 38));
    line-height: 1.08;
    letter-spacing: -0.005em;
    color: #f2f9fc;
    text-wrap: balance;
  }
  .sub {
    margin-top: max(5px, calc(var(--u) * 8));
    font-size: max(13.5px, calc(var(--u) * 19));
    color: #c3d3db;
  }
  .sub .v {
    color: var(--foam);
  }
  .sub .alt {
    color: var(--muted);
    font-size: 0.88em;
  }
  .sep {
    margin: 0 0.6em;
    color: var(--muted);
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1.15fr) minmax(0, 1fr);
    column-gap: max(18px, calc(var(--u) * 40));
    margin-top: max(10px, calc(var(--u) * 16));
  }
  .lines {
    margin: max(6px, calc(var(--u) * 9)) 0 0;
    padding: 1px 0 1px max(12px, calc(var(--u) * 18));
    border-left: 3px solid var(--amber);
    font-size: max(11.5px, calc(var(--u) * 14.5));
    line-height: 1.5;
    color: var(--foam);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .side {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .q {
    color: var(--muted);
  }
  .spark {
    position: relative;
    flex: 1 1 auto;
    min-height: max(56px, calc(var(--u) * 76));
    margin-top: max(4px, calc(var(--u) * 6));
  }
  .spark svg {
    position: absolute;
    inset: 0;
    overflow: visible;
  }
  .scale {
    margin-top: max(3px, calc(var(--u) * 5));
    text-align: right;
    font-size: max(10.5px, calc(var(--u) * 12.5));
    color: var(--muted);
  }
  .axis {
    stroke: var(--rule);
    stroke-width: 1;
  }
  .line {
    fill: none;
    stroke: var(--sky);
    stroke-width: 1.8;
    stroke-linejoin: round;
  }
  .glow {
    fill: none;
    stroke: var(--sky);
    stroke-opacity: 0.16;
    stroke-width: 6;
    stroke-linejoin: round;
  }
  .halo {
    fill: var(--amber);
    fill-opacity: 0.22;
  }
  .end {
    fill: var(--amber);
  }
  .steps {
    list-style: none;
    margin: max(6px, calc(var(--u) * 9)) 0 0;
    padding: 0;
    display: grid;
    grid-template-columns: 1fr 1fr;
    /* read down the first column, then the second (the order the steps run in) */
    grid-auto-flow: column;
    gap: max(4px, calc(var(--u) * 6)) max(12px, calc(var(--u) * 20));
    font-size: max(12px, calc(var(--u) * 14.5));
  }
  .steps li {
    display: flex;
    align-items: baseline;
    gap: 0.5em;
    color: var(--muted);
    min-width: 0;
    white-space: nowrap;
  }
  .steps li.done {
    color: #c3d3db;
  }
  .steps li.failed {
    color: var(--amber);
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
  .fwhy {
    margin-top: max(6px, calc(var(--u) * 9));
    font-size: max(11.5px, calc(var(--u) * 13.5));
    color: var(--amber);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
