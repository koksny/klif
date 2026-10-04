// Helpers every skin and shell surface shares for reading Systems out of the view model. Pure functions: no
// state, no DOM. Skins switch on `kind` (never on ids) and on `status` (never on a tier name).
import type { Availability, NodeView, System, SystemId, SystemKind, SystemStatus, ViewModel } from './types';

/** The selected System (first one when the selection is stale); null when there are no Systems. */
export function selectedSystem(vm: ViewModel): System | null {
  return vm.systems.find((s) => s.id === vm.selected) ?? vm.systems[0] ?? null;
}

export function systemById(vm: ViewModel, id: SystemId | null | undefined): System | undefined {
  return id ? vm.systems.find((s) => s.id === id) : undefined;
}

/** Display label of a System id (the id itself when it is not in the list). */
export function systemLabel(vm: ViewModel, id: SystemId | null | undefined): string {
  return systemById(vm, id)?.label ?? id ?? '';
}

/** "System 2" -> "S2", "System CGI" -> "CGI"; any other label is kept (the caller truncates). */
export function shortLabel(label: string): string {
  const n = /^SYSTEM\s+(\d+)$/i.exec(label.trim());
  if (n) return `S${n[1]}`;
  return label.trim().replace(/^(SYSTEM|AGENT)\s+/i, '');
}

export function nodeOf(vm: ViewModel, system: Pick<System, 'node'>): NodeView | undefined {
  return system.node ? vm.nodes.find((n) => n.id === system.node) : undefined;
}

/** Display name of the remote node a System lives on (undefined for local Systems). */
export function nodeName(vm: ViewModel, system: Pick<System, 'node'>): string | undefined {
  if (!system.node) return undefined;
  return nodeOf(vm, system)?.name ?? system.node;
}

export const KIND_LABEL: Record<SystemKind, string> = {
  llm: 'LLM',
  image: 'Image',
  tts: 'Speech',
  stt: 'Transcription',
  video: 'Video',
  music: 'Music',
};

/** What a unit of work of this kind is called in counters. */
export const KIND_WORK: Record<SystemKind, string> = {
  llm: 'requests',
  image: 'images',
  tts: 'requests',
  stt: 'requests',
  video: 'clips',
  music: 'songs',
};

export const STATUS_TEXT: Record<SystemStatus, string> = {
  'not-set': 'Not set',
  invalid: 'Invalid',
  offline: 'Offline',
  starting: 'Starting',
  online: 'Online',
  busy: 'Busy',
  stopping: 'Stopping',
  fault: 'Fault',
  unreachable: 'Unreachable',
};

export const AVAILABILITY_TEXT: Record<Availability, string> = {
  ready: 'ready',
  unsupported: 'not supported here',
  invalid: 'needs fixing',
  'exe-missing': 'program missing',
  'model-missing': 'model missing',
  busy: 'port in use',
};

/** One sentence for a System that cannot be launched, or '' when it can (reason first, then the availability). */
export function blockedText(s: System): string {
  if (s.status === 'not-set') return s.reason ?? 'No preset is set.';
  if (s.status === 'unreachable') return s.reason ?? 'Its node is not reachable.';
  if (s.external) return 'External server: start it where it runs.';
  if (!s.controllable) return 'This node does not allow launching.';
  if (s.availability !== 'ready') return s.reason ?? `Cannot start: ${AVAILABILITY_TEXT[s.availability]}.`;
  if (s.status === 'invalid') return s.reason ?? 'The preset needs fixing.';
  return '';
}

/**
 * Why a System that is not running shows no measurement (the idle view's word), lower-case. `warn` = worth the
 * skin's amber (a broken preset, an unreachable node, an external server that does not answer).
 */
export function idleState(s: System | null | undefined): { text: string; warn: boolean } {
  if (!s) return { text: 'no system', warn: false };
  switch (s.status) {
    case 'not-set':
      return { text: 'not set', warn: false };
    case 'invalid':
      return { text: s.availability === 'ready' ? 'invalid' : AVAILABILITY_TEXT[s.availability], warn: true };
    case 'unreachable':
      return { text: 'unreachable', warn: true };
    case 'offline':
      return s.external ? { text: 'not answering', warn: true } : { text: 'not running', warn: false };
    default:
      return { text: STATUS_TEXT[s.status].toLowerCase(), warn: false };
  }
}

/** A server process of ours exists for this System (anything between starting and stopping, or a fault). */
export function isHeld(s: System): boolean {
  return s.status === 'starting' || s.status === 'online' || s.status === 'busy' || s.status === 'stopping' || s.status === 'fault';
}

/** Launch is possible right now (not running, not external, controllable, preset ready). */
export function canLaunch(s: System | null | undefined): boolean {
  return !!s && s.status === 'offline' && s.availability === 'ready' && s.controllable && !s.external;
}

/**
 * A launch that waits for other Systems to stop ("Stop System 1 & launch" in progress): `starting` with no
 * session yet. Its Launch control is off; Stop cancels it.
 */
export function isPendingLaunch(s: System | null | undefined): boolean {
  return !!s && s.status === 'starting' && !s.session;
}

/**
 * An external server never gets Launch / Stop / Restart (KLIF only watches it): every skin shows this quiet note
 * in the place of its primary control instead, with EXTERNAL_TITLE as the tooltip.
 */
export const EXTERNAL_NOTE = 'External server';
export const EXTERNAL_TITLE = 'External server: KLIF watches it and never starts or stops it.';

/** Stop is possible right now (we run it and may stop it). */
export function canStop(s: System | null | undefined): boolean {
  return !!s && s.controllable && !s.external && (s.status === 'starting' || s.status === 'online' || s.status === 'busy');
}

/**
 * Why a System cannot launch because of where it is: already up, on its way or stopping. A pending launch ("Stop
 * System 1 & launch" in progress) is `starting` with no session and reads "Waiting for System 1 to stop." from
 * its reason. Offline launches, and so does a fault (launching over a fault ends that session first).
 */
function statusBlock(s: System): string {
  return s.status === 'offline' || s.status === 'fault' ? '' : (s.reason ?? `${STATUS_TEXT[s.status]}.`);
}

export interface LaunchCtl {
  /** Launch is possible (the control is live). */
  enabled: boolean;
  /** Send `stopOthers: true`. */
  stopOthers: boolean;
  /** The labels of the Systems that must stop first (empty when none). */
  names: string[];
  /** The button text: "Launch" or "Stop System 1 & launch" (sentence case; skins re-case it). */
  text: string;
  /** Why it is disabled ('' when enabled). */
  blocked: string;
}

function joinNames(names: string[]): string {
  if (names.length <= 2) return names.join(' & ');
  if (names.length === 3) return `${names[0]}, ${names[1]} & ${names[2]}`;
  return `${names.length} Systems`;
}

/**
 * The Launch control for a System. With conflicts the control IS the question: "Stop System 1 & launch" and it
 * sends stopOthers: true. `short` uses tight labels (S1, CGI) for mini panels and narrow buttons.
 */
export function launchCtl(vm: ViewModel, s: System | null | undefined, opts: { short?: boolean; verb?: string } = {}): LaunchCtl {
  const verb = opts.verb ?? 'Launch';
  if (!s) return { enabled: false, stopOthers: false, names: [], text: verb, blocked: 'Nothing selected.' };
  const names = s.conflicts.map((id) => (opts.short ? shortLabel(systemLabel(vm, id)) : systemLabel(vm, id)));
  const blocked = canLaunch(s) ? '' : blockedText(s) || statusBlock(s);
  const stopOthers = names.length > 0;
  return {
    enabled: blocked === '',
    stopOthers,
    names,
    text: stopOthers ? `Stop ${joinNames(names)} & ${verb.toLowerCase()}` : verb,
    blocked,
  };
}

/** Run a launch control (launch the System, with stopOthers when the control said so). Errors become toasts upstream. */
export function doLaunch(
  actions: { launch(system?: SystemId, opts?: { stopOthers?: boolean }): Promise<void> },
  s: System | null | undefined,
  ctl: LaunchCtl,
) {
  if (!s || !ctl.enabled) return;
  void actions.launch(s.id, ctl.stopOthers ? { stopOthers: true } : undefined).catch(() => {});
}

/** Every System of one kind, in tab order. */
export function systemsOfKind(vm: ViewModel, kind: SystemKind): System[] {
  return vm.systems.filter((s) => s.kind === kind);
}

/** A kind that has a dedicated live display (llm / image); everything else gets the generic one. */
export function hasDedicatedLive(kind: SystemKind): boolean {
  return kind === 'llm' || kind === 'image';
}

/** The "model line" parts joined without empty pieces. */
export function joinParts(parts: (string | null | undefined | false)[], sep = ' · '): string {
  return parts.filter(Boolean).join(sep);
}

/** Backend compare that ignores case ("HIP" vs "hip"). */
export function sameBackend(a: string | undefined, b: string): boolean {
  return (a ?? '').toLowerCase() === b.toLowerCase();
}
