<script lang="ts">
  // 5-minute decode history. Zero-based scale; the grid IS the scale (10 tok/s x 30 s cells).
  let { values, capacity = 300 }: { values: number[]; capacity?: number } = $props();

  const VW = 1000;
  const VH = 100;

  const yMax = $derived.by(() => {
    let m = 0;
    for (const v of values) if (v > m) m = v;
    return Math.max(20, Math.ceil((m * 1.4) / 20) * 20);
  });

  const points = $derived.by(() => {
    const n = values.length;
    if (n === 0) return '';
    const off = capacity - n;
    let s = '';
    for (let i = 0; i < n; i++) {
      const x = ((off + i) / (capacity - 1)) * VW;
      const y = VH - (Math.max(0, values[i]) / yMax) * VH;
      s += `${x.toFixed(1)},${y.toFixed(2)} `;
    }
    return s;
  });

  const hLines = $derived(Array.from({ length: Math.max(0, yMax / 10 - 1) }, (_, i) => VH - ((i + 1) * 10 * VH) / yMax));
  const vLines = Array.from({ length: 9 }, (_, i) => ((i + 1) * VW) / 10);
</script>

<div class="spark">
  <svg viewBox="0 0 {VW} {VH}" preserveAspectRatio="none" aria-hidden="true">
    {#each hLines as y (y)}<line x1="0" x2={VW} y1={y} y2={y} class="g" />{/each}
    {#each vLines as x (x)}<line x1={x} x2={x} y1="0" y2={VH} class="g" />{/each}
    <rect x="0" y="0" width={VW} height={VH} class="frame" />
    {#if points}<polyline {points} class="trace" />{/if}
  </svg>
  <span class="scale">{yMax}</span>
</div>

<style>
  .spark {
    position: relative;
    width: 100%;
    height: 100%;
  }
  svg {
    display: block;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  .g {
    stroke: #1d2830;
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .frame {
    fill: none;
    stroke: #26333d;
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .trace {
    fill: none;
    stroke: #5ab6eb;
    stroke-width: 1.6;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
  .scale {
    position: absolute;
    right: calc(var(--u) * 4);
    top: calc(var(--u) * 2);
    font-size: max(9px, calc(var(--u) * 10));
    color: #6f8392;
    line-height: 1;
  }
</style>
