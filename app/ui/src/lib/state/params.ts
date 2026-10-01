// URL contract (skin agents depend on it; keep exactly):
//   ?skin=<id>&size=full|mini|auto&scenario=<name>&speed=<n>&shot=1
// Extras: t=<sim seconds to start at>, run=1 (keep ticking even with shot=1), drawer=console|tune,
//         slot=<slot id for the tune drawer>, tier=off|calm|ambient|live (pin the tier), seed=<n>,
//         frameless=1 (host.frameless: the skin draws its own window controls).
import type { SlotId } from '../model/types';
import type { Tier } from '../render/scheduler';

export type SizeParam = 'auto' | 'full' | 'mini';

export interface Params {
  skin: string | null;
  size: SizeParam;
  scenario: string | null;
  speed: number;
  shot: boolean;
  t: number | null;
  run: boolean;
  drawer: 'console' | 'tune' | null;
  slot: SlotId | null;
  tier: Tier | null;
  seed: number;
  frameless: boolean;
}

const SLOT_IDS: SlotId[] = ['high', 'medium', 'low', 'krea'];
const TIERS: Tier[] = ['off', 'calm', 'ambient', 'live'];

function num(v: string | null): number | null {
  if (v === null || v.trim() === '') return null;
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

export function parseParams(search: string): Params {
  const q = new URLSearchParams(search);
  const size = q.get('size');
  const drawer = q.get('drawer');
  const slot = q.get('slot');
  const tier = q.get('tier');
  const speed = num(q.get('speed'));
  const shot = q.get('shot') === '1' || q.get('shot') === 'true';
  return {
    skin: q.get('skin'),
    size: size === 'full' || size === 'mini' ? size : 'auto',
    scenario: q.get('scenario'),
    speed: speed !== null ? Math.min(120, Math.max(0.1, speed)) : 1,
    shot,
    t: (() => {
      const t = num(q.get('t'));
      return t !== null && t >= 0 ? t : null;
    })(),
    run: q.get('run') === '1',
    drawer: drawer === 'console' || drawer === 'tune' ? drawer : null,
    slot: SLOT_IDS.includes(slot as SlotId) ? (slot as SlotId) : null,
    tier: TIERS.includes(tier as Tier) ? (tier as Tier) : null,
    seed: num(q.get('seed')) ?? 0,
    frameless: q.get('frameless') === '1' || q.get('frameless') === 'true',
  };
}
