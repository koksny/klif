<script lang="ts">
  // One line, always shown: the running model, or (idle) the selected tier's.
  import type { ModelRef, SystemKind } from '../../../lib/model/types';
  import { modelFacts } from '../util';

  let { model, kind, label, dim = false }: { model: ModelRef | null; kind: SystemKind; label: string; dim?: boolean } = $props();
  const facts = $derived(model ? modelFacts(model, kind) : []);
</script>

<section class="ml c-panel" class:dim title={model ? `${model.engine} · ${facts.join(' · ')}` : ''}>
  <span class="c-lbl">{label}</span>
  <span class="line">
    {#each facts as f, i (i)}{#if i}<span class="sep" aria-hidden="true">·</span>{/if}<span class:name={i === 0}>{f}</span>{/each}
  </span>
</section>

<style>
  .ml {
    display: flex;
    align-items: center;
    gap: max(12px, calc(var(--u) * 16));
    height: max(28px, calc(var(--u) * 30));
    padding: 0 max(10px, calc(var(--u) * 14));
  }
  .ml .c-lbl {
    flex: none;
    min-width: max(70px, calc(var(--u) * 92));
  }
  .line {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-m);
    color: var(--mist);
  }
  .name {
    font-weight: 500;
    color: var(--foam);
  }
  .dim .line,
  .dim .name {
    color: #9fb2bc;
  }
  .sep {
    margin: 0 0.55em;
    color: var(--dim);
  }
</style>
