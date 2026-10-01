<script lang="ts">
  // Horizontal LED segment bar. Lit count = round(fraction * segments); the caller prints the value.
  let {
    fraction,
    segments = 10,
    label = '',
  }: { fraction: number; segments?: number; label?: string } = $props();

  const lit = $derived(Math.round(Math.min(1, Math.max(0, fraction || 0)) * segments));
</script>

<div class="leds" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((fraction || 0) * 100)}>
  {#each { length: segments } as _, i (i)}
    <span class="seg" class:on={i < lit}></span>
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
</style>
