// Ether text helpers: display strings derived from ViewModel fields (never invented values).
import type { Availability, GpuMemory, LastSession, ModelArch, ModelRef, System, SystemKind } from '../../lib/model/types';
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

/** The model line: "Qwen 3.8 27B · GSQ-RCO IQ3_S · HIP · RX 9070 XT · ctx 96k · kv q8_0" (image: engine and size). */
export function modelLine(m: ModelRef | undefined, kind: SystemKind): string {
  if (!m) return '';
  const parts: (string | undefined | false)[] = [
    m.name,
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

/** The mini model line: name · quant · context or size · mode. */
export function modelCompact(m: ModelRef | undefined): string {
  if (!m) return '';
  return [m.name, m.quant, m.ctxTokens ? `ctx ${fmtCtx(m.ctxTokens)}` : m.imageSize, m.mode].filter(Boolean).join(' · ');
}

/** The second line of a tier tab: "Qwen 3.8 27B · IQ3_S". */
export function modelShort(m: ModelRef): string {
  return `${m.name} · ${m.quant}`;
}

/** "48 layers · 10+1 of 512 experts" / "64 layers · dense" / "shape after first load". */
export function shapeText(arch: ModelArch | null | undefined): string {
  if (!arch || !(arch.layers > 0)) return 'shape after first load';
  if (arch.experts > 0) return `${arch.layers} layers · ${arch.expertsUsed}${arch.sharedExperts ? `+${arch.sharedExperts}` : ''} of ${fmtInt(arch.experts)} experts`;
  return `${arch.layers} layers · dense`;
}

/** The previous session's median speed as a figure + unit (LLM tok/s, image s/image), or null. */
export function lastSpeed(ls: LastSession): { value: string; unit: string } | null {
  if (ls.decodeTps !== undefined) return { value: fmtTps(ls.decodeTps), unit: 'tok/s' };
  if (ls.secondsPerImage !== undefined) return { value: ls.secondsPerImage.toFixed(1), unit: 's/image' };
  return null;
}

/** "last S2 · ran 2 h 14 min · 412 requests · stopped 12 min ago" (compact: tier and end only). The speed is the figure. */
export function lastSessionText(ls: LastSession, systems: System[], compact = false): string {
  const label = tierShort(systems.find((x) => x.id === ls.system)?.label ?? ls.model.name);
  const end = ls.ended === 'fault' ? `fault ${fmtAgo(ls.endedAgoS)}` : `stopped ${fmtAgo(ls.endedAgoS)}`;
  if (compact) return [`last ${label}`, end].join(' · ');
  const count = ls.requests !== undefined ? `${fmtInt(ls.requests)} requests` : ls.images !== undefined ? `${fmtInt(ls.images)} images` : '';
  return [`last ${label}`, `ran ${fmtDur(ls.uptimeS)}`, count, end].filter(Boolean).join(' · ');
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
