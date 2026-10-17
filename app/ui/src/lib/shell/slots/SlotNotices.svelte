<script lang="ts">
  // Says how a slot save or load ended (the engine's notice) as a toast, in every layout: the slot keys in the skins
  // and auto-save / auto-load on stop and start have no other place to say it. Notices that were there before this
  // window saw them are not repeated.
  import { untrack } from 'svelte';
  import { player } from '../../state/player.svelte';
  import { ui } from '../../state/ui.svelte';

  /** The last notice time per System; a System seen for the first time only sets it. */
  const seen = new Map<string, number>();

  $effect(() => {
    const notices = player.vm.systems.map((s) => [s.id, s.slots?.notice?.at ?? 0, s.slots?.notice] as const);
    untrack(() => {
      for (const [id, at, n] of notices) {
        const before = seen.get(id);
        seen.set(id, at);
        if (n && before !== undefined && at !== before) ui.toast(n.text, n.ok ? 3200 : 15000, { error: !n.ok });
      }
    });
  });
</script>
