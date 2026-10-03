<script lang="ts">
  // Status block for kinds without a dedicated display (speech, transcription, video, any generic server): the
  // number of requests served (the hero), whether one is in flight (a pulsing bar), and two fixed rows with
  // what KLIF knows about the server: model and backend, endpoint and the time since the last request. No tok/s.
  import type { GenericLive, ModelRef, SystemKind } from '../../../lib/model/types';
  import { KIND_LABEL } from '../../../lib/model/systems';
  import { fmtInt, fmtSeconds } from '../../../lib/model/format';

  let {
    generic,
    model,
    kind,
    port,
    word = null,
    wordAmber = false,
  }: {
    generic: GenericLive | null;
    model: ModelRef | null;
    kind: SystemKind;
    port?: number;
    word?: string | null;
    wordAmber?: boolean;
  } = $props();

  const live = $derived(!!generic && word === null);
  const inFlight = $derived(generic?.requestsInFlight ?? 0);
  const working = $derived(live && inFlight > 0);
</script>

<section class="c-hero c-panel">
  <div class="c-hl">
    <span class="c-lbl">{KIND_LABEL[kind]}{working ? ' · working' : ''}</span>
    <div class="c-fig" class:dim={!live}>
      <b>{live && generic?.requestsTotal !== undefined ? fmtInt(generic.requestsTotal) : '—'}</b><span class="u">requests</span>
    </div>
  </div>
  {#if !live}
    <div class="c-hr one"><div class="c-wait" class:amb={wordAmber}><span>{word ?? 'waiting for data'}</span></div></div>
  {:else}
    <div class="c-hr">
      <div class="c-hline">
        <span>{working ? `${inFlight} in flight` : 'waiting for the next request'}</span>
      </div>
      <div class="bar">
        <span class="c-bar tall" class:work={working} role="meter" aria-label="Request in flight" aria-valuemin="0" aria-valuemax="1" aria-valuenow={working ? 1 : 0}>
          <i style:transform="scaleX({working ? 1 : 0})"></i>
        </span>
      </div>
      <div class="c-hline sm">
        <span>{generic?.lastActivityS !== undefined ? `last activity ${fmtSeconds(generic.lastActivityS)} ago` : 'no activity seen yet'}</span>
      </div>
    </div>
  {/if}
</section>

<section class="c-rows c-panel" class:dim={!live}>
  <div class="c-row">
    <span class="c-lbl">Model</span>
    <span class="c-v"><span class="t">{#if model?.name}<b>{model.name}</b>{model.quant ? ` · ${model.quant}` : ''}{:else}—{/if}</span></span>
    <span class="c-lbl">Port</span>
    <span class="c-v"><span class="t">{#if port}<b>:{port}</b>{:else}—{/if}</span></span>
  </div>
  <div class="c-row">
    <span class="c-lbl">Backend</span>
    <span class="c-v"><span class="t">{model?.backend || model?.engine || '—'}</span></span>
    <span class="c-lbl">Reports</span>
    <span class="c-v"><span class="t">{generic?.modelId ?? '—'}</span></span>
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
  }
  .c-bar.work i {
    animation: sweep 1.4s ease-in-out infinite;
    transform-origin: left center;
  }
  @keyframes sweep {
    0%,
    100% {
      opacity: 0.45;
    }
    50% {
      opacity: 1;
    }
  }
</style>
