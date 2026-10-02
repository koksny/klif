// Floor plan of the RX 9070 XT die, drawn on canvas.
//
// The linework is plainly background (an engineering drawing). Only these encode data:
//   - 64 compute-unit tiles (4 shader engines x 16 CUs): the token stream, 1 tile = 1 token
//     (prefill and image jobs fill them by progress instead; the full drawing's caption says which).
//     The small cells inside a tile are texture: a tile always lights as a whole.
//   - 8 GDDR6 blocks: a segmented VRAM capacity gauge, each block = totalGiB / 8, filled in order
//     up to usedGiB.
//   - full only: the central cache block, prompt-cache reuse of the last request
//     (cachedTokens / promptTokens), labelled. In the mini it cannot carry a >= 30 px label, so there
//     it is drawn as plain background.
//
// The static layer (everything that never changes) is rendered once to an offscreen canvas and
// re-used; the dynamic layer is a few dozen fillRects per frame from precomputed rectangles.
import { C, FONT_DATA } from './palette';
import { CU_PER_SE, SE_COUNT } from './stream';

export type Variant = 'full' | 'mini';

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface MemBlock extends Rect {
  vertical: boolean;
}

type Pt = [number, number];

export interface DieGeom {
  variant: Variant;
  W: number;
  H: number;
  /** Top of the visible viewport (virtual units); the HTML title sits above the die. */
  vy: number;
  die: Rect;
  /** In fill order. */
  mem: MemBlock[];
  se: Rect[];
  /** Outline of each shader engine (notched around the cache in the mini), and its inner line. */
  seShape: Pt[][];
  seInner: Pt[][];
  /** 64 tiles, index = se * 16 + order slot (slot 0 is nearest the die centre). */
  tiles: Rect[];
  /** Texture cells of each tile. */
  cells: Rect[][];
  cache: Rect;
  centre: { x: number; y: number };
  /** A tile used as the anchor of the "compute units" / hero leader. */
  leaderTile: number;
}

interface GridSpec {
  pad: [number, number, number, number];
  cols: number;
  rows: number;
  gx: number;
  gy: number;
  /** Extra gap after this row (shader-array split), and its size. */
  saAfter?: number;
  saGap?: number;
  skip?: (c: number, r: number) => boolean;
}

interface PhysTile extends Rect {
  c: number;
  r: number;
}

function gridTiles(se: Rect, g: GridSpec): PhysTile[] {
  const [pl, pt, pr, pb] = g.pad;
  const aw = se.w - pl - pr;
  const ah = se.h - pt - pb;
  const sa = g.saGap ?? 0;
  const tw = (aw - (g.cols - 1) * g.gx) / g.cols;
  const th = (ah - (g.rows - 1) * g.gy - sa) / g.rows;
  const out: PhysTile[] = [];
  for (let r = 0; r < g.rows; r++) {
    for (let c = 0; c < g.cols; c++) {
      if (g.skip?.(c, r)) continue;
      out.push({
        x: se.x + pl + c * (tw + g.gx),
        y: se.y + pt + r * (th + g.gy) + (g.saAfter !== undefined && r > g.saAfter ? sa : 0),
        w: tw,
        h: th,
        c,
        r,
      });
    }
  }
  return out;
}

/** Order tiles outward from the die centre, so the token stream flows out of the cache. */
function byCentre<T extends Rect>(tiles: T[], cx: number, cy: number): T[] {
  return tiles
    .map((t) => {
      const dx = t.x + t.w / 2 - cx;
      const dy = t.y + t.h / 2 - cy;
      return { t, d: Math.round(Math.hypot(dx, dy) * 100), a: Math.atan2(dy, dx) };
    })
    .sort((p, q) => p.d - q.d || p.a - q.a)
    .map((p) => p.t);
}

function cellsOf(t: Rect, nx: number, ny: number, inset: number, gap: number): Rect[] {
  const iw = t.w - inset * 2;
  const ih = t.h - inset * 2;
  const cw = (iw - (nx - 1) * gap) / nx;
  const ch = (ih - (ny - 1) * gap) / ny;
  const out: Rect[] = [];
  for (let j = 0; j < ny; j++) for (let i = 0; i < nx; i++) out.push({ x: t.x + inset + i * (cw + gap), y: t.y + inset + j * (ch + gap), w: cw, h: ch });
  return out;
}

function chamferPts(r: Rect, c: number): Pt[] {
  const { x, y, w, h } = r;
  return [
    [x + c, y],
    [x + w - c, y],
    [x + w, y + c],
    [x + w, y + h - c],
    [x + w - c, y + h],
    [x + c, y + h],
    [x, y + h - c],
    [x, y + c],
  ];
}

function buildFull(): DieGeom {
  const W = 800;
  const H = 640;
  const die = { x: 70, y: 47, w: 665, h: 563 };
  const xs = [95, 251, 407, 563];
  const mem: MemBlock[] = [
    ...xs.map((x) => ({ x, y: 57, w: 146, h: 44, vertical: false })),
    ...xs.map((x) => ({ x, y: 556, w: 146, h: 44, vertical: false })),
  ];
  const sw = 257;
  const sh = 188;
  const se: Rect[] = [
    { x: 98, y: 111, w: sw, h: sh },
    { x: 450, y: 111, w: sw, h: sh },
    { x: 98, y: 357, w: sw, h: sh },
    { x: 450, y: 357, w: sw, h: sh },
  ];
  const cache = { x: 363.5, y: 248, w: 78, h: 160 };
  const centre = { x: cache.x + cache.w / 2, y: cache.y + cache.h / 2 };
  const spec: GridSpec = { pad: [20, 38, 20, 12], cols: 4, rows: 4, gx: 6, gy: 5, saAfter: 1, saGap: 5 };
  const tiles: Rect[] = [];
  let leaderTile = 0;
  se.forEach((s, i) => {
    const phys = byCentre(gridTiles(s, spec), centre.x, centre.y);
    phys.forEach((t, k) => {
      if (i === 0 && t.c === 0 && t.r === 1) leaderTile = i * CU_PER_SE + k;
    });
    tiles.push(...phys.map(({ x, y, w, h }) => ({ x, y, w, h })));
  });
  // 3 x 2 cells per CU tile: 12 x 8 squares per engine, the density of the drawing.
  const cells = tiles.map((t) => cellsOf(t, 3, 2, 2, 3.4));
  return {
    variant: 'full',
    W,
    H,
    vy: 4,
    die,
    mem,
    se,
    seShape: se.map((s) => chamferPts(s, 10)),
    seInner: se.map((s) => chamferPts({ x: s.x + 5, y: s.y + 5, w: s.w - 10, h: s.h - 10 }, 7)),
    tiles,
    cells,
    cache,
    centre,
    leaderTile,
  };
}

function buildMini(): DieGeom {
  const W = 700;
  const H = 620;
  const die = { x: 4, y: 4, w: 692, h: 612 };
  const mem: MemBlock[] = [
    { x: 104, y: 14, w: 214, h: 46, vertical: false },
    { x: 382, y: 14, w: 214, h: 46, vertical: false },
    { x: 12, y: 84, w: 46, h: 176, vertical: true },
    { x: 642, y: 84, w: 46, h: 176, vertical: true },
    { x: 12, y: 360, w: 46, h: 176, vertical: true },
    { x: 642, y: 360, w: 46, h: 176, vertical: true },
    { x: 104, y: 560, w: 214, h: 46, vertical: false },
    { x: 382, y: 560, w: 214, h: 46, vertical: false },
  ];
  const centre = { x: 350, y: 310 };
  const cache = { x: 275, y: 240, w: 150, h: 140 };
  // Top-left engine; the other three are mirror images about the die centre.
  const tl: Rect = { x: 70, y: 72, w: 268, h: 214 };
  const notched = (r: Rect, nx: number, ny: number, c: number): Pt[] => [
    [r.x + c, r.y],
    [r.x + r.w - c, r.y],
    [r.x + r.w, r.y + c],
    [r.x + r.w, ny],
    [nx, ny],
    [nx, r.y + r.h],
    [r.x + c, r.y + r.h],
    [r.x, r.y + r.h - c],
    [r.x, r.y + c],
  ];
  const tlShape = notched(tl, cache.x - 10, cache.y - 10, 8);
  const tlInner = notched({ x: tl.x + 5, y: tl.y + 5, w: tl.w - 10, h: tl.h - 10 }, cache.x - 15, cache.y - 15, 5);
  // 5 x 4 positions minus the 2 x 2 corner at the cache = 16 CU tiles.
  const spec: GridSpec = { pad: [12, 12, 12, 12], cols: 5, rows: 4, gx: 8, gy: 8, skip: (cc, r) => cc >= 3 && r >= 2 };
  const tlPhys = gridTiles(tl, spec);
  const mx = (x: number, w = 0) => 2 * centre.x - x - w;
  const my = (y: number, h = 0) => 2 * centre.y - y - h;
  const flips: [boolean, boolean][] = [
    [false, false],
    [true, false],
    [false, true],
    [true, true],
  ];
  const se: Rect[] = [];
  const seShape: Pt[][] = [];
  const seInner: Pt[][] = [];
  const tiles: Rect[] = [];
  let leaderTile = 0;
  flips.forEach(([fx, fy], i) => {
    se.push({ x: fx ? mx(tl.x, tl.w) : tl.x, y: fy ? my(tl.y, tl.h) : tl.y, w: tl.w, h: tl.h });
    seShape.push(tlShape.map(([x, y]) => [fx ? mx(x) : x, fy ? my(y) : y] as Pt));
    seInner.push(tlInner.map(([x, y]) => [fx ? mx(x) : x, fy ? my(y) : y] as Pt));
    const phys = tlPhys.map((t) => ({ ...t, x: fx ? mx(t.x, t.w) : t.x, y: fy ? my(t.y, t.h) : t.y }));
    const ordered = byCentre(phys, centre.x, centre.y);
    ordered.forEach((t, k) => {
      // Top-right engine, physical row 1, outermost column: anchor of the leader to the speed.
      if (i === 1 && t.c === 0 && t.r === 1) leaderTile = i * CU_PER_SE + k;
    });
    tiles.push(...ordered.map(({ x, y, w, h }) => ({ x, y, w, h })));
  });
  const cells = tiles.map((t) => cellsOf(t, 2, 2, 2.5, 3));
  return { variant: 'mini', W, H, vy: 0, die, mem, se, seShape, seInner, tiles, cells, cache, centre, leaderTile };
}

export const GEOM: Record<Variant, DieGeom> = { full: buildFull(), mini: buildMini() };

/** Device-pixel pen with a virtual->device transform and hairline snapping. */
export class Pen {
  readonly lw: number;
  constructor(
    readonly ctx: CanvasRenderingContext2D,
    readonly s: number,
    readonly ox: number,
    readonly oy: number,
    dpr: number,
  ) {
    this.lw = Math.max(1, Math.round(dpr));
  }
  X(x: number) {
    return this.ox + x * this.s;
  }
  Y(y: number) {
    return this.oy + y * this.s;
  }
  private snap(v: number) {
    const o = this.lw % 2 ? 0.5 : 0;
    return Math.round(v - o) + o;
  }
  rect(r: Rect, color: string, mul = 1) {
    const { ctx } = this;
    ctx.strokeStyle = color;
    ctx.lineWidth = this.lw * mul;
    const x0 = this.snap(this.X(r.x));
    const y0 = this.snap(this.Y(r.y));
    const x1 = this.snap(this.X(r.x + r.w));
    const y1 = this.snap(this.Y(r.y + r.h));
    ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
  }
  fill(r: Rect, color: string) {
    const x0 = Math.round(this.X(r.x));
    const y0 = Math.round(this.Y(r.y));
    const x1 = Math.round(this.X(r.x + r.w));
    const y1 = Math.round(this.Y(r.y + r.h));
    this.ctx.fillStyle = color;
    this.ctx.fillRect(x0, y0, Math.max(1, x1 - x0), Math.max(1, y1 - y0));
  }
  /** Fill covering a rect AND its hairline frame (so a lit cell has no dark rim). */
  fillOver(r: Rect) {
    const h = this.lw / 2;
    const x0 = Math.round(this.X(r.x) - h);
    const y0 = Math.round(this.Y(r.y) - h);
    const x1 = Math.round(this.X(r.x + r.w) + h);
    const y1 = Math.round(this.Y(r.y + r.h) + h);
    this.ctx.fillRect(x0, y0, Math.max(1, x1 - x0), Math.max(1, y1 - y0));
  }
  /** Dashed hairline rectangle (virtual-unit dash lengths). */
  dashRect(r: Rect, color: string, dash: number[]) {
    const { ctx } = this;
    ctx.strokeStyle = color;
    ctx.lineWidth = this.lw;
    ctx.setLineDash(dash.map((d) => d * this.s));
    const x0 = this.snap(this.X(r.x));
    const y0 = this.snap(this.Y(r.y));
    const x1 = this.snap(this.X(r.x + r.w));
    const y1 = this.snap(this.Y(r.y + r.h));
    ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
    ctx.setLineDash([]);
  }
  /** Diagonal hatch (45 deg, rising to the right) clipped to a rectangle; `step` in virtual units. */
  hatch(r: Rect, color: string, step: number) {
    const { ctx } = this;
    const x0 = Math.round(this.X(r.x));
    const y0 = Math.round(this.Y(r.y));
    const w = Math.max(1, Math.round(this.X(r.x + r.w)) - x0);
    const h = Math.max(1, Math.round(this.Y(r.y + r.h)) - y0);
    const d = Math.max(4, step * this.s);
    ctx.save();
    ctx.beginPath();
    ctx.rect(x0, y0, w, h);
    ctx.clip();
    ctx.strokeStyle = color;
    ctx.lineWidth = this.lw;
    ctx.beginPath();
    for (let k = -h; k < w; k += d) {
      ctx.moveTo(x0 + k, y0 + h);
      ctx.lineTo(x0 + k + h, y0);
    }
    ctx.stroke();
    ctx.restore();
  }
  line(x1: number, y1: number, x2: number, y2: number, color: string, dash?: number[]) {
    const { ctx } = this;
    ctx.strokeStyle = color;
    ctx.lineWidth = this.lw;
    if (dash) ctx.setLineDash(dash.map((d) => d * this.s));
    ctx.beginPath();
    const ax = x1 === x2 ? this.snap(this.X(x1)) : this.X(x1);
    const bx = x1 === x2 ? ax : this.X(x2);
    const ay = y1 === y2 ? this.snap(this.Y(y1)) : this.Y(y1);
    const by = y1 === y2 ? ay : this.Y(y2);
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
    ctx.stroke();
    if (dash) ctx.setLineDash([]);
  }
  poly(points: Pt[], color: string, close = false) {
    const { ctx } = this;
    ctx.strokeStyle = color;
    ctx.lineWidth = this.lw;
    ctx.beginPath();
    points.forEach(([x, y], i) => {
      const px = this.snap(this.X(x));
      const py = this.snap(this.Y(y));
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    });
    if (close) ctx.closePath();
    ctx.stroke();
  }
  polyFill(points: Pt[], color: string) {
    const { ctx } = this;
    ctx.fillStyle = color;
    ctx.beginPath();
    points.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(this.X(x), this.Y(y)) : ctx.lineTo(this.X(x), this.Y(y))));
    ctx.closePath();
    ctx.fill();
  }
  /** Rectangle with chamfered corners (drafting style). */
  chamfer(r: Rect, c: number, color: string) {
    this.poly(chamferPts(r, c), color, true);
  }
  dot(x: number, y: number, r: number, color: string) {
    const { ctx } = this;
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(this.X(x), this.Y(y), Math.max(1.5, r * this.s), 0, Math.PI * 2);
    ctx.fill();
  }
  circle(x: number, y: number, r: number, color: string) {
    const { ctx } = this;
    ctx.strokeStyle = color;
    ctx.lineWidth = this.lw;
    ctx.beginPath();
    ctx.arc(this.X(x), this.Y(y), r * this.s, 0, Math.PI * 2);
    ctx.stroke();
  }
  text(str: string, x: number, y: number, size: number, color: string, opts: { weight?: number; align?: CanvasTextAlign; spacing?: number } = {}) {
    const { ctx } = this;
    ctx.fillStyle = color;
    ctx.font = `${opts.weight ?? 500} ${size * this.s}px ${FONT_DATA}`;
    ctx.textAlign = opts.align ?? 'left';
    ctx.textBaseline = 'alphabetic';
    if ('letterSpacing' in ctx) (ctx as CanvasRenderingContext2D & { letterSpacing: string }).letterSpacing = `${(opts.spacing ?? 0) * this.s}px`;
    ctx.fillText(str, this.X(x), this.Y(y));
  }
}

export function memSlot(m: MemBlock, variant: Variant): Rect {
  if (variant === 'full') return { x: m.x + 7, y: m.y + 22, w: m.w - 14, h: m.h - 28 };
  return m.vertical ? { x: m.x + 9, y: m.y + 14, w: m.w - 18, h: m.h - 28 } : { x: m.x + 14, y: m.y + 9, w: m.w - 28, h: m.h - 18 };
}

export interface StaticOpts {
  perBlockGiB: number;
  /** Full only: what one tile means right now ("1 tile =" / "1 token"). */
  caption: string[];
  /** The GPU is powered down (vram.dormant): dark tiles, and the memory caption is drawn with the data. */
  dormant?: boolean;
}

const CELL_FILL = '#1B2933';
const CELL_EDGE = '#2A3B48';
/** Powered-down compute units: the same tiles, dark. */
const CELL_FILL_OFF = '#111A21';
const CELL_EDGE_OFF = '#1B2832';

/** A row of short parallel ticks (pad rows / ladders): background only. */
function comb(p: Pen, x0: number, x1: number, y0: number, y1: number, step: number, color: string) {
  for (let x = x0; x <= x1 + 0.01; x += step) p.line(x, y0, x, y1, color);
}

function drawStaticFull(p: Pen, g: DieGeom, o: StaticOpts) {
  const { die, cache, centre } = g;
  const inner = { x: die.x + 7, y: die.y + 7, w: die.w - 14, h: die.h - 14 };
  const cx = centre.x;
  const busY = centre.y;

  // Edge tabs (package bumps) on all four sides.
  for (const y of [82, 200, 456, 576]) {
    p.rect({ x: die.x - 6, y: y - 9, w: 6, h: 18 }, C.line);
    p.rect({ x: die.x + die.w, y: y - 9, w: 6, h: 18 }, C.line);
  }
  for (const x of [190, 330, 475, 615]) {
    p.rect({ x: x - 9, y: die.y - 6, w: 18, h: 6 }, C.line);
    p.rect({ x: x - 9, y: die.y + die.h, w: 18, h: 6 }, C.line);
  }

  // Margin traces along the left and right edges, with small passives (background).
  for (const [x0, dir] of [
    [inner.x + 6, 1],
    [inner.x + inner.w - 6, -1],
  ] as const) {
    for (const [ya, yb] of [
      [118, 296],
      [362, 540],
    ]) {
      p.line(x0, ya, x0, yb, C.lineDim);
      p.line(x0 + dir * 6, ya + 14, x0 + dir * 6, yb - 14, C.lineDim);
      for (const y of [ya + 40, yb - 52]) {
        const r = { x: dir > 0 ? x0 - 3 : x0 - 9, y, w: 12, h: 18 };
        p.fill(r, C.dieFill);
        p.rect(r, C.line);
      }
    }
  }

  // Vertical spine: two bus pairs from the memory rows into the cache.
  for (const dx of [-13, -6, 6, 13]) {
    const col = Math.abs(dx) > 10 ? C.lineDim : C.line;
    p.line(cx + dx, 101, cx + dx, cache.y - 8, col);
    p.line(cx + dx, cache.y + cache.h + 8, cx + dx, 556, col);
  }
  for (const y of [150, 506]) {
    const r = { x: cx - 17, y: y - 18, w: 34, h: 36 };
    p.fill(r, C.dieFill);
    p.rect(r, C.lineBright);
    p.rect({ x: r.x + 5, y: r.y + 5, w: r.w - 10, h: r.h - 10 }, C.lineDim);
  }

  // Horizontal bus: three rails from the die edge into the cache, with ladder rungs.
  for (const dy of [-10, 0, 10]) {
    const col = dy === 0 ? C.lineDim : C.line;
    p.line(inner.x, busY + dy, cache.x - 8, busY + dy, col);
    p.line(cache.x + cache.w + 8, busY + dy, inner.x + inner.w, busY + dy, col);
  }
  comb(p, 168, 346, busY - 10, busY - 4, 6, C.lineDim);
  comb(p, 458, 636, busY + 4, busY + 10, 6, C.lineDim);
  for (const x of [104, 132, 673, 701]) {
    const r = { x: x - 9, y: busY - 15, w: 18, h: 30 };
    p.fill(r, C.dieFill);
    p.rect(r, C.lineBright);
    p.line(r.x + 4, busY - 5, r.x + r.w - 4, busY - 5, C.lineDim);
    p.line(r.x + 4, busY + 5, r.x + r.w - 4, busY + 5, C.lineDim);
  }
  for (const x of [cache.x - 8, cache.x + cache.w]) p.rect({ x, y: busY - 16, w: 8, h: 32 }, C.lineBright);
  for (const y of [cache.y - 8, cache.y + cache.h]) p.rect({ x: cx - 18, y, w: 36, h: 8 }, C.lineBright);

  // Pad rows between the engines and the bus, and engine-to-bus stubs.
  g.se.forEach((s) => {
    const top = s.y < busY;
    const y0 = top ? s.y + s.h + 4 : s.y - 4;
    const y1 = top ? busY - 14 : busY + 14;
    comb(p, s.x + 22, s.x + s.w - 22, Math.min(y0, y1), Math.min(y0, y1) + 5, 5, C.lineDim);
    const midX = s.x + s.w / 2;
    for (const dx of [-40, 40]) p.line(midX + dx, top ? s.y + s.h : s.y, midX + dx, busY + (top ? -10 : 10), C.line);
  });

  // Memory blocks (frames + empty slots; the fill is dynamic), with links to the engines.
  g.mem.forEach((mb, i) => {
    p.rect(mb, C.lineBright);
    p.rect({ x: mb.x + 3, y: mb.y + 3, w: mb.w - 6, h: mb.h - 6 }, C.lineDim);
    p.text(`GDDR6 ${i + 1}`, mb.x + mb.w / 2, mb.y + 16, 11.5, C.ink, { align: 'center', weight: 500, spacing: 0.3 });
    p.line(mb.x - 5, mb.y + 10, mb.x - 5, mb.y + mb.h - 10, C.line);
    p.line(mb.x + mb.w + 5, mb.y + 10, mb.x + mb.w + 5, mb.y + mb.h - 10, C.line);
    const top = mb.y < busY;
    for (const f of [0.22, 0.5, 0.78]) {
      const x = mb.x + mb.w * f;
      p.line(x, top ? mb.y + mb.h : mb.y, x, top ? mb.y + mb.h + 10 : mb.y - 11, C.lineDim);
    }
    const slot = memSlot(mb, 'full');
    p.fill(slot, '#141E26');
    p.rect(slot, C.line);
  });

  // Shader engines.
  g.se.forEach((s) => {
    p.fill({ x: s.x + 4, y: s.y + 4, w: s.w - 8, h: s.h - 8 }, o.dormant ? '#0B1319' : '#0E1820');
    p.chamfer(s, 10, C.lineBright);
    p.chamfer({ x: s.x + 5, y: s.y + 5, w: s.w - 10, h: s.h - 10 }, 7, C.lineDim);
    p.text('SHADER ENGINE', s.x + 20, s.y + 26, 14, C.ink, { weight: 500, spacing: 0.5 });
    p.rect({ x: s.x + s.w - 44, y: s.y + 15, w: 16, h: 8 }, C.line);
    p.rect({ x: s.x + s.w - 24, y: s.y + 17, w: 6, h: 4 }, C.line);
    // Shader-array split (between tile rows 1 and 2).
    const th = (s.h - 50 - 3 * 5 - 5) / 4;
    const ay = s.y + 38 + 2 * th + 5 + 5;
    p.line(s.x + 14, ay, s.x + s.w - 14, ay, C.lineDim, [2, 3]);
  });

  // CU tiles: frames + idle cells.
  g.tiles.forEach((t, i) => {
    p.rect({ x: t.x - 1, y: t.y - 1, w: t.w + 2, h: t.h + 2 }, C.lineDim);
    for (const c of g.cells[i]) {
      p.fill(c, o.dormant ? CELL_FILL_OFF : CELL_FILL);
      p.rect(c, o.dormant ? CELL_EDGE_OFF : CELL_EDGE);
    }
  });

  // Cache block frame + label (fill and slats are dynamic).
  p.fill(cache, '#0C141A');
  p.rect(cache, C.lineBright);
  p.rect({ x: cache.x + 5, y: cache.y + 5, w: cache.w - 10, h: cache.h - 10 }, C.lineDim);
  p.text('PROMPT', cache.x + cache.w / 2, cache.y + 64, 12.5, C.ink, { align: 'center', weight: 500, spacing: 0.3 });
  p.text('CACHE', cache.x + cache.w / 2, cache.y + 79, 12.5, C.ink, { align: 'center', weight: 500, spacing: 0.3 });

  // Callout: compute units, and what one tile means right now.
  const lt = g.tiles[g.leaderTile];
  const tx = lt.x + lt.w * 0.3;
  const ty = lt.y + lt.h / 2;
  p.text('COMPUTE', 4, 262, 12, C.ink, { weight: 500 });
  p.text('UNITS', 4, 277, 12, C.ink, { weight: 500 });
  o.caption.forEach((line, i) => p.text(line, 4, 302 + i * 15, 11.5, C.label, { weight: 500 }));
  p.poly(
    [
      [46, 282],
      [60, 282],
    ],
    C.lineHi,
  );
  p.line(60, 282, tx, ty, C.lineHi);
  p.dot(tx, ty, 2.6, C.hot);

  // Memory caption under the bottom row (dormant: the dynamic layer says what is paged out instead).
  if (!o.dormant)
    p.text(`GDDR6 · 8 × ${o.perBlockGiB.toFixed(2)} GiB · filled in order up to VRAM used`, die.x + die.w / 2, 630, 12, C.inkDim, {
      align: 'center',
      weight: 500,
    });
}

function drawStaticMini(p: Pen, g: DieGeom, o: StaticOpts) {
  const { die, cache, centre } = g;
  const inner = { x: die.x + 7, y: die.y + 7, w: die.w - 14, h: die.h - 14 };

  for (const [x, y] of [
    [22, 22],
    [678, 22],
    [22, 598],
    [678, 598],
  ])
    p.circle(x, y, 6, C.lineBright);

  // Buses in the cross channels.
  for (const d of [-5, 5]) {
    p.line(centre.x + d, inner.y + 4, centre.x + d, cache.y - 10, C.line);
    p.line(centre.x + d, cache.y + cache.h + 10, centre.x + d, inner.y + inner.h - 4, C.line);
    p.line(inner.x + 4, centre.y + d, cache.x - 10, centre.y + d, C.line);
    p.line(cache.x + cache.w + 10, centre.y + d, inner.x + inner.w - 4, centre.y + d, C.line);
  }

  // Memory blocks with end caps and pins to the engines.
  g.mem.forEach((mb) => {
    p.rect(mb, C.lineBright);
    if (mb.vertical) {
      p.rect({ x: mb.x + 5, y: mb.y - 7, w: mb.w - 10, h: 7 }, C.lineBright);
      p.rect({ x: mb.x + 5, y: mb.y + mb.h, w: mb.w - 10, h: 7 }, C.lineBright);
      const left = mb.x < centre.x;
      for (let i = 0; i < 7; i++) {
        const y = mb.y + 16 + i * 24;
        p.line(left ? mb.x + mb.w : mb.x, y, left ? mb.x + mb.w + 12 : mb.x - 12, y, C.lineDim);
      }
    } else {
      p.rect({ x: mb.x - 7, y: mb.y + 5, w: 7, h: mb.h - 10 }, C.lineBright);
      p.rect({ x: mb.x + mb.w, y: mb.y + 5, w: 7, h: mb.h - 10 }, C.lineBright);
      const top = mb.y < centre.y;
      for (let i = 0; i < 9; i++) {
        const x = mb.x + 11 + i * 24;
        p.line(x, top ? mb.y + mb.h : mb.y, x, top ? mb.y + mb.h + 12 : mb.y - 12, C.lineDim);
      }
    }
    const slot = memSlot(mb, 'mini');
    p.fill(slot, '#141E26');
    p.rect(slot, C.line);
  });

  // Shader engines, notched around the cache (double outline).
  g.seShape.forEach((shape) => {
    p.polyFill(shape, '#0E1820');
    p.poly(shape, C.lineBright, true);
  });
  g.seInner.forEach((shape) => p.poly(shape, C.lineDim, true));

  // CU tiles.
  g.tiles.forEach((t, i) => {
    p.rect({ x: t.x - 1, y: t.y - 1, w: t.w + 2, h: t.h + 2 }, C.lineDim);
    for (const c of g.cells[i]) {
      p.fill(c, o.dormant ? CELL_FILL_OFF : CELL_FILL);
      p.rect(c, o.dormant ? CELL_EDGE_OFF : CELL_EDGE);
    }
  });

  // Cache package (background in the mini): pins, fan-out to the engines, a fine grid die.
  for (let x = cache.x + 10; x <= cache.x + cache.w - 10; x += 10) {
    p.line(x, cache.y - 8, x, cache.y, C.line);
    p.line(x, cache.y + cache.h, x, cache.y + cache.h + 8, C.line);
  }
  for (let y = cache.y + 10; y <= cache.y + cache.h - 10; y += 10) {
    p.line(cache.x - 8, y, cache.x, y, C.line);
    p.line(cache.x + cache.w, y, cache.x + cache.w + 8, y, C.line);
  }
  for (const [x, y, sx, sy] of [
    [cache.x, cache.y, -1, -1],
    [cache.x + cache.w, cache.y, 1, -1],
    [cache.x, cache.y + cache.h, -1, 1],
    [cache.x + cache.w, cache.y + cache.h, 1, 1],
  ] as const) {
    p.line(x, y, x + sx * 10, y + sy * 10, C.lineBright);
    p.line(x + sx * 10, y + sy * 10, x + sx * 10, y + sy * 34, C.lineDim);
    p.line(x + sx * 10, y + sy * 10, x + sx * 34, y + sy * 10, C.lineDim);
  }
  p.fill(cache, '#0C141A');
  p.rect(cache, C.lineBright);
  const ci = { x: cache.x + 9, y: cache.y + 9, w: cache.w - 18, h: cache.h - 18 };
  p.rect(ci, C.lineDim);
  const n = 12;
  const m = 11;
  const pw = (ci.w - 6) / n;
  const ph = (ci.h - 6) / m;
  for (let j = 0; j < m; j++)
    for (let i = 0; i < n; i++) p.fill({ x: ci.x + 3 + i * pw + 1, y: ci.y + 3 + j * ph + 1, w: pw - 2.5, h: ph - 2.5 }, '#172430');
}

/** Everything that does not change frame to frame. */
export function drawStatic(p: Pen, g: DieGeom, o: StaticOpts) {
  const { die } = g;
  p.fill(die, C.dieFill);
  p.rect(die, C.lineBright);
  p.rect({ x: die.x + 7, y: die.y + 7, w: die.w - 14, h: die.h - 14 }, C.lineDim);
  // Corner brackets outside the die outline (drafting registration marks).
  const k = 10;
  const m = 5;
  for (const [cx, cy, sx, sy] of [
    [die.x, die.y, -1, -1],
    [die.x + die.w, die.y, 1, -1],
    [die.x, die.y + die.h, -1, 1],
    [die.x + die.w, die.y + die.h, 1, 1],
  ] as const) {
    const bx = cx + sx * m;
    const by = cy + sy * m;
    p.poly(
      [
        [bx, by - sy * k],
        [bx, by],
        [bx - sx * k, by],
      ],
      C.lineBright,
    );
  }
  if (g.variant === 'full') drawStaticFull(p, g, o);
  else drawStaticMini(p, g, o);
}

export interface DynamicState {
  usedGiB: number;
  perBlockGiB: number;
  /** 0..1 prompt-cache reuse of the last request, or null if there is none (full only). */
  cacheFrac: number | null;
  /** Tile glow 0..1, by tile id. */
  glow: (tile: number) => number;
  /** GPU dormant: GiB of the session's allocations paged out to system RAM, else null. */
  pagedOutGiB?: number | null;
}

/** The data layer. Cheap: a few dozen fillRects from precomputed rectangles. */
export function drawDynamic(p: Pen, g: DieGeom, d: DynamicState) {
  const full = g.variant === 'full';
  const { ctx } = p;

  // Memory gauge.
  g.mem.forEach((mb, i) => {
    const f = d.perBlockGiB > 0 ? Math.min(1, Math.max(0, d.usedGiB / d.perBlockGiB - i)) : 0;
    if (f <= 0) return;
    const s = memSlot(mb, g.variant);
    const r = { x: s.x + 2, y: s.y + 2, w: s.w - 4, h: s.h - 4 };
    if (mb.vertical) p.fill({ x: r.x, y: r.y + r.h * (1 - f), w: r.w, h: r.h * f }, C.cyan);
    else p.fill({ x: r.x, y: r.y, w: r.w * f, h: r.h }, C.cyan);
  });

  // Dormant: the allocations that are not resident sit in system RAM. Drawn as empty, hatched, dashed
  // outlines over [resident, resident + paged out], block by block, at the same GiB scale as the fill.
  const paged = d.pagedOutGiB ?? null;
  if (paged !== null && d.perBlockGiB > 0) {
    const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
    g.mem.forEach((mb, i) => {
      const fa = clamp01(d.usedGiB / d.perBlockGiB - i);
      const fb = clamp01((d.usedGiB + paged) / d.perBlockGiB - i);
      if (fb - fa < 0.004) return;
      const s = memSlot(mb, g.variant);
      const r = { x: s.x + 2, y: s.y + 2, w: s.w - 4, h: s.h - 4 };
      const part = mb.vertical ? { x: r.x, y: r.y + r.h * (1 - fb), w: r.w, h: r.h * (fb - fa) } : { x: r.x + r.w * fa, y: r.y, w: r.w * (fb - fa), h: r.h };
      p.hatch(part, 'rgba(120,190,236,0.34)', full ? 5 : 8);
      p.dashRect(part, C.lineHi, full ? [3, 2.5] : [6, 5]);
    });
    if (full) {
      p.text(`PAGED OUT · ${paged.toFixed(2)} GiB IN SYSTEM RAM`, g.die.x + g.die.w / 2, 630, 12, C.amber, { align: 'center', weight: 600, spacing: 0.5 });
    }
  }

  if (full) {
    // Prompt cache: fill from the bottom by reuse fraction, slats lit below the level.
    const c = g.cache;
    const ci = { x: c.x + 8, y: c.y + 8, w: c.w - 16, h: c.h - 16 };
    const frac = d.cacheFrac ?? 0;
    const fillY = ci.y + ci.h * (1 - frac);
    // Dormant: the last request's reuse is a remembered figure, not a live one.
    const off = paged !== null;
    if (frac > 0) {
      ctx.globalAlpha = off ? 0.13 : 0.3;
      p.fill({ x: ci.x, y: fillY, w: ci.w, h: ci.h * frac }, C.cyan);
      ctx.globalAlpha = 1;
    }
    for (const [y0, y1] of [
      [c.y + 10, c.y + 46],
      [c.y + 100, c.y + c.h - 10],
    ]) {
      for (let x = ci.x + 3; x < ci.x + ci.w - 1; x += 5) {
        if (frac > 0 && fillY < y1) {
          const ly = Math.max(y0, fillY);
          if (ly > y0) p.line(x, y0, x, ly, C.lineDim);
          p.line(x, ly, x, y1, off ? '#2A5877' : C.cyanSoft);
        } else p.line(x, y0, x, y1, C.lineDim);
      }
    }
    p.text(d.cacheFrac === null ? '—' : `${Math.round(frac * 100)}%`, c.x + c.w / 2, c.y + 96, 16, off ? C.muted : C.hot, { align: 'center', weight: 700 });
  }

  // Token stream: a tile lights as a whole.
  for (let t = 0; t < SE_COUNT * CU_PER_SE; t++) {
    const b = d.glow(t);
    if (b < 0.03) continue;
    ctx.globalAlpha = Math.min(1, b * 1.05);
    ctx.fillStyle = b > 0.8 ? C.cyanLit : C.cyan;
    for (const cell of g.cells[t]) p.fillOver(cell);
  }
  ctx.globalAlpha = 1;
}
