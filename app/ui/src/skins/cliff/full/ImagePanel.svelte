<script lang="ts">
  // Image (Krea) status block: the current job's diffusion progress (step n / m, s/it, elapsed, bar) over
  // two fixed rows: LAST IMAGE / IMAGES and SIZE / MODE. With nothing to measure the same boxes stay,
  // dimmed, with a word over the hero; SIZE and MODE are the tier's configured values.
  import type { ImageLive, ModelRef } from '../../../lib/model/types';
  import { fmtInt, fmtPct, fmtSeconds } from '../../../lib/model/format';

  let {
    image,
    model,
    word = null,
    wordAmber = false,
    stale = false,
  }: {
    image: ImageLive | null;
    model: ModelRef | null;
    word?: string | null;
    wordAmber?: boolean;
    /** GPU asleep with nothing in flight. */
    stale?: boolean;
  } = $props();

  const live = $derived(!!image && word === null);
  const gen = $derived(live && image?.activity === 'generating' && image.steps > 0);
  const frac = $derived(gen && image ? Math.min(1, image.step / image.steps) : 0);
  const lastJob = $derived(image && image.recent.length ? image.recent[image.recent.length - 1] : null);
</script>

<!-- Hero -->
<section class="c-hero c-panel">
  <div class="c-hl">
    <span class="c-lbl">Diffusion progress{gen && image?.edit ? ' · edit' : ''}</span>
    <div class="c-fig" class:dim={!gen} class:faded={stale}>
      <span class="u">step</span><b>{gen && image ? image.step : '—'}</b>{#if image}<span class="u">/ {image.steps}</span>{/if}
    </div>
  </div>
  {#if !live}
    <div class="c-hr one"><div class="c-wait" class:amb={wordAmber}><span>{word ?? 'waiting for data'}</span></div></div>
  {:else}
    <div class="c-hr">
      {#if gen && image}
        <div class="c-hline">
          {#if image.sPerIt > 0}<span><b>{image.sPerIt.toFixed(2)}</b> s/it</span>{/if}
          <span>elapsed <b>{fmtSeconds(image.elapsedS)}</b></span>
          <span class="end">{image.width}×{image.height} · {image.edit ? 'edit' : 'new image'}</span>
        </div>
      {:else}
        <div class="c-hline"><span>waiting for the next job</span></div>
      {/if}
      <div class="bar">
        <span class="c-bar tall" role="meter" aria-label="Job progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(frac * 100)}>
          <i style:transform="scaleX({frac})"></i>
        </span>
        <b class="pct" class:off={!gen}>{gen ? fmtPct(frac) : '—'}</b>
      </div>
      <div class="c-hline sm"><span>{gen && image ? `${image.steps} sampling steps` : 'no job running'}</span></div>
    </div>
  {/if}
</section>

<!-- Rows -->
<section class="c-rows c-panel" class:dim={!live}>
  <div class="c-row">
    <span class="c-lbl">Last image</span>
    <span class="c-v">
      {#if lastJob}
        <span class="t"><b>{fmtSeconds(lastJob.seconds)}</b> · {lastJob.width}×{lastJob.height}{lastJob.edit ? ' · edit' : ''}</span>
      {:else}
        <span class="t q">{image ? 'none yet' : '—'}</span>
      {/if}
    </span>
    <span class="c-lbl">Images</span>
    <span class="c-v">
      {#if image}<span class="t"><b>{fmtInt(image.imagesThisSession)}</b> this session</span>{:else}<span class="t q">—</span>{/if}
    </span>
  </div>
  <div class="c-row">
    <span class="c-lbl">Size</span>
    <span class="c-v"><span class="t">{#if model?.imageSize}<b>{model.imageSize}</b>{:else}—{/if}</span></span>
    <span class="c-lbl">Mode</span>
    <span class="c-v"><span class="t">{#if model?.mode}<b>{model.mode}</b>{:else}—{/if}</span></span>
  </div>
</section>

<style>
  .c-hr.one {
    grid-template-rows: minmax(0, 1fr);
    align-content: stretch;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: max(10px, calc(var(--u) * 14));
  }
  .pct {
    flex: none;
    min-width: 3em;
    text-align: right;
    font-weight: 500;
    font-size: var(--fs-m);
    color: var(--foam);
  }
  .pct.off {
    color: var(--dim);
  }
</style>
