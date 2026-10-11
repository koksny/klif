<script lang="ts">
  // Image (Krea) status block: the current job's diffusion progress (step n / m, s/it, elapsed, bar) over
  // two fixed rows: LAST IMAGE / IMAGES and SIZE / MODE. With nothing to measure the same boxes stay,
  // dimmed, with a word over the hero; SIZE and MODE are the tier's configured values.
  // A music System (audio.cpp) uses the same block: the stage is the step (a bar that drifts while the stage
  // count is not known), LAST SONG / SONGS and BACKEND / QUANT.
  import type { ImageLive, ModelRef, SystemKind } from '../../../lib/model/types';
  import { fmtInt, fmtPct, fmtSeconds } from '../../../lib/model/format';
  import { stageText } from '../../../lib/model/music';

  let {
    image,
    model,
    kind = 'image',
    word = null,
    wordAmber = false,
    stale = false,
  }: {
    image: ImageLive | null;
    model: ModelRef | null;
    kind?: SystemKind;
    word?: string | null;
    wordAmber?: boolean;
    /** GPU asleep with nothing in flight. */
    stale?: boolean;
  } = $props();

  const live = $derived(!!image && word === null);
  const music = $derived(kind === 'music');
  const gen = $derived(live && image?.activity === 'generating' && (image.steps > 0 || music));
  /** A song whose stage count is not known yet (the model loading): the bar drifts. */
  const indet = $derived(gen && !!image && image.steps <= 0);
  const frac = $derived(gen && image && image.steps > 0 ? Math.min(1, image.step / image.steps) : 0);
  const lastJob = $derived(image && image.recent.length ? image.recent[image.recent.length - 1] : null);
</script>

<!-- Hero -->
<section class="c-hero c-panel">
  <div class="c-hl">
    <span class="c-lbl">{music ? 'Song progress' : 'Diffusion progress'}{gen && image?.edit ? ' · edit' : ''}</span>
    <div class="c-fig" class:dim={!gen} class:faded={stale}>
      {#if music}
        <span class="u">stage</span><b>{gen && image && image.steps > 0 ? image.step : '—'}</b>{#if image && image.steps > 0}<span class="u">/ {image.steps}</span>{/if}
      {:else}
        <span class="u">step</span><b>{gen && image ? image.step : '—'}</b>{#if image}<span class="u">/ {image.steps}</span>{/if}
      {/if}
    </div>
  </div>
  {#if !live}
    <div class="c-hr one"><div class="c-wait" class:amb={wordAmber}><span>{word ?? 'waiting for data'}</span></div></div>
  {:else}
    <div class="c-hr">
      {#if gen && image}
        <div class="c-hline">
          {#if !music && image.sPerIt > 0}<span><b>{image.sPerIt.toFixed(2)}</b> s/it</span>{/if}
          <span>elapsed <b>{fmtSeconds(image.elapsedS)}</b></span>
          {#if music}
            <span class="end">{stageText(image)}</span>
          {:else}
            <span class="end">{image.width}×{image.height} · {image.edit ? 'edit' : 'new image'}</span>
          {/if}
        </div>
      {:else}
        <div class="c-hline"><span>{music ? 'waiting for the next song' : 'waiting for the next job'}</span></div>
      {/if}
      <div class="bar">
        <span class="c-bar tall" class:indet role="meter" aria-label="Job progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={indet ? undefined : Math.round(frac * 100)}>
          <i style:transform={indet ? undefined : `scaleX(${frac})`}></i>
        </span>
        <b class="pct" class:off={!gen}>{gen ? (indet ? '…' : fmtPct(frac)) : '—'}</b>
      </div>
      <div class="c-hline sm">
        <span>{gen && image ? (music ? (image.steps > 0 ? `${image.steps} stages` : 'stages not known yet') : `${image.steps} sampling steps`) : music ? 'no song running' : 'no job running'}</span>
      </div>
    </div>
  {/if}
</section>

<!-- Rows -->
<section class="c-rows c-panel" class:dim={!live}>
  <div class="c-row">
    <span class="c-lbl">{music ? 'Last song' : 'Last image'}</span>
    <span class="c-v">
      {#if lastJob}
        <span class="t"><b>{fmtSeconds(lastJob.seconds)}</b>{#if !music} · {lastJob.width}×{lastJob.height}{lastJob.edit ? ' · edit' : ''}{/if}</span>
      {:else}
        <span class="t q">{image ? 'none yet' : '—'}</span>
      {/if}
    </span>
    <span class="c-lbl">{music ? 'Songs' : 'Images'}</span>
    <span class="c-v">
      {#if image}<span class="t"><b>{fmtInt(image.imagesThisSession)}</b> this session</span>{:else}<span class="t q">—</span>{/if}
    </span>
  </div>
  <div class="c-row">
    {#if music}
      <span class="c-lbl">Backend</span>
      <span class="c-v"><span class="t">{#if model?.backend || model?.engine}<b>{model.backend || model.engine}</b>{:else}—{/if}</span></span>
      <span class="c-lbl">Quant</span>
      <span class="c-v"><span class="t">{#if model?.quant}<b>{model.quant}</b>{:else}—{/if}</span></span>
    {:else}
      <span class="c-lbl">Size</span>
      <span class="c-v"><span class="t">{#if model?.imageSize}<b>{model.imageSize}</b>{:else}—{/if}</span></span>
      <span class="c-lbl">Mode</span>
      <span class="c-v"><span class="t">{#if model?.mode}<b>{model.mode}</b>{:else}—{/if}</span></span>
    {/if}
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
  /* No stage count yet: a short fill drifts along the track (a still bar would read as idle). */
  .c-bar.indet i {
    right: auto;
    width: 30%;
    transition: none;
    animation: drift 1.8s ease-in-out infinite alternate;
  }
  @keyframes drift {
    from {
      left: 0;
    }
    to {
      left: 70%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .c-bar.indet i {
      animation: none;
      left: 35%;
    }
  }
</style>
