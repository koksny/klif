// KLIF fx "Ether": a glowing plasma cloud in black that reacts to the session (MIT, part of KLIF).
// The look is nimitz's Ether (see ether.glsl.ts); the motion and the reactions are KLIF's:
//   load    the cloud condenses out of nothing as the model loads
//   prefill the field boils (faster, deeper warp, brighter)
//   decode  echoes travel outward along the surface, at a pace set by tok/s; the clock runs faster
//   Krea    a fine grain covers the surface and eases away with every sampling step
//   context the cloud grows a little as the context fills
//   asleep  frozen, dimmed and desaturated; fault: tinted red
// Colours never change otherwise. Every change is continuous.

import type { Drive } from '../drive';
import { FxQuad, fitCanvas, mix, mix3 } from '../gl';
import { ETHER_FRAG } from './ether.glsl';

const RED = [1.7, 0.35, 0.3];
const ONE = [1, 1, 1];

export class EtherVis {
  /** Move the cloud left by this many screen heights (0 = the reference composition, centre at 0.9, 0.5). */
  shift = 0;
  /** Move the cloud up by this many screen heights. */
  lift = 0;
  /** Render scale for weak GPUs (0.25..1); the canvas is stretched back to its CSS size. */
  quality = 1;
  private q: FxQuad;
  private t = 0;
  private grain = 0;
  private spin = new Float32Array(9);

  constructor(private canvas: HTMLCanvasElement) {
    this.q = new FxQuad(canvas, ETHER_FRAG);
  }

  resize(cssW: number, cssH: number) {
    fitCanvas(this.canvas, cssW, cssH, this.quality);
  }

  frame(d: Drive, dt: number) {
    let rate = 1 + 3 * d.pre + d.dec * (0.8 + Math.min(d.tps, 120) / 150) + 0.5 * d.gen;
    rate = mix(rate * (1 - d.sleep), 0.35, d.fault);
    this.t += dt * rate;
    const t = this.t;

    // The tumble: a turn about y (rate 0.4) then about z (rate 0.3), as one matrix for the whole frame.
    const c1 = Math.cos(0.4 * t), s1 = Math.sin(0.4 * t);
    const c2 = Math.cos(0.3 * t), s2 = Math.sin(0.3 * t);
    const m = this.spin; // column-major
    m[0] = c2 * c1; m[1] = s2 * c1; m[2] = s1;
    m[3] = -s2;     m[4] = c2;      m[5] = 0;
    m[6] = -c2 * s1; m[7] = -s2 * s1; m[8] = c1;

    const grainTo = d.steps > 0 ? 0.04 + 0.3 * (1 - d.stepFrac) : 0;
    this.grain += (grainTo * d.gen - this.grain) * (1 - Math.exp(-dt * 2.5));

    const q = this.q;
    q.m3('uSpin', m);
    q.f('uTime', t);
    q.f('uWobble', Math.sin(0.7 * t));
    q.f('uShells', 5);
    q.v2('uCenter', 0.9 - this.shift, 0.5 + this.lift);
    q.f('uWarp', 0.5 + 0.4 * d.pre + 0.3 * d.fault);
    q.f('uRadius', 1 + 0.2 * d.ctx);
    q.f('uForm', (1 - d.form) * 3);
    q.f('uRipple', 0.11 * d.dec * (1 - d.sleep));
    q.f('uRipplePhase', d.wave);
    q.f('uGrain', this.grain * (1 - d.sleep));
    q.f('uGain', (1 + 0.45 * d.pre + 0.4 * d.breath) * (1 - 0.45 * d.sleep));
    q.v3('uTint', mix3(ONE, RED, d.fault));
    q.f('uSat', 1 - 0.8 * d.sleep);
    q.draw();
  }

  dispose() {
    this.q.dispose();
  }
}
