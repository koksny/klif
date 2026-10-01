<script lang="ts">
  // The two short transitional panels: stopping (the session is shutting down) and a live session
  // whose first telemetry has not arrived yet. Every other state has its own panel.
  import type { ViewModel } from '../../lib/model/types';
  import { fmtClock } from '../../lib/model/format';

  let { vm }: { vm: ViewModel } = $props();

  const s = $derived(vm.session);
  const slot = $derived(vm.slots.find((x) => x.id === (s?.slot ?? vm.selected)));
</script>

{#if s && s.phase === 'stopping'}
  <section class="panel grat state">
    <div class="lbl mut">Stopping</div>
    <div class="hero">{slot?.label ?? ''}</div>
    <div class="val sub">{s.model.name} · ran {fmtClock(s.uptimeS)}</div>
  </section>
{:else if s}
  <section class="panel grat state">
    <div class="lbl">Live</div>
    <div class="val sub">{s.model.name} · waiting for telemetry</div>
  </section>
{/if}

<style>
  .state {
    flex: 1.05 1 0;
    min-height: calc(160px * var(--k));
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: calc(14px * var(--k));
    padding: calc(16px * var(--k)) calc(24px * var(--k));
    overflow: hidden;
  }
  .hero {
    font-family: var(--ph-display);
    font-size: calc(64px * var(--k));
    line-height: 1;
    color: var(--ph-muted);
    letter-spacing: 0.04em;
    white-space: nowrap;
  }
  .sub {
    font-size: calc(20px * var(--k));
  }
</style>
