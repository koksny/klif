// Words, glyphs and short labels for klif-webui's tiles and sheet.
import type { WebSystem } from './types';

export const RUNNING = (s: WebSystem) => s.status === 'online' || s.status === 'busy' || s.status === 'starting' || s.status === 'stopping';

export function statusWord(s: WebSystem): string {
  switch (s.status) {
    case 'online':
      return 'Online';
    case 'busy':
      return 'Working';
    case 'starting':
      return 'Starting';
    case 'stopping':
      return 'Stopping';
    case 'offline':
      return 'Off';
    case 'not-set':
      return 'Not set';
    case 'invalid':
      return 'Can’t launch';
    case 'fault':
      return 'Fault';
    case 'unreachable':
      return 'Unreachable';
  }
}

/** The status colour from the skin's tokens. */
export function statusColor(s: WebSystem): string {
  switch (s.status) {
    case 'online':
    case 'busy':
      return 'var(--k-accent)';
    case 'starting':
    case 'stopping':
      return 'var(--k-warn)';
    case 'fault':
    case 'invalid':
      return 'var(--k-danger)';
    default:
      return 'var(--k-muted)';
  }
}

/** The glyph's shape carries the status; its colour repeats it. */
export function glyph(s: WebSystem): 'on' | 'half' | 'bad' | 'off' | 'none' {
  if (s.status === 'online' || s.status === 'busy') return 'on';
  if (s.status === 'starting' || s.status === 'stopping') return 'half';
  if (s.status === 'fault' || s.status === 'invalid') return 'bad';
  if (s.status === 'offline') return 'off';
  return 'none';
}

/** "System 1" -> "S1", "System CGI" -> "CGI", anything else as written (a tile shows up to about 8 characters). */
export function shortLabel(label: string): string {
  const rest = label.replace(/^system\s+/i, '').trim();
  if (/^\d+$/.test(rest)) return `S${rest}`;
  return rest || label;
}

export function metricText(s: WebSystem): string {
  if (!s.metric) return '';
  return s.metric.u === 'tok/s' ? `${s.metric.v} tok/s` : `${s.metric.v} ${s.metric.u}`;
}

/** The tile's third line. */
export function tileLine(s: WebSystem): string {
  if (s.status === 'unreachable') return 'last known';
  if (s.reason && (s.status === 'fault' || s.status === 'invalid' || s.status === 'not-set')) return s.reason;
  if (s.metric) return metricText(s);
  if (s.load) return `${s.load.pct}% · ${s.load.step.toLowerCase()}`;
  if (RUNNING(s) && s.uptimeS !== undefined) return `up ${uptime(s.uptimeS)}`;
  if (s.last) return `last ${s.last}`;
  if (s.status === 'online') return 'ready';
  return '';
}

export function uptime(sec?: number): string {
  if (sec === undefined) return '';
  if (sec < 60) return `${Math.round(sec)} s`;
  const m = Math.floor(sec / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

export const gb = (v: number) => (v >= 100 ? Math.round(v).toString() : v.toFixed(1));

/** The main button: what it says and how it looks. */
export function mainAction(s: WebSystem): { label: string; kind: 'stop' | 'go' | 'warn' } | null {
  if (s.status === 'not-set' || s.status === 'unreachable' || s.external) return null;
  if (RUNNING(s)) return { label: 'Stop', kind: 'stop' };
  if (s.conflicts.length) return { label: `Stop ${s.conflicts.join(', ')} & launch`, kind: 'warn' };
  return { label: s.status === 'fault' ? 'Launch again' : 'Launch', kind: 'go' };
}
