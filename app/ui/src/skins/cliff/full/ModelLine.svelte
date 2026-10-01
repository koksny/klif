<script lang="ts">
  import type { ModelRef, SlotKind } from '../../../lib/model/types';
  import { modelFacts } from '../util';

  let { model, kind, label }: { model: ModelRef; kind: SlotKind; label: string } = $props();
  const facts = $derived(modelFacts(model, kind));
</script>

<section class="ml">
  <div class="c-lbl">{label}</div>
  <div class="line c-data" title="{model.engine} · {facts.join(' · ')}">
    {#each facts as f, i (i)}
      {#if i}<span class="sep" aria-hidden="true">·</span>{/if}<span class="f" class:name={i === 0}>{f}</span>
    {/each}
  </div>
</section>

<style>
  .ml {
    padding-top: max(7px, calc(var(--u) * 12));
    padding-bottom: max(8px, calc(var(--u) * 13));
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-start;
    align-items: baseline;
    margin-top: max(3px, calc(var(--u) * 6));
    font-size: max(12.5px, calc(var(--u) * 17.5));
    color: var(--foam);
    letter-spacing: 0.03em;
  }
  .f {
    white-space: nowrap;
  }
  /* Separators share the spare width (a long LLM line spans the row like the mockup) but cap at
     2.4em, so a short image-model line stays compact instead of spreading across the window. */
  .sep {
    flex: 1 1 auto;
    min-width: 1.3em;
    max-width: 2.4em;
    text-align: center;
    color: var(--muted);
  }
</style>
