// GPU-dormant state (GpuMemory.dormant): the inference GPU powered down between requests while the session's
// allocations still exist, paged out to system RAM. Everything here is derived from the view model.
import type { ViewModel } from '../../lib/model/types';

export interface Sleep {
  /** Session allocations not resident on the device (system RAM). */
  pagedOutGiB: number;
  /** Seconds since the GPU went dormant. */
  sinceS: number;
  /** "D3", if known. */
  powerState: string | null;
  /** 0..1: the share of the session's memory that is back on the device. */
  restoredFrac: number;
}

/**
 * Reactive dormant state for a skin component (call during component init). `waking` = dormant AND the
 * restore is under way: a request is waiting on the GPU, or the resident amount is climbing.
 */
export function useSleep(get: () => ViewModel): { readonly info: Sleep | null; readonly waking: boolean } {
  const info = $derived.by<Sleep | null>(() => {
    const v = get().vram;
    const d = v.dormant;
    if (!d) return null;
    const resident = Math.max(0, v.usedGiB);
    const paged = Math.max(0, d.pagedOutGiB);
    const all = resident + paged;
    return { pagedOutGiB: paged, sinceS: Math.max(0, d.sinceS), powerState: d.powerState ?? null, restoredFrac: all > 0 ? Math.min(1, resident / all) : 0 };
  });
  const inFlight = $derived.by(() => {
    const s = get().session;
    return !!s && ((!!s.llm && s.llm.activity !== 'idle') || (!!s.image && s.image.activity === 'generating') || (s.generic?.requestsInFlight ?? 0) > 0);
  });

  // Direction of the resident amount: climbing = restoring, falling = going to sleep.
  let up = $state(false);
  let prev = get().vram.usedGiB;
  $effect(() => {
    const v = get().vram.usedGiB;
    if (v > prev + 0.004) {
      up = true;
      prev = v;
    } else if (v < prev - 0.004) {
      up = false;
      prev = v;
    }
  });

  const waking = $derived(info !== null && (inFlight || up));
  return {
    get info() {
      return info;
    },
    get waking() {
      return waking;
    },
  };
}
