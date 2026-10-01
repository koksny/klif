// Geometry of the VRAM cliff. ONE axis rule: vertical = GiB.
//   - the scene bottom (baseY) is 0 GiB, the cliff lip (lipY) is vram.totalGiB;
//   - every stratum is a horizontal band whose height is exactly gib * ppg (px per GiB);
//   - free headroom is the band between the top stratum and the lip.
// Plateau width, the face profile, the sea level and the headlands are scenery (non-data).
import type { VramLayer, VramLayerId } from '../../lib/model/types';
import { hash2, noise1 } from './noise';

export type Variant = 'full' | 'mini';

export interface Band {
  id: VramLayerId;
  label: string;
  gib: number;
  /** GiB interval on the axis (bottom, top). */
  lo: number;
  hi: number;
  yTop: number;
  yBot: number;
}

export interface Loupe {
  x: number;
  y: number;
  w: number;
  h: number;
  /** Stated magnification relative to the main cliff. */
  scale: number;
  /** GiB window shown (lo..hi, hi = total). */
  lo: number;
  hi: number;
  /** px per GiB inside the loupe. */
  k: number;
}

export type Pt = [number, number];

export interface Geom {
  W: number;
  H: number;
  variant: Variant;
  total: number;
  /** Top of the stacked layers in GiB (sum of layers, clamped to total). */
  stackTop: number;
  lipX: number;
  lipY: number;
  baseY: number;
  /** px per GiB on the main cliff. */
  ppg: number;
  rockY: number;
  /** Headroom tall enough to read as open air (see TALL_PX). */
  tall: boolean;
  waterY: number;
  seaX: number;
  bands: Band[];
  /** Cross-section right edge and outer face edge, indexed by integer y (0..H). */
  inner: Float32Array;
  outer: Float32Array;
  loupe: Loupe | null;
  /** Cubic bezier of the spill channel, lip to the foot of the cliff. */
  spill: [Pt, Pt, Pt, Pt];
}

const SCALES = [10, 8, 6, 5, 4, 3, 2];

/** Headroom (px) from which the free band reads as open air: the lip turns into a dashed capacity
 *  line, the glow moves to the surface of the rock and the face starts its shoulder at that surface. */
export const TALL_PX = { full: 44, mini: 34 } as const;

export function yOfGiB(g: Geom, gib: number): number {
  return g.baseY - gib * g.ppg;
}

export function buildGeom(
  W: number,
  H: number,
  total: number,
  layers: VramLayer[],
  variant: Variant,
  topY: number,
  sidePad: number,
  /** The face starts at the lip even when the rock is low (fault: the emptied shell keeps its face). */
  shell = false,
): Geom {
  const T = Math.max(total, 0.01);
  const lipY = Math.round(topY);
  const baseY = H;
  const span = Math.max(1, baseY - lipY);
  const ppg = span / T;
  const yOf = (gib: number) => baseY - gib * ppg;

  let cum = 0;
  const bands: Band[] = [];
  for (const l of layers) {
    if (!(l.gib > 0)) continue;
    const lo = Math.min(cum, T);
    const hi = Math.min(cum + l.gib, T);
    cum += l.gib;
    bands.push({ id: l.id, label: l.label, gib: l.gib, lo, hi, yTop: yOf(hi), yBot: yOf(lo) });
  }
  const stackTop = Math.min(cum, T);
  const rockY = yOf(stackTop);

  const full = variant === 'full';
  const tall = rockY - lipY >= TALL_PX[variant];
  // Where the face's rounded shoulder begins: at the lip, or at the surface of a low rock.
  const shoulderY = tall && !shell ? rockY : lipY;
  const lipX = Math.round(W * (full ? 0.635 : 0.677));
  const waterY = Math.round(lipY + span * (full ? 0.42 : 0.22));

  // Face profile (scenery). The lip corner itself is exact: inner(lipY) = lipX.
  // inner = where the cross-section ends (near vertical); outer = the face silhouette, which drops
  // steeply under the lip and flares into a talus at the foot (convex, like the painted cliff).
  const inner = new Float32Array(H + 1);
  const outer = new Float32Array(H + 1);
  for (let y = 0; y <= H; y++) {
    const t = Math.max(0, Math.min(1, (y - lipY) / span));
    const ramp = Math.min(1, t * 7);
    // A few broad ledges: piecewise-constant offsets per block of rows.
    const k1 = Math.floor((y - lipY) / (full ? 23 : 19));
    const step = (hash2(k1, 1, 91) - 0.5) * ramp;
    const fine = (noise1(y / 6, 7) - 0.5) * ramp;
    const broad = (noise1(y / 26, 11) - 0.5) * ramp;
    // Rounded shoulder: the face bulges out of the lip over the first few rows.
    const r = full ? 15 : 12;
    const d = Math.max(0, Math.min(r, y - shoulderY));
    const shoulder = Math.sqrt(Math.max(0, r * r - (r - d) * (r - d)));
    if (full) {
      inner[y] = lipX + W * 0.03 * Math.pow(t, 1.3) + broad * 7 + fine * 2;
      outer[y] = inner[y] + shoulder + W * 0.098 * Math.pow(t, 1.35) + Math.max(-2, step * 9 * t + fine * 3 + broad * 4);
    } else {
      inner[y] = lipX + W * 0.012 * Math.pow(t, 1.2) + broad * 5 + fine * 2;
      outer[y] = inner[y] + shoulder + W * 0.085 * Math.pow(t, 1.5) + Math.max(-2, step * 14 * t + fine * 3 + broad * 3);
    }
    if (outer[y] < inner[y] + 3) outer[y] = inner[y] + 3;
  }
  const seaX = Math.floor(inner[Math.min(H, Math.max(0, waterY))]);

  // Spill channel: from the lip corner, outside the face, to the foot of the cliff.
  const footY = baseY - Math.max(12, span * 0.055);
  const p0: Pt = [lipX + 1, lipY + 1];
  const p3: Pt = [outer[Math.round(footY)] + Math.max(6, W * 0.008), footY];
  const p1: Pt = [lipX + W * 0.06, lipY];
  const p2: Pt = [lipX + W * 0.095, baseY - span * 0.4];

  let loupe: Loupe | null = null;
  if (full) {
    const lw = Math.round(Math.max(140, Math.min(200, W * 0.17)));
    const x = Math.round(W - sidePad - lw);
    const win = Math.min(1, T);
    const maxH = Math.min(span * 0.62, span - 70);
    let scale = SCALES[SCALES.length - 1];
    for (const s of SCALES) {
      if (ppg * s * win <= maxH) {
        scale = s;
        break;
      }
    }
    const k = ppg * scale;
    // Only when some rock reaches into the window: a loupe full of headroom magnifies nothing.
    if (stackTop > T - win + 0.01) loupe = { x, y: lipY, w: lw, h: k * win, scale, lo: T - win, hi: T, k };
  }

  return {
    W,
    H,
    variant,
    total: T,
    stackTop,
    lipX,
    lipY,
    baseY,
    ppg,
    rockY,
    tall,
    waterY,
    seaX,
    bands,
    inner,
    outer,
    loupe,
    spill: [p0, p1, p2, p3],
  };
}

export function bezierPoints(b: [Pt, Pt, Pt, Pt], n: number): Pt[] {
  const [p0, p1, p2, p3] = b;
  const out: Pt[] = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const u = 1 - t;
    const a = u * u * u;
    const c1 = 3 * u * u * t;
    const c2 = 3 * u * t * t;
    const d = t * t * t;
    out.push([
      a * p0[0] + c1 * p1[0] + c2 * p2[0] + d * p3[0],
      a * p0[1] + c1 * p1[1] + c2 * p2[1] + d * p3[1],
    ]);
  }
  return out;
}

export function bezierPathD(b: [Pt, Pt, Pt, Pt]): string {
  const f = (p: Pt) => `${p[0].toFixed(1)} ${p[1].toFixed(1)}`;
  return `M${f(b[0])} C${f(b[1])} ${f(b[2])} ${f(b[3])}`;
}
