<script lang="ts">
  // Recent jobs: finished image jobs as bars on one shared time scale (bar width = job seconds, stated
  // inside the bar when it fits). New images and edits differ in tone; the span is in the caption. The
  // frame is there in every phase, the same height as the request timeline.
  import type { ImageJob } from '../../../lib/model/types';
  import { fmtInt } from '../../../lib/model/format';
  import type { BlockView } from '../util';

  let {
    recent,
    total = 0,
    view,
    hadWork = false,
    lastText = '',
    lastFault = false,
  }: {
    recent: ImageJob[];
    /** Images this session. */
    total?: number;
    view: BlockView;
    /** Fault: the server was serving when it died. */
    hadWork?: boolean;
    lastText?: string;
    lastFault?: boolean;
  } = $props();

  const MAX = 12;
  const last = $derived(view === 'idle' || view === 'loading' ? [] : recent.slice(view === 'fault' ? -(MAX - 1) : -MAX));
  const sum = $derived(last.reduce((a, j) => a + j.seconds, 0));
  const span = $derived(Math.max(120, sum));
  const marked = $derived(view === 'fault' && hadWork);
  let w = $state(0);
  /** px per second on the shared scale, to decide where the seconds fit inside a bar. */
  const pps = $derived(w > 0 ? (w - 10 * Math.max(0, last.length - 1)) / span : 0);

  const caption = $derived.by(() => {
    const n = last.length;
    if (view === 'idle') return '· not running';
    if (view === 'loading') return '· no jobs yet';
    if (view === 'fault') {
      if (!hadWork) return '· no jobs: failed during startup';
      return n ? `· last ${n}, then the fault` : '· no finished jobs, then the fault';
    }
    if (n === 0) return '· no finished jobs yet';
    return `· last ${n} of ${fmtInt(Math.max(total, n))} · ${Math.round(sum)} s`;
  });
</script>

<section class="c-track c-panel">
  <div class="c-tmain">
    <div class="c-tcap">
      <span class="c-lbl">Recent jobs</span><span class="note">{caption}</span>
      {#if view === 'idle' && lastText}<span class="last" class:bad={lastFault}>last session: {lastText}</span>{/if}
    </div>
    <div class="c-tbars" bind:clientWidth={w}>
      {#each last as j, i (`${j.at}:${i}`)}
        {@const txt = `${Math.round(j.seconds)} s`}
        <div
          class="job"
          class:edit={j.edit}
          style:flex-basis="calc((100% - var(--tgap) * {last.length - 1 + (marked ? 1 : 0)} - {marked ? 'max(18px, calc(var(--u) * 24))' : '0px'}) * {j.seconds / span})"
          title="{j.width}×{j.height} · {j.seconds.toFixed(1)} s{j.edit ? ' · edit' : ''}"
        >
          {#if j.seconds * pps >= txt.length * 7 + 10}<span>{txt}</span>{/if}
        </div>
      {:else}
        {#if !marked}<div class="c-tempty"></div>{/if}
      {/each}
      {#if marked}<div class="c-tfail" title="The server died"></div>{/if}
    </div>
  </div>
  <div class="c-legend">
    <span class="c-lbl"><i class="nw"></i>New</span>
    <span class="c-lbl"><i class="ed"></i>Edit</span>
  </div>
</section>

<style>
  .job {
    display: flex;
    align-items: center;
    flex-grow: 0;
    flex-shrink: 1;
    min-width: 3px;
    padding-left: 5px;
    border-radius: 2px;
    background: var(--sky);
    overflow: hidden;
  }
  .job.edit {
    background: #6b7c86;
  }
  .job span {
    font-size: max(10px, calc(var(--u) * 10));
    font-weight: 600;
    color: var(--basalt);
    white-space: nowrap;
  }
  .job.edit span {
    color: #f2f9fc;
  }
  .nw {
    background: var(--sky);
  }
  .ed {
    background: #6b7c86;
  }
</style>
