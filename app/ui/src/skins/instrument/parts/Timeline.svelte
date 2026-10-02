<script lang="ts">
  // Last 8 requests, one fixed-width bar each (as in the mockup). The bar is the request's wall time
  // split by share: cream = prefill seconds, cyan = decode seconds. The caption prints the request id and
  // the total time, so the length itself never pretends to encode duration. Empty troughs while nothing
  // ran. failed: the server died; the last place marks it (hatched orange) after the last 7 requests.
  import type { RequestRecord } from '../../../lib/model/types';

  let { requests, failed = false, failedTitle = '' }: { requests: RequestRecord[]; failed?: boolean; failedTitle?: string } = $props();

  const SLOTS = 8;
  const n = $derived(failed ? SLOTS - 1 : SLOTS);
  const last = $derived(requests.slice(-n));
  const cells = $derived(
    Array.from({ length: n }, (_, i) => {
      const r = last[i - (n - last.length)];
      if (!r) return null;
      const t = r.prefillS + r.decodeS;
      return {
        r,
        t,
        p: t > 0 ? (r.prefillS / t) * 100 : 0,
      };
    }),
  );
  const fmt = (s: number) => (s < 10 ? `${s.toFixed(1)} s` : s < 120 ? `${Math.round(s)} s` : `${Math.round(s / 60)} min`);
</script>

<div class="tl">
  {#each cells as c, i (i)}
    <div class="cell">
      <div class="trough">
        {#if c}
          <div
            class="bar"
            title="#{c.r.id}: prefill {c.r.prefillS.toFixed(1)} s ({c.r.promptTokens} tok, {c.r.cachedTokens} cached) · decode {c.r.decodeS.toFixed(1)} s ({c.r.generatedTokens} tok)"
          >
            <span class="pf" style="width:{c.p}%"></span><span class="dc"></span>
          </div>
        {/if}
      </div>
      <div class="cap">{c ? `#${c.r.id} · ${fmt(c.t)}` : ''}</div>
    </div>
  {/each}
  {#if failed}
    <div class="cell">
      <div class="trough failed" title={failedTitle}></div>
      <div class="cap hot">fault</div>
    </div>
  {/if}
</div>

<style>
  .tl {
    display: grid;
    grid-template-columns: repeat(8, minmax(0, 1fr));
    gap: calc(16 * var(--u));
    min-width: 0;
  }
  .cell {
    min-width: 0;
  }
  .trough {
    height: calc(18 * var(--u));
    border-radius: 2px;
    background: rgba(0, 0, 0, 0.22);
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.5);
  }
  .trough.failed {
    background-color: rgba(255, 107, 44, 0.08);
    background-image: repeating-linear-gradient(-45deg, rgba(255, 107, 44, 0.6) 0 1.5px, transparent 1.5px 6px);
    box-shadow: inset 0 0 0 1px rgba(255, 107, 44, 0.75);
  }
  .bar {
    display: flex;
    width: 100%;
    height: 100%;
    border-radius: 2px;
    overflow: hidden;
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.5);
  }
  .pf {
    background: #e9e1d0;
  }
  .dc {
    flex: 1 1 auto;
    background: #4fb2ea;
  }
  .cap {
    margin-top: calc(5 * var(--u));
    font-family: var(--font-text);
    font-size: var(--fs-lbl);
    color: rgba(237, 230, 214, 0.55);
    letter-spacing: 0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.1;
    min-height: 1.1em;
  }
  .cap.hot {
    color: var(--orange);
  }
</style>
