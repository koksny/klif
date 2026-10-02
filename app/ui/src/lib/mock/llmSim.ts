// LLM request simulator. Replays REAL request timelines (token counts, speeds, gaps, speculative
// acceptance) from the numeric fixtures, with seeded jitter. It produces the LlmLive part of a
// ViewModel plus console lines in the llama.cpp log format.
import type { LlmLive, RequestRecord } from '../model/types';
import type { BigPrefill, ReqTuple } from './fixtures';
import { llamaLine, padNum } from './consoleBuf';
import type { ConsoleBuf } from './consoleBuf';
import { clamp, type Rng } from './rng';

export interface LlmSimConfig {
  ctxTotal: number;
  /** Replay pool: one array of real requests per real session. */
  sessions: ReqTuple[][];
  /** Normalised prefill rate by progress decile (instantaneous / average). */
  ramp: number[];
  jitter: { ratioMean: number; ratioSd: number; ar1: number };
  /** Longest idle gap the mock waits, in seconds (real gaps reach many minutes). */
  gapCapS: number;
  /** "MTP + n-gram" style label, or null when the model runs without speculative decoding. */
  specMode: string | null;
  /** Start the session with this real long prefill instead of the replay. */
  big?: BigPrefill | null;
  /** Rate of the first progress log: batch size in tokens (1024 for the 27B profile, 8192 for Flash-Next). */
  logEveryTokens: number;
  /**
   * Scripted cadence instead of the replayed gaps: short requests only, one arrival every ~periodS, on a
   * server that is already warm (no empty-cache first prompt). Used by the dormant-GPU scenarios.
   */
  paced?: PacedCfg | null;
}

export interface PacedCfg {
  /** Seconds between request ARRIVALS (the wake-up wait counts towards it). */
  periodS: number;
  /** Seeded +/- jitter on each period. */
  jitterS: number;
  /** The first request arrives this long after the session goes live. */
  firstInS: number;
  maxNewTokens: number;
  maxGenTokens: number;
}

interface Req {
  id: number;
  taskId: number;
  cached: number;
  newTokens: number;
  gen: number;
  prefillS: number;
  decodeTps: number;
  accept: number;
  meanLen: number;
  sim: number;
  /** Real checkpoints for the big prefill, else null. */
  checkpoints: [number, number][] | null;
  decodeSeries: number[] | null;
  // runtime
  pT: number;
  doneNew: number;
  dT: number;
  doneGen: number;
  loggedNew: number;
  loggedDecodeT: number;
  genAtLastLog: number;
  gaps: number;
  /** Seconds the request waited with no progress because the GPU was asleep (restore in flight). */
  waitT: number;
  /** Session-local time at which the request arrived. */
  arriveT: number;
}

const HIST = 300;

export class LlmSim {
  activity: 'idle' | 'prefill' | 'decode' = 'idle';
  decodeTps = 0;
  decodeHistory: number[] = [];
  requests: RequestRecord[] = [];
  totals = { requests: 0, promptTokens: 0, generatedTokens: 0 };
  /** Average decode speed of every finished request this session (for the last-session summary). */
  private rates: number[] = [];
  /**
   * Set by the engine: true while the GPU cannot run kernels (it is asleep or restoring). A request that
   * is in prefill then makes no progress at all (0 tok/s) until this turns false.
   */
  blocked: (() => boolean) | null = null;

  private cfg: LlmSimConfig;
  private rng: Rng;
  private log: ConsoleBuf;
  private clock: () => { epoch: number; sessionT: number };

  private pool: ReqTuple[];
  private cursor = 0;
  private idleLeft: number;
  private req: Req | null = null;
  private last: Req | null = null;
  private ratio: number;
  private nextId = 1;
  private taskId = 1;
  private histAcc = 0;
  private histT = 0;
  private acceptShown = 0;
  private acceptTarget = 0;
  private prefillCurve: number[] = [];
  private ctxUsed = 0;
  private bigPending: BigPrefill | null;
  /** Simulated seconds this sim has run (paced arrivals are measured on it). */
  private age = 0;

  constructor(cfg: LlmSimConfig, rng: Rng, log: ConsoleBuf, clock: () => { epoch: number; sessionT: number }) {
    this.cfg = cfg;
    this.rng = rng;
    this.log = log;
    this.clock = clock;
    // Replay one real session (seeded pick among the long ones) from its first request, so the context
    // grows from an empty cache exactly as it did.
    const longest = cfg.sessions.filter((s) => s.length >= 30);
    this.pool = (longest.length ? rng.pick(longest) : rng.pick(cfg.sessions)).slice();
    this.ratio = cfg.jitter.ratioMean;
    this.bigPending = cfg.big ?? null;
    this.idleLeft = this.bigPending ? 2.5 : rng.range(0.6, 2.4);
    this.prefillCurve = buildCurve(cfg.ramp);
    if (cfg.paced) {
      this.pool = pacedPool(cfg, cfg.paced, this.pool, rng);
      this.idleLeft = Math.max(0.2, cfg.paced.firstInS);
    }
  }

  // ----- state -> view model --------------------------------------------------------------------

  toLive(): LlmLive {
    const r = this.req ?? this.last;
    let prefill: LlmLive['prefill'] = null;
    if (r) {
      const active = this.activity === 'prefill';
      // Time a request spends waiting for a sleeping GPU counts as prefill time, as the server logs it.
      const elapsed = (active ? r.pT : r.prefillS) + r.waitT;
      prefill = {
        tokens: r.newTokens,
        doneTokens: active ? Math.round(r.doneNew) : r.newTokens,
        cachedTokens: r.cached,
        tps: round1(active ? r.doneNew / Math.max(0.5, elapsed) : r.newTokens / Math.max(0.05, elapsed)),
        elapsedS: round1(elapsed),
        etaS: active ? round1(Math.max(0, r.prefillS - r.pT)) : 0,
      };
    }
    const used = r
      ? r.cached + (this.activity === 'prefill' ? r.doneNew : r.newTokens) + (this.activity === 'prefill' ? 0 : r.doneGen)
      : 0;
    this.ctxUsed = Math.round(Math.min(used, this.cfg.ctxTotal));
    return {
      activity: this.activity,
      decodeTps: round1(this.decodeTps),
      decodeHistory: this.decodeHistory.slice(),
      prefill,
      generatedTokens: r ? Math.round(this.activity === 'decode' ? r.doneGen : this.activity === 'prefill' ? 0 : r.doneGen) : 0,
      context: { usedTokens: this.ctxUsed, totalTokens: this.cfg.ctxTotal },
      spec: this.cfg.specMode
        ? { acceptancePct: Math.round(this.acceptShown * 100), mode: this.cfg.specMode, active: this.activity === 'decode' && this.acceptShown > 0 }
        : null,
      requests: this.requests.slice(),
      totals: { ...this.totals },
    };
  }

  /** Progress 0..1 of the running prefill, or -1 when no prefill is running (drives the VRAM swell). */
  prefillFraction(): number {
    const r = this.req;
    if (this.activity !== 'prefill' || !r) return -1;
    return clamp(r.doneNew / Math.max(1, r.newTokens), 0, 1);
  }

  /** Median per-request decode speed over the whole session, 0 when nothing finished yet. */
  medianDecodeTps(): number {
    if (!this.rates.length) return 0;
    const a = [...this.rates].sort((x, y) => x - y);
    return a[Math.floor(a.length / 2)];
  }

  // ----- stepping ---------------------------------------------------------------------------------

  step(dt: number) {
    let left = dt;
    let guard = 0;
    while (left > 1e-6 && guard++ < 64) {
      const was = left;
      if (this.activity === 'idle') {
        const use = Math.min(left, this.idleLeft);
        this.idleLeft -= use;
        this.sample(0, use);
        left -= use;
        this.age += use;
        if (this.idleLeft <= 1e-9) this.startRequest();
      } else if (this.activity === 'prefill') {
        left = this.stepPrefill(left);
        this.age += was - left;
      } else {
        left = this.stepDecode(left);
        this.age += was - left;
      }
    }
  }

  private sample(tps: number, dt: number) {
    this.histAcc += tps * dt;
    this.histT += dt;
    while (this.histT >= 1) {
      // The accumulated mean of this second; remainder carries on.
      const mean = this.histAcc / this.histT;
      this.decodeHistory.push(round1(mean));
      if (this.decodeHistory.length > HIST) this.decodeHistory.shift();
      this.histT -= 1;
      this.histAcc = mean * this.histT;
    }
  }

  private nextTuple(): ReqTuple {
    const t = this.pool[this.cursor % this.pool.length];
    this.cursor++;
    return t;
  }

  private startRequest() {
    const { cfg } = this;
    let r: Req;
    const taskId = (this.taskId += this.rng.int(1, 120));
    if (this.bigPending) {
      const b = this.bigPending;
      this.bigPending = null;
      r = this.makeReq(taskId, {
        gapS: 0,
        newTokens: b.processedTokens,
        cachedTokens: 0,
        prefillTps: b.prefillTpsPlateau,
        genTokens: b.genTokens,
        decodeTps: b.decodeTps,
        accept: b.accept,
        meanLen: 0,
        ctxAfter: b.processedTokens + b.genTokens,
      });
      r.prefillS = b.prefillS;
      r.checkpoints = b.progress;
      r.decodeSeries = b.decodeSeries;
    } else {
      const tp = this.nextTuple();
      r = this.makeReq(taskId, tp);
      if (this.nextId === 2 && !cfg.paced) {
        // A freshly started server has an empty prompt cache: the first prompt is processed in full.
        r.newTokens += r.cached;
        r.cached = 0;
        r.sim = 0;
        r.prefillS = Math.max(0.05, r.newTokens / Math.max(1, tp.prefillTps));
      }
    }
    // Keep the request inside the configured context window.
    const room = cfg.ctxTotal - 64;
    if (r.cached + r.newTokens + r.gen > room) {
      r.gen = Math.max(8, Math.min(r.gen, room - r.cached - r.newTokens));
      if (r.cached + r.newTokens > room) {
        r.newTokens = Math.max(1, room - r.cached - 8);
        r.gen = 8;
      }
    }
    r.arriveT = this.age;
    this.req = r;
    this.activity = 'prefill';
    const c = this.clock();
    const sim = r.sim > 0 ? `selected slot by LCP similarity, f_sim_best = ${r.sim.toFixed(3)} (> 0.100 thold), f_keep = 1.000` : 'selected slot by LRU, t_last = -1';
    this.log.push(llamaLine(c.sessionT, 'I', `slot get_availabl: id  0 | task -1 | ${sim}`));
    this.log.push(llamaLine(c.sessionT, 'I', `slot launch_slot_: id  0 | task ${r.taskId} | processing task, is_child = 0`));
  }

  private makeReq(taskId: number, tp: ReqTuple): Req {
    const total = tp.cachedTokens + tp.newTokens;
    return {
      id: this.nextId++,
      taskId,
      cached: tp.cachedTokens,
      newTokens: Math.max(1, tp.newTokens),
      gen: Math.max(1, tp.genTokens),
      prefillS: Math.max(0.05, tp.newTokens / Math.max(1, tp.prefillTps)),
      decodeTps: Math.max(1, tp.decodeTps),
      accept: tp.accept,
      meanLen: tp.meanLen,
      sim: total > 0 ? tp.cachedTokens / total : 0,
      checkpoints: null,
      decodeSeries: null,
      pT: 0,
      doneNew: 0,
      dT: 0,
      doneGen: 0,
      loggedNew: 0,
      loggedDecodeT: 0,
      genAtLastLog: 0,
      gaps: tp.gapS,
      waitT: 0,
      arriveT: 0,
    };
  }

  private prefillTokensAt(r: Req, t: number): number {
    if (r.checkpoints) {
      const cp = r.checkpoints;
      if (t <= cp[0][1]) return (cp[0][0] * t) / cp[0][1];
      for (let i = 1; i < cp.length; i++) {
        if (t <= cp[i][1]) {
          const [n0, t0] = cp[i - 1];
          const [n1, t1] = cp[i];
          return n0 + ((n1 - n0) * (t - t0)) / Math.max(1e-6, t1 - t0);
        }
      }
      return r.newTokens;
    }
    const u = clamp(t / r.prefillS, 0, 1);
    // Short prompts: linear. Long prompts follow the measured ramp.
    const f = r.newTokens < 2500 ? u : curveAt(this.prefillCurve, u);
    return r.newTokens * f;
  }

  private stepPrefill(dt: number): number {
    const r = this.req!;
    // A sleeping GPU runs nothing: the request waits (0 tok/s) while the driver restores the VRAM.
    if (this.blocked?.()) {
      r.waitT += dt;
      this.sample(0, dt);
      return 0;
    }
    const use = Math.min(dt, r.prefillS - r.pT);
    r.pT += use;
    r.doneNew = Math.min(r.newTokens, this.prefillTokensAt(r, r.pT));
    this.sample(0, use);
    const c = this.clock();
    // Progress log lines, one per batch worth of tokens (like the real server).
    while (r.doneNew - r.loggedNew >= this.cfg.logEveryTokens && r.pT < r.prefillS) {
      r.loggedNew += this.cfg.logEveryTokens;
      const n = r.cached + r.loggedNew;
      const total = r.cached + r.newTokens;
      this.log.push(
        llamaLine(
          c.sessionT,
          'I',
          `slot print_timing: id  0 | task ${r.taskId} | prompt processing, n_tokens = ${padNum(n, 6)}, progress = ${(n / total).toFixed(2)}, t = ${padNum(r.pT + r.waitT, 6, 2)} s / ${(r.loggedNew / Math.max(0.1, r.pT + r.waitT)).toFixed(2)} tokens per second`,
        ),
      );
    }
    if (r.pT >= r.prefillS - 1e-9) {
      r.doneNew = r.newTokens;
      this.activity = 'decode';
      this.acceptTarget = r.accept >= 0 ? r.accept : this.acceptShown;
      if (this.acceptShown === 0) this.acceptShown = this.acceptTarget;
      // First decode rate follows the request's own mean.
      this.ratio = this.cfg.jitter.ratioMean;
    }
    return dt - use;
  }

  private stepDecode(dt: number): number {
    const r = this.req!;
    const { jitter } = this.cfg;
    // AR(1) around the request mean, calibrated on real 3 s throughput windows.
    const phi = Math.pow(jitter.ar1, dt / 3);
    const innov = Math.sqrt(Math.max(0, 1 - phi * phi)) * jitter.ratioSd;
    this.ratio = jitter.ratioMean + phi * (this.ratio - jitter.ratioMean) + innov * this.rng.gauss();
    this.ratio = clamp(this.ratio, 0.35, 1.7);
    let rate = r.decodeTps * this.ratio;
    if (r.decodeSeries) {
      const idx = Math.min(r.decodeSeries.length - 1, Math.floor(r.dT / 3));
      rate = r.decodeSeries[idx];
    }
    const remaining = r.gen - r.doneGen;
    const need = remaining / Math.max(0.5, rate);
    const use = Math.min(dt, need);
    r.dT += use;
    r.doneGen = Math.min(r.gen, r.doneGen + rate * use);
    // EMA-smoothed displayed speed (tau 1.5 s).
    const a = 1 - Math.exp(-use / 1.5);
    this.decodeTps = this.decodeTps === 0 || this.last === null ? rate : this.decodeTps + (rate - this.decodeTps) * a;
    if (this.decodeTps < 0.5) this.decodeTps = rate;
    this.acceptShown += (this.acceptTarget - this.acceptShown) * (1 - Math.exp(-use / 4));
    this.sample(rate, use);
    const c = this.clock();
    while (r.dT - r.loggedDecodeT >= 3 && r.doneGen < r.gen) {
      r.loggedDecodeT += 3;
      const avg = r.doneGen / Math.max(1, r.dT);
      this.log.push(
        llamaLine(
          c.sessionT,
          'I',
          `slot print_timing: id  0 | task ${r.taskId} | n_gen = ${padNum(r.doneGen, 6)}, tg = ${padNum(avg, 5, 2)} t/s, tg_3s = ${padNum(rate, 5, 2)} t/s`,
        ),
      );
    }
    if (r.doneGen >= r.gen - 1e-6) this.finishRequest(r);
    return dt - use;
  }

  private finishRequest(r: Req) {
    const c = this.clock();
    const s = c.sessionT;
    // The server's prompt eval time includes the wait for a sleeping GPU: that is the cost the client sees.
    const prefillTotalS = r.prefillS + r.waitT;
    const prefillMs = prefillTotalS * 1000;
    const decodeMs = r.dT * 1000;
    const pPerTok = prefillMs / r.newTokens;
    const dPerTok = decodeMs / Math.max(1, r.gen);
    this.log.push(
      llamaLine(s, 'I', `slot print_timing: id  0 | task ${r.taskId} | prompt eval time = ${padNum(prefillMs, 10, 2)} ms / ${padNum(r.newTokens, 5)} tokens (${padNum(pPerTok, 8, 2)} ms per token, ${padNum(r.newTokens / prefillTotalS, 8, 2)} tokens per second)`),
    );
    this.log.push(
      llamaLine(s, 'I', `slot print_timing: id  0 | task ${r.taskId} |        eval time = ${padNum(decodeMs, 10, 2)} ms / ${padNum(r.gen, 5)} tokens (${padNum(dPerTok, 8, 2)} ms per token, ${padNum(r.gen / Math.max(0.01, r.dT), 8, 2)} tokens per second)`),
    );
    this.log.push(
      llamaLine(s, 'I', `slot print_timing: id  0 | task ${r.taskId} |       total time = ${padNum(prefillMs + decodeMs, 10, 2)} ms / ${padNum(r.newTokens + r.gen, 5)} tokens`),
    );
    if (this.cfg.specMode && r.accept >= 0) {
      const generated = Math.round(r.gen * (1 + 0.1));
      const accepted = Math.round(generated * r.accept);
      this.log.push(
        llamaLine(s, 'I', `slot print_timing: id  0 | task ${r.taskId} | draft acceptance = ${r.accept.toFixed(5)} (${padNum(accepted, 5)} accepted / ${padNum(generated, 5)} generated), mean len = ${padNum(r.meanLen || 2, 5, 2)}`),
      );
    }
    this.log.push(llamaLine(s, 'I', `slot      release: id  0 | task ${r.taskId} | stop processing: n_tokens = ${r.cached + r.newTokens + r.gen}, truncated = 0`));
    this.requests.push({
      id: r.id,
      at: Math.round(c.epoch),
      promptTokens: r.cached + r.newTokens,
      cachedTokens: r.cached,
      prefillS: round1(prefillTotalS),
      generatedTokens: r.gen,
      decodeS: round1(r.dT),
    });
    if (this.requests.length > 12) this.requests.shift();
    this.totals.requests++;
    this.totals.promptTokens += r.cached + r.newTokens;
    this.totals.generatedTokens += r.gen;
    this.rates.push(r.gen / Math.max(0.01, r.dT));
    this.last = r;
    this.req = null;
    this.activity = 'idle';
    const paced = this.cfg.paced;
    if (paced) {
      // Arrival to arrival is ~periodS whatever the request took (the wake-up wait included).
      const period = paced.periodS + this.rng.range(-paced.jitterS, paced.jitterS);
      this.idleLeft = Math.max(1, period - (this.age - r.arriveT));
      return;
    }
    // The next real request defines the idle gap that precedes it.
    const peek = this.pool[this.cursor % this.pool.length];
    this.idleLeft = clamp(peek.gapS, 0.2, this.cfg.gapCapS);
  }
}

const round1 = (v: number) => Math.round(v * 10) / 10;

/**
 * Replay pool for a paced session: the short real requests of every long session, shuffled (seeded), so
 * the cadence is the scripted one and each request still has real token counts and speeds. Falls back to
 * the picked session when the fixtures hold too few short ones.
 */
function pacedPool(cfg: LlmSimConfig, p: PacedCfg, fallback: ReqTuple[], rng: Rng): ReqTuple[] {
  const room = cfg.ctxTotal - 256;
  const short = cfg.sessions
    .filter((s) => s.length >= 30)
    .flat()
    .filter((t) => t.newTokens <= p.maxNewTokens && t.genTokens <= p.maxGenTokens && t.cachedTokens + t.newTokens + t.genTokens <= room);
  if (short.length < 8) return fallback;
  for (let i = short.length - 1; i > 0; i--) {
    const j = rng.int(0, i);
    [short[i], short[j]] = [short[j], short[i]];
  }
  return short;
}

/** Cumulative time at each decile boundary, normalised to 1, from the measured rate ramp. */
function buildCurve(ramp: number[]): number[] {
  const inv = ramp.map((r) => 1 / Math.max(0.2, r));
  const sum = inv.reduce((a, b) => a + b, 0);
  const cum = [0];
  for (const v of inv) cum.push(cum[cum.length - 1] + v / sum);
  return cum;
}

/** Progress fraction (0..1) at normalised time u, by inverting the piecewise-linear cumulative-time curve. */
function curveAt(cum: number[], u: number): number {
  for (let i = 1; i < cum.length; i++) {
    if (u <= cum[i]) {
      const t0 = cum[i - 1];
      const t1 = cum[i];
      return (i - 1 + (u - t0) / Math.max(1e-9, t1 - t0)) / (cum.length - 1);
    }
  }
  return 1;
}
