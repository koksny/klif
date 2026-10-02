// Per-model simulation profiles: which real timelines drive a model, how long it takes to boot, and what
// it costs the rest of the machine. Chosen by model FAMILY (the tiers are swappable), not by slot id.
import type { ModelRef, SlotId } from '../model/types';
import type { BootDurations } from './boot';
import { cardFor } from './catalog';
import { HIGH, KREA, LOW, MEDIUM, type BigPrefill, type ReqTuple } from './fixtures';
import type { ImageSimConfig } from './imageSim';
import type { LlmSimConfig } from './llmSim';
import { clamp, type Rng } from './rng';

export type Family = 'qwen27' | 'flashnext' | 'gemma' | 'krea';

export function familyOf(model: ModelRef): Family {
  const n = model.name.toLowerCase();
  if (n.includes('krea')) return 'krea';
  if (n.includes('flash')) return 'flashnext';
  if (n.includes('gemma')) return 'gemma';
  return 'qwen27';
}

/** Spec label shown in LlmLive.spec: "MTP+ngram" -> "MTP + n-gram"; "off"/undefined -> null. */
export function specLabel(mode: string | undefined): string | null {
  if (!mode || mode === 'off') return null;
  return mode.replace(/\+/g, ' + ').replace(/ngram/gi, 'n-gram');
}

/** Gemma 4 has no session logs yet. Its decode speed is measured (benchmark summary); the request mix is the
 *  27B profile's, scaled to a 16k context and the measured speed. Prefill speed is an ESTIMATE. */
function gemmaSessions(speedScale: number, ctxTotal: number): ReqTuple[][] {
  const src = MEDIUM.sessions.filter((s) => s.length >= 30);
  const tpsLong = LOW.decodeTpsAt128k ?? 76.4;
  const tpsShort = Math.max(LOW.decodeTpsAt32k ?? 91.8, 92.5);
  const prefillPlateau = 2200 * speedScale; // estimate
  const med = (a: number[]) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];
  return src.slice(0, 3).map((session) => {
    const medDecode = med(session.map((r) => r.decodeTps));
    const medPrefill = med(session.map((r) => r.prefillTps));
    return session.map((r) => {
      const newTokens = clamp(Math.round(r.newTokens * 0.18), 1, Math.round(ctxTotal * 0.55));
      const genTokens = clamp(Math.round(r.genTokens * 0.22), 8, 2200);
      const cached = clamp(Math.round(r.cachedTokens * 0.18), 0, ctxTotal - newTokens - genTokens - 128);
      const ctxAfter = cached + newTokens + genTokens;
      const ctxFrac = ctxAfter / 131072;
      const decode = (tpsShort - (tpsShort - tpsLong) * clamp(ctxFrac * 8, 0, 1)) * speedScale;
      return {
        gapS: r.gapS,
        newTokens,
        cachedTokens: cached,
        prefillTps: Math.round(prefillPlateau * Math.pow(clamp(r.prefillTps / medPrefill, 0.4, 1.6), 0.5) * 10) / 10,
        genTokens,
        decodeTps: Math.round(clamp(decode * clamp(0.9 + 0.1 * (r.decodeTps / medDecode), 0.9, 1.1), 20, 100) * 100) / 100,
        accept: -1,
        meanLen: 0,
        ctxAfter,
      };
    });
  });
}

export function llmConfig(model: ModelRef, opts: { big?: boolean } = {}): LlmSimConfig {
  const fam = familyOf(model);
  const ctxTotal = model.ctxTokens ?? cardFor(model)?.defaultCtx ?? 16384;
  const spec = specLabel(model.specMode);
  switch (fam) {
    case 'flashnext':
      return {
        ctxTotal,
        sessions: HIGH.sessions,
        ramp: HIGH.prefillRamp.rampByDecile,
        jitter: HIGH.decodeJitter,
        gapCapS: 90,
        specMode: spec,
        big: opts.big ? bigPrefill() : null,
        logEveryTokens: 8192,
      };
    case 'gemma': {
      const scale = /12B/i.test(model.name) ? 0.72 : 1;
      return {
        ctxTotal,
        sessions: gemmaSessions(scale, ctxTotal),
        ramp: MEDIUM.prefillRamp.rampByDecile,
        jitter: MEDIUM.decodeJitter,
        gapCapS: 90,
        specMode: spec,
        logEveryTokens: 1024,
      };
    }
    default:
      return {
        ctxTotal,
        sessions: MEDIUM.sessions,
        ramp: MEDIUM.prefillRamp.rampByDecile,
        jitter: MEDIUM.decodeJitter,
        gapCapS: 90,
        specMode: spec,
        big: opts.big ? bigPrefill() : null,
        logEveryTokens: 1024,
      };
  }
}

/** The real 66k prefill; its log was cut before the draft statistics, so acceptance is the session median. */
function bigPrefill(): BigPrefill | null {
  const b = HIGH.bigPrefill;
  if (!b) return null;
  return b.accept >= 0 ? b : { ...b, accept: HIGH.acceptQuantiles?.[2] ?? 0.79 };
}

export function imageConfig(crashOnJob?: number): ImageSimConfig {
  return {
    jobs: KREA.jobs,
    editShare: KREA.editShare,
    // The measured duty cycle (<= 33% busy) implies ~40 s between jobs; the mock waits ~18 s so a skin sees activity.
    meanGapS: 18,
    crashOnJob,
  };
}

const pickFrom = (rng: Rng, a: number[], fallback: number) => (a.length ? rng.pick(a) : fallback);

/** Boot step durations. weights/kv come from real log timestamps; process/device/warmup are estimates. */
export function bootDurations(model: ModelRef, rng: Rng): BootDurations {
  const fam = familyOf(model);
  const processS = rng.range(0.9, 1.5);
  const deviceS = rng.range(1.2, 2.0);
  const warmupS = rng.range(0.5, 1.1);
  switch (fam) {
    case 'flashnext':
      return { processS, deviceS, weightsS: pickFrom(rng, HIGH.load.weightsS, 12), kvS: pickFrom(rng, HIGH.load.auxS, 1) + 0.4, warmupS };
    case 'gemma': {
      const total = pickFrom(rng, LOW.loadTotalS ?? [], 12.2);
      return { processS, deviceS, weightsS: total * 0.9, kvS: total * 0.1 + 0.4, warmupS };
    }
    case 'krea':
      // No timestamps in the image-server log: estimates (weights live in RAM, text encoder + VAE follow).
      return { processS, deviceS, weightsS: rng.range(7, 10), kvS: rng.range(2, 3.4), warmupS };
    default:
      return { processS, deviceS, weightsS: pickFrom(rng, MEDIUM.load.weightsS, 6.7), kvS: pickFrom(rng, MEDIUM.load.auxS, 3), warmupS };
  }
}

export function deviceDetail(model: ModelRef): string {
  // The device detail is whatever the real core reports at runtime; the mock shows a generic one.
  return 'ROCm · gfx1201';
}

/** Resident system RAM a running model adds on top of the idle desktop (GiB). */
export function ramFootprintGiB(model: ModelRef): number {
  switch (familyOf(model)) {
    case 'krea':
      return 11.7; // from the image-server log: all parameters resident in RAM
    case 'flashnext':
      return 9.5;
    case 'gemma':
      return 1.4;
    default:
      return model.vision ? 7.2 : 5.4;
  }
}

export function defaultPort(slot: SlotId): number {
  return slot === 'krea' ? 1234 : 7030;
}
