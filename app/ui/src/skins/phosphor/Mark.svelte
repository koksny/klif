<script lang="ts">
  // The koksny.com mark (sky-cyan K with a white pivot dot on a dark rounded square), redrawn as
  // vectors from docs/assets/koksny-mark.png so it stays crisp at any size. The plate carries the
  // Phosphor panel rule as its outline.
  let { size = 64 }: { size?: number } = $props();
</script>

<svg class="mark" width={size} height={size} viewBox="0 0 256 256" aria-hidden="true">
  <rect x="4" y="4" width="248" height="248" rx="52" class="plate" />
  <g class="k">
    <polygon points="106,46 106,114 126,129 124,136 106,152 106,170 68,206 68,78" />
    <polygon points="186,55 178,101 137,134 114,113" />
    <polygon points="136,136 188,175 188,214 114,152" />
  </g>
  <circle cx="129" cy="133" r="6" class="pin" />
</svg>

<style>
  .mark {
    display: block;
  }
  .plate {
    fill: #0a1419;
    stroke: var(--ph-rule);
    stroke-width: 5;
  }
  .k polygon {
    fill: var(--ph-brand);
  }
  .pin {
    fill: var(--ph-hot);
  }
</style>
