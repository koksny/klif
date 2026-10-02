// The GPU-dormant state (vm.vram.dormant): the 9070 XT powered down (AMD ULPS, device state D3) while a
// model is loaded. The session's allocations still exist, but the VRAM is paged out to system RAM; the
// next request waits while it is restored. Everything here is derived from the view model only.
import type { ViewModel } from '../../lib/model/types';
import { sumGiB, viewState } from './util';

export type GpuPhase =
  /** Nothing is resident: the whole session sits in system RAM. */
  | 'asleep'
  /** A request is restoring the VRAM: the resident amount is rising. */
  | 'waking'
  /** The VRAM is still being paged out: the resident amount is falling. */
  | 'sleeping';

export interface GpuView {
  phase: GpuPhase;
  /** The session's allocations (vm.vram.layers while dormant). */
  allocGiB: number;
  /** Resident in VRAM right now (vm.vram.usedGiB), never more than the allocations. */
  residentGiB: number;
  /** residentGiB / allocGiB, 0..1: how much of the rock is solid. */
  frac: number;
  /** Allocations sitting in system RAM (vm.vram.dormant.pagedOutGiB). */
  pagedOutGiB: number;
  sinceS: number;
  powerState?: string;
}

/** Below this resident fraction the GPU counts as fully asleep (the driver keeps a sliver resident). */
const ASLEEP_FRAC = 0.02;

/** A request is in flight (or waiting for the restore). */
function busy(vm: ViewModel): boolean {
  const s = vm.session;
  if (!s) return false;
  if (s.llm) return s.llm.activity !== 'idle';
  if (s.image) return s.image.activity !== 'idle';
  return false;
}

/**
 * `rising`: direction of vm.vram.usedGiB over the last snapshots (true = being restored, false = being
 * paged out, null = not seen moving yet). While unknown, a request in flight means it is waking.
 */
export function gpuView(vm: ViewModel, rising: boolean | null): GpuView | null {
  const d = vm.vram.dormant;
  if (!d) return null;
  const v = viewState(vm);
  if (v !== 'llm' && v !== 'image') return null;
  const alloc = sumGiB(vm.vram.layers) || Math.max(0, d.pagedOutGiB + vm.vram.usedGiB);
  const resident = Math.max(0, Math.min(alloc, vm.vram.usedGiB));
  const frac = alloc > 0 ? resident / alloc : 0;
  const phase: GpuPhase =
    frac < ASLEEP_FRAC ? 'asleep' : (rising ?? busy(vm)) ? 'waking' : 'sleeping';
  return {
    phase,
    allocGiB: alloc,
    residentGiB: resident,
    frac,
    pagedOutGiB: d.pagedOutGiB,
    sinceS: d.sinceS,
    powerState: d.powerState,
  };
}

/** The status chip: "GPU ASLEEP" / "GPU WAKING" / "GPU SLEEPING". */
export function gpuStatus(g: GpuView): string {
  return g.phase === 'asleep' ? 'GPU ASLEEP' : g.phase === 'waking' ? 'GPU WAKING' : 'GPU SLEEPING';
}

/** The tooltip / aria detail: state, power state, how long. */
export function gpuDetail(g: GpuView): string {
  const since = g.sinceS >= 1 ? ` · ${Math.round(g.sinceS)} s` : '';
  return `${g.powerState ? `${g.powerState} · ` : ''}VRAM ${g.phase === 'waking' ? 'being restored from' : 'paged out to'} system RAM${since}`;
}
