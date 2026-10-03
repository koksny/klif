<script lang="ts">
  // What goes inside one System tab / pill / row: [status dot] label [ · node] [!]. The surrounding control
  // (button, pill, selector row) stays the skin's own; this only lays out the label content so every skin
  // shows the same thing: the label truncates with an ellipsis, a remote System carries a muted node suffix,
  // not-set / invalid Systems have a muted label (invalid adds a "!" in the warn colour).
  import { shortLabel } from '../../model/systems';
  import { labelMuted, tabTitle, type SystemTab } from './tabs';
  import StatusDot from './StatusDot.svelte';

  interface Props {
    tab: SystemTab;
    /** Tight label (S1, CGI) for mini panels. */
    short?: boolean;
    /** Hide the node suffix (very tight places). */
    noNode?: boolean;
    /** Hide the dot (a skin that draws status itself). */
    noDot?: boolean;
    /** Dot diameter in px. */
    dotSize?: number;
  }
  let { tab, short = false, noNode = false, noDot = false, dotSize = 6 }: Props = $props();

  const text = $derived(short ? shortLabel(tab.label) : tab.label);
  const muted = $derived(labelMuted(tab.status));
</script>

<span class="tl" title={tabTitle(tab)}>
  {#if !noDot}<StatusDot status={tab.status} reason={tab.reason} size={dotSize} />{/if}
  <span class="txt" class:muted>{text}</span>
  {#if tab.nodeName && !noNode}<span class="node">&nbsp;· {tab.nodeName}</span>{/if}
  {#if tab.status === 'invalid'}<span class="bang" aria-hidden="true">!</span>{/if}
</span>

<style>
  .tl {
    display: inline-flex;
    align-items: center;
    min-width: 0;
    max-width: 100%;
    vertical-align: middle;
  }
  .txt {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .txt.muted {
    color: var(--k-muted);
  }
  .node {
    flex: 0 100 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--k-muted);
  }
  .bang {
    flex: none;
    margin-left: 3px;
    color: var(--k-warn);
    font-weight: 700;
  }
</style>
