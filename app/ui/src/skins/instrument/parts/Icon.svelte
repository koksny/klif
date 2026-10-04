<script lang="ts">
  // Screen-printed line icons (24 x 24 grid).
  type Name =
    | 'ram' | 'cpu' | 'restart' | 'open' | 'console' | 'stop' | 'play' | 'tune' | 'records' | 'chev'
    | 'lock' | 'warn' | 'log' | 'back' | 'min' | 'max' | 'restore' | 'close' | 'screen' | 'window' | 'plus' | 'up' | 'down';
  let { name, size = '1em' }: { name: Name; size?: string } = $props();
</script>

<svg class="ic" viewBox="0 0 24 24" width={size} height={size} aria-hidden="true">
  {#if name === 'ram'}
    <path d="M2.5 7.5h19v8h-19z M4 15.5v2.5 M7 15.5v2.5 M10 15.5v2.5 M14 15.5v2.5 M17 15.5v2.5 M20 15.5v2.5" />
    <path d="M6 10h3v3H6z M10.5 10h3v3h-3z M15 10h3v3h-3z" class="fill" />
  {:else if name === 'cpu'}
    <path d="M6.5 6.5h11v11h-11z" />
    <path d="M9 9h6v6H9z" class="fill" />
    <path d="M9 3.5v3 M12 3.5v3 M15 3.5v3 M9 17.5v3 M12 17.5v3 M15 17.5v3 M3.5 9h3 M3.5 12h3 M3.5 15h3 M17.5 9h3 M17.5 12h3 M17.5 15h3" />
  {:else if name === 'restart'}
    <path d="M19 12a7 7 0 1 1-2.05-4.95" />
    <path d="M17.5 3.5v4h-4" />
  {:else if name === 'open'}
    <path d="M13.5 4.5h6v6 M19.5 4.5l-8.5 8.5" />
    <path d="M17 13.5v6h-12.5v-12.5h6" />
  {:else if name === 'console'}
    <path d="M4 6.5l5.5 5.5L4 17.5 M12 18h8" />
  {:else if name === 'stop'}
    <path d="M7 7h10v10H7z" class="fill" />
  {:else if name === 'play'}
    <path d="M8 5.5l11 6.5-11 6.5z" class="fill" />
  {:else if name === 'tune'}
    <path d="M4 7h10 M18 7h2 M4 17h3 M11 17h9" />
    <circle cx="16" cy="7" r="2" />
    <circle cx="9" cy="17" r="2" />
  {:else if name === 'records'}
    <path d="M4 20v-8h5v8 M9 20V4h6v16 M15 20V9h5v11" />
  {:else if name === 'chev'}
    <path d="M9 6l6 6-6 6" />
  {:else if name === 'lock'}
    <rect x="6" y="10.5" width="12" height="9.5" rx="1.6" class="fill" />
    <path d="M8.6 10.5V8a3.4 3.4 0 0 1 6.8 0v2.5" />
  {:else if name === 'warn'}
    <path d="M12 3.2L22 20.3H2z" />
    <path d="M12 9.3v5.6" />
    <circle cx="12" cy="17.4" r="1.05" class="fill" />
  {:else if name === 'log'}
    <path d="M6 3.5h8.5l3.5 3.5v13.5H6z M14.5 3.5V7H18" />
    <path d="M9 11h6 M9 14h6 M9 17h4" />
  {:else if name === 'back'}
    <path d="M19.5 12H5 M11 5.5L4.5 12l6.5 6.5" />
  {:else if name === 'min'}
    <path d="M5 12.5h14" />
  {:else if name === 'max'}
    <path d="M5.5 5.5h13v13h-13z" />
  {:else if name === 'restore'}
    <path d="M5.5 8.5h10v10h-10z M8.5 8.5V5.5h10v10h-3" />
  {:else if name === 'close'}
    <path d="M5.5 5.5l13 13 M18.5 5.5l-13 13" />
  {:else if name === 'screen'}
    <path d="M3 5h18v11.5H3z M9 20h6 M12 16.5V20" />
    <path d="M6.5 9h6 M6.5 12.5h3.5" />
  {:else if name === 'plus'}
    <path d="M12 5v14 M5 12h14" />
  {:else if name === 'up'}
    <path d="M6 15l6-6 6 6" />
  {:else if name === 'down'}
    <path d="M6 9l6 6 6-6" />
  {:else if name === 'window'}
    <path d="M3.5 5h17v14h-17z M3.5 9.5h17" />
    <path d="M6.4 7.25h.01 M9 7.25h.01" />
  {/if}
</svg>

<style>
  .ic {
    display: block;
    flex: 0 0 auto;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .ic :global(.fill) {
    fill: currentColor;
    stroke: none;
  }
</style>
