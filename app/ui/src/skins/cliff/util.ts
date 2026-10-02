import type { Availability, LastSession, ModelRef, Slot, SlotKind, ViewModel, VramLayer } from '../../lib/model/types';
import { fmtCtx, fmtInt, fmtTps } from '../../lib/model/format';

/** What the main area shows. 'stopping' renders the live view of its kind. */
export type ViewState = 'idle' | 'loading' | 'fault' | 'llm' | 'image';

export type Tone = 'live' | 'busy' | 'idle' | 'stop' | 'fault';

/**
 * What the status block shows. The skeleton is the same in every one of these; only its contents change.
 * 'waiting' = live, but the first telemetry of its kind has not arrived yet.
 */
export type BlockView = 'idle' | 'loading' | 'live' | 'stopping' | 'waiting' | 'fault';

export function blockView(vm: ViewModel): BlockView {
  const s = vm.session;
  if (!s) return 'idle';
  if (s.phase === 'fault') return 'fault';
  if (s.phase === 'starting' || s.phase === 'loading') return 'loading';
  if (s.phase === 'stopping') return 'stopping';
  return (sessionKind(vm) === 'image' ? s.image : s.llm) ? 'live' : 'waiting';
}

export function selectedSlot(vm: ViewModel): Slot | null {
  return vm.slots.find((s) => s.id === vm.selected) ?? vm.slots[0] ?? null;
}

export function sessionSlot(vm: ViewModel): Slot | null {
  const s = vm.session;
  if (!s) return null;
  return vm.slots.find((x) => x.id === s.slot) ?? null;
}

export function sessionKind(vm: ViewModel): SlotKind {
  const s = vm.session;
  if (!s) return selectedSlot(vm)?.kind ?? 'llm';
  return sessionSlot(vm)?.kind ?? (s.image ? 'image' : 'llm');
}

export function viewState(vm: ViewModel): ViewState {
  const s = vm.session;
  if (!s) return 'idle';
  if (s.phase === 'fault') return 'fault';
  if (s.phase === 'starting' || s.phase === 'loading') return 'loading';
  if (sessionKind(vm) === 'image') return s.image ? 'image' : 'loading';
  return s.llm ? 'llm' : 'loading';
}

export function statusOf(vm: ViewModel): { text: string; tone: Tone } {
  const s = vm.session;
  if (!s) return { text: 'IDLE', tone: 'idle' };
  switch (s.phase) {
    case 'live':
      return { text: 'LIVE', tone: 'live' };
    case 'starting':
      return { text: 'STARTING', tone: 'busy' };
    case 'loading':
      return { text: 'LOADING', tone: 'busy' };
    case 'stopping':
      return { text: 'STOPPING', tone: 'stop' };
    case 'fault':
      return { text: 'FAULT', tone: 'fault' };
  }
}

/** The tier word without the family prefix: "AGENT MEDIUM" -> "MEDIUM". */
export function tierWord(label: string): string {
  return label.replace(/^AGENT\s+/i, '');
}

/** The tier strip's model line: "Qwen 3.8 27B · GSQ-RCO IQ3_S · 96k" / "Krea 2 Realism Turbo · Q8_0 · 512x768". */
export function modelShort(m: ModelRef): string {
  const f = [m.name, m.quant];
  if (m.ctxTokens) f.push(fmtCtx(m.ctxTokens));
  else if (m.imageSize) f.push(m.imageSize);
  return f.join(' · ');
}

/** The facts line under "Active model". */
export function modelFacts(m: ModelRef, kind: SlotKind): string[] {
  const f = kind === 'llm' ? [m.name, m.quant, m.backend, m.device] : [m.name, m.quant, m.engine, m.backend, m.device];
  if (kind === 'llm') {
    if (m.ctxTokens) f.push(`ctx ${fmtCtx(m.ctxTokens)}`);
    if (m.kvType) f.push(`KV ${m.kvType}`);
    if (m.specMode) f.push(m.specMode);
    if (m.vision) f.push('vision');
    if (m.mode) f.push(m.mode);
  } else if (m.imageSize) {
    f.push(m.imageSize);
  }
  return f.filter(Boolean);
}

/** Layer GiB label: one decimal unless that would hide a small value (0.12 stays 0.12). */
export function fmtLayer(g: number): string {
  const one = g.toFixed(1);
  if (g < 1 && Math.abs(Number(one) - g) > 0.004) return g.toFixed(2);
  return one;
}

export function sumGiB(layers: VramLayer[] | undefined): number {
  let s = 0;
  for (const l of layers ?? []) s += l.gib > 0 ? l.gib : 0;
  return s;
}

/** Fault: VRAM the failure released, from the usage history around the moment it happened (GiB). */
export function releasedGiB(vm: ViewModel): number {
  const f = vm.session?.fault;
  if (!f) return 0;
  const hist = vm.vram.history ?? [];
  const back = Math.min(hist.length, Math.ceil(f.sinceS) + 6);
  let peak = 0;
  for (let i = hist.length - back; i < hist.length; i++) peak = Math.max(peak, hist[i]);
  return Math.max(0, peak - vm.vram.usedGiB);
}

/** Why a slot cannot be launched, in words (null when it can). */
export function availabilityText(a: Availability): string | null {
  switch (a) {
    case 'ready':
      return null;
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
    default:
      return String(a);
  }
}

/** 15.2 -> "15 s ago", 184 -> "3 min ago", 7300 -> "2 h 1 min ago". */
export function fmtAgo(seconds: number): string {
  return `${fmtSpan(seconds)} ago`;
}

/** Coarse duration: "15 s", "3 min", "2 h 14 min". */
export function fmtSpan(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return m ? `${h} h ${m} min` : `${h} h`;
}

/** Compact time left: "8.4 s", "1:28". */
export function fmtEta(sec: number): string {
  if (sec < 60) return `${Math.max(0, sec).toFixed(1)} s`;
  const m = Math.floor(sec / 60);
  const r = Math.round(sec % 60);
  return `${m}:${r.toString().padStart(2, '0')}`;
}

/** The previous session in one line: "AGENT MEDIUM · 2 h 14 min · 12 requests · ... · stopped 21 min ago". */
export function lastSessionLine(last: LastSession, slots: Slot[]): string {
  const f = [slots.find((x) => x.id === last.slot)?.label ?? last.model.name, fmtSpan(last.uptimeS)];
  if (last.requests !== undefined) f.push(`${fmtInt(last.requests)} requests`);
  if (last.generatedTokens !== undefined) f.push(`${fmtInt(last.generatedTokens)} tok`);
  if (last.decodeTps !== undefined) f.push(`${fmtTps(last.decodeTps)} tok/s`);
  if (last.images !== undefined) f.push(`${fmtInt(last.images)} images`);
  if (last.secondsPerImage !== undefined) f.push(`${last.secondsPerImage.toFixed(1)} s / image`);
  f.push(`${last.ended === 'fault' ? 'ended in a fault' : 'stopped'} ${fmtAgo(last.endedAgoS)}`);
  return f.join(' · ');
}

/** "8.4 / 11.6 GiB" -> 0.72: progress encoded in a load step's own detail text, if any. */
export function detailFraction(detail: string | undefined): number | null {
  const m = detail?.match(/(\d+(?:\.\d+)?)\s*\/\s*(\d+(?:\.\d+)?)/);
  if (!m) return null;
  const a = Number(m[1]);
  const b = Number(m[2]);
  return b > 0 ? Math.max(0, Math.min(1, a / b)) : null;
}

export function prefersReducedMotion(): boolean {
  try {
    return matchMedia('(prefers-reduced-motion: reduce)').matches;
  } catch {
    return false;
  }
}
