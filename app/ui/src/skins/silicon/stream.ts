// Token stream on the die: 64 compute-unit tiles (4 shader engines x 16 CUs, the real RX 9070 XT
// count). 1 tile = 1 token.
//
// - decode: every generated token lights the next tile at the real decode rate. The head is
//   anchored to session.llm.generatedTokens and only extrapolated between telemetry snapshots at
//   the reported decodeTps (never more than EXTRAPOLATE_MS past the last snapshot).
// - prefill: tiles fill steadily in the same order by prefill progress (doneTokens / tokens).
// - image jobs: tiles fill the same way by sampling progress (step / steps).
// - idle: nothing new lights; tiles fade out.
//
// Token k goes to shader engine k % 4 (round robin, like a dispatcher), CU slot floor(k / 4) % 16;
// slots are ordered outward from the die centre (see die.ts), so the stream flows out of the cache.
import type { LlmLive } from '../../lib/model/types';

export const SE_COUNT = 4;
export const CU_PER_SE = 16;
export const TILE_COUNT = SE_COUNT * CU_PER_SE;

/** A tile holds full glow for HOLD_MS after its token, then decays (time constant DECAY_MS). */
const HOLD_MS = 280;
const DECAY_MS = 380;
const FADE_CUTOFF_MS = 3000;
/** Max time the head may run ahead of the last snapshot at the reported rate. */
const EXTRAPOLATE_MS = 8000;
const PREFILL_LEVEL = 0.62;

/** Tile id (se * 16 + slot) for token number k. */
export function tileForToken(k: number): number {
  const m = ((Math.floor(k) % TILE_COUNT) + TILE_COUNT) % TILE_COUNT;
  const se = m % SE_COUNT;
  const slot = Math.floor(m / SE_COUNT);
  return se * CU_PER_SE + slot;
}

export class TokenStream {
  /** performance.now() time each tile was last lit by a decoded token. */
  readonly litAt = new Float64Array(TILE_COUNT).fill(-1e12);
  /** Steady prefill fill level per tile (0 or PREFILL_LEVEL). */
  readonly steady = new Float32Array(TILE_COUNT);

  private mode: 'idle' | 'steady' | 'decode' = 'idle';
  private head = 0;
  private synced = false;
  private rate = 0;
  private anchorG = 0;
  private anchorT = 0;

  /**
   * Feed a telemetry snapshot. `now` is performance.now().
   * `jobFill` (0..1) is the progress of a non-token job (image sampling steps); tiles then fill in
   * stream order by that fraction, exactly like prefill.
   */
  update(llm: LlmLive | null, now: number, live: boolean, jobFill: number | null = null) {
    if (jobFill !== null) {
      this.fill(jobFill);
      return;
    }
    const activity = live && llm ? llm.activity : 'idle';

    if (activity === 'prefill' && llm?.prefill) {
      const p = llm.prefill;
      this.fill(p.tokens > 0 ? p.doneTokens / p.tokens : 0);
      return;
    }

    // Leaving prefill / a job: hand the steady fill over to the normal fade.
    if (this.mode === 'steady') this.releaseSteady(now);

    if (activity === 'decode' && llm) {
      const g = llm.generatedTokens;
      this.rate = Math.max(0, llm.decodeTps);
      if (!this.synced || g < this.anchorG || Math.abs(g - this.head) > TILE_COUNT) {
        // First sight of this stream (or a new request): place the head at the real count and
        // back-fill the recent tokens at the reported rate so the trail is already established.
        this.head = g;
        if (this.rate > 0) {
          const n = Math.min(TILE_COUNT, Math.floor(g), Math.ceil((this.rate * FADE_CUTOFF_MS) / 1000));
          for (let i = 0; i < n; i++) {
            this.litAt[tileForToken(g - 1 - i)] = now - ((i + 0.5) / this.rate) * 1000;
          }
        }
        this.synced = true;
      }
      this.anchorG = g;
      this.anchorT = now;
      this.mode = 'decode';
      return;
    }

    this.mode = 'idle';
    this.rate = 0;
    this.synced = false;
  }

  private fill(frac: number) {
    const n = Math.round(Math.min(1, Math.max(0, Number.isFinite(frac) ? frac : 0)) * TILE_COUNT);
    this.steady.fill(0);
    for (let k = 0; k < n; k++) this.steady[tileForToken(k)] = PREFILL_LEVEL;
    this.mode = 'steady';
    this.synced = false;
  }

  private releaseSteady(now: number) {
    for (let t = 0; t < TILE_COUNT; t++) {
      if (this.steady[t] > 0) {
        // Equivalent glow age for the steady level, so it decays continuously.
        this.litAt[t] = Math.max(this.litAt[t], now - HOLD_MS + DECAY_MS * Math.log(this.steady[t]));
        this.steady[t] = 0;
      }
    }
  }

  /** Advance the head to `now`. Returns true while anything is still moving or glowing. */
  step(now: number): boolean {
    if (this.mode === 'decode' && this.rate > 0) {
      const ahead = Math.min(now - this.anchorT, EXTRAPOLATE_MS);
      const target = this.anchorG + (this.rate * ahead) / 1000;
      if (target > this.head) {
        // Close the gap smoothly (tokens are lit in order, never skipped silently).
        const next = this.head + Math.max(0, target - this.head) * 0.5 + 0.0001;
        const nextHead = target - this.head < 1 ? target : next;
        const from = Math.floor(this.head) + 1;
        const to = Math.floor(nextHead);
        const span = Math.max(1e-6, nextHead - this.head);
        for (let k = from; k <= to; k++) {
          // Spread the lighting moments across the frame interval.
          this.litAt[tileForToken(k - 1)] = now - ((nextHead - k) / span) * 16;
        }
        this.head = nextHead;
      }
    }
    return this.active(now);
  }

  active(now: number): boolean {
    if (this.mode === 'decode' && this.rate > 0 && now - this.anchorT < EXTRAPOLATE_MS + FADE_CUTOFF_MS) return true;
    for (let t = 0; t < TILE_COUNT; t++) if (now - this.litAt[t] < FADE_CUTOFF_MS) return true;
    return false;
  }

  /** 0..1 glow of a tile. */
  brightness(tile: number, now: number): number {
    const age = now - this.litAt[tile];
    const glow = age < HOLD_MS ? 1 : age < FADE_CUTOFF_MS ? Math.exp(-(age - HOLD_MS) / DECAY_MS) : 0;
    return Math.max(glow, this.steady[tile]);
  }
}
