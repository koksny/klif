// What the Records screen knows about each metric: label, unit, direction, how a value and its conditions read.
import type { HardwareInfo, RecordEntry, RecordEvent, RecordMetric, RecordValue, SystemKind, ViewModel } from '../../model/types';

export interface MetricMeta {
  id: RecordMetric;
  /** Tab label. */
  label: string;
  /** What the number is, for titles and the export card. */
  long: string;
  unit: string;
  /** Higher is better (tok/s, x real time); otherwise lower (seconds). */
  higher: boolean;
  kind: SystemKind;
}

export const METRICS: MetricMeta[] = [
  { id: 'decodeTps', label: 'Decode', long: 'Decode speed', unit: 'tok/s', higher: true, kind: 'llm' },
  { id: 'prefillTps', label: 'Prefill', long: 'Prefill speed', unit: 'tok/s', higher: true, kind: 'llm' },
  { id: 'ttftS', label: 'TTFT', long: 'Time to first token', unit: 's', higher: false, kind: 'llm' },
  { id: 'imageS', label: 'Image', long: 'Time per image', unit: 's / image', higher: false, kind: 'image' },
  { id: 'videoS', label: 'Video', long: 'Time per video', unit: 's / video', higher: false, kind: 'video' },
  { id: 'ttsRtf', label: 'Speech', long: 'Speech synthesis', unit: 'x real time', higher: true, kind: 'tts' },
  { id: 'sttRtf', label: 'Transcription', long: 'Transcription', unit: 'x real time', higher: true, kind: 'stt' },
  { id: 'musicRtf', label: 'Music', long: 'Music generation', unit: 'x real time', higher: true, kind: 'music' },
];

export const metricMeta = (m: RecordMetric): MetricMeta => METRICS.find((x) => x.id === m) ?? METRICS[0];

/** The metrics that have at least one record, in tab order (Decode when there is nothing yet). */
export function presentMetrics(entries: RecordEntry[]): MetricMeta[] {
  const out = METRICS.filter((m) => entries.some((e) => e.best[m.id]));
  return out.length ? out : [METRICS[0]];
}

/** "93.1", "2,840", "0.21", "3.9", "4.2". */
export function fmtValue(metric: RecordMetric, v: number): string {
  if (!Number.isFinite(v)) return '—';
  switch (metric) {
    case 'prefillTps':
      return v >= 100 ? Math.round(v).toLocaleString('en-US') : v.toFixed(1);
    case 'decodeTps':
      return v >= 1000 ? Math.round(v).toLocaleString('en-US') : v.toFixed(1);
    case 'ttftS':
      return v < 10 ? v.toFixed(2) : v.toFixed(1);
    default:
      return v < 100 ? v.toFixed(1) : Math.round(v).toString();
  }
}

const k = (n: number) => (n >= 1024 ? `${Math.round(n / 1024)}k` : `${n}`);
const tok = (n: number) => (n >= 10000 ? `${(n / 1000).toFixed(0)}k` : n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`);

/** The conditions a value was reached under, short: "ctx 32k · 8.2k prompt" / "1024x1024 · 8 steps". */
export function conditions(metric: RecordMetric, v: RecordValue): string {
  const parts: string[] = [];
  if (v.width && v.height) parts.push(`${v.width}x${v.height}`);
  if (v.frames) parts.push(`${v.frames} frames`);
  if (v.steps) parts.push(`${v.steps} steps`);
  if (v.ctx) parts.push(`ctx ${k(v.ctx)}`);
  if ((metric === 'prefillTps' || metric === 'ttftS') && v.promptTokens) {
    const fresh = v.promptTokens - (v.cachedTokens ?? 0);
    parts.push(v.cachedTokens ? `${tok(fresh)} new of ${tok(v.promptTokens)} prompt` : `${tok(v.promptTokens)} prompt`);
  }
  if (metric === 'decodeTps' && v.genTokens) parts.push(`${tok(v.genTokens)} generated`);
  return parts.join(' · ');
}

/** "Oct 2" this year, else "Oct 2, 2025". */
export function fmtDate(at: number, now = Date.now() / 1000): string {
  const d = new Date(at * 1000);
  const same = new Date(now * 1000).getFullYear() === d.getFullYear();
  return d.toLocaleDateString('en-US', same ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' });
}

/** Entries that have the metric, best first. */
export function ranked(entries: RecordEntry[], metric: RecordMetric): RecordEntry[] {
  const m = metricMeta(metric);
  return entries
    .filter((e) => e.best[metric])
    .sort((a, b) => {
      const d = a.best[metric]!.value - b.best[metric]!.value;
      return m.higher ? -d : d;
    });
}

/** 0..1, the leader = 1 (lower-is-better metrics inverted so the best still gets the full bar). */
export function share(metric: RecordMetric, v: number, leader: number): number {
  if (!(v > 0) || !(leader > 0)) return 0;
  return Math.max(0.02, Math.min(1, metricMeta(metric).higher ? v / leader : leader / v));
}

/** How long a broken record counts as new on the screen (seconds). */
export const FRESH_S = 24 * 3600;

/** The newest event of this entry and metric within FRESH_S, if any. */
export function freshEvent(events: RecordEvent[], e: RecordEntry, metric: RecordMetric, now: number): RecordEvent | undefined {
  for (let i = events.length - 1; i >= 0; i--) {
    const ev = events[i];
    if (ev.key === e.key && ev.metric === metric && now - ev.at <= FRESH_S) return ev;
  }
  return undefined;
}

/** "+7.2 tok/s, was 89.7 (8%)" / "−0.14 s, was 0.42 (33%)" / "first record". */
export function deltaText(metric: RecordMetric, ev: RecordEvent): string {
  if (ev.old === undefined || ev.old === null) return 'first record';
  const m = metricMeta(metric);
  const d = Math.abs(ev.new - ev.old);
  const pct = ev.old > 0 ? Math.round((d / ev.old) * 100) : 0;
  const unit = m.unit.replace(' / image', '').replace(' / video', '');
  return `${m.higher ? '+' : '−'}${fmtValue(metric, d)} ${unit}, was ${fmtValue(metric, ev.old)}${pct >= 1 ? ` (${pct}%)` : ''}`;
}

/** "HIP", or "?" when the backend is unknown. */
export const backendLabel = (b: string) => b.trim() || '?';

/** The hardware of the machine an entry (or a machine filter) belongs to. */
export function hardwareOf(vm: ViewModel, node?: string): HardwareInfo | undefined {
  if (!node) return vm.hardware;
  return vm.nodes.find((n) => n.id === node)?.hardware;
}
