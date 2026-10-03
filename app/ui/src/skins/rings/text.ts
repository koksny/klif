// Rings text helpers: display strings derived from ViewModel fields (never invented values).
import type { Availability, GpuMemory, LastSession, ModelRef, Phase, System, SystemKind } from '../../lib/model/types';
import { fmtCtx, fmtInt, fmtTps } from '../../lib/model/format';
import { shortLabel as tierShort } from '../../lib/model/systems';

/** Why a System cannot launch, in plain words. */
export function availabilityText(a: Availability): string {
  switch (a) {
    case 'ready':
      return 'ready';
    case 'model-missing':
      return 'model missing';
    case 'exe-missing':
      return 'program missing';
    case 'invalid':
      return 'needs fixing';
    case 'unsupported':
      return 'unsupported';
    case 'busy':
      return 'port busy';
  }
}

/** 8047 -> "2 h 14 min", 754 -> "12 min", 42 -> "42 s" */
export function fmtDur(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return m ? `${h} h ${m} min` : `${h} h`;
}

export function fmtAgo(seconds: number): string {
  return `${fmtDur(seconds)} ago`;
}

/** Compact remaining time: "8.4 s", "1:28". */
export function fmtEta(sec: number): string {
  if (sec < 60) return `${sec.toFixed(1)} s`;
  const m = Math.floor(sec / 60);
  const r = Math.round(sec % 60);
  return `${m}:${r.toString().padStart(2, '0')}`;
}

/** Baseline VRAM held by others (driver, other processes); falls back to the 'other' layer. */
export function baselineOf(vram: GpuMemory): number {
  if (typeof vram.baselineGiB === 'number' && Number.isFinite(vram.baselineGiB)) return vram.baselineGiB;
  return vram.layers.filter((l) => l.id === 'other').reduce((a, l) => a + l.gib, 0);
}

/** The selected tier's expected footprint on top of what is in use (idle fit preview), or null. */
export function fitOf(vram: GpuMemory, slot: System | undefined): { base: number; top: number; spare: number } | null {
  if (!slot?.expectedVram?.length || slot.external) return null;
  const base = Math.max(baselineOf(vram), vram.usedGiB);
  const top = base + slot.expectedVram.reduce((a, l) => a + l.gib, 0);
  return { base, top, spare: vram.totalGiB - top };
}

/** The model line: "Qwen 3.8 27B · GSQ-RCO IQ3_S · HIP · RX 9070 XT · ctx 96k · kv q8_0". The name is separate. */
export function modelRest(m: ModelRef | undefined, kind: SystemKind): string {
  if (!m) return '';
  const parts: (string | undefined | false)[] = [
    m.quant,
    kind !== 'llm' && m.engine,
    m.backend,
    m.device,
    m.ctxTokens ? `ctx ${fmtCtx(m.ctxTokens)}` : m.imageSize,
    m.kvType && `kv ${m.kvType}`,
    m.vision && 'vision',
    m.mode,
  ];
  return parts.filter(Boolean).join(' · ');
}

/** Short model line for the mini panel: "Qwen 3.8 27B · GSQ-RCO IQ3_S · 96k". */
export function modelShort(m: ModelRef | undefined): string {
  if (!m) return '';
  return [m.name, m.quant, m.ctxTokens ? fmtCtx(m.ctxTokens) : m.imageSize, m.mode].filter(Boolean).join(' · ');
}

/** The model's shape as the server reported it: "48 layers · 10+1 of 512 experts" (compact: no "experts"), "64 layers · dense". */
export function shapeText(m: ModelRef | undefined, kind: SystemKind, compact = false): string {
  const a = m?.arch;
  if (kind !== 'llm' && kind !== 'image') return m?.backend || m?.engine || '—';
  if (kind === 'image') return a && a.layers > 0 ? `${a.layers} blocks · dit` : `dit · ${m?.engine ?? 'blocks not reported'}`;
  if (!a || a.layers <= 0) return 'shape after first load';
  if (a.experts > 0) return `${a.layers} layers · ${a.expertsUsed}${a.sharedExperts ? `+${a.sharedExperts}` : ''} of ${fmtInt(a.experts)}${compact ? '' : ' experts'}`;
  return `${a.layers} layers · dense`;
}

/** "last s2 · 2 h 14 min · 412 requests · 38.1 tok/s · stopped 12 min ago" */
export function lastSessionText(ls: LastSession, systems: System[]): string {
  const label = systems.find((x) => x.id === ls.system)?.label;
  const facts = [fmtDur(ls.uptimeS)];
  if (ls.requests !== undefined) facts.push(`${fmtInt(ls.requests)} requests`);
  if (ls.decodeTps !== undefined) facts.push(`${fmtTps(ls.decodeTps)} tok/s`);
  if (ls.images !== undefined) facts.push(`${fmtInt(ls.images)} images`);
  if (ls.secondsPerImage !== undefined) facts.push(`${ls.secondsPerImage.toFixed(1)} s/image`);
  const end = ls.ended === 'fault' ? `fault ${fmtAgo(ls.endedAgoS)}` : `stopped ${fmtAgo(ls.endedAgoS)}`;
  return `last ${label ? tierShort(label) : ls.model.name} · ${facts.join(' · ')} · ${end}`;
}

/** "last s2 · stopped 21 min ago" */
export function lastSessionShort(ls: LastSession, systems: System[]): string {
  const label = systems.find((x) => x.id === ls.system)?.label;
  return `last ${label ? tierShort(label) : ls.model.name} · ${ls.ended === 'fault' ? 'fault' : 'stopped'} ${fmtAgo(ls.endedAgoS)}`;
}

export function phaseWord(phase: Phase | 'idle'): string {
  return phase === 'idle' ? 'Idle' : phase === 'live' ? 'Live' : phase === 'fault' ? 'Fault' : phase === 'stopping' ? 'Stopping' : phase === 'loading' ? 'Loading' : 'Starting';
}
