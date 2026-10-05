// A small QR Code encoder (ISO/IEC 18004): byte mode, versions 1-40, error correction L/M/Q/H.
// The settings drawer draws the pairing code with it; it has no dependencies.

export type Ecc = 'L' | 'M' | 'Q' | 'H';

// Index = version (slot 0 is unused). Error correction codewords in each block, and the number of blocks.
const ECC_LEN: Record<Ecc, number[]> = {
  L: [
    0, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30,
    30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
  ],
  M: [
    0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28,
    28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
  ],
  Q: [
    0, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30,
    30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
  ],
  H: [
    0, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30,
    30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30,
  ],
};
const BLOCKS: Record<Ecc, number[]> = {
  L: [
    0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18,
    19, 19, 20, 21, 22, 24, 25,
  ],
  M: [
    0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31,
    33, 35, 37, 38, 40, 43, 45, 47, 49,
  ],
  Q: [
    0, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40,
    43, 45, 48, 51, 53, 56, 59, 62, 65, 68,
  ],
  H: [
    0, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48,
    51, 54, 57, 60, 63, 66, 70, 74, 77, 81,
  ],
};
// The two-bit level code that goes into the format information.
const ECC_BITS: Record<Ecc, number> = { L: 1, M: 0, Q: 3, H: 2 };

const MASKS: ((x: number, y: number) => boolean)[] = [
  (x, y) => (x + y) % 2 === 0,
  (_x, y) => y % 2 === 0,
  (x) => x % 3 === 0,
  (x, y) => (x + y) % 3 === 0,
  (x, y) => (Math.floor(y / 2) + Math.floor(x / 3)) % 2 === 0,
  (x, y) => ((x * y) % 2) + ((x * y) % 3) === 0,
  (x, y) => (((x * y) % 2) + ((x * y) % 3)) % 2 === 0,
  (x, y) => (((x + y) % 2) + ((x * y) % 3)) % 2 === 0,
];

// GF(256) with x^8 + x^4 + x^3 + x^2 + 1; the exp table is doubled so a product needs no modulo.
const EXP = new Uint8Array(512);
const LOG = new Uint8Array(256);
{
  let v = 1;
  for (let i = 0; i < 255; i++) {
    EXP[i] = v;
    LOG[v] = i;
    v <<= 1;
    if (v & 0x100) v ^= 0x11d;
  }
  for (let i = 255; i < 512; i++) EXP[i] = EXP[i - 255];
}
const gfMul = (a: number, b: number): number => (a && b ? EXP[LOG[a] + LOG[b]] : 0);

/** Coefficients (highest power first) of (x - a^0)(x - a^1)...(x - a^(degree-1)). */
function rsGenerator(degree: number): number[] {
  let gen = [1];
  for (let i = 0; i < degree; i++) {
    const next = new Array<number>(gen.length + 1).fill(0);
    gen.forEach((c, j) => {
      next[j] ^= c;
      next[j + 1] ^= gfMul(c, EXP[i]);
    });
    gen = next;
  }
  return gen;
}

/** Remainder of data(x) * x^degree divided by the generator: the error correction codewords. */
function rsRemainder(data: number[], gen: number[]): number[] {
  const rem = new Array<number>(gen.length - 1).fill(0);
  for (const byte of data) {
    const factor = byte ^ (rem.shift() as number);
    rem.push(0);
    for (let i = 0; i < rem.length; i++) rem[i] ^= gfMul(gen[i + 1], factor);
  }
  return rem;
}

const sizeOf = (version: number): number => 17 + 4 * version;

/** Alignment pattern centre coordinates for a version (empty for version 1). */
function alignCoords(version: number): number[] {
  if (version === 1) return [];
  const count = Math.floor(version / 7) + 2;
  const last = sizeOf(version) - 7;
  // Even spacing counted back from the last centre; version 32 is the one irregular case in the standard.
  const step = version === 32 ? 26 : 2 * Math.ceil((last - 6) / (count - 1) / 2);
  const out = [6];
  for (let i = count - 2; i >= 0; i--) out.push(last - i * step);
  return out;
}

/** Modules available to codewords: everything minus finders, timing, alignment, format and version areas. */
function rawModules(version: number): number {
  let n = (16 * version + 128) * version + 64; // symbol minus finders, separators, timing and format areas
  if (version >= 2) {
    const a = alignCoords(version).length;
    n -= 25 * a * a - 10 * a - 55; // alignment patterns, less their overlap with the timing lines
  }
  if (version >= 7) n -= 36; // two version information blocks
  return n;
}

const dataCodewords = (version: number, ecc: Ecc): number =>
  (rawModules(version) >> 3) - BLOCKS[ecc][version] * ECC_LEN[ecc][version];

/** Mode, length, payload, terminator and pad bytes, then Reed-Solomon blocks interleaved. */
function makeCodewords(bytes: Uint8Array, version: number, ecc: Ecc): number[] {
  const total = rawModules(version) >> 3;
  const blocks = BLOCKS[ecc][version];
  const eccLen = ECC_LEN[ecc][version];
  const capacity = (total - blocks * eccLen) * 8;

  const bits: number[] = [];
  const put = (value: number, count: number): void => {
    for (let i = count - 1; i >= 0; i--) bits.push((value >>> i) & 1);
  };
  put(0b0100, 4);
  put(bytes.length, version < 10 ? 8 : 16);
  bytes.forEach((b) => put(b, 8));
  put(0, Math.min(4, capacity - bits.length));
  put(0, (8 - (bits.length % 8)) % 8);
  for (let pad = 0xec; bits.length < capacity; pad = pad === 0xec ? 0x11 : 0xec) put(pad, 8);

  const data: number[] = [];
  for (let i = 0; i < bits.length; i += 8) {
    data.push(parseInt(bits.slice(i, i + 8).join(''), 2));
  }

  // Short blocks come first; the long ones carry one more data codeword.
  const shortBlocks = blocks - (total % blocks);
  const shortLen = Math.floor(total / blocks) - eccLen;
  const gen = rsGenerator(eccLen);
  const dataBlocks: number[][] = [];
  const eccBlocks: number[][] = [];
  let at = 0;
  for (let i = 0; i < blocks; i++) {
    const part = data.slice(at, (at += shortLen + (i < shortBlocks ? 0 : 1)));
    dataBlocks.push(part);
    eccBlocks.push(rsRemainder(part, gen));
  }
  const out: number[] = [];
  for (let i = 0; i <= shortLen; i++) for (const b of dataBlocks) if (i < b.length) out.push(b[i]);
  for (let i = 0; i < eccLen; i++) for (const b of eccBlocks) out.push(b[i]);
  return out;
}

interface Grid {
  size: number;
  dark: boolean[][]; // [y][x]
  fixed: boolean[][]; // function pattern modules: not data, never masked
}

function setModule(grid: Grid, x: number, y: number, dark: boolean): void {
  grid.dark[y][x] = dark;
  grid.fixed[y][x] = true;
}

/** Format information (level + mask, BCH(15,5), xor-masked) in both of its places, plus the always-dark module. */
function drawFormat(grid: Grid, ecc: Ecc, mask: number): void {
  const data = (ECC_BITS[ecc] << 3) | mask;
  let rem = data;
  for (let i = 0; i < 10; i++) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
  const bits = ((data << 10) | rem) ^ 0x5412;
  const n = grid.size;
  const bit = (i: number): boolean => ((bits >>> i) & 1) === 1;
  for (let i = 0; i < 15; i++) {
    // First copy around the top-left finder, skipping the timing lines at 6.
    if (i < 6) setModule(grid, 8, i, bit(i));
    else if (i < 8) setModule(grid, 8, i + 1, bit(i));
    else if (i === 8) setModule(grid, 7, 8, bit(i));
    else setModule(grid, 14 - i, 8, bit(i));
    // Second copy split between the top-right and bottom-left finders.
    if (i < 8) setModule(grid, n - 1 - i, 8, bit(i));
    else setModule(grid, 8, n - 15 + i, bit(i));
  }
  setModule(grid, 8, n - 8, true);
}

/** Version information (BCH(18,6)) in the two 6x3 blocks next to the top-right and bottom-left finders. */
function drawVersion(grid: Grid, version: number): void {
  let rem = version;
  for (let i = 0; i < 12; i++) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25);
  const bits = (version << 12) | rem;
  const n = grid.size;
  for (let i = 0; i < 18; i++) {
    const dark = ((bits >>> i) & 1) === 1;
    const a = n - 11 + (i % 3);
    const b = Math.floor(i / 3);
    setModule(grid, a, b, dark);
    setModule(grid, b, a, dark);
  }
}

function drawFunctionPatterns(grid: Grid, version: number): void {
  const n = grid.size;
  for (let i = 0; i < n; i++) {
    setModule(grid, i, 6, i % 2 === 0);
    setModule(grid, 6, i, i % 2 === 0);
  }
  // Finder with its one-module separator, clipped by the symbol edge.
  for (const [cx, cy] of [[3, 3], [n - 4, 3], [3, n - 4]]) {
    for (let dy = -4; dy <= 4; dy++) {
      for (let dx = -4; dx <= 4; dx++) {
        const x = cx + dx;
        const y = cy + dy;
        const ring = Math.max(Math.abs(dx), Math.abs(dy));
        if (x >= 0 && x < n && y >= 0 && y < n) setModule(grid, x, y, ring !== 2 && ring !== 4);
      }
    }
  }
  const pos = alignCoords(version);
  for (const cy of pos) {
    for (const cx of pos) {
      // The three positions that would land on a finder are skipped.
      const onFinder = (cx === 6 && (cy === 6 || cy === n - 7)) || (cy === 6 && cx === n - 7);
      if (onFinder) continue;
      for (let dy = -2; dy <= 2; dy++) {
        for (let dx = -2; dx <= 2; dx++) setModule(grid, cx + dx, cy + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
      }
    }
  }
  drawFormat(grid, 'L', 0); // placeholder: reserves the format areas
  if (version >= 7) drawVersion(grid, version);
}

/** Zigzag over two-column strips from the bottom-right, most significant bit first; leftover bits stay light. */
function placeData(grid: Grid, codewords: number[]): void {
  const n = grid.size;
  let i = 0;
  let up = true;
  for (let right = n - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5; // the vertical timing line takes column 6
    for (let k = 0; k < n; k++) {
      const y = up ? n - 1 - k : k;
      for (const x of [right, right - 1]) {
        if (grid.fixed[y][x]) continue;
        grid.dark[y][x] = i < codewords.length * 8 && ((codewords[i >> 3] >>> (7 - (i & 7))) & 1) === 1;
        i++;
      }
    }
    up = !up;
  }
}

/** Penalty rules N1 to N4: runs, 2x2 blocks, finder-like patterns, dark/light balance. */
function penalty(m: boolean[][]): number {
  const n = m.length;
  let score = 0;
  let darkCount = 0;
  for (let pass = 0; pass < 2; pass++) {
    for (let a = 0; a < n; a++) {
      let run = 0;
      let last = false;
      let window = 0;
      for (let b = 0; b < n; b++) {
        const c = pass === 0 ? m[a][b] : m[b][a];
        if (b > 0 && c === last) {
          run++;
          if (run === 5) score += 3;
          else if (run > 5) score += 1;
        } else {
          run = 1;
        }
        last = c;
        // 1:1:3:1:1 finder-like pattern with four light modules on one side, as an 11-module window in the symbol.
        window = ((window << 1) | (c ? 1 : 0)) & 0x7ff;
        if (b >= 10 && (window === 0x5d0 || window === 0x05d)) score += 40;
        if (pass === 0 && c) darkCount++;
      }
    }
  }
  for (let y = 0; y < n - 1; y++) {
    for (let x = 0; x < n - 1; x++) {
      const c = m[y][x];
      if (c === m[y][x + 1] && c === m[y + 1][x] && c === m[y + 1][x + 1]) score += 3;
    }
  }
  // One step of ten points for every full 5% the dark share is away from 50%.
  return score + 10 * Math.floor(Math.abs(20 * darkCount - 10 * n * n) / (n * n));
}

/**
 * The QR symbol for `text` (UTF-8, byte mode), smallest version that fits, best mask by the standard penalty rules.
 * Rows of modules, true = dark, no quiet zone. Throws an Error with a plain sentence when the text is too long.
 */
export function encodeQr(text: string, ecc: Ecc = 'M'): boolean[][] {
  const bytes = new TextEncoder().encode(text);
  // The mode (4 bits) and the length field (8 bits up to version 9, else 16) come before the payload.
  const fits = (v: number): boolean => 4 + (v < 10 ? 8 : 16) + bytes.length * 8 <= dataCodewords(v, ecc) * 8;
  let version = 1;
  while (version <= 40 && !fits(version)) version++;
  if (version > 40) {
    const max = Math.floor((dataCodewords(40, ecc) * 8 - 4 - 16) / 8);
    throw new Error(
      `The text is too long for a QR code: the limit is ${max} bytes at level ${ecc} and it has ${bytes.length}.`,
    );
  }

  const n = sizeOf(version);
  const base: Grid = {
    size: n,
    dark: Array.from({ length: n }, () => new Array<boolean>(n).fill(false)),
    fixed: Array.from({ length: n }, () => new Array<boolean>(n).fill(false)),
  };
  drawFunctionPatterns(base, version);
  placeData(base, makeCodewords(bytes, version, ecc));

  let best: boolean[][] = [];
  let bestScore = Infinity;
  for (let mask = 0; mask < 8; mask++) {
    const grid: Grid = { size: n, dark: base.dark.map((row) => row.slice()), fixed: base.fixed };
    for (let y = 0; y < n; y++) {
      for (let x = 0; x < n; x++) if (!grid.fixed[y][x] && MASKS[mask](x, y)) grid.dark[y][x] = !grid.dark[y][x];
    }
    drawFormat(grid, ecc, mask);
    const score = penalty(grid.dark);
    if (score < bestScore) {
      bestScore = score;
      best = grid.dark;
    }
  }
  return best;
}

/**
 * One SVG path `d` string drawing every dark module as a unit square (a horizontal run is one rectangle),
 * coordinates offset by `margin` modules.
 */
export function qrPath(modules: boolean[][], margin = 4): string {
  let d = '';
  modules.forEach((row, y) => {
    for (let x = 0; x < row.length; ) {
      if (!row[x]) {
        x++;
        continue;
      }
      const start = x;
      while (x < row.length && row[x]) x++;
      d += `M${start + margin} ${y + margin}h${x - start}v1h${start - x}z`;
    }
  });
  return d;
}
