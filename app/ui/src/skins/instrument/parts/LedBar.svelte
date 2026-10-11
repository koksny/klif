<script lang="ts">
  // Horizontal LED segment bar. Lit count = round(fraction * segments); the caller prints the value.
  // dark: every segment unlit (a meter with nothing to measure, e.g. while the GPU is asleep).
  // tone: amber for the GPU wake-up (restore progress); cyan everywhere else.
  // indeterminate: no fraction to show (a song whose model loads): a short run of lit segments chases along.
  let {
    fraction,
    segments = 10,
    label = '',
    dark = false,
    tone = 'cyan',
    indeterminate = false,
  }: { fraction: number; segments?: number; label?: string; dark?: boolean; tone?: 'cyan' | 'amber'; indeterminate?: boolean } = $props();

  const lit = $derived(dark ? 0 : Math.round(Math.min(1, Math.max(0, fraction || 0)) * segments));
</script>

<div
  class="leds"
  class:amber={tone === 'amber'}
  class:chase={indeterminate && !dark}
  role="meter"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={indeterminate || dark ? undefined : Math.round((fraction || 0) * 100)}
>
  {#each { length: segments } as _, i (i)}
    <span class="seg" class:on={!indeterminate && i < lit} style={indeterminate ? `--i:${i};--n:${segments}` : undefined}></span>
  {/each}
</div>

<style>
  .leds {
    display: flex;
    gap: var(--seg-gap, 3px);
    height: 100%;
    min-width: 0;
  }
  .seg {
    flex: 1 1 0;
    min-width: 0;
    border-radius: var(--seg-radius, 2px);
    background: var(--seg-off, #33353a);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
  }
  .seg.on {
    background: var(--cyan, #5ab6eb);
    box-shadow:
      0 0 var(--seg-glow, 6px) rgba(90, 182, 235, 0.45),
      inset 0 1px 0 rgba(255, 255, 255, 0.25);
  }
  .leds.chase .seg {
    animation: chase 1.8s linear infinite;
    animation-delay: calc(var(--i) * 1.8s / var(--n) * -1);
  }
  @keyframes chase {
    0%,
    20% {
      background: var(--cyan, #5ab6eb);
      box-shadow:
        0 0 var(--seg-glow, 6px) rgba(90, 182, 235, 0.45),
        inset 0 1px 0 rgba(255, 255, 255, 0.25);
    }
    21%,
    100% {
      background: var(--seg-off, #33353a);
      box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .leds.chase .seg {
      animation: none;
    }
  }
  .leds.amber .seg.on {
    background: var(--amber, #ffb02e);
    box-shadow:
      0 0 var(--seg-glow, 6px) rgba(255, 176, 46, 0.45),
      inset 0 1px 0 rgba(255, 255, 255, 0.3);
  }
</style>
