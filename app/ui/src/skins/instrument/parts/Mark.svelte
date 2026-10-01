<script lang="ts">
  // The koksny.com mark (sky-cyan K with a white pivot dot on a dark rounded square), redrawn as vector.
  let { size = '100%' }: { size?: string } = $props();
</script>

<svg class="mark" viewBox="0 0 256 256" width={size} height={size} aria-hidden="true">
  <rect x="0" y="0" width="256" height="256" rx="58" fill="#11141B" />
  <g fill="#5AB6EB">
    <polygon points="106,46 106,114 126,129 124,136 106,152 106,170 68,206 68,78" />
    <polygon points="186,55 178,101 137,134 114,113" />
    <polygon points="136,136 188,175 188,214 114,152" />
  </g>
  <circle cx="129" cy="133" r="6" fill="#F2F4F6" />
</svg>

<style>
  .mark {
    display: block;
  }
</style>
