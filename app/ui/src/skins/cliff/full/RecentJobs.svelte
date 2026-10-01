<script lang="ts">
  // Finished image jobs as bars on one shared time scale: bar width = job seconds (stated inside the
  // bar when it fits). New images and edits differ in tone; the span of the scale is in the label.
  import type { ImageJob } from '../../../lib/model/types';

  let { recent }: { recent: ImageJob[] } = $props();

  const MAX = 12;
  const last = $derived(recent.slice(-MAX));
  const sum = $derived(last.reduce((a, j) => a + j.seconds, 0));
  const span = $derived(Math.max(120, sum));
  let w = $state(0);
  const gap = 8;
  /** px per second on the shared scale, to decide where the seconds fit inside a bar. */
  const pps = $derived(w > 0 ? (w - gap * Math.max(0, last.length - 1)) / span : 0);
</script>

<section class="rj">
  <div class="head">
    <div class="c-lbl">Recent jobs{#if last.length}<span class="q">{` · last ${last.length} · ${Math.round(sum)} s`}</span>{/if}</div>
    <div class="legend">
      <span><i class="nw"></i>new</span>
      <span><i class="ed"></i>edit</span>
    </div>
  </div>
  <div class="bars" bind:clientWidth={w} style:--gap="{gap}px">
    {#each last as j, i (`${j.at}:${i}`)}
      {@const txt = `${Math.round(j.seconds)} s`}
      <div
        class="job"
        class:edit={j.edit}
        style:flex-basis="calc((100% - var(--gap) * {Math.max(0, last.length - 1)}) * {j.seconds / span})"
        title="{j.width}×{j.height} · {j.seconds.toFixed(1)} s{j.edit ? ' · edit' : ''}"
      >
        {#if j.seconds * pps >= txt.length * 7.4 + 10}<span class="c-data">{txt}</span>{/if}
      </div>
    {:else}
      <div class="none">no images yet</div>
    {/each}
  </div>
</section>

<style>
  .rj {
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
  .nw {
    background: var(--sky);
  }
  .ed {
    background: #6b7c86;
  }
  .bars {
    display: flex;
    gap: var(--gap);
    min-width: 0;
    overflow: hidden;
    height: max(16px, calc(var(--u) * 20));
    margin-top: max(8px, calc(var(--u) * 12));
  }
  .job {
    display: flex;
    align-items: center;
    flex-grow: 0;
    flex-shrink: 1;
    min-width: 3px;
    padding-left: 6px;
    border-radius: 2px;
    background: var(--sky);
    overflow: hidden;
  }
  .job.edit {
    background: #6b7c86;
  }
  .job span {
    font-size: max(10.5px, calc(var(--u) * 12));
    font-weight: 500;
    color: var(--basalt);
    white-space: nowrap;
  }
  .job.edit span {
    color: #f2f9fc;
  }
  .none {
    font-size: max(12px, calc(var(--u) * 14));
    color: var(--muted);
  }
</style>
