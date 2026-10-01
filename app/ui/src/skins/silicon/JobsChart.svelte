<script lang="ts">
  // Recent image jobs as dimensioned columns: height is exactly proportional to the job's seconds
  // (one scale, the tallest shown job = full height), each column labelled with its time. Edit jobs
  // are hatched. With `fault`, the last slot marks the job the server died in (its length is unknown,
  // so it is drawn as a dashed red marker, not a bar).
  import type { ImageJob } from '../../lib/model/types';

  let { jobs, fault = false, slots = 12 }: { jobs: ImageJob[]; fault?: boolean; slots?: number } = $props();

  const shown = $derived(jobs.slice(-(fault ? slots - 1 : slots)));
  const maxS = $derived(Math.max(0.1, ...shown.map((j) => j.seconds)));
  type Cell = { kind: 'job'; j: ImageJob; max: boolean } | { kind: 'fault' } | { kind: 'empty' };
  const cells = $derived.by<Cell[]>(() => {
    const out: Cell[] = [];
    const n = fault ? slots - 1 : slots;
    const pad = n - shown.length;
    let maxDone = false;
    const maxIdx = shown.reduce((bi, j, i) => (j.seconds > shown[bi].seconds ? i : bi), 0);
    for (let i = 0; i < pad; i++) out.push({ kind: 'empty' });
    shown.forEach((j, i) => {
      const isMax = i === maxIdx && !maxDone;
      if (isMax) maxDone = true;
      out.push({ kind: 'job', j, max: isMax });
    });
    if (fault) out.push({ kind: 'fault' });
    return out;
  });

  function secs(v: number): string {
    return v >= 100 ? `${Math.round(v)}` : v.toFixed(1);
  }
</script>

<div class="jc" style="--n:{slots}">
  {#each cells as c, i (i)}
    <div class="slot">
      {#if c.kind === 'job'}
        <span class="v" class:max={c.max}>{secs(c.j.seconds)}<small>&#8201;s</small></span>
        <span class="area">
          <span
            class="b"
            class:edit={c.j.edit}
            style="height:{(c.j.seconds / maxS) * 100}%"
            title="{c.j.width}×{c.j.height}{c.j.edit ? ' edit' : ''} · {c.j.seconds.toFixed(1)} s"
          ></span>
        </span>
      {:else if c.kind === 'fault'}
        <span class="v red">FAULT</span>
        <span class="area"><span class="b fault" title="The server died during this job"></span></span>
      {:else}
        <span class="v"></span>
        <span class="area"><span class="b empty"></span></span>
      {/if}
    </div>
  {/each}
</div>

<style>
  .jc {
    display: grid;
    grid-template-columns: repeat(var(--n), minmax(0, 1fr));
    gap: calc(var(--u) * 12);
    height: 100%;
    min-height: 0;
    border-bottom: 1px solid #4e6779;
    padding: 0 calc(var(--u) * 2);
  }
  .slot {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
  }
  .v {
    height: calc(var(--u) * 15);
    font-size: max(9px, calc(var(--u) * 11.5));
    line-height: 1;
    color: #8fa6b6;
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
  }
  .v small {
    font-size: 0.9em;
  }
  .v.max {
    color: #f4faff;
    font-weight: 600;
  }
  .v.red {
    color: #ff5a36;
    font-weight: 600;
    letter-spacing: 0.06em;
  }
  .area {
    position: relative;
    min-height: 0;
  }
  .b {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    background: #5ab6eb;
  }
  .b.edit {
    background-color: #3e9ad0;
    background-image: repeating-linear-gradient(-45deg, rgba(4, 18, 27, 0.55) 0 1.5px, transparent 1.5px 6px);
  }
  .b.fault {
    top: 0;
    border: 1px dashed #ff5a36;
    background-color: rgba(255, 90, 54, 0.08);
    background-image: repeating-linear-gradient(-45deg, rgba(255, 90, 54, 0.45) 0 1px, transparent 1px 6px);
  }
  .b.empty {
    height: calc(var(--u) * 6);
    border: 1px dashed #2c3c49;
    border-bottom: 0;
    background: transparent;
  }
</style>
