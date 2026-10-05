<script lang="ts">
  // "Will it fit": the System's GPU (vm.gpus, or the node's GPUs for a remote System) with what holds it now
  // (other processes, other running Systems) plus this System's expected layers (or its measured use while it
  // runs). Conflicts line: "Needs System 1 stopped" with the reason.
  import { isHeld } from '../../model/systems';
  import type { GpuMemory, System, ViewModel } from '../../model/types';
  import { fitLayers } from '../fit';
  import { labelsOf, listNames } from './util';

  interface Props {
    vm: ViewModel;
    system: System;
  }
  let { vm, system }: Props = $props();

  const gpus = $derived<GpuMemory[]>(system.node ? (vm.nodes.find((n) => n.id === system.node)?.gpus ?? []) : vm.gpus);
  const onCpu = $derived(system.gpu === 'cpu');
  // Its GPU; without a reading for it: the node's first GPU, or locally vm.vram ([gpu] inference).
  const gpu = $derived<GpuMemory | undefined>(
    onCpu ? undefined : (gpus.find((g) => g.id === system.gpu) ?? (system.node ? gpus[0] : vm.vram)),
  );
  const running = $derived(isHeld(system) && system.status !== 'fault');

  interface Seg {
    key: string;
    label: string;
    gib: number;
    tone: 'other' | 'peer' | 'self';
  }

  const fit = $derived.by(() => {
    if (!gpu) return null;
    const total = gpu.totalGiB;
    const peers = vm.systems.filter(
      (s) =>
        s.id !== system.id &&
        (s.node ?? '') === (system.node ?? '') &&
        s.session &&
        s.session.phase !== 'fault' &&
        (s.session.gpu ?? s.gpu) === gpu.id &&
        (s.session.vramGiB ?? 0) > 0,
    );
    const peerSegs: Seg[] = peers.map((s) => ({ key: `peer-${s.id}`, label: `${s.label} (running)`, gib: s.session?.vramGiB ?? 0, tone: 'peer' }));
    const peerSum = peerSegs.reduce((a, l) => a + l.gib, 0);
    const self = running ? (system.session?.vramGiB ?? 0) : 0;
    const foreign = Math.max(0, gpu.usedGiB - peerSum - self);
    const segs: Seg[] = [];
    if (foreign > 0.005) segs.push({ key: 'other', label: 'other (in use)', gib: foreign, tone: 'other' });
    segs.push(...peerSegs);
    let overGiB = 0;
    let trimmedGiB = 0;
    if (running) {
      if (self > 0) segs.push({ key: 'self', label: `${system.label} (now)`, gib: self, tone: 'self' });
    } else {
      const f = fitLayers(system.expectedVram ?? [], foreign + peerSum, total);
      overGiB = f.overGiB;
      trimmedGiB = f.trimmedGiB;
      for (const l of f.layers) segs.push({ key: `exp-${l.id}`, label: l.label, gib: l.gib, tone: 'self' });
    }
    const need = segs.reduce((a, l) => a + l.gib, 0);
    const free = total - need;
    // A server that keeps weights in system RAM takes the VRAM it finds: more than the card is not an overflow.
    const state = overGiB > 0 && !system.ramOffload ? 'over' : free < gpu.warnBelowGiB && !system.ramOffload ? 'tight' : 'ok';
    return { total, segs, need, free, overGiB, trimmedGiB, state };
  });

  const expected = $derived((system.expectedVram ?? []).length > 0);
  const caption = $derived(
    running
      ? 'Measured now.'
      : !expected
        ? 'Expected use unknown until it has run once.'
        : system.expectedVramSource === 'measured'
          ? 'Expected use, measured on its last run.'
          : 'Expected use, estimated from file sizes.',
  );
  const fitText = $derived.by(() => {
    if (!fit || running || !expected) return '';
    if (system.ramOffload) {
      return 'It keeps weights in system RAM and takes the VRAM it finds, so it fits; the more of the card is free, the faster it runs.';
    }
    if (fit.state === 'over') return `Over by ${fit.overGiB.toFixed(2)} GiB: the overflow spills to shared system memory.`;
    const trim = fit.trimmedGiB > 0 ? ` The loader trims its buffers by ${fit.trimmedGiB.toFixed(2)} GiB to fit.` : '';
    return fit.state === 'tight'
      ? `Fits with ${fit.free.toFixed(2)} GiB to spare. Very little headroom.${trim}`
      : `Fits with ${fit.free.toFixed(2)} GiB to spare.${trim}`;
  });
  const conflictNames = $derived(listNames(labelsOf(vm, system.conflicts)));
  const shades = [100, 74, 52, 34, 22];

  function shadeOf(s: Seg, i: number): number {
    if (s.tone === 'other') return 22;
    if (s.tone === 'peer') return 40;
    return shades[i % shades.length];
  }
</script>

<section class="fit" data-fit={fit?.state ?? 'ok'} aria-label="VRAM fit">
  {#if onCpu}
    <p class="fitnote">Runs on the CPU: no VRAM to fit.</p>
  {:else if !gpu || !fit}
    <p class="fitnote">No GPU reading for this System yet.</p>
  {:else}
    <div class="gpu">
      <span>{gpu.name || gpu.id}</span>
      <b>{fit.total.toFixed(2)} GiB</b>
    </div>
    <div class="fitbar" role="img" aria-label="VRAM: {fit.need.toFixed(2)} of {fit.total.toFixed(2)} GiB">
      {#each fit.segs as s, i (s.key)}
        <span class="seg {s.tone}" style="flex: {s.gib} 0 0; --shade: {shadeOf(s, i)}%" title="{s.label} {s.gib.toFixed(2)} GiB"></span>
      {/each}
      <span class="seg free" style="flex: {Math.max(0, fit.free)} 0 0"></span>
    </div>
    <ul class="legend">
      {#each fit.segs as s, i (s.key)}
        <li><i style="--shade: {shadeOf(s, i)}%" class={s.tone}></i>{s.label}<b>{s.gib.toFixed(2)}</b></li>
      {/each}
      <li class="sum">of {fit.total.toFixed(2)} GiB<b>{fit.need.toFixed(2)}</b></li>
    </ul>
    <p class="fitnote caption">{caption}</p>
    {#if fitText}<p class="fitnote verdict">{fitText}</p>{/if}
  {/if}
  {#if system.conflicts.length && !running}
    <!-- The engine's reason is the conflict sentence when there is one; else name the Systems. -->
    <p class="conflicts"><b>{system.reason && system.status === 'offline' ? system.reason : `Needs ${conflictNames} stopped.`}</b></p>
  {/if}
</section>

<style>
  .fit {
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
    padding: 12px;
    background: var(--k-surface, #141414);
  }
  .gpu {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 8px;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-ui, system-ui, sans-serif);
  }
  .gpu b {
    color: var(--k-ink, #e6e6e6);
    font-weight: 500;
  }
  .fitbar {
    display: flex;
    height: 14px;
    border-radius: 3px;
    overflow: hidden;
    gap: 2px;
  }
  .seg {
    background: color-mix(in srgb, var(--k-accent, #5ab6eb) var(--shade, 100%), var(--k-surface, #141414));
    min-width: 0;
  }
  .seg.other,
  .seg.peer {
    background: color-mix(in srgb, var(--k-muted, #8a8a8a) var(--shade, 40%), var(--k-surface, #141414));
  }
  .seg.free {
    background: transparent;
    outline: 1px dashed var(--k-line, #2e2e2e);
    outline-offset: -1px;
  }
  [data-fit='over'] .seg.self {
    background: color-mix(in srgb, var(--k-danger, #e05a5a) var(--shade, 100%), var(--k-surface, #141414));
  }
  [data-fit='tight'] .verdict {
    color: var(--k-warn, #f2a33a);
  }
  [data-fit='over'] .verdict {
    color: var(--k-danger, #e05a5a);
  }
  .legend {
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
    display: grid;
    gap: 4px;
    font: 500 12px/1.3 var(--k-font-ui, system-ui, sans-serif);
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--k-muted, #8a8a8a);
  }
  .legend li b {
    margin-left: auto;
    font: 500 12px/1 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-ink, #e6e6e6);
  }
  .legend i {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--k-accent, #5ab6eb) var(--shade, 100%), var(--k-surface, #141414));
  }
  .legend i.other,
  .legend i.peer {
    background: color-mix(in srgb, var(--k-muted, #8a8a8a) var(--shade, 40%), var(--k-surface, #141414));
  }
  .legend .sum {
    border-top: 1px solid var(--k-line, #2e2e2e);
    padding-top: 6px;
    margin-top: 2px;
  }
  .fitnote {
    margin: 10px 0 0;
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .fitnote.verdict {
    margin-top: 4px;
  }
  .fitnote:first-child {
    margin-top: 0;
  }
  .conflicts {
    margin: 10px 0 0;
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
  .conflicts b {
    color: var(--k-warn, #f2a33a);
    font-weight: 600;
  }
</style>
