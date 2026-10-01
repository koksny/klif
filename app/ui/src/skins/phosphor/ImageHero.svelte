<script lang="ts">
  // Image session hero: the current job's sampling step on a glowing progress bar (step / steps),
  // seconds per iteration and elapsed time; then the last finished image and the session count.
  import type { ImageLive } from '../../lib/model/types';
  import { fmtInt, fmtSeconds } from '../../lib/model/format';
  import { fmtSPerIt } from './geom';

  let { img }: { img: ImageLive } = $props();

  const gen = $derived(img.activity === 'generating');
  const frac = $derived(gen && img.steps > 0 ? Math.max(0, Math.min(1, img.step / img.steps)) : 0);
  const last = $derived(img.recent.length ? img.recent[img.recent.length - 1] : null);
</script>

<section class="panel grat ihero" aria-label="Image generation">
  <div class="dhead">
    <span class="lbl">{gen ? 'Generating image' : 'Image server'}</span>
    <span class="act" class:on={gen}>{gen ? 'sampling' : 'idle'}</span>
    <span class="note">{img.width}x{img.height}{img.edit ? ' · edit' : ''}</span>
  </div>
  {#if gen}
    <div class="readout"><span class="w">step</span><span class="num">{img.step} / {img.steps}</span></div>
    <div class="sub">
      <span class="val">{fmtSPerIt(img.sPerIt)} s/it</span>
      <span class="val">elapsed {fmtSeconds(img.elapsedS)}</span>
    </div>
  {:else}
    <div class="readout dim"><span class="num">waiting</span></div>
    <div class="sub"><span class="mut">no job running · the next request starts sampling here</span></div>
  {/if}
  <div class="prog">
    <div class="track" role="img" aria-label="Sampling step {img.step} of {img.steps}">
      <div class="fill" style="transform:scaleX({frac.toFixed(4)})"></div>
      {#if gen}<div class="hw" style="transform:translateX({(frac * 100).toFixed(2)}%)"><div class="head"></div></div>{/if}
    </div>
    <span class="pct" class:dim={!gen}>{gen ? `${Math.round(frac * 100)}%` : '—'}</span>
  </div>
</section>

<section class="panel ilast">
  <div class="cell">
    <div class="lbl">Last image</div>
    <div class="big">
      {#if last}{fmtSeconds(last.seconds)}<span class="det">{last.width}x{last.height}{last.edit ? ' · edit' : ''}</span>{:else}<span class="mut">—</span>{/if}
    </div>
  </div>
  <span class="vrule" aria-hidden="true"></span>
  <div class="cell">
    <div class="lbl">Images this session</div>
    <div class="big">{fmtInt(img.imagesThisSession)}</div>
  </div>
</section>

<style>
  .ihero {
    flex: none;
    height: calc(214px * var(--k));
    display: grid;
    grid-template-rows: auto auto auto 1fr;
    padding: calc(14px * var(--k)) calc(22px * var(--k)) calc(16px * var(--k));
  }
  .dhead {
    display: flex;
    align-items: baseline;
    gap: calc(16px * var(--k));
  }
  .act {
    font-size: calc(13px * var(--k));
    letter-spacing: 0.1em;
    color: var(--ph-muted);
    text-transform: uppercase;
  }
  .act.on {
    color: var(--ph-cyan);
  }
  .note {
    margin-left: auto;
    font-size: calc(15px * var(--k));
    letter-spacing: 0.04em;
    color: var(--ph-cyan);
    opacity: 0.85;
  }
  .readout {
    display: flex;
    align-items: baseline;
    gap: calc(22px * var(--k));
    margin-top: calc(6px * var(--k));
    font-family: var(--ph-display);
    color: var(--ph-cyan);
    line-height: 1;
    white-space: nowrap;
  }
  .w,
  .num {
    font-size: calc(72px * var(--k));
    font-weight: 380;
    font-stretch: 112%;
    letter-spacing: 0.03em;
    font-variant-numeric: tabular-nums;
    text-shadow:
      0 0 10px rgba(127, 227, 255, 0.55),
      0 0 30px rgba(90, 182, 235, 0.3);
  }
  .readout.dim .num {
    color: var(--ph-muted);
    text-shadow: none;
    font-size: calc(56px * var(--k));
  }
  .sub {
    display: flex;
    gap: calc(48px * var(--k));
    margin-top: calc(10px * var(--k));
    font-size: calc(24px * var(--k));
    letter-spacing: 0.05em;
  }
  .sub .mut {
    font-size: calc(18px * var(--k));
  }
  .prog {
    align-self: end;
    display: flex;
    align-items: center;
    gap: calc(16px * var(--k));
  }
  .track {
    position: relative;
    flex: 1;
    height: calc(24px * var(--k));
    border: 1px solid #2a7f93;
    border-radius: 3px;
    overflow: hidden;
    background: rgba(3, 9, 12, 0.75);
  }
  .fill {
    position: absolute;
    inset: 2px;
    transform-origin: left center;
    border-radius: 2px;
    background: linear-gradient(90deg, rgba(90, 182, 235, 0.75), var(--ph-cyan) 70%, #c8f5ff);
    box-shadow: 0 0 14px rgba(127, 227, 255, 0.55);
    transition: transform 0.45s ease-out;
  }
  .hw {
    position: absolute;
    inset: 0;
    transition: transform 0.45s ease-out;
  }
  .head {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -3px;
    width: 4px;
    background: #ffe2a8;
    box-shadow:
      0 0 8px var(--ph-amber),
      0 0 16px rgba(232, 176, 74, 0.6);
  }
  .pct {
    flex: none;
    min-width: 2.6em;
    text-align: right;
    font-family: var(--ph-display);
    font-size: calc(30px * var(--k));
    color: var(--ph-cyan);
    text-shadow: var(--ph-glow);
    font-variant-numeric: tabular-nums;
  }
  .pct.dim {
    color: var(--ph-muted);
    text-shadow: none;
  }

  .ilast {
    flex: none;
    height: calc(96px * var(--k));
    display: flex;
    align-items: center;
    gap: calc(28px * var(--k));
    padding: 0 calc(24px * var(--k));
  }
  .cell {
    flex: 1 1 0;
    min-width: 0;
    display: grid;
    gap: calc(8px * var(--k));
  }
  .big {
    display: flex;
    align-items: baseline;
    gap: calc(18px * var(--k));
    font-family: var(--ph-display);
    font-stretch: 110%;
    font-size: calc(40px * var(--k));
    line-height: 1;
    color: var(--ph-cyan);
    text-shadow:
      0 0 8px rgba(127, 227, 255, 0.5),
      0 0 22px rgba(90, 182, 235, 0.25);
    white-space: nowrap;
  }
  .det {
    font-family: var(--ph-ui);
    font-size: calc(16px * var(--k));
    letter-spacing: 0.04em;
    color: var(--ph-muted);
    text-shadow: none;
  }
  .vrule {
    flex: none;
    width: 1px;
    align-self: stretch;
    margin: calc(14px * var(--k)) 0;
    background: var(--ph-rule);
  }
</style>
