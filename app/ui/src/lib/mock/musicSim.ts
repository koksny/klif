// Song-job simulator (audio.cpp, YuE2). A song runs through stages: the model load (first song only), the score
// (every other song), the audio codes, the render and the waveform decode. Timings follow a real YuE2 Instrumental
// Q8 run on one GPU. It reports through the image path (the stage counts as the step), like the real core.
import type { GenericLive, ImageJob, ImageLive } from '../model/types';
import type { ConsoleBuf } from './consoleBuf';
import { clamp, type Rng } from './rng';

interface Stage {
  phase: 'load' | 'score' | 'codes' | 'render' | 'decode';
  label: string;
  /** Seconds the stage takes (before jitter). */
  s: number;
}

interface Job {
  stages: Stage[];
  /** The stages with a stage number (everything but the load). */
  counted: number;
  /** Seconds each stage takes, jittered. */
  lenS: number[];
  totalS: number;
  t: number;
  /** Stages already announced on the console. */
  logged: number;
}

const MAX_RECENT = 24;

export class MusicSim {
  activity: 'idle' | 'generating' = 'idle';
  recent: ImageJob[] = [];
  songs = 0;
  /** Requests in flight and finished, for the generic side of the session. */
  requestsTotal = 0;
  sinceLastS = 0;

  private job: Job | null = null;
  private idleLeft: number;
  private started = 0;
  private secs: number[] = [];

  constructor(
    private readonly rng: Rng,
    private readonly log: ConsoleBuf,
    private readonly epoch: () => number,
  ) {
    this.idleLeft = rng.range(1, 2.5);
  }

  /** Never set: a song does not crash the mock server. */
  readonly crashed = false;

  get busy(): boolean {
    return !!this.job;
  }

  /** Finished jobs (same name as the image simulator's counter). */
  get images(): number {
    return this.songs;
  }

  toLive(): ImageLive {
    const j = this.job;
    if (!j) {
      return {
        activity: 'idle',
        step: 0,
        steps: 0,
        sPerIt: 0,
        elapsedS: 0,
        width: 0,
        height: 0,
        edit: false,
        recent: this.recent.slice(),
        imagesThisSession: this.songs,
      };
    }
    const i = this.stageIndex(j);
    const st = j.stages[i];
    const counted = st.phase !== 'load';
    return {
      activity: 'generating',
      step: counted ? i - (j.stages.length - j.counted) + 1 : 0,
      steps: counted ? j.counted : 0,
      sPerIt: 0,
      elapsedS: Math.round(j.t * 10) / 10,
      width: 0,
      height: 0,
      edit: false,
      recent: this.recent.slice(),
      imagesThisSession: this.songs,
      phase: st.phase,
    };
  }

  toGeneric(): GenericLive {
    return { requestsInFlight: this.job ? 1 : 0, requestsTotal: this.requestsTotal, lastActivityS: Math.round(this.sinceLastS * 10) / 10 };
  }

  medianSeconds(): number {
    if (!this.secs.length) return 0;
    const a = [...this.secs].sort((x, y) => x - y);
    return a[Math.floor(a.length / 2)];
  }

  /** Aborts the running song (the process died). */
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
    while (left > 1e-6 && guard++ < 64) {
      if (!this.job) {
        const use = Math.min(left, this.idleLeft);
        this.idleLeft -= use;
        this.sinceLastS += use;
        left -= use;
        if (this.idleLeft <= 1e-9) this.start();
      } else {
        const j = this.job;
        const use = Math.min(left, j.totalS - j.t);
        j.t += use;
        left -= use;
        this.announce(j);
        if (j.t >= j.totalS - 1e-9) this.finish(j);
      }
    }
  }

  private start() {
    this.started++;
    // The first song loads the model; every other song has a score, the rest of them skip it.
    const stages: Stage[] = [];
    if (this.started === 1) stages.push({ phase: 'load', label: 'loading the model', s: 3 });
    if (this.started % 2 === 1) stages.push({ phase: 'score', label: 'writing the score', s: 28 });
    stages.push({ phase: 'codes', label: 'composing the audio codes', s: 22 }, { phase: 'render', label: 'rendering the audio', s: 16 }, { phase: 'decode', label: 'decoding to waveform', s: 3 });
    const lenS = stages.map((s) => Math.max(1, s.s * (1 + this.rng.gauss() * 0.04)));
    this.job = {
      stages,
      counted: stages.filter((s) => s.phase !== 'load').length,
      lenS,
      totalS: lenS.reduce((a, b) => a + b, 0),
      t: 0,
      logged: 0,
    };
    this.activity = 'generating';
    this.sinceLastS = 0;
    this.log.push(`[yue2] request ${this.requestsTotal + 1}: POST /release_task`);
  }

  private stageIndex(j: Job): number {
    let acc = 0;
    for (let i = 0; i < j.stages.length; i++) {
      acc += j.lenS[i];
      if (j.t < acc) return i;
    }
    return j.stages.length - 1;
  }

  private announce(j: Job) {
    const i = this.stageIndex(j);
    while (j.logged <= i) {
      const st = j.stages[j.logged];
      this.log.push(st.phase === 'load' ? `[yue2] ${st.label}` : `[yue2] stage ${j.logged - (j.stages.length - j.counted) + 1}/${j.counted}: ${st.label}`);
      j.logged++;
    }
  }

  private finish(j: Job) {
    this.log.push(`[yue2] song ${this.songs + 1} done in ${j.totalS.toFixed(1)} s`);
    this.recent.push({ at: Math.round(this.epoch()), seconds: Math.round(j.totalS * 10) / 10, width: 0, height: 0, edit: false });
    if (this.recent.length > MAX_RECENT) this.recent.shift();
    this.songs++;
    this.requestsTotal++;
    this.secs.push(j.totalS);
    this.job = null;
    this.activity = 'idle';
    this.sinceLastS = 0;
    this.idleLeft = clamp(this.rng.range(20, 40), 20, 40);
  }
}
