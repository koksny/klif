<script lang="ts">
  // Recent image jobs: one bar per finished job (the last 12, oldest left), height strictly
  // proportional to the job's seconds on a 0-based scale printed at the left. Every bar carries its
  // seconds; edit jobs are cream, plain generations cyan (legend beside it). Empty plot while nothing
  // ran (the caller's caption says why); `none` is printed only when a session is up with no jobs yet.
  import type { ImageJob } from '../../../lib/model/types';
  import { fmtJobS } from '../theme';

  let { jobs, slots = 12, none = 'no images yet' }: { jobs: ImageJob[]; slots?: number; none?: string } = $props();

  const last = $derived(jobs.slice(-slots));
  const top = $derived.by(() => {
    const mx = Math.max(0, ...last.map((j) => j.seconds));
    if (mx <= 0) return 0;
    const step = mx <= 20 ? 5 : mx <= 60 ? 10 : 30;
    return Math.ceil(mx / step) * step;
  });
  const cells = $derived(
    Array.from({ length: slots }, (_, i) => last[i - (slots - last.length)] ?? null),
  );
</script>

<div class="rj">
  <div class="axis" aria-hidden="true">
    <span>{top ? `${top} s` : ''}</span>
    <span>0</span>
  </div>
  <div class="plot" style="grid-template-columns: repeat({slots}, minmax(0, 1fr))">
    <span class="grid g1"></span>
    <span class="grid g2"></span>
    {#each cells as j, i (i)}
      <div class="cell">
        {#if j && top}
          <div class="col" style="height:{(j.seconds / top) * 100}%">
            <span class="v">{fmtJobS(j.seconds)}</span>
            <span class="bar" class:edit={j.edit} title="{j.width}×{j.height}{j.edit ? ' edit' : ''} · {fmtJobS(j.seconds)}"></span>
          </div>
        {/if}
      </div>
    {/each}
    {#if !last.length && none}<span class="none">{none}</span>{/if}
  </div>
</div>

<style>
  .rj {
    display: flex;
    gap: calc(10 * var(--u));
    height: 100%;
    min-width: 0;
  }
  .axis {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    font-family: var(--font-text);
    font-size: var(--fs-lbl);
    color: rgba(237, 230, 214, 0.55);
    text-align: right;
    line-height: 1;
    margin: calc(-5 * var(--u)) 0;
    min-width: calc(28 * var(--u));
  }
  .plot {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    display: grid;
    gap: calc(14 * var(--u));
    align-items: end;
    border-bottom: 1px solid rgba(237, 230, 214, 0.45);
  }
  .grid {
    position: absolute;
    left: 0;
    right: 0;
    height: 0;
    border-top: 1px dashed rgba(237, 230, 214, 0.14);
  }
  .g1 {
    top: 0;
  }
  .g2 {
    top: 50%;
  }
  .cell {
    height: 100%;
    display: flex;
    align-items: flex-end;
    min-width: 0;
  }
  .col {
    position: relative;
    width: 100%;
    min-height: 2px;
  }
  .bar {
    position: absolute;
    inset: 0;
    border-radius: 2px 2px 0 0;
    background: #4fb2ea;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.25);
  }
  .bar.edit {
    background: #e9e1d0;
  }
  .v {
    position: absolute;
    left: 50%;
    bottom: 100%;
    transform: translateX(-50%);
    margin-bottom: calc(3 * var(--u));
    font-family: var(--font-text);
    font-size: var(--fs-lbl);
    color: rgba(237, 230, 214, 0.75);
    white-space: nowrap;
    line-height: 1;
  }
  .none {
    position: absolute;
    left: 0;
    right: 0;
    top: 35%;
    text-align: center;
    font-family: var(--font-text);
    font-size: var(--fs-small);
    color: var(--muted);
  }
</style>
