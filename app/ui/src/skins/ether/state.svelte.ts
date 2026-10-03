// Reactive helpers shared by the full window and the mini panel.
import type { ViewModel } from '../../lib/model/types';

/**
 * A value that follows its source but changes at most once per `ms`, so the hero figure never flickers faster
 * than ~2x per second whatever the telemetry rate is. Call during component init.
 */
export function held<T>(get: () => T, ms = 500): { readonly current: T } {
  let value = $state(get());
  let last = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const next = get();
    const now = performance.now();
    clearTimeout(timer);
    if (now - last >= ms) {
      value = next;
      last = now;
    } else {
      timer = setTimeout(() => {
        value = get();
        last = performance.now();
      }, ms - (now - last));
    }
    return () => clearTimeout(timer);
  });

  return {
    get current() {
      return value;
    },
  };
}

export interface Sleep {
  /** Session allocations not resident on the device (system RAM). */
  pagedOutGiB: number;
  sinceS: number;
  powerState: string | null;
  /** 0..1: the share of the session's memory back on the device. */
  restoredFrac: number;
}

/**
 * GPU dormant (vm.vram.dormant): the inference GPU powered down while a model is loaded. `waking` = a request
 * is waiting on it, or the resident amount is climbing back. Call during component init.
 */
export function useSleep(get: () => ViewModel): { readonly info: Sleep | null; readonly waking: boolean } {
  const info = $derived.by<Sleep | null>(() => {
    const v = get().vram;
    const d = v.dormant;
    if (!d) return null;
    const resident = Math.max(0, v.usedGiB);
    const paged = Math.max(0, d.pagedOutGiB);
    return {
      pagedOutGiB: paged,
      sinceS: Math.max(0, d.sinceS),
      powerState: d.powerState ?? null,
      restoredFrac: resident + paged > 0 ? Math.min(1, resident / (resident + paged)) : 0,
    };
  });
  const inFlight = $derived.by(() => {
    const s = get().session;
    return !!s && ((!!s.llm && s.llm.activity !== 'idle') || (!!s.image && s.image.activity === 'generating') || (s.generic?.requestsInFlight ?? 0) > 0);
  });
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
