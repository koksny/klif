<script lang="ts">
  // Image (Krea) session hero: the current job's sampling step, speed and progress, then the
  // session counters. The recent-jobs bars sit under the cliff (see RecentJobs).
  import type { ImageLive } from '../../../lib/model/types';
  import { fmtInt } from '../../../lib/model/format';

  let { image }: { image: ImageLive } = $props();
  const gen = $derived(image.activity === 'generating');
  const frac = $derived(gen && image.steps > 0 ? Math.min(1, image.step / image.steps) : 0);
  const lastJob = $derived(image.recent.length ? image.recent[image.recent.length - 1] : null);
</script>

<section class="job">
  <div class="c-lbl">
    Current job{#if gen}<span class="q">{` · ${image.width}×${image.height} · ${image.edit ? 'edit' : 'new image'}`}</span>{:else}<span
        class="q">{' · none running'}</span
      >{/if}
  </div>
  <div class="row">
    {#if gen}
      <span class="big">step {image.step} / {image.steps}</span>
      {#if image.sPerIt > 0}<span class="spi"><span class="n">{image.sPerIt.toFixed(2)}</span> s/it</span>{/if}
      <span class="el">elapsed <span class="c-data">{image.elapsedS.toFixed(1)} s</span></span>
    {:else}
      <span class="big idle">idle</span>
      <span class="el">waiting for the next request</span>
    {/if}
  </div>
  <div class="prog">
    <div class="bar" role="meter" aria-label="Job progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(frac * 100)}>
      <i style:transform="scaleX({frac})"></i>
    </div>
    <span class="pct c-data">{gen ? `${Math.round(frac * 100)}%` : '—'}</span>
  </div>
</section>

<div class="c-rule"></div>

<section class="stats">
  <div class="cell">
    <div class="c-lbl">last image</div>
    <div class="v c-data">{lastJob ? `${lastJob.seconds.toFixed(1)} s` : '—'}</div>
  </div>
  <div class="vr" aria-hidden="true"></div>
  <div class="cell">
    <div class="c-lbl">last size</div>
    <div class="v c-data">{lastJob ? `${lastJob.width}×${lastJob.height}${lastJob.edit ? ' · edit' : ''}` : '—'}</div>
  </div>
  <div class="vr" aria-hidden="true"></div>
  <div class="cell">
    <div class="c-lbl">images this session</div>
    <div class="v c-data">{fmtInt(image.imagesThisSession)}</div>
  </div>
</section>

<style>
  .job {
    padding-top: max(7px, calc(var(--u) * 13));
    padding-bottom: max(9px, calc(var(--u) * 15));
  }
  .q {
    color: var(--muted);
  }
  .row {
    display: flex;
    align-items: baseline;
    gap: max(14px, calc(var(--u) * 30));
    margin-top: max(2px, calc(var(--u) * 4));
    white-space: nowrap;
    min-width: 0;
  }
  .big {
    font-family: var(--f-disp);
    font-stretch: 125%;
    font-weight: 800;
    font-size: max(46px, calc(var(--u) * 74));
    line-height: 0.95;
    letter-spacing: -0.01em;
    color: #f2f9fc;
    font-variant-numeric: tabular-nums;
  }
  .big.idle {
    color: #5d6e77;
  }
  .spi {
    font-family: var(--f-disp);
    font-weight: 600;
    font-size: max(24px, calc(var(--u) * 38));
    color: var(--sky);
  }
  .spi .n {
    font-variant-numeric: tabular-nums;
  }
  .el {
    font-family: var(--f-ui);
    font-size: max(15px, calc(var(--u) * 22));
    color: #9fb2bc;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .el .c-data {
    color: var(--foam);
  }
  .prog {
    display: flex;
    align-items: center;
    gap: max(12px, calc(var(--u) * 20));
    margin-top: max(9px, calc(var(--u) * 14));
  }
  .bar {
    position: relative;
    flex: 1 1 auto;
    height: max(9px, calc(var(--u) * 12));
    border-radius: 99px;
    background: var(--s1);
    overflow: hidden;
  }
  .bar i {
    position: absolute;
    inset: 0;
    background: var(--sky);
    border-radius: 99px;
    transform-origin: left;
    transition: transform 400ms ease-out;
  }
  .pct {
    flex: none;
    min-width: 3.2em;
    text-align: right;
    font-size: max(13px, calc(var(--u) * 17));
    color: var(--foam);
  }

  .stats {
    display: grid;
    grid-template-columns: 1fr 1px 1fr 1px 1fr;
    column-gap: max(16px, calc(var(--u) * 34));
    padding-top: max(7px, calc(var(--u) * 11));
    padding-bottom: max(8px, calc(var(--u) * 13));
  }
  .vr {
    background: var(--rule);
  }
  .v {
    margin-top: max(2px, calc(var(--u) * 4));
    font-size: max(15px, calc(var(--u) * 22));
    font-weight: 500;
    color: var(--foam);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
