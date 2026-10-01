<script lang="ts">
  // Idle launcher body: the VRAM dial as a fit preview of the selected tier, its preset context, the
  // last session's decode speed on the drum, the big Launch + Tune keys, system, and the last-session
  // strip. Nothing here is live inference data; every number is a configured fact or the last session.
  import type { Actions, ViewModel } from '../../../lib/model/types';
  import { fmtCtx, fmtInt } from '../../../lib/model/format';
  import { availLabel, fmtAgo, fmtDur, fmtJobS, shortLabel, slotById } from '../theme';
  import Drum from '../parts/Drum.svelte';
  import VramDial from '../parts/VramDial.svelte';
  import ContextDial from '../parts/ContextDial.svelte';
  import Icon from '../parts/Icon.svelte';
  import SysPanel from './SysPanel.svelte';

  let { vm, actions }: { vm: ViewModel; actions: Actions } = $props();

  const sel = $derived(slotById(vm, vm.selected));
  const ready = $derived(sel?.availability === 'ready');
  const isImage = $derived(sel?.kind === 'image');
  const expected = $derived(sel?.expectedVram ?? []);
  // Memory held by others while nothing of ours runs: the floor the preview stacks on.
  const baseline = $derived(vm.vram.baselineGiB ?? vm.vram.usedGiB);

  const ls = $derived(vm.lastSession ?? null);
  const lsSlot = $derived(ls ? slotById(vm, ls.slot) : undefined);
  const lsLabel = $derived(lsSlot?.label ?? ls?.slot.toUpperCase() ?? '');
  const lastKind = $derived(!ls ? 'none' : ls.decodeTps !== undefined ? 'llm' : ls.secondsPerImage !== undefined ? 'image' : 'none');
  const lastValue = $derived(lastKind === 'llm' ? (ls?.decodeTps ?? 0) : lastKind === 'image' ? (ls?.secondsPerImage ?? 0) : 0);

  const lastLine = $derived.by(() => {
    if (!ls) return '';
    const parts: string[] = [lsLabel, ls.model.name, fmtDur(ls.uptimeS)];
    if (ls.requests !== undefined) parts.push(`${fmtInt(ls.requests)} requests`);
    if (ls.generatedTokens !== undefined) parts.push(`${fmtInt(ls.generatedTokens)} tok`);
    if (ls.images !== undefined) parts.push(`${fmtInt(ls.images)} images`);
    if (ls.secondsPerImage !== undefined && ls.decodeTps === undefined) parts.push(`${fmtJobS(ls.secondsPerImage)} per image`);
    return parts.join(' · ');
  });

  let ctxW = $state(0);
  let ctxH = $state(0);
  const dialPx = $derived(Math.max(60, Math.min(ctxH - 40, ctxW * 0.42)));
</script>

<div class="dials">
  <section class="panel vram">
    <VramDial vram={vm.vram} mode="fit" fit={{ baseline, layers: expected }} />
  </section>
  <div class="rcol">
    <section class="panel ctx" bind:clientWidth={ctxW} bind:clientHeight={ctxH}>
      {#if !isImage}
        <div class="cdial" style="width:{dialPx}px; height:{dialPx}px">
          <ContextDial used={0} total={sel?.model.ctxTokens ?? 0} dim />
        </div>
      {/if}
      <div class="ctext">
        {#if isImage}
          <span class="h">OUTPUT</span>
          <span class="v">default image size</span>
          <span class="osz">{sel?.model.imageSize ? sel.model.imageSize.replace(/\s*x\s*/i, ' × ') : '—'}</span>
        {:else}
          <span class="h">CONTEXT</span>
          <span class="v">{sel?.model.ctxTokens ? `${fmtCtx(sel.model.ctxTokens)} preset` : 'no preset'}{sel?.model.kvType ? ` · KV ${sel.model.kvType}` : ''}</span>
        {/if}
        <div class="last">
          <span class="lbl">
            {lastKind === 'image' ? 'LAST IMAGE' : 'LAST DECODE'}{#if ls}<span class="who">&nbsp;· {shortLabel(lsLabel)}</span>{/if}
          </span>
          <div class="lrow">
            <div class="ldrum" style="--cells:{lastValue >= 99.95 ? 4 : 3}">
              <Drum value={lastValue} intDigits={lastValue >= 99.95 ? 3 : 2} narrowLead={lastValue < 9.95} blank={lastKind === 'none'} />
            </div>
            <span class="lu">{lastKind === 'image' ? 's/img' : 'tok/s'}</span>
          </div>
          <span class="lcap">{lastKind === 'none' ? 'no previous session' : 'median of the last session'}</span>
        </div>
      </div>
    </section>
    <section class="panel go">
      <button class="btn primary launch" disabled={!ready} onclick={() => actions.launch(vm.selected)}>
        <span class="play"><Icon name="play" size="calc(34 * var(--u))" /></span>
        <span class="lt">
          <span class="l1">Launch {sel?.label ?? ''}</span>
          {#if !ready && sel}<span class="l2">cannot launch: {availLabel(sel.availability)}</span>{/if}
        </span>
      </button>
      <button class="btn tune" onclick={() => actions.openTune(vm.selected)}>
        <Icon name="tune" size="calc(34 * var(--u))" />
        <span>Tune</span>
      </button>
    </section>
    <SysPanel system={vm.system} />
  </div>
</div>

<section class="panel lastp">
  <span class="lbl big">LAST SESSION</span>
  <span class="vsep"></span>
  {#if ls}
    <span class="ltxt" title={lastLine}>{lastLine}</span>
    <span class="end" class:hot={ls.ended === 'fault'}>{ls.ended === 'fault' ? 'faulted' : 'stopped'} {fmtAgo(ls.endedAgoS)}</span>
  {:else}
    <span class="ltxt dim">none since KLIF started</span>
  {/if}
</section>

<style>
  .dials {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1.16fr) minmax(0, 1fr);
    gap: calc(4 * var(--u));
  }
  .vram {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(10 * var(--u));
    overflow: hidden;
  }
  .rcol {
    display: flex;
    flex-direction: column;
    gap: calc(4 * var(--u));
    min-height: 0;
  }
  .ctx {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: calc(18 * var(--u));
    padding: calc(18 * var(--u)) calc(18 * var(--u)) calc(18 * var(--u)) calc(20 * var(--u));
    overflow: hidden;
  }
  .cdial {
    flex: 0 0 auto;
    align-self: flex-start;
    margin-top: calc(8 * var(--u));
  }
  .ctext {
    flex: 1 1 auto;
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: calc(12 * var(--u));
    min-width: 0;
    padding-top: calc(40 * var(--u));
  }
  .ctext .h {
    font-weight: 600;
    font-size: calc(31 * var(--u));
    letter-spacing: 0.04em;
    line-height: 1;
  }
  .ctext .v {
    font-size: calc(19 * var(--u));
    letter-spacing: 0.03em;
    line-height: 1.3;
    color: var(--cream);
  }
  .osz {
    font-weight: 500;
    font-size: calc(46 * var(--u));
    line-height: 1;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  .last {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: calc(8 * var(--u));
    padding: calc(12 * var(--u)) calc(14 * var(--u));
    border-radius: calc(5 * var(--u));
    background: rgba(0, 0, 0, 0.16);
    border: 1px solid #0a0b0c;
    box-shadow:
      inset 0 1px 3px rgba(0, 0, 0, 0.5),
      0 1px 0 rgba(255, 255, 255, 0.04);
  }
  .last .lbl {
    font-size: calc(16 * var(--u));
    color: var(--cream);
  }
  .who {
    color: var(--cream-2);
  }
  .lrow {
    display: flex;
    align-items: flex-end;
    gap: calc(12 * var(--u));
  }
  .ldrum {
    height: calc(68 * var(--u));
    width: calc((var(--cells) * 34 + 16 + 10) * var(--u));
    flex: 0 0 auto;
    font-size: calc(56 * var(--u));
    --drum-gap: calc(3 * var(--u));
    --drum-pad: calc(4 * var(--u));
    --dot-w: calc(16 * var(--u));
    --digit-dy: 0.015em;
  }
  .lu {
    font-size: calc(23 * var(--u));
    white-space: nowrap;
    font-weight: 500;
    margin-bottom: calc(8 * var(--u));
  }
  .lcap {
    font-size: calc(14.5 * var(--u));
    color: var(--muted);
    letter-spacing: 0.03em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .go {
    flex: 0 0 calc(150 * var(--u));
    display: flex;
    align-items: stretch;
    gap: calc(14 * var(--u));
    padding: calc(20 * var(--u));
  }
  .launch {
    flex: 1 1 auto;
    height: auto;
    justify-content: flex-start;
    gap: calc(16 * var(--u));
    padding: 0 calc(18 * var(--u));
    border-width: calc(2 * var(--u));
    background: linear-gradient(180deg, #24272c, #191b1e);
  }
  .launch:not([disabled]) {
    box-shadow:
      inset 0 0 0 1px rgba(90, 182, 235, 0.35),
      0 0 calc(16 * var(--u)) rgba(90, 182, 235, 0.32);
  }
  .play {
    display: flex;
    color: var(--cyan);
    filter: drop-shadow(0 0 calc(6 * var(--u)) rgba(90, 182, 235, 0.55));
  }
  .launch[disabled] .play {
    color: var(--muted);
    filter: none;
  }
  /* A tier that cannot launch: an inert key, but its reason stays fully legible. */
  .launch[disabled] {
    opacity: 1;
    border-color: rgba(237, 230, 214, 0.16);
    box-shadow: none;
  }
  .launch[disabled] .l1 {
    color: var(--muted);
  }
  .lt {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: calc(6 * var(--u));
    min-width: 0;
  }
  .l1 {
    font-weight: 600;
    font-size: calc(24 * var(--u));
    letter-spacing: 0.03em;
    line-height: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .l2 {
    font-size: calc(16 * var(--u));
    color: var(--orange);
    line-height: 1;
  }
  .tune {
    flex: 0 0 calc(96 * var(--u));
    height: auto;
    flex-direction: column;
    gap: calc(10 * var(--u));
    font-size: calc(22 * var(--u));
  }
  .lastp {
    flex: 0 0 auto;
    height: calc(64 * var(--u));
    display: flex;
    align-items: center;
    gap: calc(20 * var(--u));
    padding: 0 calc(26 * var(--u));
  }
  .big {
    font-size: calc(17 * var(--u));
    letter-spacing: 0.06em;
    color: var(--cream);
  }
  .lastp .vsep {
    height: calc(34 * var(--u));
    align-self: center;
  }
  .ltxt {
    flex: 1 1 auto;
    min-width: 0;
    font-size: calc(20 * var(--u));
    letter-spacing: 0.035em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ltxt.dim {
    color: var(--muted);
  }
  .end {
    flex: 0 0 auto;
    font-size: calc(17 * var(--u));
    color: var(--cream-2);
    white-space: nowrap;
  }
  .end.hot {
    color: var(--orange);
  }
</style>
