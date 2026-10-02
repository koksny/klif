// "Will it fit": shared by the Tune drawer preview and the mock loader so both agree.
import type { VramLayer } from '../model/types';

/** Headroom the loader tries to keep free below the edge (llama.cpp style auto-fit). */
export const FIT_HEADROOM_GIB = 0.05;

export interface FitResult {
  /** The session layers after the loader trimmed its compute buffers (baseline NOT included). */
  layers: VramLayer[];
  /** How much the compute buffers were trimmed to fit, GiB. */
  trimmedGiB: number;
  /** What still does not fit after trimming, GiB. Anything above 0 spills to shared system memory. */
  overGiB: number;
}

const r2 = (v: number) => Math.round(v * 100) / 100;

/**
 * The real loaders shrink their compute buffers (up to half) to squeeze under the edge before they let
 * anything spill.
 */
export function fitLayers(layers: VramLayer[], baselineGiB: number, totalGiB: number): FitResult {
  const out = layers.map((l) => ({ ...l }));
  const sum = () => out.reduce((a, l) => a + l.gib, 0) + baselineGiB;
  const want = sum() - (totalGiB - FIT_HEADROOM_GIB);
  let trimmed = 0;
  if (want > 0) {
    const b = out.find((l) => l.id === 'buffers');
    if (b) {
      trimmed = r2(Math.min(want, b.gib * 0.5));
      b.gib = r2(b.gib - trimmed);
    }
  }
  return { layers: out, trimmedGiB: trimmed, overGiB: Math.max(0, r2(sum() - totalGiB)) };
}
