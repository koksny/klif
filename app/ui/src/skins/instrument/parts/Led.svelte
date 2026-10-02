<script lang="ts">
  // Round indicator LED. on = cyan, warn = orange, sleep = amber, off = dark lens. `pulse` breathes
  // the lens (a request waiting on a GPU that is still waking up); still when reduced motion is on.
  let {
    on = false,
    tone = 'cyan',
    size = '12px',
    title = '',
    pulse = false,
  }: { on?: boolean; tone?: 'cyan' | 'orange' | 'amber' | 'cream'; size?: string; title?: string; pulse?: boolean } = $props();
</script>

<span class="led {tone}" class:on class:pulse style="--s:{size}" {title} aria-hidden={title ? undefined : 'true'}></span>

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
  .led.on.amber {
    background: radial-gradient(circle at 50% 45%, #fff0c8 0%, #ffb02e 50%, #d98a0a 100%);
    box-shadow:
      0 0 calc(var(--s) * 0.7) rgba(255, 176, 46, 0.6),
      0 0 0 calc(var(--s) * 0.12) #0b0c0d;
  }
  .led.pulse {
    animation: breathe 1.1s ease-in-out infinite;
  }
  @keyframes breathe {
    50% {
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .led.pulse {
      animation: none;
    }
  }
  .led.on.cream {
    background: radial-gradient(circle at 50% 45%, #fffaf0 0%, #ede6d6 60%, #bdb6a6 100%);
    box-shadow: 0 0 0 calc(var(--s) * 0.12) #0b0c0d;
  }
</style>
