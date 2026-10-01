// Small geometry and text helpers shared by the Phosphor widgets.
import type { ModelRef } from '../../lib/model/types';
import { fmtCtx } from '../../lib/model/format';

export type Pt = [number, number];

/** SVG path through the points, smoothed with quadratic segments through the midpoints. */
export function smoothSvg(pts: Pt[]): string {
  if (pts.length === 0) return '';
  if (pts.length < 3) return 'M' + pts.map((p) => `${p[0].toFixed(1)},${p[1].toFixed(1)}`).join('L');
  let d = `M${pts[0][0].toFixed(1)},${pts[0][1].toFixed(1)}`;
  for (let i = 1; i < pts.length - 1; i++) {
    const [x, y] = pts[i];
    const mx = (x + pts[i + 1][0]) / 2;
    const my = (y + pts[i + 1][1]) / 2;
    d += `Q${x.toFixed(1)},${y.toFixed(1)} ${mx.toFixed(1)},${my.toFixed(1)}`;
  }
  const last = pts[pts.length - 1];
  return d + `L${last[0].toFixed(1)},${last[1].toFixed(1)}`;
}

/** Same smoothing on a canvas path (caller strokes). Coordinates are pre-scaled by `s`. */
export function smoothCanvas(ctx: CanvasRenderingContext2D, xs: Float32Array, ys: Float32Array, n: number, s = 1) {
  ctx.beginPath();
  if (n === 0) return;
  ctx.moveTo(xs[0] * s, ys[0] * s);
  if (n < 3) {
    for (let i = 1; i < n; i++) ctx.lineTo(xs[i] * s, ys[i] * s);
    return;
  }
  for (let i = 1; i < n - 1; i++) {
    ctx.quadraticCurveTo(xs[i] * s, ys[i] * s, ((xs[i] + xs[i + 1]) / 2) * s, ((ys[i] + ys[i + 1]) / 2) * s);
  }
  ctx.lineTo(xs[n - 1] * s, ys[n - 1] * s);
}

/** Round a data range outward to a readable step so the scale labels stay calm. */
export function niceRange(min: number, max: number, minSpan: number): [number, number] {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [0, minSpan];
  const span = Math.max(max - min, minSpan);
  const step = span > 40 ? 10 : span > 16 ? 5 : 2;
  let lo = Math.floor(min / step) * step;
  let hi = Math.ceil(max / step) * step;
  if (hi - lo < minSpan) hi = lo + Math.ceil(minSpan / step) * step;
  if (hi === max) hi += step;
  if (lo < 0) lo = 0;
  return [lo, hi];
}

/**
 * How many new 1 Hz samples arrived between two snapshots of a rolling history.
 * 0 = no shift (re-emitted, or only the newest sample was revised), -1 = unrelated history.
 * The newest previous sample is allowed to differ: a live sample may be refined before it rolls.
 */
export function newSamples(prev: number[] | null, next: number[]): number {
  if (!prev || prev.length === 0) return -1;
  if (next.length > prev.length && next.length - prev.length <= 5) {
    for (let i = 0; i < prev.length - 1; i++) if (prev[i] !== next[i]) return -1;
    return next.length - prev.length;
  }
  if (next.length !== prev.length) return -1;
  const n = next.length;
  const probe = Math.min(24, n - 6);
  for (let k = 0; k <= 5; k++) {
    let ok = true;
    // prev[n-1] is excluded from the comparison (index i + k < n - 1).
    for (let i = Math.max(0, n - 1 - k - probe); i < n - 1 - k; i++) {
      if (next[i] !== prev[i + k]) {
        ok = false;
        break;
      }
    }
    if (ok) return k;
  }
  return -1;
}

export function sameSeries(a: number[] | null, b: number[]): boolean {
  if (!a || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

/** The one-line recipe under the job selector, from configured facts only. */
export function recipeLine(m: ModelRef): string[] {
  const parts: (string | undefined | false)[] = [
    m.name,
    m.quant,
    m.backend,
    m.device,
    m.ctxTokens ? `ctx ${fmtCtx(m.ctxTokens)}` : undefined,
    m.kvType ? `KV ${m.kvType}` : undefined,
    m.specMode,
    m.vision && 'vision',
    m.mode,
    m.imageSize,
  ];
  return parts.filter((p): p is string => typeof p === 'string' && p.length > 0);
}

export const clamp = (v: number, a: number, b: number) => (v < a ? a : v > b ? b : v);

/** Why a slot cannot be launched, in words (undefined when it is ready). */
export const AVAIL_TEXT: Record<string, string> = {
  unsupported: 'unsupported',
  'script-missing': 'script missing',
  'model-missing': 'model missing',
  'build-required': 'build required',
};

/** Short model line for a tier card: name · quant · ctx (LLM) or name · quant · size (image). */
export function modelLine(m: ModelRef): string[] {
  const parts: (string | undefined)[] = [m.name, m.quant, m.ctxTokens ? fmtCtx(m.ctxTokens) : m.imageSize];
  return parts.filter((p): p is string => !!p);
}

/** 8047 -> "2 h 14 min", 754 -> "12 min 34 s", 12.4 -> "12 s". */
export function fmtDur(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min ${s % 60} s`;
  return `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`;
}

/** 12.4 -> "12 s ago", 1260 -> "21 min ago". */
export function fmtAgo(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  return `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min ago`;
}

/** Seconds per iteration: 2 decimals below 10 s, 1 above. */
export function fmtSPerIt(v: number): string {
  return v >= 10 ? v.toFixed(1) : v.toFixed(2);
}

/** Width of a time window in words for axis labels: 300 -> "−5 min", 45 -> "−45 s". */
export function fmtSpan(seconds: number): string {
  return seconds >= 120 ? `−${Math.round(seconds / 60)} min` : `−${Math.round(seconds)} s`;
}

/**
 * Window of the VRAM history shown while a model loads: the climb since launch plus a margin, in 30 s
 * steps so the scale does not change every second.
 */
export function loadSpanS(elapsedS: number): number {
  return Math.min(300, Math.max(30, Math.ceil((elapsedS + 10) / 30) * 30));
}
