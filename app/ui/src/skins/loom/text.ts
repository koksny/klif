// Loom text helpers: display strings derived from ViewModel fields (never invented values).
import type { Availability, GpuMemory, LastSession, ModelRef, Slot } from '../../lib/model/types';
import { fmtCtx, fmtInt, fmtTps, tierShort } from '../../lib/model/format';

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

/** The prompt line: "qwen 3.8 27b · gsq-rco iq3_s · hip · rx 9070 xt · ctx 96k · kv q8_0" (lower case, as on a terminal). */
export function modelText(m: ModelRef | undefined): string {
  if (!m) return '';
  const parts = [m.name, m.quant, m.backend, m.device];
  if (m.ctxTokens) parts.push(`ctx ${fmtCtx(m.ctxTokens)}`);
  else if (m.imageSize) parts.push(m.imageSize);
  if (m.kvType) parts.push(`kv ${m.kvType}`);
  if (m.vision) parts.push('vision');
  if (m.mode) parts.push(m.mode);
  return parts.join(' · ').toLowerCase();
}

/** Tier strip second line: "qwen 3.8 27b · gsq-rco iq3_s". */
export function modelShort(m: ModelRef): string {
  return `${m.name} · ${m.quant}`.toLowerCase();
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

/** "last agent high · 2 h 14 min · 412 requests · 38.1 tok/s · stopped 12 min ago" */
export function lastSessionText(ls: LastSession, slots: Slot[]): string {
  const label = slots.find((x) => x.id === ls.slot)?.label ?? ls.model.name;
  const facts = [fmtDur(ls.uptimeS)];
  if (ls.requests !== undefined) facts.push(`${fmtInt(ls.requests)} requests`);
  if (ls.decodeTps !== undefined) facts.push(`${fmtTps(ls.decodeTps)} tok/s`);
  if (ls.images !== undefined) facts.push(`${fmtInt(ls.images)} images`);
  if (ls.secondsPerImage !== undefined) facts.push(`${ls.secondsPerImage.toFixed(1)} s/image`);
  const end = ls.ended === 'fault' ? `fault ${fmtAgo(ls.endedAgoS)}` : `stopped ${fmtAgo(ls.endedAgoS)}`;
  return `last ${label.toLowerCase()} · ${facts.join(' · ')} · ${end}`;
}

/** Baseline VRAM held by others (driver, other processes); falls back to the 'other' layer. */
export function baselineOf(vram: GpuMemory): number {
  if (typeof vram.baselineGiB === 'number' && Number.isFinite(vram.baselineGiB)) return vram.baselineGiB;
  return vram.layers.filter((l) => l.id === 'other').reduce((a, l) => a + l.gib, 0);
}

/** The selected tier's expected footprint on top of what is in use (idle fit preview), or null. */
export function fitOf(vram: GpuMemory, slot: Slot | undefined): { base: number; top: number; spare: number } | null {
  if (!slot?.expectedVram?.length) return null;
  const base = Math.max(baselineOf(vram), vram.usedGiB);
  const top = base + slot.expectedVram.reduce((a, l) => a + l.gib, 0);
  return { base, top, spare: vram.totalGiB - top };
}

/** "SYSTEM 2" -> "S2" */
export function shortTier(label: string): string {
  return tierShort(label);
}
