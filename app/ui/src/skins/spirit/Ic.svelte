<script lang="ts">
  // OSD indicator marks drawn in CSS (IBM Plex Mono has no ◌ / ‖ / ■ glyphs of its own): sized by the
  // font-size, coloured by currentColor.
  import type { IcKind } from './osd.svelte';
  let { kind }: { kind: IcKind } = $props();
</script>

{#if kind === 'restart'}<i class="ic txt" aria-hidden="true">↻</i>{:else if kind}<i class="ic {kind}" aria-hidden="true"></i>{/if}

<style>
  .ic {
    display: inline-block;
    flex: none;
    box-sizing: border-box;
    font-style: normal;
  }
  .txt {
    line-height: 1;
  }
  .dot {
    width: 0.62em;
    height: 0.62em;
    border-radius: 50%;
    background: currentColor;
  }
  .ring {
    width: 0.64em;
    height: 0.64em;
    border-radius: 50%;
    border: max(1.5px, 0.09em) dashed currentColor;
  }
  .square {
    width: 0.54em;
    height: 0.54em;
    background: currentColor;
  }
  .pause {
    width: 0.5em;
    height: 0.62em;
    background: linear-gradient(90deg, currentColor 0 32%, transparent 32% 68%, currentColor 68%);
  }
  .play {
    width: 0;
    height: 0;
    border-left: 0.54em solid currentColor;
    border-top: 0.32em solid transparent;
    border-bottom: 0.32em solid transparent;
  }
</style>
