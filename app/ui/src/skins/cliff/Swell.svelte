<script lang="ts">
  // The swell line: llm.decodeHistory (1 sample/s, up to 300 = 5 min). The x axis is fixed at
  // 5 minutes (newest at the right edge). The y range is rounded to 10 tok/s steps and spans at
  // least the top 40 % of the scale, so small ripples stay small; it is stated in the caption.
  let {
    history,
    variant = 'full',
  }: { history: number[]; variant?: 'full' | 'mini' } = $props();

  const N = 300;
  let w = $state(0);
  let h = $state(0);

  const range = $derived.by(() => {
    if (!history.length) return null;
    let mn = Infinity;
    let mx = -Infinity;
    for (const v of history) {
      if (v < mn) mn = v;
      if (v > mx) mx = v;
    }
    const hi = Math.max(10, Math.ceil((mx + 0.5) / 10) * 10);
    const lo = Math.max(0, Math.floor(Math.min(mn, hi * 0.6) / 10) * 10);
    return { lo, hi };
  });

  // Light centred moving average; the newest sample stays exact.
  function smooth(a: number[], r: number): number[] {
    const out = new Array<number>(a.length);
    for (let i = 0; i < a.length; i++) {
      const k = Math.min(r, i, a.length - 1 - i);
      let s = 0;
      for (let j = i - k; j <= i + k; j++) s += a[j];
      out[i] = s / (2 * k + 1);
    }
    return out;
  }

  const pad = $derived(variant === 'mini' ? 8 : 7);

  const geo = $derived.by(() => {
    if (!range || w <= 0 || h <= 0) return null;
    const { lo, hi } = range;
    const n = Math.min(history.length, N);
    // Mini is read from 1 m: a 25 s centred mean turns it into a trend line (the end stays exact).
    const src = smooth(history.slice(-n), variant === 'mini' ? 12 : 2);
    const x1 = w - pad;
    const x0 = 0;
    const yOf = (v: number) => pad + (1 - (v - lo) / (hi - lo)) * (h - 2 * pad);
    let d = '';
    for (let i = 0; i < n; i++) {
      const x = x1 - ((n - 1 - i) / (N - 1)) * (x1 - x0);
      d += (i ? 'L' : 'M') + x.toFixed(1) + ' ' + yOf(src[i]).toFixed(1);
    }
    return { d, ex: x1, ey: yOf(src[n - 1]) };
  });
</script>

<div class="swell {variant}">
  <div class="plot" bind:clientWidth={w} bind:clientHeight={h}>
    {#if geo}
      <svg width={w} height={h} viewBox="0 0 {w} {h}" aria-hidden="true">
        <path d={geo.d} class="glow" />
        <path d={geo.d} class="line" />
        <circle cx={geo.ex} cy={geo.ey} r={variant === 'mini' ? 9 : 8} class="halo" />
        <circle cx={geo.ex} cy={geo.ey} r={variant === 'mini' ? 5 : 4.5} class="dot" />
      </svg>
    {/if}
  </div>
  {#if variant === 'full'}
    <div class="cap">
      5-minute history{#if range}<span class="rng">{` · ${range.lo}–${range.hi} tok/s`}</span>{/if}
    </div>
  {/if}
</div>

<style>
  .swell {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-width: 0;
  }
  .plot {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
  }
  svg {
    position: absolute;
    inset: 0;
    overflow: visible;
  }
  .line {
    fill: none;
    stroke: var(--sky);
    stroke-width: 1.8;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .glow {
    fill: none;
    stroke: var(--sky);
    stroke-opacity: 0.16;
    stroke-width: 6;
    stroke-linejoin: round;
  }
  .mini .line {
    stroke-width: 2.6;
  }
  .mini .glow {
    stroke-width: 9;
  }
  .halo {
    fill: var(--sky);
    fill-opacity: 0.22;
  }
  .dot {
    fill: var(--sky);
  }
  .cap {
    align-self: flex-end;
    font-family: var(--f-ui);
    font-size: var(--fs-s);
    color: var(--muted);
    letter-spacing: 0.01em;
    white-space: nowrap;
  }
  .rng {
    color: #74868f;
  }
</style>
