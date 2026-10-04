<script lang="ts">
  // How one record climbed: a step line through every broken record, best at the top (for seconds, too).
  import type { RecordMetric } from '../../model/types';
  import { fmtDate, fmtValue, metricMeta } from './metrics';

  let { points, metric, now }: { points: { at: number; value: number }[]; metric: RecordMetric; now: number } = $props();

  const W = 300;
  const H = 110;
  const PAD = { l: 4, r: 4, t: 12, b: 18 };
  const higher = $derived(metricMeta(metric).higher);
  const xs = $derived(points.map((p) => p.at));
  const vs = $derived(points.map((p) => p.value));
  const t0 = $derived(Math.min(...xs));
  const t1 = $derived(Math.max(now, ...xs));
  const lo = $derived(Math.min(...vs));
  const hi = $derived(Math.max(...vs));
  const X = (t: number) => PAD.l + (t1 > t0 ? (t - t0) / (t1 - t0) : 1) * (W - PAD.l - PAD.r);
  const Y = (v: number) => {
    const f = hi > lo ? (v - lo) / (hi - lo) : 0.5;
    return PAD.t + (higher ? 1 - f : f) * (H - PAD.t - PAD.b);
  };
  const d = $derived.by(() => {
    if (!points.length) return '';
    let s = `M${X(points[0].at).toFixed(1)},${Y(points[0].value).toFixed(1)}`;
    for (const p of points.slice(1)) s += ` H${X(p.at).toFixed(1)} V${Y(p.value).toFixed(1)}`;
    return s + ` H${X(t1).toFixed(1)}`;
  });
</script>

{#if points.length > 1}
  <svg viewBox="0 0 {W} {H}" class="climb" role="img" aria-label="How the record improved">
    <line x1={PAD.l} x2={W - PAD.r} y1={Y(higher ? hi : lo)} y2={Y(higher ? hi : lo)} class="grid" />
    <line x1={PAD.l} x2={W - PAD.r} y1={Y(higher ? lo : hi)} y2={Y(higher ? lo : hi)} class="grid" />
    <text x={W - PAD.r} y={Y(higher ? hi : lo) - 3} class="yl" text-anchor="end">{fmtValue(metric, higher ? hi : lo)}</text>
    <text x={W - PAD.r} y={Y(higher ? lo : hi) - 3} class="yl" text-anchor="end">{fmtValue(metric, higher ? lo : hi)}</text>
    <path {d} class="line" />
    {#each points as p, i}<circle cx={X(p.at)} cy={Y(p.value)} r={i === points.length - 1 ? 3.4 : 2.2} class:last={i === points.length - 1} />{/each}
    <text x={PAD.l} y={H - 4} class="xl">{fmtDate(t0, now)}</text>
    <text x={W - PAD.r} y={H - 4} class="xl" text-anchor="end">{fmtDate(points[points.length - 1].at, now)}</text>
  </svg>
{:else}
  <p class="one">The first value: the climb starts with the next record.</p>
{/if}

<style>
  .climb {
    width: 100%;
    height: auto;
    overflow: visible;
  }
  .grid {
    stroke: var(--k-line, #2c363c);
    stroke-dasharray: 2 3;
  }
  .line {
    fill: none;
    stroke: var(--k-accent, #5ab6eb);
    stroke-width: 1.8;
  }
  circle {
    fill: var(--k-accent, #5ab6eb);
  }
  circle.last {
    fill: var(--k-record, #f2a33a);
  }
  .yl,
  .xl {
    fill: var(--k-muted, #8a8a8a);
    font: 10px var(--k-font-data, monospace);
  }
  .one {
    margin: 0;
    font: 12px var(--k-font-ui, system-ui, sans-serif);
    color: var(--k-muted, #8a8a8a);
  }
</style>
