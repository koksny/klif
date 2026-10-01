<script lang="ts">
  // Decode-speed history trace (one sample per second). The vertical range is printed on the right
  // so the trace never pretends to start at zero when it does not.
  let { data, unit = 'tok/s' }: { data: number[]; unit?: string } = $props();

  const uid = $props.id();
  let w = $state(0);
  let h = $state(0);

  const range = $derived.by(() => {
    if (!data.length) return { lo: 0, hi: 1 };
    let mn = Infinity;
    let mx = -Infinity;
    for (const v of data) {
      if (v < mn) mn = v;
      if (v > mx) mx = v;
    }
    let lo = Math.max(0, Math.floor(mn - 1));
    let hi = Math.ceil(mx + 1);
    if (hi - lo < 4) hi = lo + 4;
    return { lo, hi };
  });

  const geo = $derived.by(() => {
    const n = data.length;
    if (!w || !h || n < 2) return null;
    const padT = 8;
    const padB = 4;
    const x = (i: number) => (i / (n - 1)) * (w - 6);
    const y = (v: number) => padT + (1 - (v - range.lo) / (range.hi - range.lo)) * (h - padT - padB);
    let line = '';
    for (let i = 0; i < n; i++) line += `${i ? 'L' : 'M'}${x(i).toFixed(1)},${y(data[i]).toFixed(1)}`;
    const area = `${line}L${x(n - 1).toFixed(1)},${h}L0,${h}Z`;
    return { line, area, ex: x(n - 1), ey: y(data[n - 1]) };
  });
</script>

<div class="trace">
  <div class="plot" bind:clientWidth={w} bind:clientHeight={h}>
    {#if geo}
      <svg width={w} height={h} viewBox="0 0 {w} {h}" aria-hidden="true">
        <defs>
          <linearGradient id="{uid}-a" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stop-color="rgba(90,182,235,0.26)" />
            <stop offset="1" stop-color="rgba(90,182,235,0.02)" />
          </linearGradient>
        </defs>
        <path d={geo.area} fill="url(#{uid}-a)" />
        <path d={geo.line} class="ln" />
        <circle cx={geo.ex} cy={geo.ey} r="6" fill="rgba(90,182,235,0.3)" />
        <circle cx={geo.ex} cy={geo.ey} r="3" fill="#dff3ff" />
      </svg>
    {/if}
  </div>
  <div class="axis" aria-hidden="true">
    <span>{range.hi}</span>
    <span class="u">{unit}</span>
    <span>{range.lo}</span>
  </div>
</div>

<style>
  .trace {
    display: flex;
    height: 100%;
    min-width: 0;
    gap: calc(8 * var(--u));
  }
  .plot {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    border-left: 1px solid rgba(237, 230, 214, 0.18);
    border-bottom: 1px solid rgba(237, 230, 214, 0.18);
    overflow: hidden;
  }
  svg {
    position: absolute;
    inset: 0;
    display: block;
  }
  .ln {
    fill: none;
    stroke: #5ab6eb;
    stroke-width: 2;
    stroke-linejoin: round;
  }
  .axis {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    font-size: max(11px, calc(14 * var(--u)));
    color: rgba(237, 230, 214, 0.55);
    line-height: 1;
    text-align: right;
  }
  .axis .u {
    font-size: max(10px, calc(12 * var(--u)));
    color: rgba(237, 230, 214, 0.38);
  }
</style>
