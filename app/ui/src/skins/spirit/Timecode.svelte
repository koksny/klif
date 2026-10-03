<script lang="ts">
  // Timecode HH:MM:SS:FF (24 frames) of the session clock. While recording the frames advance smoothly with
  // the frame clock between snapshots (never backwards); otherwise, and with no frames, it shows the snapshot.
  import { onMount, untrack } from 'svelte';
  import { onFrame } from '../../lib/render/scheduler';
  import { TC_IDLE, timecode } from './text';

  let { t, running }: { t: number | null; running: boolean } = $props();

  let shown = $state(untrack(() => (t === null ? TC_IDLE : timecode(t))));
  let base = 0;
  let at = 0;
  let cur = -1;

  $effect(() => {
    const v = t;
    const r = running;
    untrack(() => {
      if (v === null) {
        cur = -1;
        shown = TC_IDLE;
        return;
      }
      base = v;
      at = performance.now();
      // a new session, a long gap or a paused clock: take the snapshot as it is
      if (!r || cur < 0 || v > cur + 1.5 || v < cur - 2) cur = v;
      shown = timecode(Math.max(cur, v));
    });
  });

  onMount(() =>
    onFrame(
      (now) => {
        if (t === null || !running) return;
        const est = base + Math.min(1.2, (now - at) / 1000);
        if (est <= cur) return;
        cur = est;
        const next = timecode(cur);
        if (next !== shown) shown = next;
      },
      { maxFps: 24 },
    ),
  );
</script>

<span class="tc">{shown}</span>

<style>
  .tc {
    font-variant-numeric: tabular-nums;
    white-space: pre;
  }
</style>
