<script lang="ts">
  // Context dial, needle = usedTokens / totalTokens. Full: 270 degree sweep (7:30 to 4:30) with the
  // end values printed. Mini: 180 degree sweep (9 to 3 o'clock, as in the mockup) so the big percent
  // readout sits in clear face below the scale.
  import { fmtCtx } from '../../../lib/model/format';
  import { frac, polar } from '../theme';

  // Also the GENERATION dial of image slots: majors = one major tick per sampling step, hiLabel = steps.
  let {
    used,
    total,
    variant = 'full',
    hiLabel,
    majors = 4,
    minorPer = 10,
    dim = false,
    label = 'Context',
  }: {
    used: number;
    total: number;
    variant?: 'full' | 'mini';
    hiLabel?: string;
    majors?: number;
    minorPer?: number;
    /** No live reading (a preset or a stopped session): the needle is drawn unlit. */
    dim?: boolean;
    label?: string;
  } = $props();

  const uid = $props.id();
  const C = 200;
  const f = $derived(frac(used, total));
  const sweep = $derived(variant === 'mini' ? { a0: 180, span: 180 } : { a0: 225, span: 270 });
  // The needle is drawn pointing up (12 o'clock = 90 deg); rotate it clockwise to the value.
  const clockDeg = $derived(90 - sweep.a0 + sweep.span * f);
  // polar angle (CCW from +x) for a fraction of the sweep
  const pa = (x: number) => sweep.a0 - sweep.span * x;

  const ticks = $derived.by(() => {
    const out: { x1: number; y1: number; x2: number; y2: number; major: boolean }[] = [];
    const n = variant === 'mini' ? 6 : Math.max(1, majors) * Math.max(1, minorPer);
    const every = variant === 'mini' ? 1 : Math.max(1, minorPer);
    for (let i = 0; i <= n; i++) {
      const major = i % every === 0;
      if (variant === 'mini' && !major && i % 1 !== 0) continue;
      const a = pa(i / n);
      const rOut = variant === 'mini' ? 162 : 164;
      const rIn = variant === 'mini' ? (major ? 136 : 146) : major ? 140 : 154;
      const [x1, y1] = polar(C, C, rOut, a);
      const [x2, y2] = polar(C, C, rIn, a);
      out.push({ x1, y1, x2, y2, major });
    }
    return out;
  });
  const lo = $derived(polar(C, C, 120, pa(0)));
  const hi = $derived(polar(C, C, 120, pa(1)));
</script>

<div class="dialbox">
<svg class="ctx {variant}" viewBox="0 0 400 400" role="img" aria-label="{label} {Math.round(f * 100)}%">
  <defs>
    <linearGradient id="{uid}-bz" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#3b3c40" />
      <stop offset="0.5" stop-color="#232427" />
      <stop offset="1" stop-color="#111214" />
    </linearGradient>
    <radialGradient id="{uid}-fc" cx="50%" cy="45%" r="55%">
      <stop offset="0" stop-color="#1f2023" />
      <stop offset="0.85" stop-color="#17181a" />
      <stop offset="1" stop-color="#0f1011" />
    </radialGradient>
  </defs>
  <circle cx={C} cy={C} r="199" fill="#0b0c0d" />
  <circle cx={C} cy={C} r="194" fill="url(#{uid}-bz)" />
  <circle cx={C} cy={C} r="180" fill="#0a0b0c" />
  <circle cx={C} cy={C} r="176" fill="url(#{uid}-fc)" />
  {#each ticks as t, i (i)}
    <line x1={t.x1} y1={t.y1} x2={t.x2} y2={t.y2} class="tk" class:major={t.major} />
  {/each}
  {#if variant === 'full'}
    <text x={lo[0] + 6} y={lo[1] + 4} class="end">0</text>
    <text x={hi[0] - 6} y={hi[1] + 4} class="end">{hiLabel ?? fmtCtx(total)}</text>
  {:else}
    <text x={C} y="318" class="pct">{Math.round(f * 100)}%</text>
  {/if}
</svg>
<div class="nlayer" style="transform: rotate({clockDeg}deg)" aria-hidden="true">
  <svg class="ctx" viewBox="0 0 400 400">
    {#if variant === 'full'}
      <path d="M200,222 L191,208 L196,58 L200,48 L204,58 L209,208 Z" fill="#0b0c0d" stroke="#0b0c0d" stroke-width="5" stroke-linejoin="round" />
      <path d="M200,222 L191,208 L196,58 L200,48 L204,58 L209,208 Z" fill={dim ? '#6f6b64' : '#5AB6EB'} />
      <circle cx={C} cy={C} r="21" fill="#0b0c0d" />
      <circle cx={C} cy={C} r="16" fill={dim ? '#6f6b64' : '#5AB6EB'} />
      <circle cx={C - 4} cy={C - 5} r="5" fill="rgba(255,255,255,0.45)" />
    {:else}
      <path d="M200,224 L192,210 L197,62 L200,52 L203,62 L208,210 Z" fill="#0d0e0f" />
      <path d="M197.6,110 L197,62 L200,52 L203,62 L202.4,110 Z" fill={dim ? '#6f6b64' : '#5AB6EB'} />
      <circle cx={C} cy={C} r="24" fill="#0b0c0d" />
      <circle cx={C} cy={C} r="19" fill="#2a2b2e" />
    {/if}
  </svg>
</div>
</div>

<style>
  .ctx {
    display: block;
    width: 100%;
    height: 100%;
    font-family: var(--font-label);
  }
  .tk {
    stroke: rgba(237, 230, 214, 0.42);
    stroke-width: 1.8;
  }
  .tk.major {
    stroke: #ede6d6;
    stroke-width: 3.4;
  }
  .mini .tk {
    stroke: rgba(237, 230, 214, 0.85);
    stroke-width: 4;
  }
  .mini .tk.major {
    stroke-width: 5;
  }
  .end {
    fill: rgba(237, 230, 214, 0.6);
    font-size: 23px;
    font-weight: 500;
    text-anchor: middle;
  }
  .pct {
    fill: #ede6d6;
    font-family: var(--font-hero);
    font-weight: 800;
    font-size: 76px;
    text-anchor: middle;
  }
  .dialbox {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .nlayer {
    position: absolute;
    inset: 0;
    transform-origin: 50% 50%;
    transition: transform 400ms cubic-bezier(0.3, 0.8, 0.3, 1);
    will-change: transform;
    pointer-events: none;
  }
</style>
