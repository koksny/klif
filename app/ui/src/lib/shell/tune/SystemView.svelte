<script lang="ts">
  // One System tab of the drawer: System row, Preset row, Params, Fit, Bench, Command, Recommended (local), API
  // key (local), Nodes, then the footer. A remote tab reads its node's presets and GPUs and passes the node on
  // every preset/download action.
  import { untrack } from 'svelte';
  import { presetsFor } from '../../model/presets';
  import { nodeOf } from '../../model/systems';
  import type { System, ViewModel } from '../../model/types';
  import ApiKeyRow from './ApiKeyRow.svelte';
  import DisplayRow from './DisplayRow.svelte';
  import BenchLine from './BenchLine.svelte';
  import CommandSection from './CommandSection.svelte';
  import FitPanel from './FitPanel.svelte';
  import NodesSection from './NodesSection.svelte';
  import ParamsBlock from './ParamsBlock.svelte';
  import PresetRow from './PresetRow.svelte';
  import Recommendations from './Recommendations.svelte';
  import { getTune } from './state.svelte';
  import SystemRow from './SystemRow.svelte';
  import TuneFooter from './TuneFooter.svelte';

  interface Props {
    vm: ViewModel;
    system: System;
  }
  let { vm, system }: Props = $props();
  const t = getTune();

  const node = $derived(nodeOf(vm, system));
  const presets = $derived(presetsFor(vm, system));
  const viewedId = $derived(t.viewedId(system));

  // Primitives via $derived: the System object itself is replaced at every 2 Hz snapshot.
  const nodeId = $derived(system.node);
  const activeId = $derived(system.preset);
  const activeHash = $derived(system.command?.hash);
  // The stored spec's hash (every field: also notes, health, gpu...), which Apply sends back as baseHash.
  const activeSpecHash = $derived(presets.find((p) => p.id === activeId)?.specHash);

  // The shown preset's draft is loaded once per drawer (feeds the Command editor, footer and badges).
  $effect(() => {
    if (viewedId) t.ensure(nodeId, viewedId);
  });

  // A clean draft of the active preset follows changes made elsewhere (an agent via klif-cli, another window):
  // its command or any other stored field.
  let seenHash: string | undefined;
  $effect(() => {
    const h = `${activeHash ?? ''}|${activeSpecHash ?? ''}`;
    const id = activeId;
    untrack(() => {
      if (seenHash !== undefined && h !== seenHash && id) {
        const d = t.draft(nodeId, id);
        if (d && !d.loading && !d.saving && !t.isDirty(d)) void t.load(d.key);
      }
      seenHash = h;
    });
  });
</script>

<div class="scroll">
  <SystemRow {vm} {system} {node} />
  <PresetRow {vm} {system} {presets} {viewedId} />
  {#if system.params.length}
    <ParamsBlock {system} otherViewed={!!viewedId && viewedId !== system.preset} />
  {/if}
  <FitPanel {vm} {system} />
  <BenchLine {system} />
  <CommandSection {vm} {system} {viewedId} />
  {#if !system.node}
    <Recommendations {vm} {system} />
    <ApiKeyRow info={vm.config.apiKey} />
    <DisplayRow config={vm.config} />
  {/if}
  <NodesSection nodes={vm.nodes} />
</div>
<TuneFooter {vm} {system} {viewedId} />
