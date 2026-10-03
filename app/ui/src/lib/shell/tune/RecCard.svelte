<script lang="ts">
  // One recommendation: name · quant · size · license · measured, and its download state (Installed /
  // Download / % + Cancel / Failed + reason). Downloads start only from this explicit button.
  import { fmtTps } from '../../model/format';
  import { joinParts } from '../../model/systems';
  import type { DownloadInfo, RecommendationInfo, ViewModel } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import type { Snippet } from 'svelte';
  import { attempt, fmtBytes, QUIET } from './util';

  interface Props {
    vm: ViewModel;
    rec: RecommendationInfo;
    /** Remote node the download would run on (absent = this machine). */
    node?: string;
    /** Extra control at the end of the action row (Use / a radio). */
    action?: Snippet;
    selected?: boolean;
  }
  let { vm, rec, node, action, selected = false }: Props = $props();

  const size = $derived(rec.files.reduce((a, f) => a + (f.sizeBytes ?? 0), 0));
  const measured = $derived(
    rec.measured
      ? joinParts([
          rec.measured.decodeTps !== undefined ? `${fmtTps(rec.measured.decodeTps)} tok/s` : '',
          rec.measured.secondsPerImage !== undefined ? `${rec.measured.secondsPerImage.toFixed(1)} s/image` : '',
          rec.measured.hardware,
        ])
      : '',
  );
  const dls = $derived<DownloadInfo[]>(vm.downloads.filter((d) => d.id === rec.id));
  const running = $derived(dls.some((d) => d.state === 'running' || d.state === 'verifying'));
  const failed = $derived(dls.find((d) => d.state === 'failed'));
  const verifying = $derived(dls.some((d) => d.state === 'verifying'));
  const pct = $derived.by(() => {
    const total = dls.reduce((a, d) => a + (d.totalBytes ?? 0), 0) || size;
    const done = dls.reduce((a, d) => a + d.doneBytes, 0);
    return total > 0 ? Math.min(100, Math.floor((done / total) * 100)) : 0;
  });
  const noDir = $derived(!node && !vm.config.modelsDir);

  let err = $state('');
  let busy = $state(false);

  async function download() {
    busy = true;
    err = await attempt(() => player.actions.downloadRecommendation(rec.id, node, QUIET));
    busy = false;
  }
  async function cancel() {
    busy = true;
    err = await attempt(() => player.actions.cancelDownload(rec.id, node, QUIET));
    busy = false;
  }
</script>

<div class="rec" class:selected>
  <div class="top">
    <b class="name">{rec.name}</b>
    <span class="facts">{joinParts([rec.quant, size ? fmtBytes(size) : '', rec.license])}</span>
  </div>
  {#if measured || rec.minVramGiB !== undefined || rec.notes}
    <div class="sub">
      {joinParts([measured ? `measured ${measured}` : '', rec.minVramGiB !== undefined ? `needs ≥ ${rec.minVramGiB} GiB` : '', rec.notes])}
    </div>
  {/if}
  <div class="acts">
    {#if rec.installed}
      <span class="badge accent">Installed</span>
    {:else if running}
      <span class="progress" role="progressbar" aria-label="Download {rec.name}" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct}>
        <i style="width: {pct}%"></i>
      </span>
      <span class="pct">{verifying ? 'verifying' : `${pct}%`}</span>
      <button type="button" class="mini text" disabled={busy} onclick={() => void cancel()}>Cancel</button>
    {:else}
      {#if failed}<span class="failed" title={failed.error ?? ''}>Failed{failed.error ? `: ${failed.error}` : ''}</span>{/if}
      <button
        type="button"
        class="mini text"
        disabled={busy || noDir}
        title={noDir ? 'Set [paths] models_dir in klif.toml first.' : `Download from huggingface.co/${rec.hfRepo}`}
        onclick={() => void download()}>{failed ? 'Retry' : 'Download'}</button
      >
    {/if}
    <span class="grow"></span>
    {#if action}{@render action()}{/if}
  </div>
  {#if err}<p class="err" role="alert">{err}</p>{/if}
</div>

<style>
  .rec {
    display: grid;
    gap: 6px;
    padding: 10px 12px;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    background: var(--k-surface, #141414);
  }
  .rec.selected {
    border-color: var(--k-accent, #5ab6eb);
  }
  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 10px;
  }
  .name {
    font: 600 13px/1.3 var(--k-font-ui, system-ui, sans-serif);
  }
  .facts,
  .sub {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
  }
  .progress {
    flex: 0 1 140px;
    height: 6px;
    border-radius: 3px;
    background: var(--k-surface-raised, #1a1a1a);
    border: 1px solid var(--k-line, #2e2e2e);
    overflow: hidden;
  }
  .progress i {
    display: block;
    height: 100%;
    background: var(--k-accent, #5ab6eb);
  }
  .pct {
    font: 500 12px/1 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
    min-width: 3.5em;
  }
  .failed {
    color: var(--k-danger, #e05a5a);
    font: 500 12px/1.3 var(--k-font-ui, system-ui, sans-serif);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
