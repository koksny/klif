// Decode field: a terminal that is always full of random characters, read as the model's context window.
//   noise          = context not used yet; it churns
//   locked text    = context in use (its share of the screen is the real context fill), read prompt in darker
//                    blue, generated text in light blue; it stops churning
//   prefill        = the frontier sweeps forward and freezes the noise into the prompt
//   decode         = the cursor at the end of the context FINDS whole tokens in the noise: the characters there
//                    shuffle, lock into the token and glow, then cool down; the rate follows tok/s, drafts
//                    accepted by speculation land in bursts
//   load           = the terminal boots: characters print in as the model loads
//   Krea           = the noise settles step by step into an ASCII picture
//   dormant GPU    = everything freezes and goes amber (waking: it thaws as the VRAM comes back)
//   fault          = red glitch, the context dissolves
//   request end    = a bright scan sweeps and a "request N done" line resolves from the noise
// The right margin (full window only) is VRAM as rows of a memory dump. Drawn with WebGL2 instanced glyph
// quads. The owner drives it: tick() from the shared frame scheduler, setVm() at the telemetry rate.
import type { ViewModel, VramLayer } from '../../lib/model/types';

type Mode = 'idle' | 'decode' | 'prefill' | 'load' | 'denoise' | 'sleep' | 'wake' | 'fault' | 'stop';
type RGB = [number, number, number];

// ---- palette ----------------------------------------------------------------------------------------------------
const ICE: RGB = [0.91, 0.97, 1.0];
const ARC: RGB = [0.36, 0.78, 1.0];
const STREAM: RGB = [0.18, 0.55, 1.0];
const NOISE_RGB: RGB = [0.11, 0.3, 0.6];
const PROMPT_RGB: RGB = [0.16, 0.45, 0.78];
const WRITTEN_RGB: RGB = [0.42, 0.68, 0.98];
const DEEP: RGB = [0.1, 0.3, 0.72];
const AMBER: RGB = [1.0, 0.71, 0.28];
const RED: RGB = [1.0, 0.3, 0.42];
const LAYER_RGB: Record<string, RGB> = {
  weights: [0.16, 0.42, 0.95],
  kv: [0.36, 0.78, 1.0],
  buffers: [0.62, 0.88, 1.0],
  draft: [0.55, 0.6, 1.0],
  projector: [0.45, 0.7, 0.95],
  other: [0.2, 0.3, 0.45],
};

// ---- glyphs -----------------------------------------------------------------------------------------------------
const ASCII = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!"#$%&\'()*+,-./:;<=>?@[\\]^_`{|}~';
const SHAPES = ['█', '░', '·', '─', '¦'];
const CHARS = [...ASCII, ...SHAPES];
const HALO = CHARS.length;
const GLYPH = new Map<string, number>(CHARS.map((c, i) => [c, i]));
const BLANK = -1;
const g = (c: string) => (c === ' ' ? BLANK : (GLYPH.get(c) ?? GLYPH.get('?')!));
const HEX = [...'0123456789abcdef'].map(g);
const NOISE = [...'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789{}[]<>/\\=+-*:;._#$%&@!?|~^'].map(g);
const RAMP = [...' .:-=+*#%@'].map(g); // ASCII-art density ramp for the Krea picture
const CURSOR = g('█');
const EDGE = g('¦');

// Pseudo text the "model" writes (KLIF never sees real prompts or answers): words split into BPE-like tokens,
// a leading space belongs to the token as in real tokenizers.
const WORDS = (
  'the model returns a tensor of shape for each layer and the attention weights are cached in the kv buffer ' +
  'when the context window fills up the oldest tokens are evicted so we keep the system prompt pinned ' +
  'function decode step takes the logits applies temperature and samples the next token from the top k ' +
  'experts are routed per token while the shared expert runs on every step of the batch ' +
  'let result equal await stream next if done then return else continue the loop ' +
  'quantized weights load into vram and the draft head proposes tokens the target model accepts'
).split(' ');

function tokenize(word: string): string[] {
  if (word.length <= 4) return [' ' + word];
  const cut = Math.max(2, Math.min(word.length - 2, Math.round(word.length * 0.55)));
  return [' ' + word.slice(0, cut), word.slice(cut)];
}

/** A long run of pseudo tokens for one session (deterministic). */
function tokenRun(seed: number, n: number): string[] {
  const out: string[] = [];
  let i = 0;
  while (out.length < n) out.push(...tokenize(WORDS[Math.floor(hash(seed, i++) * WORDS.length)]));
  return out;
}

// ---- hashing / helpers -------------------------------------------------------------------------------------------
function hash(a: number, b: number, c = 0): number {
  let h = Math.imul(a | 0, 0x27d4eb2d) ^ Math.imul(b | 0, 0x165667b1) ^ Math.imul(c | 0, 0x9e3779b1);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}
const pick = <T>(a: T[], r: number) => a[Math.min(a.length - 1, Math.floor(r * a.length))];
const mix = (a: RGB, b: RGB, k: number): RGB => [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
const clamp = (x: number, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const approach = (x: number, target: number, rate: number, dt: number) => x + (target - x) * (1 - Math.exp(-rate * dt));

interface Find {
  i0: number; // first cell (reading order)
  text: number[];
  t0: number;
  hold: number;
}

interface Banner {
  i0: number;
  text: number[];
  t0: number;
  hold: number;
  tint: RGB;
}

const VS = `#version 300 es
layout(location=0) in vec2 a_corner;
layout(location=1) in vec4 i_rect;
layout(location=2) in float i_glyph;
layout(location=3) in vec4 i_color;
uniform vec2 u_res;
uniform vec2 u_grid;
uniform vec4 u_hero;
out vec2 v_uv;
out vec4 v_color;
void main() {
  vec2 p = i_rect.xy + a_corner * i_rect.zw;
  vec2 c = vec2(mod(i_glyph, u_grid.x), floor(i_glyph / u_grid.x));
  v_uv = (c + a_corner) / u_grid;
  float k = 1.0;
  vec2 m = i_rect.xy + i_rect.zw * 0.5;
  if (m.x > u_hero.x && m.x < u_hero.z && m.y > u_hero.y && m.y < u_hero.w) k = 0.14;
  v_color = vec4(i_color.rgb, i_color.a * k);
  vec2 clip = p / u_res * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}`;
const FS = `#version 300 es
precision mediump float;
in vec2 v_uv;
in vec4 v_color;
uniform sampler2D u_atlas;
out vec4 o;
void main() {
  float a = texture(u_atlas, v_uv).a * v_color.a;
  o = vec4(v_color.rgb * a, a);
}`;

const CAP = 40000;
const STRIDE = 9;
const ATLAS_COLS = 16;

export interface FieldOptions {
  /** Fraction of the field width given to the VRAM margin (0 = no margin, the mini panel). */
  margin: number;
  /** Glyph size in CSS px of the field's own layout box. */
  fontPx: number;
  /** Few columns (mini panel): banners use their short form. */
  compact?: boolean;
}

/** A rectangle in the field's layout px (the hero overlay the terminal dims and keeps banners out of). */
export interface FieldRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface GlRes {
  prog: WebGLProgram;
  vao: WebGLVertexArrayObject;
  quad: WebGLBuffer;
  inst: WebGLBuffer;
  tex: WebGLTexture;
  uRes: WebGLUniformLocation | null;
  uGrid: WebGLUniformLocation | null;
  uHero: WebGLUniformLocation | null;
}

export class DecodeField {
  private gl: WebGL2RenderingContext;
  private res: GlRes | null = null;
  private lost = false;
  private data = new Float32Array(CAP * STRIDE);
  private n = 0;
  private k = 1; // backing px per layout px (devicePixelRatio x the ancestor's CSS scale)
  private w = 1;
  private h = 1;
  private atlasRows = 1;
  private fontPx: number;
  private compact: boolean;
  // grid
  private cw = 8;
  private ch = 20;
  private cols = 1; // whole field
  private rows = 1;
  private tcols = 1; // terminal (left of the margin)
  private cells = 1; // tcols * rows
  private noise = new Int16Array(0);
  private nextT = new Float32Array(0);
  private lockedAt = new Float32Array(0);
  private lockKind = new Uint8Array(0); // 1 prompt, 2 written
  // per-frame overlays (finds and banners claim their cells): written with the current stamp
  private ovStamp = new Uint32Array(0);
  private ovGlyph = new Int16Array(0);
  private ovRgb = new Float32Array(0);
  private ovA = new Float32Array(0);
  private ovHalo = new Uint8Array(0);
  private stamp = 0;
  // context text
  private text: number[] = [];
  private tokens: string[] = [];
  private tokenCursor = 0;
  private locked = 0; // cells in use (float, eased)
  private lockedInt = 0;
  private writeAt = 0; // where the next find lands (cells after the frontier)
  private finds: Find[] = [];
  private banners: Banner[] = [];
  private findAcc = 0;
  private vm: ViewModel | null = null;
  private mode: Mode = 'idle';
  private mutMean = 1.5;
  private denoise = 0;
  private bootFrac = 1;
  private sweepAt = -10;
  private faultAt = -10;
  private picture = 0;
  private t = 0;
  private heroCss: FieldRect | null = null;
  private hero = [0, 0, 0, 0];
  private sessionKey = '';
  private lastReqId = -1;
  private jobsSeen = -1;
  private lastLine = '';
  private bannerAt = -10;
  private seed = 7;
  // dormant GPU: is the restore under way (a request waits, or the resident amount climbs), and how far
  private waking = false;
  private thaw = 0;
  private prevUsed = -1;
  private climbing = false;
  private margin: number;
  private reduced = false;

  constructor(
    private canvas: HTMLCanvasElement,
    opts: FieldOptions,
  ) {
    this.margin = opts.margin;
    this.fontPx = opts.fontPx;
    this.compact = !!opts.compact;
    const gl = canvas.getContext('webgl2', { premultipliedAlpha: true, antialias: false, alpha: false });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    canvas.addEventListener('webglcontextlost', this.onLost);
    canvas.addEventListener('webglcontextrestored', this.onRestored);
    this.res = this.initGl();
    this.reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
  }

  // ---- public ---------------------------------------------------------------------------------------------------
  setVm(vm: ViewModel) {
    this.vm = vm;
    this.events(vm);
  }

  /** The hero overlay, in the field's layout px (null = none). */
  setHero(r: FieldRect | null) {
    this.heroCss = r;
    this.hero = r ? [r.x, r.y, r.x + r.w, r.y + r.h].map((v) => v * this.k) : [0, 0, 0, 0];
  }

  /**
   * Size the terminal to its layout box. `pxScale` is a CSS transform scale applied by an ancestor (the mini
   * sheet), so the backing store stays crisp; `fontPx` is the glyph size in layout px.
   */
  resize(cssW: number, cssH: number, pxScale = 1, fontPx = this.fontPx) {
    const k = Math.min(2, window.devicePixelRatio || 1) * pxScale;
    const w = Math.max(1, Math.round(cssW * k));
    const h = Math.max(1, Math.round(cssH * k));
    if (w === this.w && h === this.h && k === this.k && fontPx === this.fontPx && this.cells > 1) return;
    this.k = k;
    this.w = w;
    this.h = h;
    this.fontPx = fontPx;
    this.canvas.width = w;
    this.canvas.height = h;
    const font = fontPx * k;
    this.cw = Math.max(2, Math.round(font * 0.56));
    this.ch = Math.max(3, Math.round(font * 1.34));
    this.cols = Math.max(1, Math.floor(w / this.cw));
    this.rows = Math.max(1, Math.floor(h / this.ch));
    this.tcols = Math.max(1, Math.floor(this.cols * (1 - this.margin)));
    const oldCells = this.cells;
    const oldKind = this.lockKind;
    this.cells = this.tcols * this.rows;
    this.noise = new Int16Array(this.cells);
    this.nextT = new Float32Array(this.cells);
    this.lockedAt = new Float32Array(this.cells).fill(-10);
    this.lockKind = new Uint8Array(this.cells);
    this.ovStamp = new Uint32Array(this.cells);
    this.ovGlyph = new Int16Array(this.cells);
    this.ovRgb = new Float32Array(this.cells * 3);
    this.ovA = new Float32Array(this.cells);
    this.ovHalo = new Uint8Array(this.cells);
    for (let i = 0; i < this.cells; i++) {
      this.noise[i] = this.noiseGlyph(i, 0);
      this.nextT[i] = this.t + hash(i, 3) * 2;
    }
    // keep the context share (and which part was prompt / written) across a new grid, without a glow
    if (oldCells > 1 && oldKind.length) {
      const r = this.cells / oldCells;
      this.locked *= r;
      this.lockedInt = Math.min(this.cells, Math.floor(this.locked));
      for (let i = 0; i < this.lockedInt; i++) this.lockKind[i] = oldKind[Math.min(oldKind.length - 1, Math.floor(i / r))] || 1;
    } else {
      this.locked = 0;
      this.lockedInt = 0;
    }
    this.writeAt = 0;
    this.finds = [];
    this.banners = [];
    this.setHero(this.heroCss);
  }

  /** Advance by dt seconds and draw. A still frame (no scheduler frames) may pass a larger maxDt. */
  tick(dt: number, maxDt = 0.05) {
    if (this.lost || !this.res) return;
    this.frame(clamp(dt, 0, maxDt));
  }

  /** Release every GL resource and the context itself (skins are switched at runtime). */
  destroy() {
    this.canvas.removeEventListener('webglcontextlost', this.onLost);
    this.canvas.removeEventListener('webglcontextrestored', this.onRestored);
    if (this.res && !this.lost) this.freeGl(this.res);
    this.res = null;
    this.gl.getExtension('WEBGL_lose_context')?.loseContext();
    this.canvas.width = 1;
    this.canvas.height = 1;
  }

  private onLost = (e: Event) => {
    e.preventDefault();
    this.lost = true;
    this.res = null;
  };

  private onRestored = () => {
    this.lost = false;
    this.res = this.initGl();
  };

  // ---- view model -> state ----------------------------------------------------------------------------------------
  private modeOf(vm: ViewModel): Mode {
    const s = vm.session;
    if (!s) return 'idle';
    if (s.phase === 'fault') return 'fault';
    if (s.phase === 'stopping') return 'stop';
    if (s.phase === 'starting' || s.phase === 'loading') return 'load';
    if (vm.vram.dormant) return this.waking ? 'wake' : 'sleep';
    if (s.llm?.activity === 'prefill') return 'prefill';
    if (s.llm?.activity === 'decode') return 'decode';
    if (s.image?.activity === 'generating') return 'denoise';
    return 'idle';
  }

  /** Context in use as a share of the terminal. */
  private contextFrac(vm: ViewModel): number {
    const llm = vm.session?.llm;
    if (!llm || llm.context.totalTokens <= 0) return 0;
    return clamp(llm.context.usedTokens / llm.context.totalTokens);
  }

  private events(vm: ViewModel) {
    const s = vm.session;
    const key = s ? `${s.slot}|${s.model.name}` : '';
    if (key !== this.sessionKey) {
      this.sessionKey = key;
      this.seed = Math.floor(hash(key.length, Date.now() & 0xffff) * 1e6);
      this.tokens = tokenRun(this.seed, 9000);
      this.text = [];
      for (const tk of this.tokens) for (const ch of tk) this.text.push(g(ch));
      this.tokenCursor = 0;
      this.lastReqId = -1;
      this.jobsSeen = -1;
      if (s) this.banner([`${s.model.name.toLowerCase()} · ${s.model.quant.toLowerCase()}`, s.model.name.toLowerCase()], ICE, 3);
    }
    const reqs = s?.llm?.requests ?? [];
    const r = reqs[reqs.length - 1];
    if (r && r.id !== this.lastReqId) {
      if (this.lastReqId !== -1) {
        const tps = r.decodeS > 0 ? r.generatedTokens / r.decodeS : 0;
        this.banner([`request ${r.id} done · ${r.generatedTokens} tok · ${tps.toFixed(1)} tok/s`, `#${r.id} done · ${tps.toFixed(1)} tok/s`], ICE, 2.8);
        this.sweepAt = this.t;
      }
      this.lastReqId = r.id;
    }
    const img = s?.image;
    const jobs = img?.imagesThisSession ?? -1;
    if (jobs !== this.jobsSeen) {
      if (this.jobsSeen >= 0 && jobs > this.jobsSeen) {
        this.sweepAt = this.t;
        this.picture++;
        const j = img?.recent[img.recent.length - 1];
        if (j) this.banner([`image ${jobs} done · ${j.seconds.toFixed(1)} s · ${j.width}x${j.height}`, `image ${jobs} · ${j.seconds.toFixed(1)} s`], ICE, 2.8);
      }
      this.jobsSeen = jobs;
    }
    if (s?.phase === 'fault' && this.faultAt < 0) this.faultAt = this.t;
    if (s?.phase !== 'fault') this.faultAt = -10;

    // dormant GPU: waking = a request is waiting, or the resident amount is climbing back
    const used = vm.vram.usedGiB;
    if (this.prevUsed >= 0 && Math.abs(used - this.prevUsed) > 0.004) this.climbing = used > this.prevUsed;
    this.prevUsed = used;
    const d = vm.vram.dormant;
    const inFlight = !!s && ((!!s.llm && s.llm.activity !== 'idle') || (!!s.image && s.image.activity === 'generating'));
    this.waking = !!d && (inFlight || this.climbing);
    this.thaw = d ? clamp(used / Math.max(0.01, used + d.pagedOutGiB)) : 1;

    const line = vm.console.length ? vm.console[vm.console.length - 1] : '';
    if (line && line !== this.lastLine) {
      this.lastLine = line;
      if (this.t - this.bannerAt > 2.5 && this.mode !== 'decode' && this.mode !== 'prefill') {
        const clean = line.replace(/^\s*[\d.]+\s+[IWE]\s+/, '').replace(/\s+/g, ' ').trim().toLowerCase();
        this.banner([clean.slice(0, 60), clean.slice(0, Math.max(8, this.tcols - 4))], s?.phase === 'fault' ? RED : ARC, 2.4);
      }
    }
  }

  // ---- frame ------------------------------------------------------------------------------------------------------
  private frame(dt: number) {
    const vm = this.vm;
    if (!vm || this.cells <= 1) return;
    this.t += dt;
    this.mode = this.modeOf(vm);
    const s = vm.session;

    // how fast the noise churns
    const mean =
      this.mode === 'sleep' ? 1e9 : this.mode === 'wake' ? 2.4 : this.mode === 'fault' ? (this.t - this.faultAt < 1.2 ? 0.06 : 1e9)
      : this.mode === 'prefill' ? 0.45 : this.mode === 'decode' ? 0.7 : this.mode === 'load' ? 0.35
      : this.mode === 'denoise' ? 0.25 + 3 * this.denoise : 1.6;
    this.mutMean = this.reduced ? Math.max(mean, 4) : mean;

    // context frontier: the locked share of the terminal is the context in use (released on a fault or a stop)
    const target = this.mode === 'fault' || this.mode === 'stop' || this.mode === 'load' || !s ? 0 : this.contextFrac(vm) * this.cells;
    const before = this.lockedInt;
    this.locked = approach(this.locked, target, this.mode === 'prefill' ? 5 : 2.2, dt);
    this.lockedInt = Math.min(this.cells, Math.floor(this.locked));
    for (let i = before; i < this.lockedInt; i++) {
      this.lockedAt[i] = this.t;
      this.lockKind[i] = this.mode === 'decode' ? 2 : 1;
    }

    // Krea: the picture emerges; loading: the terminal prints in
    const img = s?.image;
    this.denoise =
      this.mode === 'denoise' && img && img.steps > 0 ? approach(this.denoise, img.step / img.steps, 4, dt)
      : this.mode === 'denoise' ? this.denoise : approach(this.denoise, 0, 1.5, dt);
    this.bootFrac = this.mode === 'load' ? approach(this.bootFrac, 0.15 + 0.85 * (s?.loading?.fraction ?? 0), 3, dt) : approach(this.bootFrac, 1, 2, dt);

    // decode: finds at the token rate (capped so each one can still be seen)
    if (this.mode === 'decode' && s?.llm) {
      const tps = s.llm.decodeTps;
      this.findAcc += Math.min(28, 3 + tps * 0.32) * dt;
      while (this.findAcc >= 1) {
        this.findAcc -= 1;
        this.find(s.llm.spec?.acceptancePct ?? 0);
      }
    } else this.findAcc = 0;
    if ((this.mode === 'idle' || this.mode === 'decode' || this.mode === 'denoise') && this.t - this.bannerAt > 8) this.ambientBanner();

    this.churn();
    this.finds = this.finds.filter((f) => this.t - f.t0 < f.hold + 0.5);
    this.banners = this.banners.filter((b) => this.t - b.t0 < b.hold + 0.6);

    this.n = 0;
    this.drawTerminal();
    this.drawMargin();
    this.render();
  }

  private churn() {
    const t = this.t;
    const mean = this.mutMean;
    if (mean > 1e8) return;
    for (let i = this.lockedInt; i < this.cells; i++) {
      if (t >= this.nextT[i]) {
        this.noise[i] = this.noiseGlyph(i, Math.floor(t * 31));
        this.nextT[i] = t + mean * (0.25 - Math.log(1 - hash(i, Math.floor(t * 17)) * 0.98));
      }
    }
  }

  private noiseGlyph(i: number, salt: number): number {
    const r = hash(i, salt, this.seed);
    return r < 0.12 ? BLANK : pick(NOISE, hash(i, salt, 5));
  }

  /** One token found in the noise just past the frontier; drafts accepted by speculation come in pairs/triples. */
  private find(acceptPct: number) {
    const burst = acceptPct > 0 && hash(this.tokenCursor, 9) < acceptPct / 100 ? 2 + Math.floor(hash(this.tokenCursor, 10) * 2) : 1;
    const band = this.tcols * 3;
    for (let b = 0; b < burst; b++) {
      const tk = this.tokens[(this.tokenCursor++ + 4000) % this.tokens.length];
      const glyphs = [...tk].map(g);
      if (this.writeAt + glyphs.length > band) this.writeAt = 0;
      const i0 = this.lockedInt + this.writeAt;
      this.writeAt += glyphs.length;
      if (i0 + glyphs.length >= this.cells) continue;
      this.finds.push({ i0, text: glyphs, t0: this.t + b * 0.03, hold: 1.1 + hash(i0, 4) * 0.6 });
    }
  }

  /** A line that resolves from the noise somewhere free; the first text that fits wins (short forms last). */
  private banner(texts: string[], tint: RGB, hold: number) {
    if (this.cells <= 1 || this.rows < 3) return;
    const room = this.tcols - 4;
    const fit = texts.find((x) => x.length <= room) ?? (this.compact ? texts[texts.length - 1].slice(0, room) : null);
    if (!fit || fit.length < 4) return;
    const glyphs = [...fit].map(g);
    for (let tries = 0; tries < 10; tries++) {
      const row = 1 + Math.floor(hash(this.seed++, 3) * (this.rows - 2));
      const col = 2 + Math.floor(hash(this.seed++, 5) * Math.max(0, this.tcols - glyphs.length - 4));
      const i0 = row * this.tcols + col;
      if (this.inHero(i0) || this.inHero(i0 + glyphs.length)) continue;
      if (this.banners.some((b) => Math.abs(Math.floor(b.i0 / this.tcols) - row) < 2)) continue;
      this.banners.push({ i0, text: glyphs, t0: this.t, hold, tint });
      this.bannerAt = this.t;
      return;
    }
  }

  private ambientBanner() {
    const vm = this.vm!;
    const s = vm.session;
    const opts: string[][] = [];
    if (s?.llm) {
      const c = s.llm.context;
      const pct = Math.round((c.usedTokens / Math.max(1, c.totalTokens)) * 100);
      opts.push([`context ${c.usedTokens} / ${c.totalTokens} · ${pct}% of the screen`, `context ${pct}%`]);
      if (s.llm.spec) opts.push([`speculative ${s.llm.spec.mode.toLowerCase()} · ${Math.round(s.llm.spec.acceptancePct)}% accepted`, `drafts ${Math.round(s.llm.spec.acceptancePct)}% accepted`]);
    }
    const arch = s?.model.arch;
    if (arch) {
      const ex = arch.experts > 0 ? `${arch.experts} experts · ${arch.expertsUsed} per token` : 'dense';
      opts.push([`${arch.layers} layers · ${ex}`, `${arch.layers} layers`]);
    }
    if (s?.image) opts.push([`${s.image.width}x${s.image.height} · ${s.image.sPerIt.toFixed(2)} s/it`, `${s.image.sPerIt.toFixed(2)} s/it`]);
    opts.push([`vram ${vm.vram.usedGiB.toFixed(2)} / ${vm.vram.totalGiB.toFixed(2)} gib`, `vram ${vm.vram.usedGiB.toFixed(1)} gib`]);
    if (!s) opts.push(['standby · pick a tier and launch', 'standby']);
    this.banner(pick(opts, hash(Math.floor(this.t), 7)), ARC, 2.6);
  }

  private inHero(i: number) {
    const x = (i % this.tcols) * this.cw;
    const y = Math.floor(i / this.tcols) * this.ch;
    const [x0, y0, x1, y1] = this.hero;
    if (x1 <= x0) return false;
    return x + this.cw > x0 - this.cw * 2 && x < x1 + this.cw * 2 && y + this.ch > y0 - this.ch && y < y1 + this.ch;
  }

  private over(i: number, glyph: number, rgb: RGB, a: number, halo: boolean) {
    if (i < 0 || i >= this.cells) return;
    this.ovStamp[i] = this.stamp;
    this.ovGlyph[i] = glyph;
    this.ovRgb[i * 3] = rgb[0];
    this.ovRgb[i * 3 + 1] = rgb[1];
    this.ovRgb[i * 3 + 2] = rgb[2];
    this.ovA[i] = a;
    this.ovHalo[i] = halo ? 1 : 0;
  }

  // ---- drawing ----------------------------------------------------------------------------------------------------
  private drawTerminal() {
    const t = this.t;
    const mode = this.mode;
    const cold = mode === 'sleep' ? 1 : mode === 'wake' ? 1 - this.thaw : 0; // how frozen amber the screen is
    const tint = cold > 0 ? mix(NOISE_RGB, AMBER, cold) : mode === 'fault' ? RED : NOISE_RGB;
    const sweepK = (t - this.sweepAt) / 0.55;
    const sweepRow = sweepK >= 0 && sweepK <= 1 ? sweepK * this.rows : -99;
    const dn = this.denoise;
    const pic = this.picture;

    // overlays: finds and banners claim their cells first
    this.stamp = (this.stamp + 1) >>> 0 || 1;
    for (const f of this.finds) {
      const age = t - f.t0;
      if (age < 0) continue;
      for (let k = 0; k < f.text.length; k++) {
        const i = f.i0 + k;
        let glyph = f.text[k];
        let rgb: RGB = ICE;
        let a = 1;
        const lockAt = 0.09 + k * 0.012;
        if (age < lockAt) {
          if (glyph === BLANK) continue;
          glyph = pick(NOISE, hash(i, Math.floor(t * 40)));
          rgb = ARC;
          a = 0.85;
        } else if (age > f.hold) {
          const k2 = (age - f.hold) / 0.5;
          rgb = mix(WRITTEN_RGB, NOISE_RGB, k2);
          a = 0.9 - k2 * 0.6;
        } else {
          const cool = clamp((age - lockAt) / f.hold);
          rgb = mix(ICE, WRITTEN_RGB, cool * cool);
        }
        this.over(i, glyph, rgb, a, age >= lockAt && age < lockAt + 0.25);
      }
    }
    for (const b of this.banners) {
      const age = t - b.t0;
      for (let k = 0; k < b.text.length; k++) {
        const i = b.i0 + k;
        let glyph = b.text[k];
        let rgb = b.tint;
        let a = 0.95;
        const lockAt = 0.08 + k * 0.014 + hash(i, 2) * 0.16;
        if (age < lockAt) {
          if (glyph === BLANK) continue;
          glyph = pick(NOISE, hash(i, Math.floor(t * 30)));
          rgb = ARC;
          a = 0.6;
        } else if (age > b.hold) {
          const k2 = (age - b.hold) / 0.6;
          if (hash(k, b.i0, 3) < k2) continue;
          a = 0.95 - k2 * 0.6;
        }
        this.over(i, glyph, rgb, a, false);
      }
    }

    const ov = this.ovRgb;
    for (let i = 0; i < this.cells; i++) {
      const row = Math.floor(i / this.tcols);
      const col = i - row * this.tcols;
      const x = col * this.cw;
      const y = row * this.ch;
      if (this.ovStamp[i] === this.stamp) {
        const rgb: RGB = [ov[i * 3], ov[i * 3 + 1], ov[i * 3 + 2]];
        if (this.ovGlyph[i] !== BLANK) this.quad(x, y, this.cw, this.ch, this.ovGlyph[i], rgb, this.ovA[i]);
        if (this.ovHalo[i]) this.halo(x, y, ICE, 0.32);
        continue;
      }
      let glyph: number;
      let rgb: RGB;
      let a: number;
      if (i < this.lockedInt) {
        // context in use: stable text; it glows when it was just taken in
        glyph = this.text[i % this.text.length];
        const age = t - this.lockedAt[i];
        const base = this.lockKind[i] === 2 ? WRITTEN_RGB : PROMPT_RGB;
        const rest = this.lockKind[i] === 2 ? 0.46 : 0.36;
        if (cold > 0) {
          rgb = mix(base, AMBER, 0.75 * cold);
          a = rest + (0.3 - rest) * cold;
        } else {
          rgb = age < 0.9 ? mix(ICE, base, age / 0.9) : base;
          a = age < 0.9 ? 1 - (age / 0.9) * (1 - rest) : rest;
        }
      } else {
        glyph = this.noise[i];
        const r = hash(i, 11);
        if (mode === 'load' && hash(i, 13) > this.bootFrac) continue; // not printed yet
        if (mode === 'denoise' || dn > 0.02) {
          // noise settles into the picture: density-ramp characters take over as the steps progress
          const b = this.picture01(col, row, pic);
          const settled = hash(i, 17) < Math.pow(dn, 1.3);
          if (settled) glyph = RAMP[Math.min(RAMP.length - 1, Math.floor(b * RAMP.length))];
          rgb = settled ? mix(NOISE_RGB, ICE, b * 0.85) : NOISE_RGB;
          a = settled ? 0.25 + b * 0.75 : 0.22 + r * 0.25;
        } else {
          rgb = tint;
          const base = mode === 'idle' ? 0.2 : 0.26;
          a = (cold > 0 ? base + (0.1 - base) * cold : base) + r * 0.22;
        }
      }
      if (sweepRow > -1) {
        const d = Math.abs(row - sweepRow);
        if (d < 2.5) {
          a = Math.min(1, a + (1 - d / 2.5) * 0.6);
          rgb = mix(rgb, ICE, (1 - d / 2.5) * 0.7);
        }
      }
      if (glyph === BLANK) continue;
      this.quad(x, y, this.cw, this.ch, glyph, rgb, a);
    }

    // the cursor at the end of the context (blinks while waiting, solid while writing); image models have none
    if (this.vm?.session?.llm && mode !== 'fault' && mode !== 'load' && mode !== 'stop') {
      const i = Math.min(this.cells - 1, this.lockedInt + (mode === 'decode' ? this.writeAt : 0));
      const on = mode === 'decode' || mode === 'prefill' || Math.floor(t * 1.8) % 2 === 0;
      if (on) {
        const x = (i % this.tcols) * this.cw;
        const y = Math.floor(i / this.tcols) * this.ch;
        const c = cold > 0 ? mix(ICE, AMBER, Math.max(0.6, cold)) : ICE;
        this.quad(x, y, this.cw, this.ch, CURSOR, c, 0.95);
        this.halo(x, y, c, 0.5);
      }
    }
    // the edge between the terminal and memory
    if (this.margin > 0) for (let r = 0; r < this.rows; r++) this.quad(this.tcols * this.cw, r * this.ch, this.cw, this.ch, EDGE, DEEP, 0.45);
  }

  /** Brightness 0..1 of the ASCII picture at a cell: sky, a moon and two ridges (varies per image). */
  private picture01(col: number, row: number, n: number): number {
    const u = col / this.tcols;
    const v = row / this.rows;
    const mx = 0.62 + (hash(n, 1) - 0.5) * 0.4;
    const my = 0.24 + hash(n, 2) * 0.12;
    const aspect = (this.tcols * this.cw) / (this.rows * this.ch);
    const dx = (u - mx) * aspect;
    const dy = v - my;
    const moon = clamp(1 - Math.sqrt(dx * dx + dy * dy) / 0.11);
    const glow = clamp(1 - Math.sqrt(dx * dx + dy * dy) / 0.32) * 0.35;
    const sky = 0.08 + (1 - v) * 0.12;
    const r1 = 0.62 + Math.sin(u * 7.1 + n) * 0.06 + Math.sin(u * 17.3 + n * 2) * 0.025;
    const r2 = 0.76 + Math.sin(u * 4.3 + n * 3) * 0.05 + Math.sin(u * 23.1) * 0.015;
    let b = Math.max(sky + glow, moon > 0 ? 0.55 + moon * 0.45 : 0);
    if (v > r1) b = 0.3 + (v - r1) * 0.4;
    if (v > r2) b = 0.12;
    // water reflection of the moon below the far ridge
    if (v > r2 && Math.abs(dx) < 0.05 + (v - r2) * 0.4 && hash(col, row, Math.floor(this.t * 3)) < 0.5) b = 0.6;
    return clamp(b);
  }

  // ---- VRAM margin ------------------------------------------------------------------------------------------------
  private drawMargin() {
    if (this.margin <= 0) return;
    const vm = this.vm!;
    const x0 = this.tcols + 1;
    const width = this.cols - x0;
    if (width < 8 || this.rows < 3) return;
    const rows = this.rows - 1;
    const total = Math.max(0.01, vm.vram.totalGiB);
    const perRow = total / rows;
    const dz = vm.vram.dormant ?? null;
    const sleeping = !!dz && vm.session?.phase === 'live';
    const fault = vm.session?.phase === 'fault';

    const segs: { label: string; gib: number; rgb: RGB; ghost?: boolean }[] = [];
    const layers: VramLayer[] = [...vm.vram.layers].sort((a, b) => (a.id === 'other' ? -1 : b.id === 'other' ? 1 : 0));
    if (sleeping) {
      let resident = vm.vram.usedGiB;
      for (const l of layers) {
        const r = Math.min(resident, l.gib);
        if (r > 0.005) segs.push({ label: l.label, gib: r, rgb: LAYER_RGB[l.id] ?? STREAM });
        if (l.gib - r > 0.005) segs.push({ label: `${l.label} (paged out)`, gib: l.gib - r, rgb: AMBER, ghost: true });
        resident -= r;
      }
    } else {
      for (const l of layers) segs.push({ label: l.label, gib: l.gib, rgb: LAYER_RGB[l.id] ?? STREAM });
    }
    if (!vm.session) {
      const sel = vm.slots.find((x) => x.id === vm.selected);
      for (const l of sel?.expectedVram ?? []) segs.push({ label: l.label, gib: l.gib, rgb: LAYER_RGB[l.id] ?? STREAM, ghost: true });
    }

    let rowFrom = 0;
    let used = 0;
    for (const seg of segs) {
      const nRows = seg.gib / perRow;
      const r0 = rowFrom;
      const r1 = rowFrom + nRows;
      const label = `${seg.label} ${seg.gib.toFixed(1)}`.toLowerCase();
      const labelRow = nRows >= 1.6 ? Math.min(rows - 1, Math.floor(r1 - 1)) : -1;
      const labelLen = label.length + 3;
      for (let r = Math.floor(r0); r < Math.ceil(r1) && r < rows; r++) {
        const cover = clamp(Math.min(r + 1, r1) - Math.max(r, r0));
        const y = this.rows - 1 - r;
        const cells = Math.round(width * cover);
        for (let c = 0; c < cells; c++) {
          if (r === labelRow && c < labelLen) continue;
          const cx = x0 + c;
          let glyph: number;
          if (seg.ghost) glyph = g('░');
          else if (c % 5 === 4) continue;
          else glyph = HEX[Math.floor(hash(cx, y, Math.floor(this.t * (fault ? 0 : 0.6) + hash(cx, y) * 9)) * 16)];
          const a = seg.ghost ? 0.34 : 0.3 + 0.28 * hash(cx, y, 4);
          this.quadCell(cx, y, glyph, fault ? mix(seg.rgb, RED, 0.6) : seg.rgb, a);
        }
      }
      if (labelRow >= 0) this.textCells(x0 + 1, this.rows - 1 - labelRow, label, seg.ghost ? (sleeping ? AMBER : ARC) : ICE, 0.95);
      rowFrom = r1;
      used += seg.gib;
    }
    const freeGiB = total - used;
    for (let r = Math.ceil(rowFrom); r < rows; r++) {
      const y = this.rows - 1 - r;
      for (let c = 0; c < width; c += 3) this.quadCell(x0 + c, y, g('·'), DEEP, 0.35);
    }
    if (freeGiB > perRow * 2) this.textCells(x0 + 1, this.rows - 1 - Math.floor((rowFrom + rows) / 2), `${freeGiB.toFixed(2)} free`, ARC, 0.85);
    const spill = vm.vram.spillMiB > 0;
    for (let c = 0; c < width; c += spill ? 1 : 2) this.quadCell(x0 + c, 0, g('─'), RED, spill ? 0.95 : 0.55);
    if (spill) this.textCells(x0 + 1, 1, `spill ${(vm.vram.spillMiB / 1024).toFixed(2)} gib`, RED, 1);
  }

  // ---- low level --------------------------------------------------------------------------------------------------
  private textCells(cx: number, cy: number, text: string, rgb: RGB, a: number) {
    let i = 0;
    for (const ch of text) {
      const x = cx + i++;
      if (x >= this.cols) break;
      if (ch !== ' ') this.quadCell(x, cy, g(ch), rgb, a);
    }
  }

  private quadCell(cx: number, cy: number, glyph: number, rgb: RGB, a: number) {
    this.quad(cx * this.cw, cy * this.ch, this.cw, this.ch, glyph, rgb, a);
  }

  private quad(x: number, y: number, w: number, h: number, glyph: number, rgb: RGB, a: number) {
    if (this.n >= CAP || glyph < 0) return;
    const o = this.n++ * STRIDE;
    const d = this.data;
    d[o] = x;
    d[o + 1] = y;
    d[o + 2] = w;
    d[o + 3] = h;
    d[o + 4] = glyph;
    d[o + 5] = rgb[0];
    d[o + 6] = rgb[1];
    d[o + 7] = rgb[2];
    d[o + 8] = a;
  }

  private halo(x: number, y: number, rgb: RGB, a: number) {
    const s = this.ch * 2.6;
    this.quad(x + this.cw / 2 - s / 2, y + this.ch / 2 - s / 2, s, s, HALO, rgb, a);
  }

  private render() {
    const gl = this.gl;
    const r = this.res;
    if (!r) return;
    gl.viewport(0, 0, this.w, this.h);
    gl.clearColor(0.004, 0.016, 0.035, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.useProgram(r.prog);
    gl.bindVertexArray(r.vao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, r.tex);
    gl.uniform2f(r.uRes, this.w, this.h);
    gl.uniform2f(r.uGrid, ATLAS_COLS, this.atlasRows);
    gl.uniform4f(r.uHero, this.hero[0], this.hero[1], this.hero[2], this.hero[3]);
    gl.bindBuffer(gl.ARRAY_BUFFER, r.inst);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, this.data, 0, this.n * STRIDE);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.n);
  }

  // ---- GL setup / teardown ----------------------------------------------------------------------------------------
  private initGl(): GlRes {
    const gl = this.gl;
    const prog = this.program();
    const vao = gl.createVertexArray()!;
    gl.bindVertexArray(vao);
    const quad = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, quad);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    const inst = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, inst);
    gl.bufferData(gl.ARRAY_BUFFER, this.data.byteLength, gl.DYNAMIC_DRAW);
    const F = 4;
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 4, gl.FLOAT, false, STRIDE * F, 0);
    gl.vertexAttribDivisor(1, 1);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 1, gl.FLOAT, false, STRIDE * F, 4 * F);
    gl.vertexAttribDivisor(2, 1);
    gl.enableVertexAttribArray(3);
    gl.vertexAttribPointer(3, 4, gl.FLOAT, false, STRIDE * F, 5 * F);
    gl.vertexAttribDivisor(3, 1);
    gl.useProgram(prog);
    gl.uniform1i(gl.getUniformLocation(prog, 'u_atlas'), 0);
    const tex = this.buildAtlas();
    return {
      prog,
      vao,
      quad,
      inst,
      tex,
      uRes: gl.getUniformLocation(prog, 'u_res'),
      uGrid: gl.getUniformLocation(prog, 'u_grid'),
      uHero: gl.getUniformLocation(prog, 'u_hero'),
    };
  }

  private freeGl(r: GlRes) {
    const gl = this.gl;
    gl.bindVertexArray(null);
    gl.deleteVertexArray(r.vao);
    gl.deleteBuffer(r.quad);
    gl.deleteBuffer(r.inst);
    gl.deleteTexture(r.tex);
    gl.deleteProgram(r.prog);
  }

  private buildAtlas(): WebGLTexture {
    const gl = this.gl;
    const cw = 40;
    const ch = 72;
    const count = CHARS.length + 1;
    this.atlasRows = Math.ceil(count / ATLAS_COLS);
    const c = document.createElement('canvas');
    c.width = ATLAS_COLS * cw;
    c.height = this.atlasRows * ch;
    const x = c.getContext('2d')!;
    x.fillStyle = '#fff';
    x.textAlign = 'center';
    x.textBaseline = 'middle';
    x.font = `500 56px "Iosevka", "JetBrains Mono", Consolas, monospace`;
    CHARS.forEach((chr, i) => {
      const gx = (i % ATLAS_COLS) * cw;
      const gy = Math.floor(i / ATLAS_COLS) * ch;
      switch (chr) {
        case '█':
          x.fillRect(gx + 4, gy + 8, cw - 8, ch - 16);
          break;
        case '░':
          for (let yy = 4; yy < ch - 4; yy += 6) for (let xx = 4 + ((yy / 6) % 2) * 3; xx < cw - 4; xx += 6) x.fillRect(gx + xx, gy + yy, 2, 2);
          break;
        case '─':
          x.fillRect(gx, gy + ch / 2 - 1, cw, 3);
          break;
        case '¦':
          x.fillRect(gx + cw / 2 - 1, gy + 6, 2, ch / 2 - 10);
          x.fillRect(gx + cw / 2 - 1, gy + ch / 2 + 4, 2, ch / 2 - 10);
          break;
        case '·':
          x.beginPath();
          x.arc(gx + cw / 2, gy + ch / 2, 3, 0, Math.PI * 2);
          x.fill();
          break;
        default:
          x.fillText(chr, gx + cw / 2, gy + ch / 2 + 2);
      }
    });
    const hx = (HALO % ATLAS_COLS) * cw;
    const hy = Math.floor(HALO / ATLAS_COLS) * ch;
    const grad = x.createRadialGradient(hx + cw / 2, hy + ch / 2, 0, hx + cw / 2, hy + ch / 2, cw / 2);
    grad.addColorStop(0, 'rgba(255,255,255,0.9)');
    grad.addColorStop(1, 'rgba(255,255,255,0)');
    x.fillStyle = grad;
    x.fillRect(hx, hy, cw, ch);

    const tex = gl.createTexture()!;
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, c);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    c.width = 0;
    c.height = 0;
    return tex;
  }

  private program(): WebGLProgram {
    const gl = this.gl;
    const sh = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? 'shader');
      return s;
    };
    const p = gl.createProgram()!;
    const vs = sh(gl.VERTEX_SHADER, VS);
    const fs = sh(gl.FRAGMENT_SHADER, FS);
    gl.attachShader(p, vs);
    gl.attachShader(p, fs);
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? 'link');
    // the program keeps the compiled code; the shader objects can go
    gl.detachShader(p, vs);
    gl.detachShader(p, fs);
    gl.deleteShader(vs);
    gl.deleteShader(fs);
    return p;
  }
}
