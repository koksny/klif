// Canvas painters for the Cliff skin.
//   rockTexture  - procedural noise + strata banding, generated ONCE per size and cached.
//   paintScene   - the static cliff (strata clipped by data paths, face, sea, headlands, loupe).
//                  Called only when the layer composition or the size changes.
//   paintSpill   - the animated spill stream (only while vram.spillMiB > 0).
//   paintDebris  - fault: rock breaking off the emptied face, played once (~2.4 s) then left still.
//   contourCanvas - the background sea-chart contours, generated once per size.
import type { VramLayerId } from '../../lib/model/types';
import { bezierPoints, type Geom, type Pt } from './geometry';
import { fbm, hash2, noise1, vnoise } from './noise';

export const C = {
  basalt: '#0F1316',
  slate: '#1D252A',
  s1: '#2C363C',
  s2: '#3A464D',
  s3: '#4A5860',
  sky: '#5AB6EB',
  foam: '#DCEFF8',
  amber: '#F2A33A',
  danger: '#E8645A',
  muted: '#8FA3AE',
};

/** Base colours of the strata (lighter towards the top of a typical stack). */
export const LAYER_COLOR: Record<VramLayerId, string> = {
  weights: '#313B41',
  kv: '#3E4A51',
  buffers: '#4E5C64',
  draft: '#687881',
  projector: '#46525A',
  other: '#37424A',
};

// ---------------------------------------------------------------- rock texture (cached)

let texCache: { w: number; h: number; s: number; c: HTMLCanvasElement } | null = null;

/** Returns a cached rock texture close to the requested CSS size, rendered at `scale` device px per
 *  CSS px (it is always drawn stretched to the scene, so a near miss is reused, not regenerated). */
export function getRockTexture(w: number, h: number, scale = 1): HTMLCanvasElement {
  const tw = Math.max(64, Math.min(1400, Math.round(w)));
  const th = Math.max(64, Math.min(800, Math.round(h)));
  const s = Math.max(1, Math.min(2, scale));
  if (texCache && texCache.s === s && Math.abs(texCache.w - tw) < 160 && Math.abs(texCache.h - th) < 160) return texCache.c;
  const c = document.createElement('canvas');
  c.width = Math.round(tw * s);
  c.height = Math.round(th * s);
  const ctx = c.getContext('2d')!;
  ctx.scale(s, s);
  // 1. Soft blotches at quarter resolution, smoothed up (low contrast).
  const sw = Math.ceil(tw / 4);
  const sh = Math.ceil(th / 4);
  const small = document.createElement('canvas');
  small.width = sw;
  small.height = sh;
  const sctx = small.getContext('2d')!;
  const simg = sctx.createImageData(sw, sh);
  for (let y = 0; y < sh; y++) {
    for (let x = 0; x < sw; x++) {
      const v = (fbm((x * 4) / 110, (y * 4) / 50, 3, 3) - 0.5) * 0.75 + (vnoise((x * 4) / 22, (y * 4) / 13, 8) - 0.5) * 0.3;
      const i = (y * sw + x) * 4;
      const light = v > 0;
      simg.data[i] = light ? 225 : 2;
      simg.data[i + 1] = light ? 238 : 5;
      simg.data[i + 2] = light ? 246 : 8;
      simg.data[i + 3] = Math.min(255, Math.abs(v) * (light ? 34 : 80));
    }
  }
  sctx.putImageData(simg, 0, 0);
  ctx.imageSmoothingEnabled = true;
  ctx.drawImage(small, 0, 0, tw, th);
  // 2. Fine grain (per device pixel).
  const gw = c.width;
  const gh = c.height;
  const grain = ctx.createImageData(gw, gh);
  for (let i = 0, n = gw * gh; i < n; i++) {
    const v = hash2(i % gw, (i / gw) | 0, 9) - 0.5;
    const j = i * 4;
    const light = v > 0;
    grain.data[j] = light ? 230 : 0;
    grain.data[j + 1] = light ? 240 : 0;
    grain.data[j + 2] = light ? 246 : 0;
    grain.data[j + 3] = Math.abs(v) * 34;
  }
  const gc = document.createElement('canvas');
  gc.width = gw;
  gc.height = gh;
  gc.getContext('2d')!.putImageData(grain, 0, 0);
  ctx.drawImage(gc, 0, 0, tw, th);
  // 3. Laminations: fine wavy lines, like contour lines drawn on the rock.
  ctx.lineWidth = 1;
  for (let i = 0, y = 2; y < th + 16; i++) {
    y += 6 + hash2(i, 0, 501) * 10;
    const amp = 2.5 + hash2(i, 1, 501) * 5;
    const light = hash2(i, 2, 501) < 0.72;
    const a = light ? 0.07 + hash2(i, 3, 501) * 0.12 : 0.1 + hash2(i, 3, 501) * 0.14;
    ctx.strokeStyle = light ? `rgba(220,239,248,${a.toFixed(3)})` : `rgba(0,3,5,${a.toFixed(3)})`;
    ctx.beginPath();
    for (let x = -12; x <= tw + 12; x += 6) {
      const yy =
        y +
        (noise1(x / 75 + i * 0.37, 61) - 0.5) * amp * 3.2 +
        (noise1(x / 160 + i * 0.11, 63) - 0.5) * 10 +
        (noise1(x / 24 + i * 3.1, 62) - 0.5) * 1.6;
      if (x === -12) ctx.moveTo(x, yy);
      else ctx.lineTo(x, yy);
    }
    ctx.stroke();
  }
  // 4. Fissures: thin dark cracks with a lit lip, mostly vertical (scenery, deterministic).
  const n = Math.round((tw * th) / 6500);
  for (let k = 0; k < n; k++) {
    let x = hash2(k, 1, 301) * tw;
    let y = hash2(k, 2, 301) * th;
    const len = 6 + hash2(k, 3, 301) * 30;
    const lean = (hash2(k, 4, 301) - 0.5) * 0.5;
    ctx.beginPath();
    ctx.moveTo(x, y);
    for (let j = 0; j < len; j += 2) {
      x += lean + (hash2(k, j, 302) - 0.5) * 1.6;
      y += 2;
      ctx.lineTo(x, y);
    }
    const a = 0.14 + hash2(k, 5, 301) * 0.24;
    ctx.strokeStyle = `rgba(3,6,8,${a.toFixed(3)})`;
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.translate(1, 0);
    ctx.strokeStyle = `rgba(220,239,248,${(a * 0.3).toFixed(3)})`;
    ctx.stroke();
    ctx.setTransform(s, 0, 0, s, 0, 0);
  }
  texCache = { w: tw, h: th, s, c };
  return c;
}

// ---------------------------------------------------------------- static scene

/** Free headroom fill: uniform within the band; a thin band glows, a tall one stays calm. */
function freeFill(bandPx: number, warn: boolean, max = 0.62): string {
  const a = Math.max(0.15, Math.min(max, max - (bandPx - 10) * 0.012));
  return warn ? `rgba(242,163,58,${a.toFixed(3)})` : `rgba(90,182,235,${a.toFixed(3)})`;
}

/**
 * What the cliff is showing:
 *   live     - the measured layers of a running session;
 *   building - the measured layers while the session loads (the rock grows from the foot);
 *   fit      - idle fit preview: measured baseline (solid) + the slot's EXPECTED layers (dashed);
 *   fault    - the session died: the cliff is an emptied shell over the measured baseline.
 */
export type SceneMode = 'live' | 'building' | 'fit' | 'fault';

export interface PaintOpts {
  mode: SceneMode;
  warn: boolean;
  /** fit: how many of the bottom bands are measured (drawn solid) rather than expected. */
  nBase: number;
  /** fit: expected GiB beyond the edge (drawn above the lip, so every band keeps its true height). */
  overGiB?: number;
  /**
   * live, GPU dormant: the bands are the session's ALLOCATIONS and are drawn as ghosts (the rock is not in
   * VRAM right now); only the bottom `solidGiB` (what is resident) is solid. `ramK` 0..1: how much of the
   * session the sea (system RAM) holds right now, which lights it a little.
   */
  dormant?: { solidGiB: number; ramK: number } | null;
}

/** Headroom tall enough to read as open air: the lip becomes a dashed capacity line, the glow moves
 *  to the surface of the rock, and the free amount gets its own dimension arrow. */
export function tallHeadroom(g: Geom): boolean {
  return g.tall;
}

function headland(
  ctx: CanvasRenderingContext2D,
  x0: number,
  x1: number,
  baseY: number,
  hmax: number,
  seed: number,
  fill: string,
  edge: string,
) {
  ctx.beginPath();
  ctx.moveTo(x0, baseY);
  for (let x = x0; x <= x1 + 4; x += 4) {
    const t = (x - x0) / Math.max(1, x1 - x0);
    const env = Math.min(1, t * 3.5) * Math.min(1, (1 - t) * 1.5 + 0.45);
    const n = 0.55 + 0.95 * (noise1(x / 70, seed) - 0.5) + 0.3 * (noise1(x / 16, seed + 1) - 0.5);
    ctx.lineTo(x, baseY - Math.max(0, hmax * env * n));
  }
  ctx.lineTo(x1 + 4, baseY);
  ctx.closePath();
  ctx.fillStyle = fill;
  ctx.fill();
  ctx.strokeStyle = edge;
  ctx.lineWidth = 1;
  ctx.stroke();
}

function sea(ctx: CanvasRenderingContext2D, g: Geom) {
  const { W, waterY, baseY, seaX } = g;
  const depth = baseY - waterY;
  if (depth <= 2) return;
  const grad = ctx.createLinearGradient(0, waterY, 0, baseY);
  grad.addColorStop(0, '#0f2b3e');
  grad.addColorStop(0.45, '#0c2333');
  grad.addColorStop(1, '#0a1924');
  ctx.fillStyle = grad;
  ctx.fillRect(seaX, waterY, W - seaX, depth);

  // Fine horizontal wave lines, denser towards the horizon (static scenery).
  const n = g.variant === 'full' ? 38 : 30;
  ctx.lineWidth = 1;
  for (let i = 0; i < n; i++) {
    const f = (i + 0.6) / n;
    const y0 = waterY + 2 + (depth - 4) * Math.pow(f, 1.45);
    const a = 0.34 * (1 - f) + 0.09;
    ctx.strokeStyle = `rgba(90,182,235,${a.toFixed(3)})`;
    ctx.beginPath();
    let first = true;
    for (let x = seaX; x <= W + 4; x += 4) {
      const y =
        y0 +
        Math.sin(x * (0.03 + (i % 7) * 0.004) + i * 1.7) * (0.5 + f * 1.6) +
        (noise1(x / 46 + i * 3.1, 5) - 0.5) * (1.5 + f * 3);
      if (first) {
        ctx.moveTo(x, y);
        first = false;
      } else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }
  // Horizon.
  ctx.fillStyle = 'rgba(90,182,235,0.55)';
  ctx.fillRect(seaX, waterY, W - seaX, 1);
}

function sectionPath(g: Geom): Path2D {
  const p = new Path2D();
  p.moveTo(0, g.lipY);
  p.lineTo(g.lipX, g.lipY);
  for (let y = Math.ceil(g.lipY); y <= g.baseY; y += 2) p.lineTo(g.inner[y], y);
  p.lineTo(g.inner[g.baseY], g.baseY);
  p.lineTo(0, g.baseY);
  p.closePath();
  return p;
}

function facePath(g: Geom, y0: number): Path2D {
  const p = new Path2D();
  const ys = Math.max(Math.ceil(g.lipY), Math.floor(y0));
  p.moveTo(g.inner[ys], ys);
  for (let y = ys; y <= g.baseY; y += 2) p.lineTo(g.outer[y], y);
  p.lineTo(g.outer[g.baseY], g.baseY);
  for (let y = g.baseY; y >= ys; y -= 2) p.lineTo(g.inner[y], y);
  p.closePath();
  return p;
}

/** A stratum boundary: a dark parting with a lit edge and a soft shadow under the ledge. The strokes
 *  wobble by < 1.3 px around the exact boundary; the band fills themselves are exact. (No hanging
 *  crack marks here: at regular spacing they read as scale ticks.) */
function seam(ctx: CanvasRenderingContext2D, y: number, x0: number, x1: number, light: number, heavy = false) {
  const seed = Math.round(y * 7);
  const sh = heavy ? 7 : 5;
  const grad = ctx.createLinearGradient(0, y, 0, y + sh);
  grad.addColorStop(0, 'rgba(3,6,8,0.32)');
  grad.addColorStop(1, 'rgba(3,6,8,0)');
  ctx.fillStyle = grad;
  ctx.fillRect(x0, y, x1 - x0, sh);
  ctx.lineWidth = heavy ? 2 : 1.4;
  ctx.strokeStyle = 'rgba(3,6,8,0.5)';
  ctx.beginPath();
  for (let x = x0; x <= x1 + 6; x += 6) {
    const yy = y + 0.6 + (noise1(x / 22, seed) - 0.5) * 2.6;
    if (x === x0) ctx.moveTo(x, yy);
    else ctx.lineTo(x, yy);
  }
  ctx.stroke();
  ctx.lineWidth = 1;
  ctx.strokeStyle = `rgba(220,239,248,${light})`;
  ctx.beginPath();
  for (let x = x0; x <= x1 + 6; x += 6) {
    const yy = y - 0.9 + (noise1(x / 22, seed) - 0.5) * 2.6;
    if (x === x0) ctx.moveTo(x, yy);
    else ctx.lineTo(x, yy);
  }
  ctx.stroke();
}

/** The cliff face as a few broad facets (ribs) that fan out towards the foot, each its own tone,
 *  parted by a lit edge and a shadow line, with sparse long cracks (scenery, deterministic). */
function faceFacets(ctx: CanvasRenderingContext2D, g: Geom, y0: number, dim: number) {
  const ghost = dim < 1;
  const ys0 = Math.max(Math.ceil(g.lipY), Math.floor(y0));
  if (ys0 >= g.baseY) return;
  const full = g.variant === 'full';
  const N = full ? 6 : 5;
  const frac = (i: number, y: number) => {
    if (i <= 0) return 0;
    if (i >= N) return 1;
    const base = i / N + (hash2(i, 0, 611) - 0.5) * (0.45 / N);
    const wob = (noise1(y / 34 + i * 2.3, 613) - 0.5) * (0.9 / N);
    return Math.max(0.03, Math.min(0.97, base + wob));
  };
  const xAt = (i: number, y: number) => g.inner[y] + (g.outer[y] - g.inner[y]) * frac(i, y);
  // Light comes from the upper left: the ribs nearest the sea turn away from it.
  const TONES = full
    ? ['#7c8991', '#95a2a9', '#7f8c94', '#6b7982', '#57656e', '#46545d']
    : ['#8a979e', '#a1adb3', '#7e8b93', '#65737c', '#4f5d66'];
  ctx.save();
  if (ghost) ctx.globalAlpha = dim;
  for (let i = 0; i < N; i++) {
    const p = new Path2D();
    p.moveTo(xAt(i, ys0), ys0);
    for (let y = ys0; y < g.baseY; y += 3) p.lineTo(xAt(i + 1, y), y);
    p.lineTo(xAt(i + 1, g.baseY), g.baseY);
    for (let y = g.baseY; y > ys0; y -= 3) p.lineTo(xAt(i, y), y);
    p.lineTo(xAt(i, ys0), ys0);
    p.closePath();
    ctx.fillStyle = TONES[i];
    ctx.fill(p);
  }
  ctx.restore();
  // Rib partings: lit edge with a shadow on its right, fading in and out along the drop.
  for (let i = 1; i < N; i++) {
    let y = ys0 + 4 + hash2(i, 1, 615) * 10;
    let seg = 0;
    while (y < g.baseY) {
      const len = 16 + hash2(i, seg, 617) * 52;
      const y1 = Math.min(g.baseY, y + len);
      const a = (0.14 + hash2(i, seg, 619) * 0.26) * (ghost ? 0.4 : 1);
      ctx.lineWidth = 1;
      ctx.strokeStyle = `rgba(232,242,247,${a.toFixed(3)})`;
      ctx.beginPath();
      for (let yy = Math.floor(y); yy <= y1; yy += 3) {
        const x = xAt(i, Math.min(g.baseY, yy));
        if (yy === Math.floor(y)) ctx.moveTo(x, yy);
        else ctx.lineTo(x, yy);
      }
      ctx.stroke();
      ctx.lineWidth = 1.6;
      ctx.strokeStyle = `rgba(4,8,10,${a.toFixed(3)})`;
      ctx.beginPath();
      for (let yy = Math.floor(y); yy <= y1; yy += 3) {
        const x = xAt(i, Math.min(g.baseY, yy)) + 1.4;
        if (yy === Math.floor(y)) ctx.moveTo(x, yy);
        else ctx.lineTo(x, yy);
      }
      ctx.stroke();
      y = y1 + 8 + hash2(i, seg, 621) * 26;
      seg++;
    }
  }
  // Sparse long cracks inside the ribs.
  ctx.lineWidth = 1;
  for (let k = 0; k < (full ? 26 : 18); k++) {
    const f = hash2(k, 1, 631);
    let y = ys0 + 6 + hash2(k, 2, 631) * (g.baseY - ys0) * 0.85;
    const len = 12 + hash2(k, 3, 631) * 40;
    const a = (0.16 + hash2(k, 4, 631) * 0.22) * (ghost ? 0.4 : 1);
    ctx.strokeStyle = `rgba(6,10,12,${a.toFixed(3)})`;
    ctx.beginPath();
    let drift = 0;
    for (let yy = Math.floor(y); yy <= Math.min(g.baseY, y + len); yy += 3) {
      drift += (hash2(k, yy, 633) - 0.5) * 1.2;
      const x = g.inner[yy] + (g.outer[yy] - g.inner[yy]) * f + drift;
      if (yy === Math.floor(y)) ctx.moveTo(x, yy);
      else ctx.lineTo(x, yy);
    }
    ctx.stroke();
  }
}

/** Scenery behind the cliff (ridge, haze, headlands, sea). Depends only on size and variant, so it
 *  is rendered once to an offscreen canvas and blitted on every repaint. */
let sceneryCache: { key: string; c: HTMLCanvasElement } | null = null;

function scenery(g: Geom, scale: number): HTMLCanvasElement {
  const key = `${g.W}x${g.H}@${scale}|${g.variant}|${g.lipY}`;
  if (sceneryCache && sceneryCache.key === key) return sceneryCache.c;
  const c = document.createElement('canvas');
  c.width = Math.round(g.W * scale);
  c.height = Math.round(g.H * scale);
  const ctx = c.getContext('2d')!;
  ctx.scale(scale, scale);
  const { W, lipX, lipY, baseY, waterY } = g;
  const span = baseY - lipY;

  // Distant ridge (mini only).
  if (g.variant === 'mini') {
    ctx.beginPath();
    ctx.moveTo(W * 0.52, 0);
    for (let x = W * 0.52; x <= W + 6; x += 6) {
      const t = (x - W * 0.52) / (W * 0.48);
      const y = (waterY - 16) * Math.pow(t, 1.55) + (noise1(x / 34, 31) - 0.5) * 34 * t;
      ctx.lineTo(x, Math.max(0, y));
    }
    ctx.lineTo(W + 6, 0);
    ctx.closePath();
    ctx.fillStyle = 'rgba(26,43,52,0.55)';
    ctx.fill();
    ctx.strokeStyle = 'rgba(130,170,190,0.12)';
    ctx.lineWidth = 1;
    ctx.stroke();
  }

  // Horizon haze above the sea.
  const hz = ctx.createLinearGradient(0, waterY - span * 0.25, 0, waterY);
  hz.addColorStop(0, 'rgba(90,182,235,0)');
  hz.addColorStop(1, 'rgba(90,182,235,0.07)');
  ctx.fillStyle = hz;
  ctx.fillRect(g.seaX, waterY - span * 0.25, W - g.seaX, span * 0.25);

  if (g.variant === 'full') {
    // Far headlands sit between the face and the loupe so the loupe does not hide them.
    headland(ctx, W * 0.735, W + 8, waterY, span * 0.06, 41, '#15222a', 'rgba(90,182,235,0.14)');
    headland(ctx, W * 0.7, W * 0.83, waterY, span * 0.1, 21, '#1b2a33', 'rgba(140,190,215,0.22)');
  } else {
    headland(ctx, lipX + W * 0.02, W * 0.88, waterY, span * 0.1, 23, '#18252d', 'rgba(130,180,205,0.16)');
    headland(ctx, W * 0.84, W + 8, waterY, span * 0.035, 43, '#131e25', 'rgba(90,182,235,0.10)');
  }
  sea(ctx, g);
  sceneryCache = { key, c };
  return c;
}

/** GPU dormant: the sea holds the model now, so it is lit a little (scenery sea, brightened in place). */
function seaGlow(ctx: CanvasRenderingContext2D, g: Geom, k: number) {
  const { W, waterY, baseY, seaX } = g;
  const depth = baseY - waterY;
  if (depth <= 2 || k <= 0) return;
  const grad = ctx.createLinearGradient(0, waterY, 0, baseY);
  grad.addColorStop(0, `rgba(96,190,240,${(0.105 * k).toFixed(3)})`);
  grad.addColorStop(1, `rgba(60,150,205,${(0.055 * k).toFixed(3)})`);
  ctx.fillStyle = grad;
  ctx.fillRect(seaX, waterY, W - seaX, depth);
  ctx.fillStyle = `rgba(150,214,245,${(0.4 * k).toFixed(3)})`;
  ctx.fillRect(seaX, waterY, W - seaX, 1);
}

/**
 * GPU dormant: the cross-section is a ghost of the session (same treatment as the idle fit preview:
 * the chart shows through, texture faded to ~25%, dashed outlines), and only the bottom `solidGiB` of it
 * (what is resident in VRAM) is solid rock. The solid part grows bottom-up while the GPU wakes.
 */
function dormantSection(ctx: CanvasRenderingContext2D, g: Geom, tex: HTMLCanvasElement, solidY: number, lipRgb: string) {
  const { W, H, lipY, baseY, rockY } = g;
  // The sea is scenery behind the cliff: keep it out of the ghosted section.
  ctx.save();
  ctx.globalCompositeOperation = 'destination-out';
  ctx.fillRect(g.seaX - 1, g.waterY - 1, W - g.seaX + 1, baseY - g.waterY + 1);
  ctx.restore();
  ctx.fillStyle = 'rgba(24,32,38,0.62)';
  ctx.fillRect(0, lipY, W, baseY - lipY);
  ctx.globalAlpha = 0.25;
  ctx.drawImage(tex, 0, 0, W, H);
  ctx.globalAlpha = 1;
  if (rockY - lipY > 0.05) {
    if (tallHeadroom(g)) {
      const air = ctx.createLinearGradient(0, lipY, 0, rockY);
      air.addColorStop(0, `rgba(${lipRgb},0.16)`);
      air.addColorStop(1, `rgba(${lipRgb},0.03)`);
      ctx.fillStyle = air;
    } else ctx.fillStyle = `rgba(${lipRgb},0.12)`;
    ctx.fillRect(0, lipY, W, rockY - lipY);
  }
  // Ghost strata: dashed outlines, only above what is resident.
  ctx.setLineDash([5, 4]);
  ctx.lineWidth = 1;
  for (const b of g.bands) {
    const bot = Math.min(b.yBot, solidY);
    if (bot <= b.yTop + 0.5) continue;
    ctx.fillStyle = 'rgba(220,239,248,0.05)';
    ctx.fillRect(0, b.yTop, W, bot - b.yTop);
    ctx.strokeStyle = 'rgba(220,239,248,0.55)';
    ctx.strokeRect(-2, Math.round(b.yTop) + 0.5, W + 4, Math.max(1, bot - Math.round(b.yTop)));
  }
  ctx.setLineDash([]);
  // Resident rock: solid strata, texture and seams from the foot up to the restore front.
  if (baseY - solidY >= 0.75) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, solidY, W, baseY - solidY);
    ctx.clip();
    for (const b of g.bands) {
      ctx.fillStyle = LAYER_COLOR[b.id] ?? C.s2;
      ctx.fillRect(0, b.yTop, W, b.yBot - b.yTop);
    }
    ctx.drawImage(tex, 0, 0, W, H);
    const sh = ctx.createLinearGradient(0, solidY, 0, baseY);
    sh.addColorStop(0, 'rgba(220,239,248,0.05)');
    sh.addColorStop(0.5, 'rgba(5,8,10,0)');
    sh.addColorStop(1, 'rgba(5,8,10,0.3)');
    ctx.fillStyle = sh;
    ctx.fillRect(0, solidY, W, baseY - solidY);
    for (const b of g.bands) {
      if (b.yTop <= solidY + 0.5) continue;
      seam(ctx, b.yTop, 0, W, 0.32, g.variant === 'mini');
    }
    ctx.restore();
    ctx.fillStyle = 'rgba(220,239,248,0.55)';
    ctx.fillRect(0, Math.round(solidY), W, 1);
  }
}

export function paintScene(
  ctx: CanvasRenderingContext2D,
  g: Geom,
  tex: HTMLCanvasElement,
  o: PaintOpts,
  scale = 1,
) {
  const { W, H, lipX, lipY, baseY, rockY } = g;
  const fit = o.mode === 'fit';
  const fault = o.mode === 'fault';
  const tall = tallHeadroom(g);
  const full = g.variant === 'full';
  /** GPU dormant (live only): ghost strata over a solid resident part of height solidGiB. */
  const dorm = !fit && !fault && !!o.dormant;
  const solidGiB = dorm ? Math.max(0, Math.min(g.stackTop, o.dormant!.solidGiB)) : 0;
  const solidY = baseY - solidGiB * g.ppg;
  ctx.clearRect(0, 0, W, H);
  const lipColor = o.warn ? C.amber : C.sky;
  const lipRgb = o.warn ? '242,163,58' : '90,182,235';
  ctx.drawImage(scenery(g, scale), 0, 0, W, H);
  if (dorm) seaGlow(ctx, g, o.dormant!.ramK);

  // ---- cross-section (data): strata clipped by the section silhouette.
  const sec = sectionPath(g);
  ctx.save();
  ctx.clip(sec);
  if (dorm) {
    dormantSection(ctx, g, tex, solidY, lipRgb);
  } else if (fit) {
    // Fit preview: the cliff is a ghost (the chart contours show through it, nothing is loaded);
    // measured baseline solid, expected layers as dashed outlines.
    // (the sea is scenery behind the cliff: keep it out of the ghosted section)
    ctx.save();
    ctx.globalCompositeOperation = 'destination-out';
    ctx.fillRect(g.seaX - 1, g.waterY - 1, W - g.seaX + 1, baseY - g.waterY + 1);
    ctx.restore();
    ctx.fillStyle = 'rgba(24,32,38,0.62)';
    ctx.fillRect(0, lipY, W, baseY - lipY);
    ctx.globalAlpha = 0.3;
    ctx.drawImage(tex, 0, 0, W, H);
    ctx.globalAlpha = 1;
    if (rockY - lipY > 0.05) {
      if (tall) {
        const air = ctx.createLinearGradient(0, lipY, 0, rockY);
        air.addColorStop(0, `rgba(${lipRgb},0.16)`);
        air.addColorStop(1, `rgba(${lipRgb},0.03)`);
        ctx.fillStyle = air;
      } else ctx.fillStyle = `rgba(${lipRgb},0.16)`;
      ctx.fillRect(0, lipY, W, rockY - lipY);
    }
    const base = g.bands.slice(0, o.nBase);
    for (const b of base) {
      ctx.fillStyle = LAYER_COLOR[b.id] ?? C.s2;
      ctx.fillRect(0, b.yTop, W, b.yBot - b.yTop);
    }
    if (base.length) {
      const top = base[base.length - 1].yTop;
      ctx.save();
      ctx.beginPath();
      ctx.rect(0, top, W, baseY - top);
      ctx.clip();
      ctx.drawImage(tex, 0, 0, W, H);
      ctx.restore();
      ctx.fillStyle = 'rgba(220,239,248,0.5)';
      ctx.fillRect(0, Math.round(top), W, 1);
    }
    ctx.setLineDash([5, 4]);
    ctx.lineWidth = 1;
    for (const b of g.bands.slice(o.nBase)) {
      ctx.fillStyle = 'rgba(220,239,248,0.05)';
      ctx.fillRect(0, b.yTop, W, b.yBot - b.yTop);
      ctx.strokeStyle = 'rgba(220,239,248,0.55)';
      ctx.strokeRect(-2, Math.round(b.yTop) + 0.5, W + 4, Math.max(1, b.yBot - b.yTop));
    }
    ctx.setLineDash([]);
  } else {
    const bandPx = rockY - lipY;
    if (bandPx > 0.05) {
      if (fault) {
        // Emptied: the shell of the cliff, dark and hollow, with the lamination still faintly there.
        const shell = ctx.createLinearGradient(0, lipY, 0, rockY);
        shell.addColorStop(0, 'rgba(30,38,43,0.95)');
        shell.addColorStop(1, 'rgba(20,26,30,0.95)');
        ctx.fillStyle = shell;
        ctx.fillRect(0, lipY, W, bandPx);
        ctx.save();
        ctx.beginPath();
        ctx.rect(0, lipY, W, bandPx);
        ctx.clip();
        ctx.globalAlpha = 0.55;
        ctx.drawImage(tex, 0, 0, W, H);
        ctx.restore();
      } else if (tall) {
        // Open air inside the section: the sea (scenery, behind the cliff) must not show through it.
        if (rockY > g.waterY) {
          ctx.save();
          ctx.globalCompositeOperation = 'destination-out';
          ctx.fillRect(g.seaX - 1, g.waterY - 1, W - g.seaX + 1, rockY - g.waterY + 1);
          ctx.restore();
        }
        // Open air under the capacity line: a faint tint near the lip only.
        const air = ctx.createLinearGradient(0, lipY, 0, Math.min(rockY, lipY + 110));
        air.addColorStop(0, `rgba(${lipRgb},0.12)`);
        air.addColorStop(1, `rgba(${lipRgb},0)`);
        ctx.fillStyle = air;
        ctx.fillRect(0, lipY, W, Math.min(bandPx, 110));
      } else if (bandPx > 24) {
        // A tall headroom reads as open air under the lip: brightest at the lip, fading to the rock.
        const air = ctx.createLinearGradient(0, lipY, 0, rockY);
        air.addColorStop(0, `rgba(${lipRgb},0.22)`);
        air.addColorStop(1, `rgba(${lipRgb},0.05)`);
        ctx.fillStyle = air;
        ctx.fillRect(0, lipY, W, bandPx);
      } else {
        ctx.fillStyle = freeFill(bandPx, o.warn);
        ctx.fillRect(0, lipY, W, bandPx);
      }
    }
    for (const b of g.bands) {
      ctx.fillStyle = LAYER_COLOR[b.id] ?? C.s2;
      ctx.fillRect(0, b.yTop, W, b.yBot - b.yTop);
    }
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, rockY, W, baseY - rockY);
    ctx.clip();
    ctx.drawImage(tex, 0, 0, W, H);
    const sh = ctx.createLinearGradient(0, rockY, 0, baseY);
    sh.addColorStop(0, 'rgba(220,239,248,0.05)');
    sh.addColorStop(0.5, 'rgba(5,8,10,0)');
    sh.addColorStop(1, 'rgba(5,8,10,0.3)');
    ctx.fillStyle = sh;
    ctx.fillRect(0, rockY, W, baseY - rockY);
    ctx.restore();
    for (const b of g.bands) {
      if (b.yTop <= rockY + 0.5) continue;
      seam(ctx, b.yTop, 0, W, 0.32, g.variant === 'mini');
    }
    if (!(tall || fault)) {
      ctx.fillStyle = 'rgba(220,239,248,0.55)';
      ctx.fillRect(0, Math.round(rockY), W, 1);
    }
  }
  ctx.restore();

  // Tall headroom while building or live: the empty part of the cliff keeps a faint face (scenery,
  // like the fit preview's), so the sea never meets an invisible wall where the rock has not reached.
  if (tall && !fit && !fault && rockY - lipY > 2) {
    const yEnd = Math.min(baseY, rockY + 2);
    ctx.save();
    ctx.clip(facePath(g, lipY));
    ctx.beginPath();
    ctx.rect(lipX - 30, lipY, W - lipX + 30, yEnd - lipY);
    ctx.clip();
    ctx.fillStyle = '#161d21';
    ctx.fillRect(lipX - 30, lipY, W - lipX + 30, yEnd - lipY);
    faceFacets(ctx, g, lipY, 0.3);
    ctx.restore();
    ctx.lineWidth = 1;
    ctx.strokeStyle = 'rgba(220,239,248,0.14)';
    ctx.beginPath();
    for (let y = Math.ceil(lipY); y <= yEnd; y += 2) {
      if (y === Math.ceil(lipY)) ctx.moveTo(g.outer[y], y);
      else ctx.lineTo(g.outer[y], y);
    }
    ctx.stroke();
  }

  // ---- the face (scenery outside the section; strata continue as ledges). After a fault the whole
  // emptied shell keeps its face, dimmed; otherwise the face only exists where there is rock.
  // GPU dormant: the whole face is a ghost (the fit preview's), with the real face again from the restore
  // front down (fromY), so the face and the section stay one rock.
  type FaceKind = 'live' | 'fit' | 'fault';
  const faceTop = fault ? lipY : rockY;
  const paintFace = (kind: FaceKind, fromY: number | null) => {
    const kFit = kind === 'fit';
    const kFault = kind === 'fault';
    const dim = kFit ? 0.32 : kFault ? 0.5 : 1;
    ctx.save();
    if (fromY !== null) {
      ctx.beginPath();
      ctx.rect(0, fromY, W, baseY - fromY);
      ctx.clip();
    }
    ctx.clip(facePath(g, faceTop));
    ctx.fillStyle = kFit ? '#161d21' : kFault ? '#1b2429' : '#2b3943';
    ctx.fillRect(lipX - 30, faceTop, W - lipX + 30, baseY - faceTop);
    faceFacets(ctx, g, faceTop, dim);
    ctx.save();
    ctx.globalAlpha = kFit ? 0.25 : kFault ? 0.3 : 0.42;
    ctx.drawImage(tex, 0, 0, W, H);
    ctx.restore();
    // Depth: lit under the lip, darker towards the foot.
    const fg = ctx.createLinearGradient(0, faceTop, 0, baseY);
    fg.addColorStop(0, kFault ? 'rgba(240,248,252,0.04)' : 'rgba(240,248,252,0.10)');
    fg.addColorStop(0.3, 'rgba(0,0,0,0)');
    fg.addColorStop(1, 'rgba(3,8,12,0.45)');
    ctx.fillStyle = fg;
    ctx.fillRect(lipX - 30, faceTop, W - lipX + 30, baseY - faceTop);
    // Ledges where the stepped face juts out (scenery).
    for (let y = Math.max(1, Math.ceil(faceTop) + 2); y <= baseY; y++) {
      const d = g.outer[y] - g.outer[y - 1];
      if (d < 2.5) continue;
      ctx.fillStyle = `rgba(225,238,245,${dim < 1 ? 0.07 : 0.2})`;
      ctx.fillRect(g.inner[y], y, g.outer[y] - g.inner[y], 1);
      ctx.fillStyle = 'rgba(0,0,0,0.25)';
      ctx.fillRect(g.inner[y], y - 2, g.outer[y - 1] - g.inner[y], 2);
    }
    if (!kFit) {
      for (const b of g.bands) {
        if (b.yTop <= rockY + 0.5) continue;
        const y = Math.round(b.yTop);
        const yi = Math.min(baseY, Math.max(0, y));
        ctx.fillStyle = 'rgba(220,239,248,0.22)';
        ctx.fillRect(g.inner[yi], y - 1, g.outer[yi] - g.inner[yi] + 2, 1);
        ctx.fillStyle = 'rgba(0,0,0,0.35)';
        ctx.fillRect(g.inner[yi], y, g.outer[yi] - g.inner[yi] + 2, 2);
      }
    }
    if (kFault) faultCracks(ctx, g);
    ctx.restore();
  };

  // Edges: lit section edge, darker outer silhouette.
  const edgeTop = Math.max(Math.ceil(lipY), Math.floor(faceTop));
  const paintEdges = (dimmed: boolean, fromY: number | null) => {
    ctx.save();
    if (fromY !== null) {
      ctx.beginPath();
      ctx.rect(0, fromY, W, baseY - fromY);
      ctx.clip();
    }
    ctx.lineWidth = 1;
    ctx.strokeStyle = dimmed ? 'rgba(220,239,248,0.18)' : 'rgba(220,239,248,0.35)';
    ctx.beginPath();
    for (let y = edgeTop; y <= baseY; y += 2) {
      if (y === edgeTop) ctx.moveTo(g.inner[y], y);
      else ctx.lineTo(g.inner[y], y);
    }
    ctx.stroke();
    // Outer silhouette: a lit rim near the lip that fades into shadow towards the foot.
    const rim = ctx.createLinearGradient(0, faceTop, 0, baseY);
    rim.addColorStop(0, dimmed ? 'rgba(220,239,248,0.16)' : 'rgba(225,238,245,0.5)');
    rim.addColorStop(0.55, 'rgba(225,238,245,0.12)');
    rim.addColorStop(1, 'rgba(0,0,0,0.4)');
    ctx.strokeStyle = rim;
    ctx.beginPath();
    for (let y = edgeTop; y <= baseY; y += 2) {
      if (y === edgeTop) ctx.moveTo(g.outer[y], y);
      else ctx.lineTo(g.outer[y], y);
    }
    ctx.stroke();
    ctx.restore();
  };

  // Fit preview / GPU dormant: the layer boundaries carry on across the face as dashed ledges.
  const paintLedges = (bands: Geom['bands'], belowY: number) => {
    ctx.save();
    ctx.setLineDash([4, 4]);
    ctx.lineWidth = 1;
    ctx.strokeStyle = 'rgba(220,239,248,0.4)';
    for (const b of bands) {
      const y = Math.round(b.yTop) + 0.5;
      const yi = Math.min(baseY, Math.max(0, Math.round(b.yTop)));
      if (y < rockY - 0.5 || y >= belowY - 0.5) continue;
      ctx.beginPath();
      ctx.moveTo(g.inner[yi], y);
      ctx.lineTo(g.outer[yi] + 2, y);
      ctx.stroke();
    }
    ctx.restore();
  };

  if (dorm) {
    paintFace('fit', null);
    paintLedges(g.bands, solidY);
    paintEdges(true, null);
    if (baseY - solidY >= 0.75) {
      paintFace('live', solidY);
      paintEdges(false, solidY);
      // The restore front carries across the face.
      const yf = Math.round(solidY);
      const yi = Math.min(baseY, Math.max(0, yf));
      ctx.fillStyle = 'rgba(220,239,248,0.4)';
      ctx.fillRect(g.inner[yi], yf, g.outer[yi] - g.inner[yi] + 2, 1);
    }
  } else {
    paintFace(fit ? 'fit' : fault ? 'fault' : 'live', null);
    if (fit) paintLedges(g.bands.slice(o.nBase), Infinity);
    paintEdges(fit || fault, null);
  }
  ctx.lineWidth = 1;

  // Headroom silhouette above the rock: the lip is fixed at total.
  if (!fault && rockY - lipY > 5) {
    ctx.save();
    ctx.setLineDash([3, 3]);
    ctx.strokeStyle = o.warn ? 'rgba(242,163,58,0.7)' : 'rgba(90,182,235,0.65)';
    ctx.beginPath();
    if (tall) {
      // The empty mould: the section edge traced from the lip down to the rock.
      ctx.moveTo(lipX + 0.5, lipY);
      for (let y = Math.ceil(lipY) + 2; y < rockY; y += 2) ctx.lineTo(g.inner[y] + 0.5, y);
      ctx.lineTo(g.inner[Math.round(rockY)] + 0.5, rockY);
    } else {
      ctx.moveTo(lipX + 0.5, lipY);
      ctx.lineTo(g.inner[Math.round(rockY)] + 0.5, rockY);
    }
    ctx.stroke();
    ctx.restore();
  }

  if (tall || fault || fit || dorm) {
    // The lip is a capacity line (dashed): nothing reaches it (fit: nothing is loaded yet; dormant:
    // the session is not in VRAM right now).
    ctx.save();
    ctx.setLineDash(full ? [6, 5] : [8, 6]);
    ctx.lineWidth = full ? 1.2 : 2;
    ctx.strokeStyle = `rgba(${lipRgb},0.7)`;
    ctx.beginPath();
    ctx.moveTo(0, lipY + 0.5);
    ctx.lineTo(lipX, lipY + 0.5);
    ctx.stroke();
    ctx.restore();
    // ...and the glow sits on the surface of the material (dormant: on the restore front, once some
    // of the rock is resident; a fully paged-out session has no surface to light).
    const glowY = dorm ? solidY : rockY;
    if (!fit && (!dorm || (baseY - solidY >= 1 && solidGiB < g.stackTop - 0.004))) {
      const yi = Math.min(baseY, Math.max(0, Math.round(glowY)));
      ctx.save();
      ctx.shadowColor = C.sky;
      ctx.shadowBlur = full ? 10 : 14;
      ctx.fillStyle = C.sky;
      const th = full ? 2 : 3;
      ctx.fillRect(0, glowY - th / 2, g.inner[yi] + 1, th);
      ctx.shadowBlur = 3;
      ctx.fillRect(0, glowY - th / 2, g.inner[yi] + 1, th);
      ctx.restore();
    }
  } else {
    // The lip: total VRAM. Static glow, painted once.
    ctx.save();
    ctx.shadowColor = lipColor;
    ctx.shadowBlur = full ? 12 : 16;
    ctx.fillStyle = lipColor;
    ctx.fillRect(0, lipY - 1, lipX + 1, full ? 2 : 3);
    ctx.shadowBlur = 4;
    ctx.fillRect(0, lipY - 1, lipX + 1, full ? 2 : 3);
    ctx.restore();
  }

  // Fit preview that does not fit: the expected material beyond the edge, at the same GiB scale.
  const over = fit ? (o.overGiB ?? 0) : 0;
  if (over > 0.004) {
    const hpx = Math.max(2, over * g.ppg);
    const y0 = Math.max(0, lipY - hpx);
    ctx.save();
    ctx.fillStyle = 'rgba(242,163,58,0.16)';
    ctx.fillRect(0, y0, lipX, lipY - y0);
    ctx.setLineDash([5, 4]);
    ctx.lineWidth = 1;
    ctx.strokeStyle = 'rgba(242,163,58,0.8)';
    ctx.strokeRect(-2, Math.round(y0) + 0.5, lipX + 2, Math.max(1, lipY - Math.round(y0) - 1));
    ctx.restore();
  }

  if (g.loupe && !fit && !fault) paintLoupe(ctx, g, tex, o);
}

/** Fault: hot fissures down the emptied face, along the rib partings (scenery, deterministic). */
function faultCracks(ctx: CanvasRenderingContext2D, g: Geom) {
  const y0 = Math.ceil(g.lipY) + 2;
  const span = g.baseY - y0;
  const n = g.variant === 'full' ? 5 : 4;
  ctx.save();
  ctx.lineJoin = 'round';
  for (let k = 0; k < n; k++) {
    const f = 0.1 + hash2(k, 1, 701) * 0.75;
    let y = y0 + hash2(k, 2, 701) * span * 0.6;
    const len = span * (0.1 + hash2(k, 3, 701) * 0.22);
    const a = 0.4 + hash2(k, 4, 701) * 0.35;
    ctx.strokeStyle = `rgba(242,163,58,${a.toFixed(3)})`;
    ctx.lineWidth = g.variant === 'full' ? 1.1 : 1.6;
    ctx.beginPath();
    let first = true;
    let drift = 0;
    const end = Math.min(g.baseY - 2, y + len);
    for (; y <= end; y += 3) {
      const yi = Math.round(y);
      drift += (hash2(k, yi, 703) - 0.5) * 2.2;
      drift *= 0.85;
      const x = g.inner[yi] + (g.outer[yi] - g.inner[yi]) * f + drift;
      if (first) {
        ctx.moveTo(x, y);
        first = false;
      } else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }
  ctx.restore();
}

// ---------------------------------------------------------------- fault debris (short, then static)

export interface Rock {
  /** Where it breaks off (on the face). */
  y0: number;
  /** Outward throw at the end of its path, px. */
  off: number;
  /** Path progress it freezes at, 0..1 (1 = at the foot). */
  pEnd: number;
  delay: number;
  rot: number;
  size: number;
  /** Polygon around the origin, unit radius. */
  verts: Pt[];
  tone: number;
}

export const DEBRIS_S = 2.4;

export function debrisRocks(g: Geom, n: number): Rock[] {
  const full = g.variant === 'full';
  const span = g.baseY - g.lipY;
  const out: Rock[] = [];
  for (let i = 0; i < n; i++) {
    const h = (j: number) => hash2(i, j, 811);
    const nv = 5 + Math.floor(h(1) * 3);
    const verts: Pt[] = [];
    for (let v = 0; v < nv; v++) {
      const a = (v / nv) * Math.PI * 2 + (hash2(i, v, 813) - 0.5) * 0.7;
      const r = 0.6 + hash2(i, v, 815) * 0.45;
      verts.push([Math.cos(a) * r, Math.sin(a) * r * 1.15]);
    }
    const big = h(2) < 0.25;
    out.push({
      y0: g.lipY + span * (0.01 + Math.pow(h(3), 1.8) * 0.3),
      off: (full ? 10 : 8) + Math.pow(h(4), 0.8) * g.W * (full ? 0.07 : 0.055),
      pEnd: i % 4 === 3 ? 1 : 0.08 + Math.pow(h(5), 1.25) * 0.72,
      delay: h(6) * 0.6,
      rot: (h(7) - 0.5) * 7,
      size: full ? 4.5 + (big ? 6 + h(8) * 5 : h(8) * 4) : (5 + (big ? 5 + h(8) * 4 : h(8) * 3.6)) * 1.3,
      verts,
      tone: h(9),
    });
  }
  return out;
}

/** k: 0..1 over DEBRIS_S seconds; 1 is the static picture that stays. */
export function paintDebris(ctx: CanvasRenderingContext2D, g: Geom, rocks: Rock[], k: number) {
  ctx.clearRect(0, 0, g.W, g.H);
  const t = k * DEBRIS_S;
  const footY = g.baseY - 3;
  for (const r of rocks) {
    const u = Math.max(0, Math.min(1, (t - r.delay) / (DEBRIS_S - 0.6)));
    // Falling accelerates; every rock freezes where its own path ends.
    const s = r.pEnd * Math.pow(u, 1.7);
    const y = r.y0 + (footY - r.y0) * s;
    const yi = Math.min(g.baseY, Math.max(0, Math.round(y)));
    const x = g.outer[yi] + 2 + r.off * (0.2 + 0.8 * Math.sqrt(s)) - (s < 0.02 ? 3 : 0);
    if (u <= 0 && r.delay > 0) {
      // Not yet loose: sits in the face.
      continue;
    }
    const inSea = x > g.seaX && y > g.waterY;
    if (inSea && s > 0.6) {
      ctx.strokeStyle = 'rgba(220,239,248,0.35)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.ellipse(x, Math.max(g.waterY, y) + r.size * 0.8, r.size * 1.8, r.size * 0.45, 0, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(r.rot * s);
    ctx.beginPath();
    r.verts.forEach(([vx, vy], i) => (i ? ctx.lineTo(vx * r.size, vy * r.size) : ctx.moveTo(vx * r.size, vy * r.size)));
    ctx.closePath();
    const l = Math.round(118 + r.tone * 60);
    ctx.fillStyle = `rgb(${l},${l + 10},${l + 16})`;
    ctx.fill();
    ctx.lineWidth = 1;
    ctx.strokeStyle = 'rgba(8,12,14,0.75)';
    ctx.stroke();
    // A lit facet (upper left) so the pieces read as broken rock.
    ctx.beginPath();
    ctx.moveTo(r.verts[0][0] * r.size * 0.2, r.verts[0][1] * r.size * 0.2);
    const a = r.verts[Math.floor(r.verts.length / 2)];
    const b = r.verts[Math.floor(r.verts.length / 2) + 1] ?? r.verts[0];
    ctx.lineTo(a[0] * r.size, a[1] * r.size);
    ctx.lineTo(b[0] * r.size, b[1] * r.size);
    ctx.closePath();
    ctx.fillStyle = 'rgba(235,244,248,0.28)';
    ctx.fill();
    ctx.restore();
  }
}

function paintLoupe(ctx: CanvasRenderingContext2D, g: Geom, tex: HTMLCanvasElement, o: PaintOpts) {
  const L = g.loupe!;
  const yL = (gib: number) => L.y + (L.hi - gib) * L.k;
  ctx.save();
  ctx.beginPath();
  ctx.roundRect(L.x, L.y, L.w, L.h, 5);
  ctx.clip();
  ctx.fillStyle = '#0c1114';
  ctx.fillRect(L.x, L.y, L.w, L.h);
  const top = Math.max(g.stackTop, L.lo);
  // GPU dormant: the bands are ghosts above what is resident (solid up to `solid` GiB, bottom-up).
  const dorm = !!o.dormant;
  const solid = dorm ? o.dormant!.solidGiB : Infinity;
  if (top < L.hi) {
    if (dorm) {
      ctx.fillStyle = `rgba(${o.warn ? '242,163,58' : '90,182,235'},0.12)`;
    } else ctx.fillStyle = freeFill(g.rockY - g.lipY, o.warn);
    ctx.fillRect(L.x, yL(L.hi), L.w, yL(top) - yL(L.hi));
  }
  if (dorm) {
    ctx.save();
    ctx.setLineDash([4, 3]);
    ctx.lineWidth = 1;
    for (const b of g.bands) {
      const hi = Math.min(b.hi, L.hi);
      const lo = Math.max(b.lo, L.lo, Math.min(solid, b.hi));
      if (hi <= lo + 1e-6) continue;
      ctx.fillStyle = 'rgba(220,239,248,0.05)';
      ctx.fillRect(L.x, yL(hi), L.w, yL(lo) - yL(hi));
      ctx.strokeStyle = 'rgba(220,239,248,0.5)';
      ctx.strokeRect(L.x - 2, Math.round(yL(hi)) + 0.5, L.w + 4, Math.max(1, yL(lo) - Math.round(yL(hi))));
    }
    ctx.restore();
  }
  // Solid part of the loupe window: everything when awake, the resident bottom when dormant.
  const rockTop = dorm ? yL(Math.min(top, Math.max(L.lo, solid))) : yL(top);
  for (const b of g.bands) {
    const hi = Math.min(b.hi, L.hi, solid);
    const lo = Math.max(b.lo, L.lo);
    if (hi <= lo) continue;
    ctx.fillStyle = LAYER_COLOR[b.id] ?? C.s2;
    ctx.fillRect(L.x, yL(hi), L.w, yL(lo) - yL(hi));
  }
  if (rockTop < L.y + L.h && !(dorm && solid <= L.lo)) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(L.x, rockTop, L.w, L.y + L.h - rockTop);
    ctx.clip();
    ctx.globalAlpha = 0.8;
    ctx.drawImage(tex, 0, 0, Math.min(tex.width, L.w * 3), Math.min(tex.height, L.h * 3), L.x, L.y, L.w * 2, L.h * 2);
    ctx.restore();
    for (const b of g.bands) {
      const y = yL(b.hi);
      if (b.hi >= L.hi || b.hi <= L.lo || y <= rockTop + 0.5) continue;
      seam(ctx, y, L.x, L.x + L.w, 0.3);
    }
    ctx.fillStyle = 'rgba(220,239,248,0.6)';
    ctx.fillRect(L.x, Math.round(rockTop), L.w, 1);
  }
  ctx.fillStyle = o.warn ? C.amber : C.sky;
  ctx.fillRect(L.x, L.y, L.w, 2);
  ctx.restore();
}

// ---------------------------------------------------------------- spill stream (animated)

/** Stream width uses the same GiB scale as the vertical axis: 1 GiB of spill = ppg px wide. */
export function spillWidth(g: Geom, spillMiB: number): number {
  return Math.max(1.5, Math.min(g.W * 0.06, (spillMiB / 1024) * g.ppg));
}

/** Box (scene CSS px) that holds the stream, its plume and the ripples, so the animated canvas is
 *  only as large as what moves. */
export function spillBox(g: Geom, spillMiB: number): { x: number; y: number; w: number; h: number } {
  const pts = bezierPoints(g.spill, 32);
  const pad = spillWidth(g, spillMiB) * 1.6 + 34;
  let x0 = Infinity;
  let x1 = -Infinity;
  for (const p of pts) {
    x0 = Math.min(x0, p[0]);
    x1 = Math.max(x1, p[0]);
  }
  const x = Math.max(0, Math.floor(x0 - pad));
  const y = Math.max(0, Math.floor(g.lipY - pad * 0.5));
  return { x, y, w: Math.min(g.W, Math.ceil(x1 + pad)) - x, h: g.H - y };
}

export function paintSpill(ctx: CanvasRenderingContext2D, g: Geom, spillMiB: number, t: number) {
  ctx.clearRect(0, 0, g.W, g.H);
  if (!(spillMiB > 0)) return;
  const pts: Pt[] = bezierPoints(g.spill, 64);
  const n = pts.length - 1;
  const w = spillWidth(g, spillMiB);
  const wet = pts.findIndex((p) => p[1] >= g.waterY);
  const cut = wet > 0 ? wet : n;
  const splash = pts[cut];
  // Unit normals along the channel, so the stream can carry parallel strands.
  const nx: number[] = [];
  const ny: number[] = [];
  for (let i = 0; i <= n; i++) {
    const a = pts[Math.max(0, i - 1)];
    const b = pts[Math.min(n, i + 1)];
    const dx = b[0] - a[0];
    const dy = b[1] - a[1];
    const l = Math.hypot(dx, dy) || 1;
    nx.push(-dy / l);
    ny.push(dx / l);
  }
  /** The stream fans out a little as it falls. */
  const spread = (i: number) => 0.75 + 0.55 * (i / n);
  const trace = (from: number, to: number, off: number) => {
    ctx.beginPath();
    for (let i = from; i <= to; i++) {
      const k = off * spread(i);
      const x = pts[i][0] + nx[i] * k;
      const y = pts[i][1] + ny[i] * k;
      if (i === from) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  };
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  // Haze around the whole fall.
  ctx.strokeStyle = 'rgba(90,182,235,0.10)';
  ctx.lineWidth = w * 2.4 + 4;
  trace(0, n, 0);
  // Body: water falling through air, then a dimmer plume where it runs into the sea.
  const body = ctx.createLinearGradient(0, pts[0][1], 0, splash[1]);
  body.addColorStop(0, 'rgba(206,236,250,0.62)');
  body.addColorStop(1, 'rgba(140,205,238,0.42)');
  ctx.strokeStyle = body;
  ctx.lineWidth = w;
  trace(0, cut, 0);
  if (cut < n) {
    ctx.strokeStyle = 'rgba(90,182,235,0.2)';
    ctx.lineWidth = w * 1.7;
    trace(cut, n, 0);
  }
  // Flowing strands: dashed lines whose dash offset runs with time (no per-particle state).
  const strands = w >= 9 ? [-0.3, 0, 0.3] : w >= 4 ? [-0.18, 0.18] : [0];
  const v = 70 + Math.min(60, w * 3);
  ctx.lineWidth = Math.max(1, w * 0.13);
  strands.forEach((o, j) => {
    ctx.setLineDash([5 + w * 0.7, 7 + w * 0.5]);
    ctx.lineDashOffset = -((t / 1000) * v + j * 11);
    ctx.strokeStyle = 'rgba(242,251,255,0.7)';
    trace(0, cut, o * w);
    if (cut < n) {
      ctx.lineDashOffset = -((t / 1000) * v * 0.45 + j * 7);
      ctx.strokeStyle = 'rgba(220,239,248,0.22)';
      trace(cut, n, o * w * 1.5);
    }
  });
  ctx.setLineDash([]);
  // Foam where it breaks over the lip.
  ctx.fillStyle = 'rgba(236,248,253,0.55)';
  ctx.beginPath();
  ctx.ellipse(pts[1][0], pts[1][1], w * 0.55 + 2, w * 0.3 + 1.2, 0, 0, Math.PI * 2);
  ctx.fill();
  // Mist and ripples where it hits the sea.
  const mr = w * 1.6 + 12;
  const mist = ctx.createRadialGradient(splash[0], splash[1], 0, splash[0], splash[1], mr);
  mist.addColorStop(0, 'rgba(220,239,248,0.32)');
  mist.addColorStop(1, 'rgba(220,239,248,0)');
  ctx.fillStyle = mist;
  ctx.beginPath();
  ctx.ellipse(splash[0], splash[1], mr, mr * 0.55, 0, 0, Math.PI * 2);
  ctx.fill();
  for (let r = 0; r < 3; r++) {
    const p = (t / 1400 + r / 3) % 1;
    const rad = w * 0.8 + 4 + p * (22 + w);
    ctx.strokeStyle = `rgba(220,239,248,${(0.5 * (1 - p)).toFixed(3)})`;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.ellipse(splash[0], splash[1] + 1, rad, rad * 0.25, 0, 0, Math.PI * 2);
    ctx.stroke();
  }
}

// ---------------------------------------------------------------- background contours

/** Topographic contour lines of a smooth field (marching squares), drawn once. */
export function contourCanvas(w: number, h: number, scale: number): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.round(w * scale));
  c.height = Math.max(1, Math.round(h * scale));
  const ctx = c.getContext('2d')!;
  ctx.scale(scale, scale);
  const step = 7;
  const nx = Math.ceil(w / step) + 1;
  const ny = Math.ceil(h / step) + 1;
  const f = new Float32Array(nx * ny);
  let mn = Infinity;
  let mx = -Infinity;
  for (let j = 0; j < ny; j++) {
    for (let i = 0; i < nx; i++) {
      const x = i * step;
      const y = j * step;
      const v = fbm(x / 340, y / 300, 4, 41) + 0.35 * fbm(x / 120, y / 120, 2, 77) + (x - y) * 0.00012;
      f[j * nx + i] = v;
      if (v < mn) mn = v;
      if (v > mx) mx = v;
    }
  }
  const levels = 26;
  for (let L = 0; L < levels; L++) {
    const v = mn + ((mx - mn) * (L + 0.5)) / levels;
    const major = L % 5 === 2;
    ctx.strokeStyle = major ? 'rgba(150,190,210,0.13)' : 'rgba(150,190,210,0.07)';
    ctx.lineWidth = major ? 1.1 : 0.8;
    ctx.beginPath();
    for (let j = 0; j < ny - 1; j++) {
      for (let i = 0; i < nx - 1; i++) {
        const a = f[j * nx + i];
        const b = f[j * nx + i + 1];
        const cc = f[(j + 1) * nx + i + 1];
        const d = f[(j + 1) * nx + i];
        const idx = (a > v ? 8 : 0) | (b > v ? 4 : 0) | (cc > v ? 2 : 0) | (d > v ? 1 : 0);
        if (idx === 0 || idx === 15) continue;
        const x0 = i * step;
        const y0 = j * step;
        const top = (): Pt => [x0 + ((v - a) / (b - a)) * step, y0];
        const right = (): Pt => [x0 + step, y0 + ((v - b) / (cc - b)) * step];
        const bottom = (): Pt => [x0 + ((v - d) / (cc - d)) * step, y0 + step];
        const left = (): Pt => [x0, y0 + ((v - a) / (d - a)) * step];
        const seg = (p: Pt, q: Pt) => {
          ctx.moveTo(p[0], p[1]);
          ctx.lineTo(q[0], q[1]);
        };
        switch (idx) {
          case 1:
          case 14:
            seg(left(), bottom());
            break;
          case 2:
          case 13:
            seg(bottom(), right());
            break;
          case 3:
          case 12:
            seg(left(), right());
            break;
          case 4:
          case 11:
            seg(top(), right());
            break;
          case 5:
            seg(top(), right());
            seg(left(), bottom());
            break;
          case 6:
          case 9:
            seg(top(), bottom());
            break;
          case 7:
          case 8:
            seg(top(), left());
            break;
          case 10:
            seg(top(), left());
            seg(right(), bottom());
            break;
        }
      }
    }
    ctx.stroke();
  }
  return c;
}
