// Decode text helpers: display strings derived from ViewModel fields (never invented values).
import type { Availability, GpuMemory, ModelRef, Phase, SlotKind } from '../../lib/model/types';
import { fmtCtx, tierShort } from '../../lib/model/format';

/** Why a slot cannot launch, in plain words. */
export function availabilityText(a: Availability): string {
  switch (a) {
    case 'ready':
      return 'ready';
    case 'model-missing':
      return 'model missing';
    case 'build-required':
      return 'build required';
    case 'script-missing':
      return 'script missing';
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

/** "SYSTEM 2" -> "s2" */
export function tierWord(label: string): string {
  return tierShort(label).toLowerCase();
}

/** The terminal model line: name · quant · backend · device · ctx or size · kv · vision · mode (lower case). */
export function modelLine(m: ModelRef | undefined, kind: SlotKind): string {
  if (!m) return '';
  const parts: (string | undefined | false)[] = [
    m.name,
    m.quant,
    kind === 'image' && m.engine,
    m.backend,
    m.device,
    m.ctxTokens ? `ctx ${fmtCtx(m.ctxTokens)}` : m.imageSize,
    m.kvType && `kv ${m.kvType}`,
    m.vision && 'vision',
    m.mode,
  ];
  return parts.filter(Boolean).join(' · ').toLowerCase();
}

/** Short form for the mini panel. */
export function modelShort(m: ModelRef | undefined): string {
  if (!m) return '';
  return [m.name, m.quant, m.ctxTokens ? fmtCtx(m.ctxTokens) : m.imageSize, m.mode].filter(Boolean).join(' · ').toLowerCase();
}

export function phaseWord(phase: Phase | 'idle'): string {
  return phase === 'idle' ? 'IDLE' : phase === 'live' ? 'LIVE' : phase === 'fault' ? 'FAULT' : phase === 'stopping' ? 'STOPPING' : phase === 'loading' ? 'LOADING' : 'STARTING';
}
