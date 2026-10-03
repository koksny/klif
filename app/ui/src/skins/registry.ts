import type { SkinId, SkinMeta, SkinModule } from './contract';

// Local-only skins: skins/private/<id>/ with a meta.ts (default export { id, name, blurb }) and an index.ts.
// That folder is not part of this repository (for art under licences that cannot ship with it); when it is
// present on a machine its skins join the list after the built-in ones, in folder order.
const privateMeta = import.meta.glob<{ default: Omit<SkinMeta, 'load'> }>('./private/*/meta.ts', { eager: true });
const privateLoad = import.meta.glob<SkinModule>('./private/*/index.ts');
const PRIVATE: SkinMeta[] = Object.entries(privateMeta).flatMap(([path, m]) => {
  const load = privateLoad[path.replace(/meta\.ts$/, 'index.ts')] as (() => Promise<SkinModule>) | undefined;
  return load ? [{ ...m.default, load }] : [];
});

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
  {
    id: 'decode',
    name: 'Decode',
    blurb: 'A terminal of noise read as the context window; decoding finds tokens in it.',
    load: () => import('./decode/index'),
  },
  {
    id: 'loom',
    name: 'Loom',
    blurb: 'The transformer in 3D on an amber phosphor screen: layers, experts, KV cache.',
    load: () => import('./loom/index'),
  },
  {
    id: 'ether',
    name: 'Ether',
    blurb: 'Luminous plasma in a black void; the UI engraved on glass floating over it.',
    load: () => import('./ether/index'),
  },
  {
    id: 'rings',
    name: 'Rings',
    blurb: 'An optical laboratory: a glass specimen sphere, ring gauges around it, frosted-glass panels.',
    load: () => import('./rings/index'),
  },
  {
    id: 'spirit',
    name: 'Spirit',
    blurb: "Edan Kwan's The Spirit through a camera viewfinder: the smoke is filmed, KLIF is the OSD.",
    load: () => import('./spirit/index'),
  },
  ...PRIVATE,
];

export const DEFAULT_SKIN: SkinId = 'cliff';

export function skinMeta(id: string | null | undefined): SkinMeta {
  return SKINS.find((s) => s.id === id) ?? SKINS.find((s) => s.id === DEFAULT_SKIN)!;
}
