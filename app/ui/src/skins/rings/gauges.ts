// The ring gauges around the specimen: three concentric 270° arcs opening at the bottom, like a dial.
//   outer  = VRAM used / total (warn below warnBelowGiB free, danger on spill); ghost: the idle fit preview, or what is paged out
//   middle = context fill (LLM) or the current job's step / steps (CGI)
//   inner  = live activity: load, prefill, decode speed (tok/s of 120), CGI sampling speed (it/s of 1), waking
// This module only turns a snapshot into targets and words; Specimen eases them on the frame clock.
import type { Slot, SlotKind, ViewModel } from '../../lib/model/types';
import { fmtTps } from '../../lib/model/format';
import type { Sleep } from './sleep.svelte';
import { fitOf } from './text';

/** Ring radii as multiples of the sphere's visible radius, outer first (clear of the sphere's glow). */
export const RADII = { full: [1.3, 1.21, 1.12], mini: [1.3, 1.2, 1.1] } as const;

/** Stroke widths and type per size class, in layout units (k = 1 design px). */
export function styleOf(variant: 'full' | 'mini', k: number) {
  return variant === 'mini'
    ? { radii: RADII.mini, track: 1.5 * k, stroke: 6 * k, glow: 7 * k, tick: 0, font: 17 * k, gap: 10 * k, sep: 22 * k, values: false }
    : { radii: RADII.full, track: Math.max(1, k), stroke: 3 * k, glow: 6 * k, tick: 4 * k, font: 11.5 * k, gap: 9 * k, sep: 16 * k, values: true };
}

/** The caption's top, in sphere radii below the centre: under the sphere's glow edge. */
export const CAPTION_Y = 1.06;

/** How far the gauge cluster reaches from the sphere's centre (up, down, sideways), for the layout. */
export function extentOf(variant: 'full' | 'mini', r: number, k: number) {
  const g = styleOf(variant, k);
  const out = g.radii[0] * r + g.stroke / 2 + (g.tick ? g.tick * 1.8 + 2 * k : 0);
  return { up: out, down: CAPTION_Y * r + g.gap + g.font * 1.1, side: out };
}

/** Full scale of the inner ring while decoding. */
export const TPS_FULL = 120;
/** Full scale of the inner ring while sampling (iterations per second). */
export const ITS_FULL = 1;

export type VramTone = 'ok' | 'warn' | 'danger';

/** One legend line: "VRAM 15.6 / 15.9", "39.4 TOK/S" (pre, the value, post). */
export interface Legend {
  pre: string;
  val: string;
  post: string;
}

export interface Targets {
  vram: number;
  /** Ghost arc on the outer ring (0..1, a === b: none). */
  ghostA: number;
  ghostB: number;
  ghostOver: boolean;
  tone: VramTone;
  mid: number;
  inner: number;
  legend: [Legend, Legend, Legend];
}

const clamp01 = (x: number) => (Number.isFinite(x) ? Math.min(1, Math.max(0, x)) : 0);
const g1 = (x: number) => x.toFixed(1);

export function targetsOf(vm: ViewModel, o: { kind: SlotKind; sel: Slot | undefined; dz: Sleep | null; waking: boolean }): Targets {
  const s = vm.session;
  const v = vm.vram;
  const total = Math.max(0.01, v.totalGiB);
  const llm = s?.llm ?? null;
  const img = s?.image ?? null;
  const loading = s?.phase === 'starting' || s?.phase === 'loading';

  // outer: VRAM
  let ghostA = 0;
  let ghostB = 0;
  let ghostOver = false;
  let vramPost = '';
  const fit = !s ? fitOf(v, o.sel) : null;
  if (o.dz) {
    ghostA = v.usedGiB / total;
    ghostB = (v.usedGiB + o.dz.pagedOutGiB) / total;
    vramPost = `· ${g1(o.dz.pagedOutGiB)} PAGED OUT`;
  } else if (fit) {
    ghostA = fit.base / total;
    ghostB = fit.top / total;
    ghostOver = fit.spare < 0;
    vramPost = `· FIT ${g1(fit.top)}`;
  } else if (v.spillMiB > 0) vramPost = `· SPILL ${g1(v.spillMiB / 1024)}`;
  const vram = v.usedGiB / total;
  // Same rule as every skin: a full inference card is normal; warn only below klif.toml's warn_below_gib free.
  const tone: VramTone = v.spillMiB > 0 || ghostOver ? 'danger' : s && total - v.usedGiB < v.warnBelowGiB ? 'warn' : 'ok';

  // middle: context (LLM) / step (CGI)
  let mid = 0;
  let midL: Legend;
  if (o.kind === 'image') {
    const on = !!img && img.activity === 'generating' && img.steps > 0;
    mid = on ? img!.step / img!.steps : 0;
    midL = { pre: 'STEP', val: on ? `${img!.step} / ${img!.steps}` : '—', post: '' };
  } else {
    const t = llm?.context.totalTokens ?? 0;
    mid = llm && t > 0 ? llm.context.usedTokens / t : 0;
    midL = { pre: 'CTX', val: llm && t > 0 ? `${Math.round(mid * 100)}%` : '—', post: '' };
  }

  // inner: live activity
  let inner = 0;
  let innL: Legend;
  if (loading) {
    inner = s?.loading?.fraction ?? 0;
    innL = { pre: 'LOAD', val: `${Math.floor(inner * 100)}%`, post: '' };
  } else if (o.dz && o.waking) {
    inner = o.dz.restoredFrac;
    innL = { pre: 'WAKE', val: `${Math.round(inner * 100)}%`, post: '' };
  } else if (o.dz) {
    innL = { pre: 'ASLEEP', val: o.dz.powerState ?? '', post: '' };
  } else if (s?.phase === 'live' && llm?.activity === 'prefill' && llm.prefill) {
    const p = llm.prefill;
    inner = p.tokens > 0 ? p.doneTokens / p.tokens : 0;
    innL = { pre: 'PREFILL', val: `${Math.floor(inner * 100)}%`, post: '' };
  } else if (s?.phase === 'live' && llm?.activity === 'decode') {
    inner = llm.decodeTps / TPS_FULL;
    innL = { pre: '', val: fmtTps(llm.decodeTps), post: 'TOK/S' };
  } else if (s?.phase === 'live' && img?.activity === 'generating' && img.sPerIt > 0) {
    inner = 1 / img.sPerIt / ITS_FULL;
    innL = { pre: '', val: img.sPerIt.toFixed(2), post: 'S/IT' };
  } else innL = { pre: o.kind === 'image' ? 'S/IT' : 'TOK/S', val: '—', post: '' };

  return {
    vram: clamp01(vram),
    ghostA: clamp01(ghostA),
    ghostB: clamp01(ghostB),
    ghostOver,
    tone,
    mid: clamp01(mid),
    inner: clamp01(inner),
    legend: [{ pre: 'VRAM', val: `${g1(v.usedGiB)} / ${g1(v.totalGiB)}`, post: vramPost }, midL, innL],
  };
}

// ---- geometry: angles clockwise from 12 o'clock; the dial runs from -135° (7:30) to +135° (4:30) ----------------
export const SPAN = 270;
export const START = -135;

export function polar(cx: number, cy: number, R: number, deg: number): [number, number] {
  const a = (deg * Math.PI) / 180;
  return [cx + R * Math.sin(a), cy - R * Math.cos(a)];
}

/** The arc from fraction a to b of the dial (empty when it has no length). */
export function arc(cx: number, cy: number, R: number, a: number, b: number): string {
  const f0 = Math.max(0, Math.min(1, a));
  const f1 = Math.max(0, Math.min(1, b));
  if (f1 - f0 < 0.0015) return '';
  const d0 = START + SPAN * f0;
  const d1 = START + SPAN * f1;
  const [x0, y0] = polar(cx, cy, R, d0);
  const [x1, y1] = polar(cx, cy, R, d1);
  return `M${x0.toFixed(2)} ${y0.toFixed(2)}A${R.toFixed(2)} ${R.toFixed(2)} 0 ${d1 - d0 > 180 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
}

/** Radial ticks every `step` of the dial, outside radius R. */
export function ticks(cx: number, cy: number, R: number, len: number, step = 0.1): string {
  let d = '';
  const n = Math.round(1 / step);
  for (let i = 0; i <= n; i++) {
    const deg = START + SPAN * i * step;
    const major = i === 0 || i === n || i === n / 2;
    const [x0, y0] = polar(cx, cy, R, deg);
    const [x1, y1] = polar(cx, cy, R + len * (major ? 1.8 : 1), deg);
    d += `M${x0.toFixed(2)} ${y0.toFixed(2)}L${x1.toFixed(2)} ${y1.toFixed(2)}`;
  }
  return d;
}

// ---- colour mixing on the frame clock ---------------------------------------------------------------------------
type RGB = [number, number, number];
export const CYAN: RGB = [57, 225, 255];
export const WARN: RGB = [255, 179, 71];
export const DANGER: RGB = [255, 92, 122];

export function mixRgb(a: RGB, b: RGB, t: number): RGB {
  const k = Math.max(0, Math.min(1, t));
  return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
}

export function rgba(c: RGB, alpha = 1): string {
  return `rgba(${Math.round(c[0])},${Math.round(c[1])},${Math.round(c[2])},${alpha})`;
}
