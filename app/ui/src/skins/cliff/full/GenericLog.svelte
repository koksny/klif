<script lang="ts">
  // The request strip for kinds that report no per-request records: the frame stays (same height as the request
  // timeline and the recent jobs), with one honest line.
  import type { BlockView } from '../util';

  let { view, requests = 0, lastText = '', lastFault = false }: { view: BlockView; requests?: number; lastText?: string; lastFault?: boolean } = $props();

  const note = $derived(
    view === 'idle' ? '· not running' : view === 'loading' ? '· starting' : view === 'fault' ? '· failed' : `· ${requests} served this session, no per-request log for this kind`,
  );
</script>

<section class="c-track c-panel">
  <div class="c-tmain">
    <div class="c-tcap">
      <span class="c-lbl">Requests</span><span class="note">{note}</span>
      {#if view === 'idle' && lastText}<span class="last" class:bad={lastFault}>last session: {lastText}</span>{/if}
    </div>
    <div class="c-tbars"><div class="c-tempty"></div></div>
  </div>
</section>
