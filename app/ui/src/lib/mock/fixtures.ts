// Typed access to the numeric fixtures extracted from real session logs (see tools/extract-fixtures.mjs).
import mediumJson from './fixtures/llm-medium.json';
import highJson from './fixtures/llm-high.json';
import kreaJson from './fixtures/image-krea.json';
import lowJson from './fixtures/llm-low.json';

/** One real request: what the server logged for one prompt. accept = -1 means no speculative decoding. */
export interface ReqTuple {
  gapS: number;
  newTokens: number;
  cachedTokens: number;
  prefillTps: number;
  genTokens: number;
  decodeTps: number;
  accept: number;
  meanLen: number;
  ctxAfter: number;
}

export interface BigPrefill {
  promptTokens: number;
  processedTokens: number;
  prefillTpsPlateau: number;
  prefillS: number;
  /** [doneTokens, elapsedS] checkpoints from the real progress lines. */
  progress: [number, number][];
  decodeTps: number;
  genTokens: number;
  accept: number;
  /** Decode tps, 3 s windows, oldest first. */
  decodeSeries: number[];
}

export interface LlmFixture {
  provenance: Record<string, unknown>;
  load: { weightsS: number[]; auxS: number[]; totalS: number[] };
  prefillRamp: { rampByDecile: number[]; firstProgressDelayS: number };
  decodeJitter: { ratioMean: number; ratioSd: number; ar1: number };
  idleGapQuantilesS: number[];
  /** Speculative acceptance quantiles [p05, p25, p50, p75, p95] over requests with >= 50 drafted tokens. */
  acceptQuantiles?: number[];
  sessions: ReqTuple[][];
  bigPrefill?: BigPrefill | null;
}

/** [width, height, edit(0|1), steps, prepS, condS, firstStepS, restStepS, decodeS, totalS] */
export type KreaJobTuple = [number, number, number, number, number, number, number, number, number, number];

export interface KreaFixture {
  provenance: Record<string, unknown>;
  editShare: number;
  dutyCycle: number | null;
  jobs: KreaJobTuple[];
}

export const MEDIUM = mediumJson as unknown as LlmFixture;
export const HIGH = highJson as unknown as LlmFixture;
export const KREA = kreaJson as unknown as KreaFixture;

export const LOW = lowJson as unknown as {
  provenance: Record<string, unknown>;
  decodeTpsAt128k?: number | null;
  decodeTpsAt32k?: number | null;
  loadTotalS?: number[];
};
