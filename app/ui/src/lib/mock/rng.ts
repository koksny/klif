// Seeded PRNG (mulberry32) so every mock run is reproducible. Math.random is never used for data.

export interface Rng {
  /** Uniform [0, 1). */
  next(): number;
  /** Standard normal (Box-Muller). */
  gauss(): number;
  range(a: number, b: number): number;
  /** Integer in [a, b] inclusive. */
  int(a: number, b: number): number;
  pick<T>(arr: readonly T[]): T;
  chance(p: number): boolean;
  /** Exponential with the given mean. */
  exp(mean: number): number;
}

export function hashSeed(text: string): number {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619) >>> 0;
  }
  return h >>> 0;
}

export function createRng(seed: number): Rng {
  let a = seed >>> 0 || 0x9e3779b9;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  let spare: number | null = null;
  const gauss = () => {
    if (spare !== null) {
      const s = spare;
      spare = null;
      return s;
    }
    let u = 0;
    let v = 0;
    while (u === 0) u = next();
    while (v === 0) v = next();
    const m = Math.sqrt(-2 * Math.log(u));
    spare = m * Math.sin(2 * Math.PI * v);
    return m * Math.cos(2 * Math.PI * v);
  };
  return {
    next,
    gauss,
    range: (lo, hi) => lo + (hi - lo) * next(),
    int: (lo, hi) => lo + Math.floor(next() * (hi - lo + 1)),
    pick: (arr) => arr[Math.floor(next() * arr.length)],
    chance: (p) => next() < p,
    exp: (mean) => -mean * Math.log(1 - next()),
  };
}

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
export const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
export const smooth01 = (t: number) => {
  const x = clamp(t, 0, 1);
  return x * x * (3 - 2 * x);
};
