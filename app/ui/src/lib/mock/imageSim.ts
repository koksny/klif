// Image-job simulator (Krea / sd.cpp). Replays REAL job timings (preparation, text conditioning, per-step
// seconds, VAE decode) from the numeric fixtures and prints sd.cpp style console lines.
import type { ImageJob, ImageLive } from '../model/types';
import { sdBar } from './consoleBuf';
import type { ConsoleBuf } from './consoleBuf';
import type { KreaJobTuple } from './fixtures';
import { clamp, type Rng } from './rng';

export interface ImageSimConfig {
  jobs: KreaJobTuple[];
  /** Probability that a job is an edit job (real share is ~0.55). */
  editShare: number;
  /** Mean idle seconds between jobs (demo cadence; the measured duty cycle implies a longer wait). */
  meanGapS: number;
  /** Force the n-th job (1-based) to be a 720x1024 edit job that crashes at the start of sampling. */
  crashOnJob?: number;
}

interface Job {
  w: number;
  h: number;
  edit: boolean;
  steps: number;
  prepS: number;
  condS: number;
  stepS: number[];
  decodeS: number;
  totalS: number;
  t: number;
  /** Console milestones already printed. */
  logged: number;
  stepsLogged: number;
  willCrash: boolean;
  seed: number;
}

const MAX_RECENT = 24;

export class ImageSim {
  activity: 'idle' | 'generating' = 'idle';
  recent: ImageJob[] = [];
  images = 0;
  crashed = false;
  /** Seconds of every finished job this session (for the last-session summary). */
  private secs: number[] = [];

  private cfg: ImageSimConfig;
  private rng: Rng;
  private log: ConsoleBuf;
  private epoch: () => number;
  private job: Job | null = null;
  private idleLeft: number;
  private jobCount = 0;
  private lastStepS = 0;
  private lastSize: { w: number; h: number; edit: boolean; steps: number } = { w: 512, h: 768, edit: false, steps: 8 };
  private byShape: { plain: KreaJobTuple[]; edit: KreaJobTuple[] };

  constructor(cfg: ImageSimConfig, rng: Rng, log: ConsoleBuf, epoch: () => number) {
    this.cfg = cfg;
    this.rng = rng;
    this.log = log;
    this.epoch = epoch;
    this.byShape = {
      plain: cfg.jobs.filter((j) => j[2] === 0),
      edit: cfg.jobs.filter((j) => j[2] === 1),
    };
    this.idleLeft = rng.range(0.8, 2.5);
  }

  toLive(): ImageLive {
    const j = this.job;
    if (!j) {
      return {
        activity: 'idle',
        step: 0,
        steps: this.lastSize.steps,
        sPerIt: 0,
        elapsedS: 0,
        width: this.lastSize.w,
        height: this.lastSize.h,
        edit: this.lastSize.edit,
        recent: this.recent.slice(),
        imagesThisSession: this.images,
      };
    }
    const sampleStart = j.prepS + j.condS;
    let step = 0;
    let acc = sampleStart;
    for (let i = 0; i < j.steps; i++) {
      if (j.t >= acc) step = i + 1;
      acc += j.stepS[i];
    }
    return {
      activity: 'generating',
      step,
      steps: j.steps,
      sPerIt: round2(this.lastStepS),
      elapsedS: round1(j.t),
      width: j.w,
      height: j.h,
      edit: j.edit,
      recent: this.recent.slice(),
      imagesThisSession: this.images,
    };
  }

  /** Median seconds per finished image, 0 when nothing finished yet. */
  medianSeconds(): number {
    if (!this.secs.length) return 0;
    const a = [...this.secs].sort((x, y) => x - y);
    return a[Math.floor(a.length / 2)];
  }

  /** Aborts the running job (used when the process dies). */
  abort() {
    this.job = null;
    this.activity = 'idle';
  }

  /** 0..1 GPU activation load, for the VRAM layers. */
  load(): number {
    return this.job ? 1 : 0;
  }

  step(dt: number) {
    let left = dt;
    let guard = 0;
    while (left > 1e-6 && guard++ < 64 && !this.crashed) {
      if (!this.job) {
        const use = Math.min(left, this.idleLeft);
        this.idleLeft -= use;
        left -= use;
        if (this.idleLeft <= 1e-9) this.startJob();
      } else {
        const j = this.job;
        const use = Math.min(left, j.totalS - j.t);
        j.t += use;
        left -= use;
        this.emitProgress(j);
        if (j.willCrash && j.t >= j.prepS + j.condS + 0.2) {
          this.crashed = true;
          this.job = null;
          this.activity = 'idle';
          return;
        }
        if (j.t >= j.totalS - 1e-9) this.finishJob(j);
      }
    }
  }

  private startJob() {
    this.jobCount++;
    const wantCrash = this.cfg.crashOnJob === this.jobCount;
    const isEdit = wantCrash || this.rng.chance(this.cfg.editShare);
    const pool = isEdit && this.byShape.edit.length ? this.byShape.edit : this.byShape.plain;
    let tp = this.rng.pick(pool);
    if (wantCrash) {
      const big = this.byShape.edit.filter((t) => t[0] * t[1] >= 700 * 1000);
      if (big.length) tp = this.rng.pick(big);
    }
    const [w, h, edit, steps, prepS, condS, firstS, restS, decodeS, totalS] = tp;
    const stepS: number[] = [];
    for (let i = 0; i < steps; i++) {
      const base = i === 0 ? firstS : restS;
      stepS.push(Math.max(0.2, base * (1 + this.rng.gauss() * 0.025)));
    }
    const summed = prepS + condS + stepS.reduce((a, b) => a + b, 0) + decodeS;
    // Whatever the log shows beyond the named phases (model streaming, graph build) lands in preparation.
    const prep = prepS + Math.max(0, totalS - summed);
    const job: Job = {
      w,
      h,
      edit: edit === 1,
      steps,
      prepS: prep,
      condS,
      stepS,
      decodeS,
      totalS: prep + condS + stepS.reduce((a, b) => a + b, 0) + decodeS,
      t: 0,
      logged: 0,
      stepsLogged: 0,
      willCrash: wantCrash,
      seed: this.rng.int(1, 999_999_999),
    };
    this.job = job;
    this.activity = 'generating';
    this.lastStepS = 0;
    this.lastSize = { w, h, edit: job.edit, steps };
    this.log.push(`[INFO ] stable-diffusion.cpp:5592 - generate_image ${w}x${h}`);
    if (job.edit) {
      this.log.push(`[INFO ] stable-diffusion.cpp:3150 - Using 'krea2_edit' preset for reference images`);
      this.log.push(`[INFO ] stable-diffusion.cpp:5084 - EDIT mode`);
    }
  }

  private emitProgress(j: Job) {
    // milestone 1: conditioning done
    const condAt = j.prepS + j.condS;
    if (j.logged < 1 && j.t >= condAt) {
      j.logged = 1;
      if (j.edit) this.log.push(`[INFO ] stable-diffusion.cpp:5162 - encode_first_stage completed, taking ${Math.max(0.2, j.prepS * 0.5).toFixed(2)}s`);
      this.log.push(`[INFO ] stable-diffusion.cpp:5272 - get_learned_condition completed, taking ${j.condS.toFixed(2)}s`);
      this.log.push(`[INFO ] stable-diffusion.cpp:5643 - generating image: 1/1 - seed ${j.seed}`);
    }
    // sampling steps
    let acc = condAt;
    for (let i = 0; i < j.steps; i++) {
      acc += j.stepS[i];
      if (j.stepsLogged === i && j.t >= acc) {
        j.stepsLogged = i + 1;
        this.lastStepS = j.stepS[i];
        this.log.push(sdBar(i + 1, j.steps, j.stepS[i]));
      }
    }
    const sampleEnd = acc;
    if (j.logged < 2 && j.t >= sampleEnd) {
      j.logged = 2;
      this.log.push(`[INFO ] stable-diffusion.cpp:5675 - sampling completed, taking ${(sampleEnd - condAt).toFixed(2)}s`);
    }
  }

  private finishJob(j: Job) {
    this.log.push(`[INFO ] stable-diffusion.cpp:5367 - decode_first_stage completed, taking ${j.decodeS.toFixed(2)}s`);
    this.log.push(`[INFO ] stable-diffusion.cpp:5825 - generate_image completed in ${j.totalS.toFixed(2)}s`);
    this.recent.push({ at: Math.round(this.epoch()), seconds: round1(j.totalS), width: j.w, height: j.h, edit: j.edit });
    if (this.recent.length > MAX_RECENT) this.recent.shift();
    this.images++;
    this.secs.push(j.totalS);
    this.job = null;
    this.activity = 'idle';
    this.idleLeft = clamp(1.2 + this.rng.exp(this.cfg.meanGapS), 1.2, this.cfg.meanGapS * 5);
  }
}

const round1 = (v: number) => Math.round(v * 10) / 10;
const round2 = (v: number) => Math.round(v * 100) / 100;
