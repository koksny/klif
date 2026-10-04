// The shareable record card: a 1200x675 poster painted on a canvas in the active skin's tokens and fonts
// (the fonts are already loaded by the skin, so nothing is embedded or fetched). PNG for X / Discord, or the
// clipboard.
import type { HardwareInfo, RecordEntry, RecordEvent, RecordMetric } from '../../model/types';
import { backendLabel, conditions, deltaText, fmtDate, fmtValue, metricMeta } from './metrics';

export interface CardData {
  entry: RecordEntry;
  metric: RecordMetric;
  hw?: HardwareInfo;
  machine: string;
  now: number;
  version: string;
  /** A record broken lately: the card says so and by how much. */
  fresh?: RecordEvent;
  /** The climb of this record (oldest first), drawn small when it has two or more steps. */
  climb?: { at: number; value: number }[];
  /** 0..1, the count-up of an animated export; 1 = the final card. */
  progress?: number;
}

export const CARD_W = 1200;
export const CARD_H = 675;

function tokens() {
  const cs = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => cs.getPropertyValue(name).trim() || fallback;
  return {
    bg: v('--k-bg', '#0f1316'),
    surface: v('--k-surface', '#1d252a'),
    line: v('--k-line', '#2c363c'),
    ink: v('--k-ink', '#dceff8'),
    muted: v('--k-muted', '#8fa3ae'),
    accent: v('--k-accent', '#5ab6eb'),
    record: v('--k-record', v('--k-warn', '#f2a33a')),
    ui: v('--k-font-ui', 'system-ui, sans-serif'),
    data: v('--k-font-data', 'monospace'),
    display: v('--k-font-display', 'system-ui, sans-serif'),
  };
}

/** Wait until the fonts a card uses can be drawn (a canvas never waits on its own). */
async function fontsReady(t: ReturnType<typeof tokens>) {
  try {
    await Promise.all([document.fonts.load(`500 40px ${t.data}`), document.fonts.load(`700 40px ${t.ui}`), document.fonts.load(`800 40px ${t.display}`)]);
    await document.fonts.ready;
  } catch {
    // An unknown font family only means a fallback face.
  }
}

/** Shrink `text` until it fits `max` px at the font, ending with an ellipsis. */
function fit(ctx: CanvasRenderingContext2D, text: string, max: number): string {
  if (ctx.measureText(text).width <= max) return text;
  let s = text;
  while (s.length > 1 && ctx.measureText(s + '…').width > max) s = s.slice(0, -1);
  return s + '…';
}

function mix(a: string, alpha: number): string {
  return `color-mix(in srgb, ${a} ${Math.round(alpha * 100)}%, transparent)`;
}

/** canvas fillStyle does not take color-mix(): resolve any CSS colour through the browser once. */
function rgba(color: string, alpha: number): string {
  const probe = document.createElement('canvas').getContext('2d')!;
  probe.fillStyle = '#000';
  probe.fillStyle = color;
  const hex = probe.fillStyle as string;
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (m) return `rgba(${parseInt(m[1], 16)}, ${parseInt(m[2], 16)}, ${parseInt(m[3], 16)}, ${alpha})`;
  const r = /rgba?\(([^)]+)\)/.exec(hex);
  if (r) {
    const [cr, cg, cb] = r[1].split(',').map((x) => parseFloat(x));
    return `rgba(${cr}, ${cg}, ${cb}, ${alpha})`;
  }
  return mix(color, alpha);
}

export async function paintCard(d: CardData, scale = 1): Promise<HTMLCanvasElement> {
  const t = tokens();
  await fontsReady(t);
  const c = document.createElement('canvas');
  c.width = CARD_W * scale;
  c.height = CARD_H * scale;
  const g = c.getContext('2d')!;
  g.scale(scale, scale);
  const m = metricMeta(d.metric);
  const v = d.entry.best[d.metric]!;
  const p = d.progress ?? 1;
  const ease = 1 - Math.pow(1 - p, 3);

  // Ground: the skin's background, a glow behind the number, faint isolines.
  g.fillStyle = t.bg;
  g.fillRect(0, 0, CARD_W, CARD_H);
  const glow = g.createRadialGradient(330, 300, 10, 330, 300, 620);
  glow.addColorStop(0, rgba(d.fresh ? t.record : t.accent, 0.2 * ease));
  glow.addColorStop(1, rgba(t.accent, 0));
  g.fillStyle = glow;
  g.fillRect(0, 0, CARD_W, CARD_H);
  g.strokeStyle = rgba(t.ink, 0.05);
  g.lineWidth = 1;
  for (let i = 0; i < 16; i++) {
    g.beginPath();
    const y0 = 30 + i * 42;
    for (let x = -10; x <= CARD_W + 10; x += 30) {
      const y = y0 + Math.sin(x / 190 + i * 0.8) * 14 + Math.sin(x / 70 + i * 1.7) * 4;
      if (x === -10) g.moveTo(x, y);
      else g.lineTo(x, y);
    }
    g.stroke();
  }

  const L = 72;
  // Brand and what the number is.
  g.textBaseline = 'alphabetic';
  g.fillStyle = t.accent;
  g.font = `800 34px ${t.display}`;
  g.fillText('K', L, 92);
  g.fillStyle = t.ink;
  g.font = `700 30px ${t.display}`;
  g.fillText('KLIF', L + 34, 92);
  g.fillStyle = t.muted;
  g.font = `600 17px ${t.ui}`;
  const head = `${m.long.toUpperCase()} RECORD`;
  g.textAlign = 'right';
  g.fillText(head, CARD_W - L, 88);
  g.textAlign = 'left';

  // The number, counting up in an animated export.
  const shown = m.higher ? v.value * ease : v.value / Math.max(ease, 0.05);
  const value = fmtValue(d.metric, p >= 1 ? v.value : shown);
  g.font = `500 196px ${t.data}`;
  g.fillStyle = d.fresh ? t.record : t.ink;
  g.shadowColor = rgba(d.fresh ? t.record : t.accent, 0.45);
  g.shadowBlur = 40;
  g.fillText(value, L - 8, 318);
  g.shadowBlur = 0;
  const vw = g.measureText(value).width;
  g.font = `500 34px ${t.ui}`;
  g.fillStyle = t.accent;
  g.fillText(m.unit, L + vw + 8, 318);

  if (d.fresh) {
    const tag = d.fresh.old === undefined ? 'FIRST RECORD' : `NEW RECORD  ${deltaText(d.metric, d.fresh)}`;
    g.font = `700 18px ${t.ui}`;
    const tw = g.measureText(tag).width;
    g.fillStyle = t.record;
    g.beginPath();
    g.roundRect(L, 140, tw + 28, 34, 5);
    g.fill();
    g.fillStyle = t.bg;
    g.fillText(tag, L + 14, 163);
  }

  // The model: name, quant, backend, the conditions of this value.
  g.fillStyle = t.ink;
  g.font = `700 46px ${t.ui}`;
  g.fillText(fit(g, d.entry.model.name, CARD_W - 2 * L - 360), L, 402);
  g.font = `400 26px ${t.ui}`;
  g.fillStyle = t.muted;
  const quant = d.entry.model.quant ?? d.entry.model.file;
  g.fillText(fit(g, quant, 520), L, 446);
  const qw = Math.min(g.measureText(quant).width, 520);
  const be = backendLabel(d.entry.backend);
  g.font = `600 20px ${t.data}`;
  const bw = g.measureText(be).width + 22;
  g.strokeStyle = t.accent;
  g.lineWidth = 1.5;
  g.beginPath();
  g.roundRect(L + qw + 18, 422, bw, 32, 5);
  g.stroke();
  g.fillStyle = t.accent;
  g.fillText(be, L + qw + 29, 445);
  g.font = `400 22px ${t.data}`;
  g.fillStyle = t.muted;
  const cond = [conditions(d.metric, v), v.source === 'bench' ? 'klif-cli bench' : ''].filter(Boolean).join(' · ');
  g.fillText(fit(g, cond, CARD_W - 2 * L - 360), L, 490);

  // The climb, small, on the right.
  const pts = d.climb ?? [];
  if (pts.length > 1) {
    const x0 = CARD_W - L - 300, y0 = 360, w = 300, h = 120;
    const ts = pts.map((q) => q.at), vs = pts.map((q) => q.value);
    const ta = Math.min(...ts), tb = Math.max(...ts);
    const lo = Math.min(...vs), hi = Math.max(...vs);
    const X = (x: number) => x0 + (tb > ta ? (x - ta) / (tb - ta) : 1) * w;
    const Y = (y: number) => {
      const f = hi > lo ? (y - lo) / (hi - lo) : 0.5;
      return y0 + (m.higher ? 1 - f : f) * h;
    };
    const n = Math.max(2, Math.ceil(pts.length * ease));
    g.strokeStyle = t.accent;
    g.lineWidth = 2.5;
    g.beginPath();
    g.moveTo(X(pts[0].at), Y(pts[0].value));
    for (let i = 1; i < n; i++) {
      g.lineTo(X(pts[i].at), Y(pts[i - 1].value));
      g.lineTo(X(pts[i].at), Y(pts[i].value));
    }
    g.stroke();
    const last = pts[n - 1];
    g.fillStyle = t.record;
    g.beginPath();
    g.arc(X(last.at), Y(last.value), 6, 0, Math.PI * 2);
    g.fill();
    g.fillStyle = t.muted;
    g.font = `600 14px ${t.ui}`;
    g.fillText('CLIMB', x0, y0 - 18);
  }

  // The machine strip.
  g.fillStyle = rgba(t.surface, 0.85);
  g.fillRect(0, CARD_H - 112, CARD_W, 112);
  g.strokeStyle = t.line;
  g.beginPath();
  g.moveTo(0, CARD_H - 112);
  g.lineTo(CARD_W, CARD_H - 112);
  g.stroke();
  const hw = d.hw;
  g.textBaseline = 'middle';
  const sy = CARD_H - 56;
  if (hw && hw.tflopsFp32 > 0) {
    g.font = `800 52px ${t.display}`;
    g.fillStyle = t.ink;
    const tf = hw.tflopsFp32.toFixed(1);
    g.fillText(tf, L, sy + 2);
    const tw = g.measureText(tf).width;
    g.font = `600 15px ${t.ui}`;
    g.fillStyle = t.accent;
    g.fillText('TFLOPS', L + tw + 12, sy - 9);
    g.fillText('FP32', L + tw + 12, sy + 11);
    const gpus = v.gpus.length ? v.gpus.join(' + ') : hw.gpus.filter((x) => x.counted).map((x) => x.name).join(' + ');
    const mem = `VRAM ${hw.vramPoolGiB.toFixed(1)} GB · RAM ${Math.round(hw.ramTotalGiB)} GB`;
    g.font = `600 22px ${t.ui}`;
    g.fillStyle = t.ink;
    g.fillText(fit(g, gpus, 560), L + tw + 100, sy - 13);
    g.font = `400 19px ${t.data}`;
    g.fillStyle = t.muted;
    g.fillText(fit(g, `${d.machine} · ${mem}`, 560), L + tw + 100, sy + 16);
  } else {
    g.font = `600 22px ${t.ui}`;
    g.fillStyle = t.ink;
    g.fillText(fit(g, `${d.machine} · ${v.gpus.join(' + ')}`, 760), L, sy);
  }
  g.textAlign = 'right';
  g.font = `500 19px ${t.data}`;
  g.fillStyle = t.muted;
  g.fillText(fmtDate(v.at, d.now), CARD_W - L, sy - 13);
  g.fillText(`klif ${d.version}${v.backendBuild ? ` · ${v.backendBuild}` : ''}`, CARD_W - L, sy + 16);
  g.textAlign = 'left';
  return c;
}

export function cardBlob(c: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => c.toBlob((b) => (b ? resolve(b) : reject(new Error('The card could not be encoded.'))), 'image/png'));
}

/** "klif-gemma-4-26b-a4b-ud-q4_k_xl-hip-decode-2026-10-04.png" */
export function cardFileName(d: CardData, ext = 'png'): string {
  const date = new Date(d.entry.best[d.metric]!.at * 1000).toISOString().slice(0, 10);
  const slug = `${d.entry.model.name} ${d.entry.model.quant ?? ''} ${d.entry.backend} ${metricMeta(d.metric).label}`
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 90);
  return `klif-${slug}-${date}.${ext}`;
}

/** Put the PNG on the clipboard (paste straight into Discord or X). False when the host does not allow it. */
export async function copyCard(blob: Blob): Promise<boolean> {
  try {
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
    return true;
  } catch {
    return false;
  }
}
