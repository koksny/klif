// Request simulator for servers KLIF has no parser for (tts / stt / video, openai-compatible, generic). It
// produces what a real core can know without one: requests in flight, a total, the time since the last one.
import type { GenericLive, SystemKind } from '../model/types';
import type { ConsoleBuf } from './consoleBuf';
import { clamp, type Rng } from './rng';

interface Cadence {
  /** Mean idle seconds between requests. */
  gapS: number;
  /** Seconds a request keeps the server busy: [min, max]. */
  busyS: [number, number];
  route: string;
}

const CADENCE: Record<SystemKind, Cadence> = {
  llm: { gapS: 25, busyS: [3, 12], route: 'POST /v1/chat/completions' },
  image: { gapS: 30, busyS: [8, 20], route: 'POST /sdapi/v1/txt2img' },
  tts: { gapS: 14, busyS: [1.2, 4], route: 'POST /v1/audio/speech' },
  stt: { gapS: 18, busyS: [1.5, 6], route: 'POST /inference' },
  video: { gapS: 70, busyS: [25, 55], route: 'POST /prompt' },
};

export class GenericSim {
  busy = false;
  requestsTotal = 0;
  /** Seconds since the last request finished (or since the sim started). */
  sinceLastS = 0;

  private left: number;
  private busyLeft = 0;
  private readonly c: Cadence;
  private nextId = 1;

  constructor(
    private readonly kind: SystemKind,
    private readonly rng: Rng,
    private readonly log: ConsoleBuf | null,
    private readonly modelId?: string,
    firstInS?: number,
  ) {
    this.c = CADENCE[kind];
    this.left = firstInS ?? rng.range(1.2, 4);
  }

  step(dt: number) {
    let rest = dt;
    let guard = 0;
    while (rest > 1e-6 && guard++ < 32) {
      if (this.busy) {
        const use = Math.min(rest, this.busyLeft);
        this.busyLeft -= use;
        rest -= use;
        if (this.busyLeft <= 1e-9) this.finish();
      } else {
        const use = Math.min(rest, this.left);
        this.left -= use;
        this.sinceLastS += use;
        rest -= use;
        if (this.left <= 1e-9) this.start();
      }
    }
  }

  private start() {
    this.busy = true;
    this.busyLeft = this.rng.range(this.c.busyS[0], this.c.busyS[1]);
    this.sinceLastS = 0;
    this.log?.push(`request ${this.nextId}: ${this.c.route}`);
  }

  private finish() {
    this.busy = false;
    this.requestsTotal++;
    this.sinceLastS = 0;
    this.log?.push(`request ${this.nextId++}: 200 OK`);
    this.left = clamp(1 + this.rng.exp(this.c.gapS), 1.5, this.c.gapS * 4);
  }

  /** 0..1 for the tab pulse. */
  get activity(): number {
    return this.busy ? 1 : 0;
  }

  toLive(): GenericLive {
    const g: GenericLive = {
      requestsInFlight: this.busy ? 1 : 0,
      requestsTotal: this.requestsTotal,
      lastActivityS: Math.round(this.sinceLastS * 10) / 10,
    };
    if (this.modelId) g.modelId = this.modelId;
    return g;
  }
}
