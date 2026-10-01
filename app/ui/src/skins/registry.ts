import type { SkinId, SkinMeta } from './contract';

// Skins are code-split: only the active one is downloaded and mounted.
export const SKINS: SkinMeta[] = [
  {
    id: 'cliff',
    name: 'Cliff',
    blurb: 'Coastal topography. VRAM as a sea cliff, RAM as the sea.',
    load: () => import('./cliff/index'),
  },
  {
    id: 'silicon',
    name: 'Silicon',
    blurb: 'Engineering blueprint of the GPU, dimensioned memory column.',
    load: () => import('./silicon/index'),
  },
  {
    id: 'instrument',
    name: 'Instrument',
    blurb: 'Industrial hardware panel: drum counter, needles, rotary selector.',
    load: () => import('./instrument/index'),
  },
  {
    id: 'phosphor',
    name: 'Phosphor',
    blurb: 'Vector oscilloscope: glowing traces with afterglow.',
    load: () => import('./phosphor/index'),
  },
];

export const DEFAULT_SKIN: SkinId = 'cliff';

export function skinMeta(id: string | null | undefined): SkinMeta {
  return SKINS.find((s) => s.id === id) ?? SKINS.find((s) => s.id === DEFAULT_SKIN)!;
}
