// Global shortcuts: Ctrl+1..4 / Alt+1..4 and F2 (Shift+F2 backwards) switch skins, F3 is panel mode (the
// desktop app moves the window onto the small status screen; a browser only switches the layout between
// mini and full), backquote toggles the dev bar, Escape closes drawers. Nothing here fires while the user is
// typing in a field (Escape then only leaves the field).
import { SKINS } from '../../skins/registry';
import { player } from '../state/player.svelte';
import { ui } from '../state/ui.svelte';

function isTyping(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el || !el.tagName) return false;
  return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable;
}

export function installKeys(): () => void {
  const onKey = (e: KeyboardEvent) => {
    if (isTyping(e.target)) {
      if (e.key === 'Escape') (e.target as HTMLElement).blur();
      return;
    }
    if (e.key === 'Escape') {
      if (ui.closeDrawers()) e.preventDefault();
      return;
    }
    if (e.key === 'F2') {
      e.preventDefault();
      ui.cycleSkin(e.shiftKey ? -1 : 1);
      return;
    }
    if (e.key === 'F3') {
      e.preventDefault();
      if (player.native && player.vm.host.panel.available) player.actions.togglePanel();
      else ui.toggleSize();
      return;
    }
    // Alt+digit is an alias: a normal browser tab swallows Ctrl+digit before the page sees it.
    if ((e.ctrlKey || e.altKey) && !e.metaKey && !e.shiftKey && /^Digit[1-9]$/.test(e.code)) {
      const n = Number(e.code.slice(5)) - 1;
      if (n < SKINS.length) {
        e.preventDefault();
        ui.skinByIndex(n);
        ui.toast(`Skin: ${SKINS[n].name}`);
      }
      return;
    }
    if (e.code === 'Backquote' && !e.ctrlKey && !e.altKey && !e.metaKey && !ui.shot) {
      e.preventDefault();
      ui.devbar = !ui.devbar;
    }
  };
  window.addEventListener('keydown', onKey);
  return () => window.removeEventListener('keydown', onKey);
}
