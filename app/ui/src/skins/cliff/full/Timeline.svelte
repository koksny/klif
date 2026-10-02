<script lang="ts">
  // Request timeline: the last requests as bars on one shared time scale (bar width = prefillS + decodeS,
  // split into its prefill and decode parts; the span is stated in the caption). The frame is there in
  // every phase: empty while nothing runs (the last session in the caption), with the request that died
  // marked after a fault.
  import type { RequestRecord } from '../../../lib/model/types';
  import { fmtInt } from '../../../lib/model/format';
  import type { BlockView } from '../util';

  let {
    requests,
    view,
    hadWork = false,
    died = false,
    lastText = '',
    lastFault = false,
  }: {
    requests: RequestRecord[];
    view: BlockView;
    /** Fault: the server was serving (it had reached live) when it died. */
    hadWork?: boolean;
    /** Fault: a request was in flight when the server died. */
    died?: boolean;
    /** Idle: the previous session in one line. */
    lastText?: string;
    lastFault?: boolean;
  } = $props();

  const MAX = 8;
  const last = $derived(view === 'idle' || view === 'loading' ? [] : requests.slice(view === 'fault' ? -(MAX - 1) : -MAX));
  const sum = $derived(last.reduce((a, r) => a + r.prefillS + r.decodeS, 0));
  const span = $derived(Math.max(120, sum));
  const marked = $derived(view === 'fault' && hadWork);

  const caption = $derived.by(() => {
    const n = last.length;
    if (view === 'idle') return '· not running';
    if (view === 'loading') return '· no requests yet';
    if (view === 'fault') {
      if (!hadWork) return '· no requests: failed during startup';
      return `· ${n ? `last ${n}` : 'no finished requests'}, then ${died ? 'the request that died' : 'the fault'}`;
    }
    if (n === 0) return '· no finished requests yet';
    return `· last ${n} · ${Math.round(sum)} s`;
  });

  function tip(r: RequestRecord): string {
    return (
      `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${r.prefillS.toFixed(1)} s` +
      ` · ${fmtInt(r.generatedTokens)} tok · decode ${r.decodeS.toFixed(1)} s`
    );
  }
</script>

<section class="c-track c-panel">
  <div class="c-tmain">
    <div class="c-tcap">
      <span class="c-lbl">Request timeline</span><span class="note">{caption}</span>
      {#if view === 'idle' && lastText}<span class="last" class:bad={lastFault}>last session: {lastText}</span>{/if}
    </div>
    <div class="c-tbars">
      {#each last as r (r.id)}
        {@const t = r.prefillS + r.decodeS}
        <div
          class="req"
          title={tip(r)}
          style:flex-basis="calc((100% - var(--tgap) * {last.length - 1 + (marked ? 1 : 0)} - {marked ? 'max(18px, calc(var(--u) * 24))' : '0px'}) * {t / span})"
        >
          <i class="pf" style:flex-grow={r.prefillS}></i><i class="dc" style:flex-grow={r.decodeS}></i>
        </div>
      {:else}
        {#if !marked}<div class="c-tempty"></div>{/if}
      {/each}
      {#if marked}<div class="c-tfail" title={died ? 'The server died during this request' : 'The server died with no request in flight'}></div>{/if}
    </div>
  </div>
  <div class="c-legend">
    <span class="c-lbl"><i class="pf"></i>Prefill</span>
    <span class="c-lbl"><i class="dc"></i>Decode</span>
  </div>
</section>

<style>
  .req {
    display: flex;
    flex-grow: 0;
    /* very short requests get a 3 px floor; the others give way so the row never overflows */
    flex-shrink: 1;
    min-width: 3px;
    border-radius: 2px;
    overflow: hidden;
  }
  .req i {
    flex-basis: 0;
    min-width: 1px;
  }
  .pf {
    background: var(--s3);
  }
  .dc {
    background: var(--sky);
  }
</style>
