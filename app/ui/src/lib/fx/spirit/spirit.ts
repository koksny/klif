// KLIF fx "Spirit": Edan Kwan's The Spirit (MIT, github.com/edankwan/The-Spirit), a curl-noise particle smoke,
// run from its original build in an iframe (app/ui/public/spirit/) and driven from here. The build is patched to
// expose its settings (__spirit), its intro progress (__spiritJ) and one frame step (__spiritFrame; with
// #external=1 it schedules nothing itself), so KLIF's frame tiers drive it.

import type { Drive } from '../drive';
import { hexRgb as hex, mix, mix3 } from '../gl';

/** The iframe source: 1m particles, frames driven by KLIF. */
export const SPIRIT_SRC = '/spirit/index.html#amount=1m&external=1';

interface SpiritSettings {
  speed: number;
  dieSpeed: number;
  radius: number;
  curlSize: number;
  attraction: number;
  shadowDarkness: number;
  color1: string;
  color2: string;
  bgColor: string;
  followMouse: boolean;
  useTriangleParticles: boolean;
  fxaa: boolean;
  motionBlur: boolean;
  motionBlurPause: boolean;
  bloom: boolean;
}
type SpiritWin = Window & { __spirit?: SpiritSettings; __spiritJ?: number | null; __spiritJNow?: number; __spiritFrame?: () => void };

// The tuned settings, in the simulator's own units.
const SPIRIT_BASE = { speed: 0.5, dieSpeed: 0.009, radius: 1.13, curlSize: 0.009, attraction: -1.71, shadow: 0.61 };
const C1 = hex('#2BC8FF');
const C2 = hex('#003A57');
const C1_SLEEP = hex('#4E6670');
const C2_SLEEP = hex('#151D22');
const C1_RED = hex('#FF4D4D');
const C2_RED = hex('#4A0A0A');
const toHex = (c: number[]) => '#' + c.map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, '0')).join('');

export class SpiritVis {
  shift = 0;
  private wasForming = false;

  constructor(private frame_: HTMLIFrameElement) {}

  private get win(): SpiritWin | null {
    return (this.frame_.contentWindow as SpiritWin | null) ?? null;
  }

  resize() {}

  frame(d: Drive) {
    const w = this.win;
    const s = w?.__spirit;
    if (!w || !s) return;
    s.followMouse = false;
    s.useTriangleParticles = true;
    s.fxaa = true;
    s.motionBlur = true;
    s.bloom = true;
    s.bgColor = '#080808';
    s.shadowDarkness = SPIRIT_BASE.shadow;

    // Load: the original intro (smoke rises from below and gathers) follows the load fraction.
    const forming = d.form < 0.995;
    if (forming) w.__spiritJ = Math.max(0.001, d.form);
    else if (this.wasForming) w.__spiritJ = null;
    this.wasForming = forming;

    const still = 1 - d.sleep;
    const tps = Math.min(d.tps, 100);
    // Decode: the head speeds up and the smoke streams out behind it as a long, smooth ribbon (comet tail)
    // instead of the idle billow. Krea: faster too, with a contraction on every sampling step.
    s.speed = (SPIRIT_BASE.speed + 0.3 * d.pre + d.dec * (0.45 + 0.2 * (tps / 100)) + 0.35 * d.gen) * still;
    s.dieSpeed = ((SPIRIT_BASE.dieSpeed * (1 - 0.25 * d.dec)) / (1 + 0.35 * d.ctx)) * still;
    s.motionBlurPause = d.sleep > 0.9;
    s.radius = SPIRIT_BASE.radius;
    s.curlSize = mix(mix(SPIRIT_BASE.curlSize, 0.005, d.dec), 0.006, d.pre);
    // Prefill pulls the smoke into a dense ball (the prompt being absorbed); Krea steps breathe it in.
    // Decode: the smoke gathers into a tighter trail behind the moving head (weaker push-out).
    s.attraction = mix(mix(SPIRIT_BASE.attraction, -0.35, d.dec), 1.2, d.pre) + 3.2 * d.breath;
    // The tuned colours in every working state; only sleep and fault recolour.
    let c1 = mix3(C1, C1_SLEEP, d.sleep);
    let c2 = mix3(C2, C2_SLEEP, d.sleep);
    c1 = mix3(c1, C1_RED, d.fault);
    c2 = mix3(c2, C2_RED, d.fault);
    s.color1 = toHex(c1);
    s.color2 = toHex(c2);
    // The page runs no loop of its own (#external=1): one simulation + render step per KLIF frame.
    w.__spiritFrame?.();
  }

  dispose() {}
}
