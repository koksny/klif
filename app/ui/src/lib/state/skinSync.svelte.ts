// The skin lives in two places: the window (localStorage, so the first frame is right) and `[ui] skin` in klif.toml
// (klif-webui follows it, klif-cli can set it). The window reports its choice to the engine and follows a change
// made elsewhere. A ?skin= URL or a screenshot run (?shot=1) reports nothing.
import { untrack } from 'svelte';
import type { SkinId } from '../../skins/contract';
import { SKINS, skinMeta } from '../../skins/registry';
import { player } from './player.svelte';
import { ui } from './ui.svelte';

/** A System skin F2 just picked, shown at once until the view model reports it (or a few seconds pass). */
let picked: { system: string; field: 'full' | 'mini'; skin: SkinId; until: number } | null = null;

/**
 * F2 / Ctrl+digit: change the skin that is on screen. The small layout with the System's own mini skin changes that
 * one; a layout showing the System's own (full) skin changes that; otherwise the global skin. `to` is a skin, or a
 * step through the list from the one shown.
 */
export function pickShownSkin(to: SkinId | 1 | -1) {
  const own = ui.params.skin ? null : ui.systemSkin;
  const field: 'full' | 'mini' | null = !own ? null : ui.size === 'mini' && own.mini ? 'mini' : own.full ? 'full' : null;
  const cur = ui.shownSkin;
  const next =
    typeof to === 'string' ? skinMeta(to) : SKINS[(SKINS.findIndex((s) => s.id === cur) + to + SKINS.length) % SKINS.length];
  const id = player.vm.selected;
  if (!field || !own || !id) {
    ui.setSkin(next.id);
    ui.toast(`Skin: ${next.name}`);
    return;
  }
  picked = { system: id, field, skin: next.id, until: Date.now() + 4000 };
  ui.systemSkin = { ...own, [field]: next.id };
  ui.toast(`Skin: ${next.name} (${own.label}${field === 'mini' ? ', small layout' : ''})`);
  player.actions.updateSystem(id, field === 'mini' ? { miniSkin: next.id } : { skin: next.id }, { quiet: true }).catch((e) => {
    picked = null;
    ui.toast(e instanceof Error ? e.message : String(e), 5000);
  });
}

export function installSkinSync() {
  // The selected System's own skins ([systems.<id>] skin and mini_skin) are shown over the global one while it is
  // selected. An id the window does not have counts as not set.
  $effect(() => {
    const vm = player.vm;
    const sel = vm.systems.find((s) => s.id === vm.selected);
    const known = (id: string | undefined) => (id && SKINS.some((s) => s.id === id) ? (id as SkinId) : null);
    let full = known(sel?.skin);
    let mini = known(sel?.miniSkin);
    // A pick on its way to klif.toml stays on screen until the view model has it.
    if (picked && sel?.id === picked.system && Date.now() < picked.until) {
      const has = picked.field === 'mini' ? mini : full;
      if (has === picked.skin) picked = null;
      else if (picked.field === 'mini') mini = picked.skin;
      else full = picked.skin;
    }
    const own = sel && (full || mini) ? { full, mini, label: sel.label } : null;
    untrack(() => {
      const cur = ui.systemSkin;
      if (own?.full !== cur?.full || own?.mini !== cur?.mini || own?.label !== cur?.label) ui.systemSkin = own;
    });
  });

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
