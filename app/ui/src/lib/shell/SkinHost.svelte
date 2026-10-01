<script lang="ts">
  // Lazy-loads the requested skin through the registry and crossfades (opacity only) between skins.
  import { untrack } from 'svelte';
  import { fade } from 'svelte/transition';
  import type { SizeClass, SkinId, SkinModule } from '../../skins/contract';
  import { skinMeta } from '../../skins/registry';
  import type { Actions, ViewModel } from '../model/types';
  import { ui } from '../state/ui.svelte';
  import { applyTokens } from './tokens';

  interface Props {
    skinId: SkinId;
    vm: ViewModel;
    actions: Actions;
    size: SizeClass;
  }
  let { skinId, vm, actions, size }: Props = $props();

  interface Layer {
    key: number;
    id: SkinId;
    mod: SkinModule;
    first: boolean;
  }
  let layers = $state.raw<Layer[]>([]);
  let seq = 0;
  let keySeq = 0;
  const reduce = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const FADE_MS = reduce ? 0 : 200;

  $effect(() => {
    const id = skinId;
    const mine = ++seq;
    skinMeta(id)
      .load()
      .then((mod) => {
        if (mine !== seq) return; // a newer request superseded this one
        untrack(() => {
          applyTokens(mod.tokens);
          layers = [{ key: ++keySeq, id, mod, first: layers.length === 0 }];
        });
      })
      .catch((err: unknown) => {
        console.error(`[klif] skin "${id}" failed to load`, err);
        ui.toast(`Skin "${id}" failed to load (see console).`);
      });
  });
</script>

<div class="host" data-skin={layers[0]?.id}>
  {#each layers as l (l.key)}
    {@const Skin = l.mod.default}
    <div class="layer" in:fade={{ duration: l.first ? 0 : FADE_MS }} out:fade={{ duration: FADE_MS }}>
      <Skin {vm} {actions} {size} />
    </div>
  {/each}
</div>

<style>
  .host {
    position: absolute;
    inset: 0;
    background: var(--k-bg, #0d1117);
  }
  .layer {
    position: absolute;
    inset: 0;
  }
</style>
