// URL contract (skin agents depend on it; keep exactly):
//   ?skin=<id>&size=full|mini|auto&scenario=<name>&speed=<n>&shot=1
// Extras: t=<sim seconds to start at>, run=1 (keep ticking even with shot=1), drawer=console|tune,
//         system=<System id for the tune drawer> (slot= is the old name), add=1 (the drawer opens in add mode),
//         tier=off|calm|ambient|live (pin the tier), seed=<n>,
//         frameless=1 (host.frameless: the skin draws its own window controls).
import type { SystemId } from '../model/types';
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
  /** The System the Tune drawer opens on. */
  system: SystemId | null;
  /** The Tune drawer opens in add mode ("Add System"). */
  add: boolean;
  tier: Tier | null;
  seed: number;
  frameless: boolean;
}

/** A System id: [a-z0-9][a-z0-9_-]{0,31}, optionally prefixed "<node>/" for a remote System. */
const SYSTEM_ID = /^[a-z0-9][a-z0-9_-]{0,31}(\/[a-z0-9][a-z0-9_-]{0,31})?$/;
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
  const system = q.get('system') ?? q.get('slot');
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
    system: system !== null && SYSTEM_ID.test(system) ? system : null,
    add: q.get('add') === '1' || q.get('add') === 'true',
    tier: TIERS.includes(tier as Tier) ? (tier as Tier) : null,
    seed: num(q.get('seed')) ?? 0,
    frameless: q.get('frameless') === '1' || q.get('frameless') === 'true',
  };
}
