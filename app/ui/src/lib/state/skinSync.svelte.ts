// The skin lives in two places: the window (localStorage, so the first frame is right) and `[ui] skin` in klif.toml
// (klif-webui follows it, klif-cli can set it). The window reports its choice to the engine and follows a change
// made elsewhere. A ?skin= URL or a screenshot run (?shot=1) reports nothing.
import { untrack } from 'svelte';
import { SKINS } from '../../skins/registry';
import { player } from './player.svelte';
import { ui } from './ui.svelte';

export function installSkinSync() {
  /** The config value last seen (undefined: no view model yet). */
  let seen: string | undefined;
  /** Reports on their way to klif.toml, oldest first: their echo in the view model is not a change made elsewhere. */
  const pending: string[] = [];
  /** A report klif.toml refused (it does not parse, is read-only...): not sent again until the skin changes. */
  let failed: string | null = null;
  const known = (id: string | undefined): id is string => !!id && SKINS.some((s) => s.id === id);

  $effect(() => {
    if (!player.ready || ui.shot || ui.params.skin) return;
    const fromConfig = player.vm.config.skin ?? '';
    const mine = ui.skinId;
    untrack(() => {
      const first = seen === undefined;
      const changed = !first && fromConfig !== seen;
      seen = fromConfig;
      // Our own report arrived: it and every older one are settled (two quick picks can land as one reload).
      const echo = pending.indexOf(fromConfig);
      if (echo >= 0) {
        pending.splice(0, echo + 1);
        return;
      }
      if (changed) {
        pending.length = 0;
        failed = null;
      }
      // At start klif.toml wins over localStorage; later a value that changed elsewhere does.
      if ((first || changed) && known(fromConfig) && fromConfig !== mine) {
        ui.setSkin(fromConfig);
        return;
      }
      if (mine !== fromConfig && !pending.includes(mine) && failed !== mine) {
        pending.push(mine);
        player.actions.updateSettings({ skin: mine }, { quiet: true }).catch(() => {
          const i = pending.indexOf(mine);
          if (i >= 0) pending.splice(i, 1);
          failed = mine;
        });
      }
    });
  });
}
