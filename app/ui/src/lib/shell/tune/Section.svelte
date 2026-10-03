<script lang="ts">
  // A collapsible section of the drawer: an uppercase header (the drawer's h2 style) with an optional one-line
  // summary and header actions, then the body.
  import type { Snippet } from 'svelte';

  interface Props {
    id: string;
    title: string;
    open: boolean;
    ontoggle: (open: boolean) => void;
    /** One line next to the title (e.g. the command line), shown open or closed. */
    summary?: Snippet;
    /** Buttons at the right end of the header (outside the toggle). */
    actions?: Snippet;
    children: Snippet;
  }
  let { id, title, open, ontoggle, summary, actions, children }: Props = $props();
</script>

<section class="sec" class:open>
  <div class="head">
    <button type="button" class="toggle" aria-expanded={open} aria-controls="{id}-body" onclick={() => ontoggle(!open)}>
      <i class="chev" aria-hidden="true"></i>
      <span class="title">{title}</span>
      {#if summary}<span class="summary">{@render summary()}</span>{/if}
    </button>
    {#if actions}<span class="actions">{@render actions()}</span>{/if}
  </div>
  {#if open}
    <div class="body" id="{id}-body">
      {@render children()}
    </div>
  {/if}
</section>

<style>
  .sec {
    border-top: 1px solid var(--k-line, #2e2e2e);
    padding-top: 10px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    border: 1px solid transparent;
    padding: 6px 4px;
    text-align: left;
  }
  .toggle:hover:not(:disabled) {
    border-color: transparent;
  }
  .toggle:hover .title {
    color: var(--k-ink, #e6e6e6);
  }
  .chev {
    width: 6px;
    height: 6px;
    flex: none;
    border-right: 1.5px solid var(--k-muted, #8a8a8a);
    border-bottom: 1.5px solid var(--k-muted, #8a8a8a);
    transform: rotate(-45deg);
    transition: transform 140ms;
  }
  .open .chev {
    transform: rotate(45deg);
  }
  .title {
    flex: none;
    font: 600 11px/1 var(--k-font-ui, system-ui, sans-serif);
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--k-muted, #8a8a8a);
  }
  .summary {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-data, ui-monospace, monospace);
  }
  .actions {
    display: flex;
    gap: 6px;
    flex: none;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 10px 0 4px;
  }
</style>
