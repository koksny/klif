// Silicon text helpers: display strings derived from ViewModel fields (never invented values).
import type { Availability, GpuMemory, LastSession, ModelRef, Slot } from '../../lib/model/types';
import { fmtCtx, fmtInt, fmtTps } from '../../lib/model/format';

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

/** "Qwen 3.8 27B · GSQ-RCO IQ3_S · 96k" / "Krea 2 Realism Turbo · Q8_0 · 512x768" */
export function modelShort(m: ModelRef): string {
  const parts = [m.name, m.quant];
  if (m.ctxTokens) parts.push(fmtCtx(m.ctxTokens));
  else if (m.imageSize) parts.push(m.imageSize);
  return parts.join(' · ');
}

/** The full model line under the tabs. */
export function modelLine(m: ModelRef | undefined, kind: 'llm' | 'image', steps?: number): string {
  if (!m) return '';
  const parts = [m.name, m.quant];
  if (kind === 'image') parts.push(m.engine);
  parts.push(m.backend, m.device);
  if (m.ctxTokens) parts.push(`ctx ${fmtCtx(m.ctxTokens)}`);
  if (m.kvType) parts.push(`KV ${m.kvType}`);
  if (m.specMode) parts.push(m.specMode);
  if (m.vision) parts.push('vision');
  if (m.mode) parts.push(m.mode);
  if (m.imageSize) parts.push(m.imageSize);
  if (kind === 'image' && steps) parts.push(`${steps} steps`);
  return parts.join(' · ');
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

/** 12.3 -> "12 s", 75 -> "1 min" */
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

/** Last session one-liner parts (label resolved from the slots). */
export function lastSessionParts(ls: LastSession, slots: Slot[]): { label: string; facts: string[] } {
  const label = slots.find((x) => x.id === ls.slot)?.label ?? ls.model.name;
  const facts = [fmtDur(ls.uptimeS)];
  if (ls.requests !== undefined) facts.push(`${fmtInt(ls.requests)} requests`);
  if (ls.generatedTokens !== undefined) facts.push(`${fmtInt(ls.generatedTokens)} tok`);
  if (ls.decodeTps !== undefined) facts.push(`${fmtTps(ls.decodeTps)} tok/s`);
  if (ls.images !== undefined) facts.push(`${fmtInt(ls.images)} images`);
  if (ls.secondsPerImage !== undefined) facts.push(`${ls.secondsPerImage.toFixed(1)} s/image`);
  return { label, facts };
}

/** Baseline VRAM held by others (driver, other processes); falls back to the 'other' layer. */
export function baselineOf(vram: GpuMemory): number {
  if (typeof vram.baselineGiB === 'number' && Number.isFinite(vram.baselineGiB)) return vram.baselineGiB;
  return vram.layers.filter((l) => l.id === 'other').reduce((a, l) => a + l.gib, 0);
}

/**
 * VRAM in use just before a fault, read from the 1 Hz history: the samples before `sinceS` seconds ago.
 * Returns null when the history does not reach back that far.
 */
export function vramAtFault(vram: GpuMemory, sinceS: number): number | null {
  const h = vram.history;
  const end = h.length - 1 - Math.ceil(sinceS);
  if (end < 0) return null;
  let m = 0;
  for (let i = Math.max(0, end - 2); i <= end; i++) m = Math.max(m, h[i]);
  return m;
}

/** "8.4 / 11.6 GiB" -> 0.724; null if the detail is not a byte progress. */
export function detailFraction(detail: string | undefined): number | null {
  if (!detail) return null;
  const m = /([\d.]+)\s*\/\s*([\d.]+)\s*GiB/.exec(detail);
  if (!m) return null;
  const a = Number(m[1]);
  const b = Number(m[2]);
  return b > 0 && Number.isFinite(a) ? Math.max(0, Math.min(1, a / b)) : null;
}
