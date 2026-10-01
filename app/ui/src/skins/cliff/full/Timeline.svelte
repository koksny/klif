<script lang="ts">
  // Last requests as bars on one shared time scale: bar width = prefillS + decodeS,
  // split into its prefill and decode parts. The span of the scale is stated in the label.
  import type { RequestRecord } from '../../../lib/model/types';
  import { fmtInt } from '../../../lib/model/format';

  let { requests }: { requests: RequestRecord[] } = $props();

  const MAX = 8;
  const last = $derived(requests.slice(-MAX));
  const sum = $derived(last.reduce((a, r) => a + r.prefillS + r.decodeS, 0));
  const span = $derived(Math.max(120, sum));

  function tip(r: RequestRecord): string {
    return (
      `#${r.id} · prompt ${fmtInt(r.promptTokens)} tok (${fmtInt(r.cachedTokens)} cached) · prefill ${r.prefillS.toFixed(1)} s` +
      ` · ${fmtInt(r.generatedTokens)} tok · decode ${r.decodeS.toFixed(1)} s`
    );
  }
</script>

<section class="tl">
  <div class="head">
    <div class="c-lbl">Request timeline{#if last.length}<span class="q">{` · last ${last.length} · ${Math.round(sum)} s`}</span>{/if}</div>
    <div class="legend">
      <span><i class="pf"></i>Prefill</span>
      <span><i class="dc"></i>Decode</span>
    </div>
  </div>
  <div class="bars" style:--n={last.length}>
    {#each last as r (r.id)}
      {@const t = r.prefillS + r.decodeS}
      <div class="req" title={tip(r)} style:flex-basis="calc((100% - var(--gap) * {Math.max(0, last.length - 1)}) * {t / span})">
        <i class="pf" style:flex-grow={r.prefillS}></i><i class="dc" style:flex-grow={r.decodeS}></i>
      </div>
    {:else}
      <div class="none">no requests yet</div>
    {/each}
  </div>
</section>

<style>
  .tl {
    padding-top: max(8px, calc(var(--u) * 15));
    padding-bottom: max(10px, calc(var(--u) * 20));
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .q {
    color: var(--muted);
  }
  .legend {
    display: flex;
    gap: max(14px, calc(var(--u) * 26));
    font-size: max(11.5px, calc(var(--u) * 13.5));
    color: #c3d3db;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 0.6em;
  }
  .legend i {
    width: 1.05em;
    height: 1.05em;
    border-radius: 2px;
  }
  .pf {
    background: var(--s2);
  }
  .dc {
    background: var(--sky);
  }
  .bars {
    --gap: max(10px, calc(var(--u) * 22));
    display: flex;
    gap: var(--gap);
    min-width: 0;
    overflow: hidden;
    height: max(12px, calc(var(--u) * 17));
    margin-top: max(8px, calc(var(--u) * 13));
  }
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
  .none {
    font-size: max(12px, calc(var(--u) * 14));
    color: var(--muted);
  }
</style>
