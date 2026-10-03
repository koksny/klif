// Spirit text helpers: OSD strings derived from ViewModel fields (never invented values). OSD text is upper case.
import type { Availability, GpuMemory, LastSession, LoadStep, ModelArch, ModelRef, Phase, System, SystemKind } from '../../lib/model/types';
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

/** 8047 -> "2 H 14 MIN", 754 -> "12 MIN", 42 -> "42 S" */
export function fmtDur(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} S`;
  if (s < 3600) return `${Math.floor(s / 60)} MIN`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return m ? `${h} H ${m} MIN` : `${h} H`;
}

export function fmtAgo(seconds: number): string {
  return `${fmtDur(seconds)} AGO`;
}

/** Compact remaining time: "8.4 S", "1:28". */
export function fmtEta(sec: number): string {
  if (sec < 60) return `${sec.toFixed(1)} S`;
  const m = Math.floor(sec / 60);
  const r = Math.round(sec % 60);
  return `${m}:${r.toString().padStart(2, '0')}`;
}

/** 15.517 -> "15.5" (the battery readout) */
export function gib1(n: number): string {
  return Math.max(0, n).toFixed(1);
}

/** Timecode from seconds: HH:MM:SS:FF at 24 frames. */
export function timecode(t: number): string {
  const v = Math.max(0, t);
  const s = Math.floor(v);
  const ff = Math.min(23, Math.floor((v - s) * 24));
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}:${p(ff)}`;
}
export const TC_IDLE = '--:--:--:--';

/** "SYSTEM 2" -> "S2", "SYSTEM CGI" -> "CGI" */
export const short = tierShort;

/** The model line: name · quant · backend · device · ctx or size · kv · vision · mode. */
export function modelLine(m: ModelRef | undefined, kind: SystemKind): string {
  if (!m) return '';
  const parts: (string | undefined | false)[] = [
    m.name,
    m.quant,
    kind !== 'llm' && m.engine,
    m.backend,
    m.device,
    m.ctxTokens ? `ctx ${fmtCtx(m.ctxTokens)}` : sizeText(m.imageSize),
    m.kvType && `kv ${m.kvType}`,
    m.vision && 'vision',
    m.mode,
  ];
  return parts.filter(Boolean).join(' · ').toUpperCase();
}

/** "512x768" -> "512×768" (stays a multiplication sign in upper case). */
export function sizeText(size: string | undefined): string | undefined {
  return size?.replace(/x/i, '×');
}

/** Short model line for the mini panel and the tier tabs. */
export function modelShort(m: ModelRef | undefined, withCtx = true): string {
  if (!m) return '';
  return [m.name, m.quant, withCtx && (m.ctxTokens ? fmtCtx(m.ctxTokens) : sizeText(m.imageSize)), withCtx && m.mode].filter(Boolean).join(' · ').toUpperCase();
}

/** "48L · 10+1/512 EXPERTS" / "64L · DENSE" / "SHAPE AFTER FIRST LOAD" (from ModelRef.arch). */
export function shapeText(arch: ModelArch | null | undefined, kind: SystemKind): string {
  if (kind !== 'llm' && kind !== 'image') return 'NO SHAPE REPORTED';
  const layers = arch && arch.layers >= 1 ? Math.round(arch.layers) : 0;
  if (!arch || !layers) return kind === 'image' ? 'DIT · SHAPE NOT REPORTED' : 'SHAPE AFTER FIRST LOAD';
  if (kind === 'image') return `${layers}L · DIT`;
  if (arch.experts > 0) return `${layers}L · ${arch.expertsUsed}${arch.sharedExperts ? `+${arch.sharedExperts}` : ''}/${fmtInt(arch.experts)} EXPERTS`;
  return `${layers}L · DENSE`;
}

/** "LAST S2 · 2 H 14 MIN · 12 REQUESTS · 47.3 TOK/S · STOPPED 21 MIN AGO" */
export function lastSessionText(ls: LastSession, systems: System[]): string {
  const label = systems.find((x) => x.id === ls.system)?.label;
  const facts = [fmtDur(ls.uptimeS)];
  if (ls.requests !== undefined) facts.push(`${fmtInt(ls.requests)} REQUESTS`);
  if (ls.decodeTps !== undefined) facts.push(`${fmtTps(ls.decodeTps)} TOK/S`);
  if (ls.images !== undefined) facts.push(`${fmtInt(ls.images)} IMAGES`);
  if (ls.secondsPerImage !== undefined) facts.push(`${ls.secondsPerImage.toFixed(1)} S/IMAGE`);
  const end = ls.ended === 'fault' ? `FAULT ${fmtAgo(ls.endedAgoS)}` : `STOPPED ${fmtAgo(ls.endedAgoS)}`;
  return `LAST ${label ? short(label) : ls.model.name.toUpperCase()} · ${facts.join(' · ')} · ${end}`;
}

/** A load step as an OSD line: "WEIGHTS 5.4 / 11.6 GIB". */
export function stepLine(st: LoadStep | null | undefined): string {
  if (!st) return '';
  const word = st.label.replace(/^load\s+/i, '');
  return `${word}${st.detail ? ` ${st.detail}` : ''}`.toUpperCase();
}

export function phaseWord(phase: Phase | 'idle'): string {
  return phase === 'idle' ? 'STANDBY' : phase === 'live' ? 'LIVE' : phase === 'fault' ? 'FAULT' : phase === 'stopping' ? 'STOPPING' : phase === 'loading' ? 'LOADING' : 'STARTING';
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

const MON = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];
/** The camera's date stamp from the snapshot time: "03 OCT 2026  14:21". */
export function dateStamp(epochS: number): string {
  const d = new Date(epochS * 1000);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getDate())} ${MON[d.getMonth()]} ${d.getFullYear()}  ${p(d.getHours())}:${p(d.getMinutes())}`;
}
