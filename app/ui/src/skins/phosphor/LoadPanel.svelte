<script lang="ts">
  // Loading: the radar's filled arc is loading.fraction (it sweeps only while the model loads), the
  // checklist is loading.steps, and the weights bar is the resident weights layer against the slot's
  // expected weights layer (both GiB, from the view model).
  import type { GpuMemory, LoadProgress, Slot } from '../../lib/model/types';
  import Radar from './Radar.svelte';
  import Steps from './Steps.svelte';
  import { clamp } from './geom';

  let { loading, slot, vram, k }: { loading: LoadProgress | null; slot: Slot | undefined; vram: GpuMemory; k: number } = $props();

  const frac = $derived(clamp(loading?.fraction ?? 0, 0, 1));
  const steps = $derived(loading?.steps ?? []);

  const weightsFrac = $derived.by(() => {
    const st = steps.find((x) => x.id === 'weights');
    if (!st || st.state === 'pending') return 0;
    if (st.state === 'done') return 1;
    const total = slot?.expectedVram?.find((l) => l.id === 'weights')?.gib ?? 0;
    const now = vram.layers.find((l) => l.id === 'weights')?.gib ?? 0;
    return total > 0 ? clamp(now / total, 0, 1) : null;
  });
</script>

<section class="panel loadp" aria-label="Load progress">
  <div class="rad">
    <Radar fraction={frac} turnsPerSec={0.32} size={Math.round(190 * k)} />
  </div>
  <span class="vrule" aria-hidden="true"></span>
  <div class="body">
    <div class="head">
      <span class="lbl">Load progress</span>
      <span class="pct">{Math.round(frac * 100)}<span class="ps">%</span></span>
    </div>
    <Steps {steps} {weightsFrac} />
  </div>
</section>

<style>
  .loadp {
    flex: none;
    height: calc(270px * var(--k));
    display: flex;
    align-items: center;
    gap: calc(26px * var(--k));
    padding: 0 calc(24px * var(--k)) 0 calc(20px * var(--k));
  }
  .rad {
    flex: none;
  }
  .vrule {
    flex: none;
    width: 1px;
    align-self: stretch;
    margin: calc(14px * var(--k)) 0;
    background: var(--ph-rule);
  }
  .body {
    flex: 1;
    min-width: 0;
    align-self: stretch;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    gap: calc(8px * var(--k));
    padding: calc(12px * var(--k)) 0 calc(10px * var(--k));
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  .pct {
    font-family: var(--ph-display);
    font-stretch: 112%;
    font-size: calc(34px * var(--k));
    line-height: 1;
    color: var(--ph-cyan);
    text-shadow:
      0 0 8px rgba(127, 227, 255, 0.55),
      0 0 22px rgba(90, 182, 235, 0.3);
    font-variant-numeric: tabular-nums;
  }
  .ps {
    font-size: 0.7em;
    margin-left: 0.06em;
  }
</style>
