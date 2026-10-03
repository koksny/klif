// Instrument (Zegar): palette, fonts, the machined panel texture and small shared helpers.
import type { GpuMemory, ModelRef, Phase, Session, System, SystemId, SystemKind, ViewModel } from '../../lib/model/types';
import { selectedSystem, shortLabel as tierShort } from '../../lib/model/systems';
import { fmtCtx } from '../../lib/model/format';

export const PAL = {
  window: '#141517',
  graphite: '#1B1C1E',
  panel: '#26272A',
  face: '#151618',
  line: '#0B0C0D',
  cream: '#EDE6D6',
  muted: '#8E8A82',
  cyan: '#5AB6EB',
  cyanInk: '#0D1117',
  orange: '#FF6B2C',
  amber: '#FFB02E',
  segOff: '#33353A',
} as const;

// Industrial equipment lettering: engraved captions in Barlow Condensed caps, text and values in Barlow
// with tabular figures, the drum counters in their own mechanical faces. Mono only for the console line,
// ports, API keys and fault log lines.
export const FONT = {
  label: "'Barlow Condensed', 'Oswald Variable', 'Arial Narrow', sans-serif",
  text: "'Barlow', 'Barlow Condensed', 'Segoe UI', sans-serif",
  hero: "'Archivo Variable', 'Barlow Condensed', sans-serif",
  mono: "'Share Tech Mono', ui-monospace, monospace",
} as const;

/** Gauge sweep of the mini VRAM half dial: 0 GiB 20 deg above 9 o'clock, totalGiB 20 deg above 3 o'clock. */
export const VRAM_A0 = 160;
export const VRAM_A1 = 20;
/** Gauge sweep of the full VRAM dial: 0 GiB 20 deg below 9 o'clock, totalGiB 20 deg above 3 o'clock (180 deg). */
export const VRAM_FULL_A0 = 200;
export const VRAM_FULL_A1 = 20;

export const clamp = (v: number, lo = 0, hi = 1) => (v < lo ? lo : v > hi ? hi : v);
export const frac = (a: number, b: number) => (b > 0 && Number.isFinite(a) ? clamp(a / b) : 0);

/** Point on a circle; angle in degrees measured from +x, counter-clockwise (screen y is flipped). */
export function polar(cx: number, cy: number, r: number, deg: number): [number, number] {
  const a = (deg * Math.PI) / 180;
  return [cx + r * Math.cos(a), cy - r * Math.sin(a)];
}

/** Annular sector between two angles (degrees, CCW from +x), from a0 to a1 with a0 > a1 (clockwise sweep). */
export function sectorPath(cx: number, cy: number, rIn: number, rOut: number, a0: number, a1: number): string {
  const [x0, y0] = polar(cx, cy, rOut, a0);
  const [x1, y1] = polar(cx, cy, rOut, a1);
  const [x2, y2] = polar(cx, cy, rIn, a1);
  const [x3, y3] = polar(cx, cy, rIn, a0);
  const large = Math.abs(a0 - a1) > 180 ? 1 : 0;
  return `M${x0},${y0} A${rOut},${rOut} 0 ${large} 1 ${x1},${y1} L${x2},${y2} A${rIn},${rIn} 0 ${large} 0 ${x3},${y3} Z`;
}

/** 11.6 -> "11.6", 0.9 -> "0.9", 0.12 -> "0.12" (layer values printed on the dial). */
export function fmtLayer(gib: number): string {
  if (gib >= 10) return gib.toFixed(1);
  const tenth = Math.abs(gib * 10 - Math.round(gib * 10)) < 0.005;
  return gib.toFixed(tenth ? 1 : 2);
}

export const PHASE_LABEL: Record<Phase | 'idle', string> = {
  idle: 'IDLE',
  starting: 'STARTING',
  loading: 'LOADING',
  live: 'LIVE',
  stopping: 'STOPPING',
  fault: 'FAULT',
};

export function phaseOf(vm: ViewModel): Phase | 'idle' {
  return vm.session?.phase ?? 'idle';
}

/** The System whose pointer the selector shows: the selected one (vm.session is always its session). */
export function pointerSlot(vm: ViewModel): SystemId {
  return selectedSystem(vm)?.id ?? vm.selected ?? '';
}

export function slotById(vm: ViewModel, id: SystemId): System | undefined {
  return vm.systems.find((s) => s.id === id);
}

/** "SYSTEM 2" -> "S2" for the tiny panel. */
export function shortLabel(label: string): string {
  return tierShort(label);
}

/** Subtitle under a selector detent: "Qwen 3.8 27B · GSQ-RCO IQ3_S · 96k". */
export function slotSubtitle(slot: System): string {
  if (slot.status === 'not-set') return 'no preset';
  const m = slot.model;
  const parts = [m.name, m.quant];
  if (slot.kind === 'llm' && m.ctxTokens) parts.push(fmtCtx(m.ctxTokens));
  if (slot.kind === 'image' && m.imageSize) parts.push(m.imageSize);
  return parts.filter(Boolean).join(' · ');
}

/** The ACTIVE MODEL line, built only from configured facts. */
export function modelLine(session: Session | null, slot: System | undefined): string {
  const m = session?.model ?? slot?.model;
  if (!m) return '';
  // Image engines are named on the line (sd.cpp): the LLM line keeps its approved form.
  const parts: string[] = m.imageSize || (slot && slot.kind !== 'llm') ? [m.name, m.quant, m.engine, m.backend, m.device] : [m.name, m.quant, m.backend, m.device];
  if (m.ctxTokens) parts.push(`ctx ${fmtCtx(m.ctxTokens)}`);
  if (m.kvType) parts.push(`KV ${m.kvType}`);
  if (m.specMode) parts.push(m.specMode);
  if (m.mode) parts.push(m.mode);
  if (m.vision) parts.push('vision');
  if (m.imageSize) parts.push(m.imageSize);
  return parts.filter(Boolean).join(' · ');
}

const AVAIL_LABEL: Record<string, string> = {
  ready: 'ready',
  unsupported: 'unsupported',
  invalid: 'needs fixing',
  'exe-missing': 'program missing',
  'model-missing': 'model missing',
  busy: 'port busy',
};
export const availLabel = (a: string) => AVAIL_LABEL[a] ?? a;

/**
 * What the panel shows, one word per phase. 'other' = a live session that has not reported its kind's
 * data yet. Every region of the full window keeps its place in all of them; only the contents change.
 */
export type View = 'idle' | 'loading' | 'live-llm' | 'live-img' | 'live-gen' | 'stopping' | 'fault' | 'other';

/** The kind the panel reads: the selected System's. */
export function kindOf(vm: ViewModel): SystemKind {
  return slotById(vm, pointerSlot(vm))?.kind ?? 'llm';
}

export function viewOf(vm: ViewModel): View {
  const s = vm.session;
  if (!s) return 'idle';
  if (s.phase === 'fault') return 'fault';
  if (s.phase === 'starting' || s.phase === 'loading') return 'loading';
  if (s.phase === 'stopping') return 'stopping';
  const kind = kindOf(vm);
  if (kind === 'llm' && s.llm) return 'live-llm';
  if (kind === 'image' && s.image) return 'live-img';
  if (kind !== 'llm' && kind !== 'image' && s.generic) return 'live-gen';
  return 'other';
}

/** "Qwen 3.8 Flash-Next · IQ2_XXS · 128k · Thinking": the mini's model line. */
export function modelShort(m: ModelRef | undefined): string {
  if (!m) return '';
  const parts = [m.name, m.quant];
  if (m.ctxTokens) parts.push(fmtCtx(m.ctxTokens));
  if (m.imageSize) parts.push(m.imageSize);
  if (m.mode) parts.push(m.mode);
  return parts.filter(Boolean).join(' · ');
}

/** The previous session as one line for the timeline caption: label, facts, how it ended. */
export function lastSessionText(vm: ViewModel): { text: string; fault: boolean } | null {
  const ls = vm.lastSession;
  if (!ls) return null;
  const label = slotById(vm, ls.system)?.label ?? ls.model.name;
  const facts = [fmtDur(ls.uptimeS)];
  if (ls.requests !== undefined) facts.push(`${fmtIntLocal(ls.requests)} requests`);
  if (ls.generatedTokens !== undefined) facts.push(`${fmtIntLocal(ls.generatedTokens)} tok`);
  if (ls.decodeTps !== undefined) facts.push(`${ls.decodeTps >= 100 ? Math.round(ls.decodeTps) : ls.decodeTps.toFixed(1)} tok/s`);
  if (ls.images !== undefined) facts.push(`${fmtIntLocal(ls.images)} images`);
  if (ls.secondsPerImage !== undefined) facts.push(`${fmtJobS(ls.secondsPerImage)} per image`);
  const end = ls.ended === 'fault' ? `ended in a fault ${fmtAgo(ls.endedAgoS)}` : `stopped ${fmtAgo(ls.endedAgoS)}`;
  return { text: `last session: ${label} · ${facts.join(' · ')} · ${end}`, fault: ls.ended === 'fault' };
}

// ---------------------------------------------------------------------------------------------
// Machined panel texture: generated ONCE per page into an offscreen canvas, then reused as a
// CSS background image. Plainly background: fine horizontal brushing plus grain, no structure.

let textureUrl: string | null = null;

export function machinedTexture(): string {
  if (textureUrl !== null) return textureUrl;
  textureUrl = '';
  try {
    const size = 384;
    const c = document.createElement('canvas');
    c.width = size;
    c.height = size;
    const g = c.getContext('2d');
    if (!g) return textureUrl;
    // Deterministic LCG so the texture is identical on every load.
    let seed = 0x2f6b1c;
    const rnd = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 4294967296;
    };
    const img = g.createImageData(size, size);
    const d = img.data;
    // Per-row brushing offset gives long horizontal streaks.
    const row = new Float32Array(size);
    for (let y = 0; y < size; y++) row[y] = (rnd() - 0.5) * 0.9 + (y > 0 ? row[y - 1] * 0.35 : 0);
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const v = row[y] + (rnd() - 0.5) * 0.7;
        const i = (y * size + x) * 4;
        const light = v > 0;
        d[i] = d[i + 1] = d[i + 2] = light ? 255 : 0;
        d[i + 3] = Math.min(255, Math.abs(v) * 6);
      }
    }
    g.putImageData(img, 0, 0);
    textureUrl = c.toDataURL('image/png');
  } catch {
    textureUrl = '';
  }
  return textureUrl;
}

// Printed rock grain for the cliff strata on the VRAM dial: generated ONCE per page, reused as an SVG
// pattern. Plainly texture: soft speckle with faint horizontal bedding, no structure that reads as data.
let rockUrl: string | null = null;

export function rockTexture(): string {
  if (rockUrl !== null) return rockUrl;
  rockUrl = '';
  try {
    const size = 256;
    const c = document.createElement('canvas');
    c.width = size;
    c.height = size;
    const g = c.getContext('2d');
    if (!g) return rockUrl;
    let seed = 0x51c3a7;
    const rnd = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 4294967296;
    };
    const img = g.createImageData(size, size);
    const d = img.data;
    const bed = new Float32Array(size);
    for (let y = 0; y < size; y++) bed[y] = (rnd() - 0.5) * 0.5 + (y > 0 ? bed[y - 1] * 0.6 : 0);
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const v = bed[y] * 0.6 + (rnd() - 0.5) + (rnd() < 0.015 ? 1.4 : 0);
        const i = (y * size + x) * 4;
        const light = v > 0;
        d[i] = d[i + 1] = d[i + 2] = light ? 255 : 0;
        d[i + 3] = Math.min(255, Math.abs(v) * 70);
      }
    }
    g.putImageData(img, 0, 0);
    rockUrl = c.toDataURL('image/png');
  } catch {
    rockUrl = '';
  }
  return rockUrl;
}

/** 8047 -> "2 h 14 min", 754 -> "12 min", 42 -> "42 s" (run lengths on the launcher). */
export function fmtDur(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h} h ${m} min` : `${m} min`;
}

/** 12.4 -> "12 s ago", 190 -> "3 min ago", 7300 -> "2 h ago". */
export function fmtAgo(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  return `${Math.floor(s / 3600)} h ago`;
}

/** Seconds of one image job: 20.63 -> "20.6 s", 56.2 -> "56 s". */
export function fmtJobS(seconds: number): string {
  return seconds < 100 ? `${seconds.toFixed(1)} s` : `${Math.round(seconds)} s`;
}

/** Remaining time: 8.4 -> "8.4 s", 88 -> "1:28", 119.6 -> "2:00" (never "1:60"). */
export function fmtLeft(seconds: number): string {
  const s = Math.max(0, seconds);
  if (s < 59.95) return `${s.toFixed(1)} s`;
  const t = Math.round(s);
  return `${Math.floor(t / 60)}:${(t % 60).toString().padStart(2, '0')}`;
}

/**
 * The cliff's time axis for a session younger than the 5-minute history: it spans the session itself
 * (plus a few seconds of the pre-launch floor) instead of minutes of idle baseline, so a load visibly
 * builds the cliff and a crash leaves its whole ghost on the dial. The axis label follows the sample
 * count ("-18 s"), so the window is printed, not implied. Sessions older than the history (and the
 * idle launcher) get the full history: the same object comes back untouched.
 */
export function sessionVram(vram: GpuMemory, spanS: number): GpuMemory {
  const n = Math.max(12, Math.ceil(Math.max(0, spanS)) + 4);
  if (!Number.isFinite(n) || vram.history.length <= n) return vram;
  const cut = vram.history.length - n;
  const layerHistory = vram.layerHistory
    ? Object.fromEntries(Object.entries(vram.layerHistory).map(([k, v]) => [k, v ? v.slice(cut) : v]))
    : undefined;
  return { ...vram, history: vram.history.slice(cut), layerHistory };
}

/** Seconds the current session has covered on the clock: uptime (frozen at a fault) plus time since. */
export function sessionSpanS(session: Session | null): number {
  if (!session) return Infinity;
  return session.uptimeS + (session.phase === 'fault' ? (session.fault?.sinceS ?? 0) : 0);
}

/** 28273 -> "28.3k" for long token counts, small counts in full. */
export function fmtKTok(n: number): string {
  return n >= 10000 ? `${(n / 1000).toFixed(1)}k` : fmtIntLocal(n);
}

function fmtIntLocal(n: number): string {
  return Math.round(n).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ' ');
}

/** Median of a list (0 for an empty list). */
export function median(xs: number[]): number {
  if (!xs.length) return 0;
  const a = [...xs].sort((p, q) => p - q);
  const m = a.length >> 1;
  return a.length % 2 ? a[m] : (a[m - 1] + a[m]) / 2;
}

// ---------------------------------------------------------------------------------------------
// GPU dormancy (GpuMemory.dormant): the inference GPU powered down (AMD ULPS / device state D3) with a
// model loaded. The session's allocations stay (layers = allocations), usedGiB is what is resident.

export type SleepPhase = 'asleep' | 'falling' | 'waking';

export interface Sleep {
  phase: SleepPhase;
  /** What the session's layers allocate (sum of vram.layers): where the ghost pointer stands. */
  allocGiB: number;
  /** Resident in VRAM right now (vram.usedGiB). */
  usedGiB: number;
  /** vram.dormant.pagedOutGiB, as reported. */
  pagedGiB: number;
  sinceS: number;
  /** "D3", if the core knows it. */
  power: string | null;
  /** Share of the session's memory resident again, 0..1: used / (used + paged out), the restore progress while waking. */
  restored: number;
}

/**
 * The dormancy read from the GPU memory, or null while the GPU is awake. The phase follows the resident
 * amount over the last ~3 s of history: rising = waking (restoring from system RAM), falling = still
 * powering down, flat = asleep (a flat but substantially resident amount reads as waking).
 */
export function sleepOf(vram: GpuMemory): Sleep | null {
  const d = vram.dormant;
  if (!d) return null;
  const used = Math.max(0, vram.usedGiB);
  const paged = Math.max(0, d.pagedOutGiB);
  const sum = vram.layers.reduce((a, l) => a + Math.max(0, l.gib), 0);
  const allocGiB = sum > 0.01 ? sum : used + paged;
  const h = vram.history;
  const ref = h.length >= 4 ? h[h.length - 4] : h.length ? h[0] : used;
  const delta = used - ref;
  // Restore progress from what the core reports: resident vs. resident + still paged out.
  const restored = frac(used, used + paged);
  const phase: SleepPhase = delta > 0.3 ? 'waking' : delta < -0.3 ? 'falling' : restored >= 0.5 ? 'waking' : 'asleep';
  return { phase, allocGiB, usedGiB: used, pagedGiB: paged, sinceS: Math.max(0, d.sinceS), power: d.powerState ?? null, restored };
}

/** The lamp's printed word. */
export const sleepWord = (s: Sleep) => (s.phase === 'waking' ? 'WAKING' : 'SLEEP');

/** "D3 · asleep 14 s", "waking from D3 · asleep 14 s", "D3 · powering down". */
export function sleepDetail(s: Sleep): string {
  const p = s.power ?? 'dormant';
  if (s.phase === 'falling') return `${p} · powering down`;
  if (s.phase === 'waking') return `waking from ${p} · asleep ${fmtDur(s.sinceS)}`;
  return `${p} · asleep ${fmtDur(s.sinceS)}`;
}
