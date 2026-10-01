<script lang="ts">
  // Round indicator LED. on = cyan, warn = orange, off = dark lens.
  let { on = false, tone = 'cyan', size = '12px', title = '' }: { on?: boolean; tone?: 'cyan' | 'orange' | 'cream'; size?: string; title?: string } = $props();
</script>

<span class="led {tone}" class:on style="--s:{size}" {title} aria-hidden={title ? undefined : 'true'}></span>

<style>
  .led {
    display: inline-block;
    flex: 0 0 auto;
    width: var(--s);
    height: var(--s);
    border-radius: 50%;
    background: radial-gradient(circle at 50% 45%, #2b2d31 0%, #141517 75%);
    box-shadow:
      inset 0 1px 2px rgba(0, 0, 0, 0.8),
      0 0 0 calc(var(--s) * 0.12) #0b0c0d,
      0 1px 0 calc(var(--s) * 0.12) rgba(255, 255, 255, 0.05);
  }
  .led.on.cyan {
    background: radial-gradient(circle at 50% 45%, #bfe6fb 0%, #5ab6eb 45%, #3d9ed4 100%);
    box-shadow:
      0 0 calc(var(--s) * 0.7) rgba(90, 182, 235, 0.6),
      0 0 0 calc(var(--s) * 0.12) #0b0c0d;
  }
  .led.on.orange {
    background: radial-gradient(circle at 50% 45%, #ffd2bd 0%, #ff6b2c 50%, #d84e14 100%);
    box-shadow:
      0 0 calc(var(--s) * 0.7) rgba(255, 107, 44, 0.6),
      0 0 0 calc(var(--s) * 0.12) #0b0c0d;
  }
  .led.on.cream {
    background: radial-gradient(circle at 50% 45%, #fffaf0 0%, #ede6d6 60%, #bdb6a6 100%);
    box-shadow: 0 0 0 calc(var(--s) * 0.12) #0b0c0d;
  }
</style>
