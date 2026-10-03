<script lang="ts">
  // Remote KLIF nodes as seen from here (name, address, state, latency, version, rights). They are configured in
  // klif.toml ([nodes.<id>]), so the section only points there.
  import type { NodeState, NodeView, SystemStatus } from '../../model/types';
  import { player } from '../../state/player.svelte';
  import StatusDot from '../SystemTabs/StatusDot.svelte';
  import Section from './Section.svelte';
  import { getTune } from './state.svelte';
  import { attempt } from './util';

  interface Props {
    nodes: NodeView[];
  }
  let { nodes }: Props = $props();
  const t = getTune();

  const DOT: Record<NodeState, SystemStatus> = {
    connecting: 'starting',
    online: 'online',
    offline: 'unreachable',
    unauthorized: 'fault',
    incompatible: 'fault',
  };
  const TEXT: Record<NodeState, string> = {
    connecting: 'connecting',
    online: 'online',
    offline: 'offline',
    unauthorized: 'not authorized',
    incompatible: 'incompatible',
  };

  let err = $state('');
  async function openConfig() {
    err = await attempt(() => player.config.openConfig());
  }
</script>

<Section id="tune-nodes" title="Nodes" open={t.nodesOpen} ontoggle={(o) => (t.nodesOpen = o)}>
  {#snippet summary()}{nodes.length ? `${nodes.filter((n) => n.state === 'online').length} of ${nodes.length} online` : 'none'}{/snippet}
  {#if nodes.length}
    <ul class="nodes">
      {#each nodes as n (n.id)}
        <li>
          <div class="nline">
            <StatusDot status={DOT[n.state]} reason={n.error} />
            <b>{n.name}</b>
            <span class="addr">{n.address}</span>
            <span class="grow"></span>
            <span class="st">{TEXT[n.state]}{n.latencyMs !== undefined && n.state === 'online' ? ` · ${Math.round(n.latencyMs)} ms` : ''}</span>
          </div>
          <div class="nsub">
            {n.version ? `KLIF ${n.version}` : 'version unknown'} · rights: {['view', ...n.allow].join(', ')}{n.error ? ` · ${n.error}` : ''}
          </div>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="hint">No remote nodes. Other machines running KLIF can be shown here as tabs.</p>
  {/if}
  <div class="foot">
    <p class="hint">Add or remove nodes in klif.toml.</p>
    <button type="button" class="mini text" onclick={() => void openConfig()}>Open klif.toml</button>
  </div>
  {#if err}<p class="err" role="alert">{err}</p>{/if}
</Section>

<style>
  .nodes {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 8px;
  }
  .nodes li {
    display: grid;
    gap: 3px;
    padding: 8px 10px;
    border: 1px solid var(--k-line, #2e2e2e);
    border-radius: var(--k-radius, 6px);
  }
  .nline {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font: 500 13px/1.3 var(--k-font-ui, system-ui, sans-serif);
  }
  .nline :global(.dot) {
    margin-right: 0;
  }
  .addr {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.2 var(--k-font-data, ui-monospace, monospace);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .st {
    flex: none;
    color: var(--k-muted, #8a8a8a);
    font-size: 12px;
  }
  .nsub {
    color: var(--k-muted, #8a8a8a);
    font: 500 12px/1.35 var(--k-font-ui, system-ui, sans-serif);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .foot .hint {
    flex: 1;
    margin: 0;
  }
</style>
