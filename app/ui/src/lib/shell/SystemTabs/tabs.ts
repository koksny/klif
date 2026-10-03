// The System tab strip data, shared by every skin's picker and by the Tune drawer (frozen seam, SPEC 16.24).
// A strip renders EXACTLY vm.systems in order: no fixed four, no id special cases.
import type { System, SystemId, SystemStatus, ViewModel } from '../../model/types';
import { nodeName, STATUS_TEXT } from '../../model/systems';

export interface SystemTab {
  id: SystemId;
  label: string;
  status: SystemStatus;
  reason?: string;
  /** Display name of the remote node (shown as a muted " · name" suffix); absent for local Systems. */
  nodeName?: string;
  /** May be launched / stopped from here. */
  controllable: boolean;
  /** The full System, for skins that need more than the tab (availability, model, session). */
  system: System;
}

/** One tab per System in vm.systems, in order. */
export function tabsFor(vm: ViewModel): SystemTab[] {
  return vm.systems.map((s) => {
    const tab: SystemTab = { id: s.id, label: s.label, status: s.status, controllable: s.controllable, system: s };
    if (s.reason) tab.reason = s.reason;
    const n = nodeName(vm, s);
    if (n) tab.nodeName = n;
    return tab;
  });
}

/** The tooltip of a tab: status text plus the reason. */
export function tabTitle(tab: Pick<SystemTab, 'label' | 'status' | 'reason' | 'nodeName'>): string {
  const where = tab.nodeName ? ` on ${tab.nodeName}` : '';
  return `${tab.label}${where}: ${STATUS_TEXT[tab.status]}${tab.reason ? `. ${tab.reason}` : ''}`;
}

/** not-set / invalid draw no dot; their label is muted (invalid adds a "!" in the warn colour after it). */
export function labelMuted(status: SystemStatus): boolean {
  return status === 'not-set' || status === 'invalid';
}
