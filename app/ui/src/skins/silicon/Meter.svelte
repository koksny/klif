<script lang="ts">
  // Thin drafting meter. The fill animates by transform only.
  import { Tween, prefersReducedMotion } from 'svelte/motion';
  import { cubicOut } from 'svelte/easing';

  let { value, warn = false, tall = false }: { value: number; warn?: boolean; tall?: boolean } = $props();
  const t = Tween.of(() => Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0)), {
    duration: prefersReducedMotion.current ? 0 : 400,
    easing: cubicOut,
  });
</script>

<span class="meter" class:warn class:tall><span class="fill" style="transform: scaleX({t.current})"></span></span>

<style>
  .meter {
    position: relative;
    display: block;
    width: 100%;
    height: calc(var(--u) * 12);
    border: 1px solid #3a4c5a;
    border-radius: calc(var(--u) * 2);
    overflow: hidden;
    background: #0b1116;
  }
  .meter.tall {
    height: calc(var(--u) * 22);
  }
  .fill {
    position: absolute;
    inset: 1px;
    background: #5ab6eb;
    transform-origin: 0 50%;
  }
  .warn .fill {
    background: #ff5a36;
  }
</style>
