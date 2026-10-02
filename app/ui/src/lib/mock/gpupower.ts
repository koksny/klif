// Mock of the inference GPU's power state. The RX 9070 XT drives no display, so AMD ULPS puts it into
// device state D3 ~10-15 s after the last request. The loaded server keeps its allocations, but the
// driver pages the VRAM out to system RAM: Dedicated Usage drops to ~0 while the allocations (committed
// memory) stay. The next request first waits ~4 s while everything is restored, then runs at full speed.
//
//   awake --(idle sleepAfterS)--> sleeping --(fallS)--> asleep --(request)--> waking --(restoreS)--> awake
//
// A request that arrives while the GPU is still falling asleep wakes it from wherever the fall got to.
import type { GpuMemory, VramLayer } from '../model/types';

export type PowerPhase = 'awake' | 'sleeping' | 'asleep' | 'waking';

export interface PowerCfg {
  /** Idle seconds after the last request before the GPU powers down (measured: 10-15 s). */
  sleepAfterS: number;
  /** Seconds the VRAM takes to be paged out. */
  fallS: number;
  /** Seconds to restore everything from system RAM (measured wake-up delay: ~4 s). */
  restoreS: number;
  /** Allocations that sit in system RAM once fully dormant (GiB). */
  pagedOutGiB: number;
  /** What stays resident on the device once fully dormant (GiB): a sliver of the 'other' layer. */
  floorGiB: number;
  powerState: string;
}

export const DEFAULT_POWER: PowerCfg = {
  sleepAfterS: 12,
  fallS: 3,
  restoreS: 4,
  pagedOutGiB: 15.42,
  floorGiB: 0.01,
  powerState: 'D3',
};

const r1 = (v: number) => Math.round(v * 10) / 10;
const r2 = (v: number) => Math.round(v * 100) / 100;

export class GpuPower {
  phase: PowerPhase = 'awake';
  /** Fraction (0..1) of the allocations that is resident in VRAM. */
  res = 1;
  readonly cfg: PowerCfg;

  private idleT = 0;
  /** Simulated time at which the GPU started going dormant. */
  private since = 0;

  constructor(cfg: Partial<PowerCfg> = {}) {
    this.cfg = { ...DEFAULT_POWER, ...cfg };
  }

  /** True while the GPU is not fully awake: kernels cannot run, a request has to wait for the restore. */
  get dormant(): boolean {
    return this.phase !== 'awake';
  }

  /** Advance by h seconds. `busy` = the server is handling a request. */
  step(h: number, now: number, busy: boolean) {
    const c = this.cfg;
    if (this.phase === 'awake') {
      if (busy) {
        this.idleT = 0;
        return;
      }
      this.idleT += h;
      if (this.idleT >= c.sleepAfterS) {
        this.phase = 'sleeping';
        this.since = now;
        this.idleT = 0;
      }
      return;
    }
    if (busy && (this.phase === 'sleeping' || this.phase === 'asleep')) this.phase = 'waking';
    if (this.phase === 'sleeping') {
      this.res = Math.max(0, this.res - h / c.fallS);
      if (this.res <= 0) this.phase = 'asleep';
    } else if (this.phase === 'waking') {
      this.res = Math.min(1, this.res + h / c.restoreS);
      if (this.res >= 1) {
        this.phase = 'awake';
        this.idleT = 0;
      }
    }
  }

  /** GpuMemory.dormant as the core reports it; null while awake. */
  view(now: number): NonNullable<GpuMemory['dormant']> | null {
    if (this.phase === 'awake') return null;
    return {
      pagedOutGiB: r2(this.cfg.pagedOutGiB * (1 - this.res)),
      sinceS: r1(Math.max(0, now - this.since)),
      powerState: this.cfg.powerState,
    };
  }

  /**
   * What is resident for these allocations (baseline layer first, as everywhere in the engine): every
   * layer scales with `res`, except the baseline which keeps the floor the driver never pages out.
   */
  resident(layers: VramLayer[]): VramLayer[] {
    const floor = this.cfg.floorGiB;
    return layers.map((l, i) => {
      const gib = i === 0 && l.id === 'other' ? Math.min(l.gib, floor) + this.res * Math.max(0, l.gib - floor) : l.gib * this.res;
      return { ...l, gib: r2(gib) };
    });
  }
}
