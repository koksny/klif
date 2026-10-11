// Music jobs (audio.cpp) ride the image path: the session carries `image` while a song is generated, and `step` /
// `steps` count stages (score, codes, render, decode) instead of sampling steps. These helpers give every skin the
// same words for it.
import type { ImageLive, SystemKind, ViewModel } from './types';

/** The stage names in the log's order; 'load' is the model loading (or the job queued), 'work' a family without stages. */
const STAGE_WORD: Record<string, string> = {
  load: 'loading',
  score: 'writing the score',
  codes: 'composing',
  render: 'rendering',
  decode: 'decoding',
  work: 'generating',
};

/** True for the job of a music System: only music sets `phase`. */
export const isMusicJob = (img: ImageLive | null | undefined): boolean => !!img && img.phase !== undefined;

/** "composing" for the stage in flight ("generating" when the stage is unknown). */
export function stageWord(img: Pick<ImageLive, 'phase'>): string {
  return STAGE_WORD[img.phase ?? 'work'] ?? 'generating';
}

/** "composing · 2/4"; without a stage count ("loading", a model with no stages) just the word. */
export function stageText(img: Pick<ImageLive, 'phase' | 'step' | 'steps'>, sep = ' · '): string {
  const w = stageWord(img);
  return img.steps > 0 ? `${w}${sep}${img.step}/${img.steps}` : w;
}

/** Whether a System of this kind keeps image-style output size, speed per iteration and an edit mode. */
export const hasPixels = (kind: SystemKind | undefined | null): boolean => kind !== 'music';

/** "image" / "images", "song" / "songs": the unit of one job of this kind. */
export function jobNoun(kind: SystemKind | undefined | null, plural = false): string {
  if (kind === 'music') return plural ? 'songs' : 'song';
  return plural ? 'images' : 'image';
}

/**
 * Whether a skin that picks its layout by kind shows this System with the image layout: an image System always,
 * a music System when no session runs (the idle look matches the live one) or the session reports jobs; a music
 * server without job telemetry keeps the request layout.
 */
export function usesImageLayout(kind: SystemKind, hasSession: boolean, img: ImageLive | null | undefined): boolean {
  return kind === 'image' || (kind === 'music' && (!hasSession || !!img));
}

/** The kind of the System the session belongs to (undefined when it is not among the Systems). */
export function sessionKindOf(vm: Pick<ViewModel, 'session' | 'systems'>): SystemKind | undefined {
  const id = vm.session?.system;
  return id ? vm.systems.find((x) => x.id === id)?.kind : undefined;
}
