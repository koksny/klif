// KLIF fx "Rings": rings of frosted glass rolling through a glass sphere, reacting to the session (MIT, part of
// KLIF). The look is tdhooper's Bubble Rings (see shader.ts); the motion and the reactions are KLIF's:
//   load    the sphere grows from small and its glow rises as the model loads
//   prefill the rings spin up and glow brighter
//   decode  bulges travel along the rings at a pace set by tok/s; the loop runs faster
//   Krea    the rings turn one notch per sampling step (a full quarter turn per job), always forward
//   context the ring walls thicken a little as the context fills
//   asleep  frozen, dimmed and desaturated; fault: tinted red
// Colours never change otherwise. Every change is continuous, including the loop wrap.

import type { Drive } from '../drive';
import { FxQuad, fitCanvas, hexRgb, mix, mix3 } from '../gl';
import { RINGS_FRAG } from './shader';

/**
 * The resting sphere's projected radius as a share of half the canvas height at zoom 1: the ball (1.85) seen from
 * the eye 13.97 away through focal length 5 (silhouette f·R/sqrt(d²−R²)). For skins that draw around the sphere.
 */
export const SPHERE_R = (5 * 1.85) / Math.sqrt(13.97 ** 2 - 1.85 ** 2);

const RED = [1.7, 0.35, 0.3];
const ONE = [1, 1, 1];
/** The tuned colours: the light at the surfaces and the haze gathered along the ray. */
const CORE = hexRgb('#4CFF99');
const HAZE = hexRgb('#9841B3');

export class RingsVis {
  /** Move the sphere left by this many screen heights (0 = centred). */
  shift = 0;
  /** Move the sphere up by this many screen heights. */
  lift = 0;
  /** Picture scale around the placed centre (1 = reference size). */
  zoom = 1;
  /** Render scale for weak GPUs (0.25..1); the canvas is stretched back to its CSS size. */
  quality = 1;
  private q: FxQuad;
  private loop = 0;
  /** Krea: loop distance still to travel (1/steps per step), consumed smoothly; always forward. */
  private pending = 0;
  private seen = 0;
  private turn = new Float32Array(16);

  constructor(private canvas: HTMLCanvasElement) {
    this.q = new FxQuad(canvas, RINGS_FRAG);
    this.q.v3('uCoreColor', CORE);
    this.q.v3('uHazeColor', HAZE);
    this.q.v2('uHue', 0.7, 0.7);
  }

  resize(cssW: number, cssH: number) {
    fitCanvas(this.canvas, cssW, cssH, this.quality);
  }

  frame(d: Drive, dt: number) {
    let rate = 1 + 4 * d.pre + d.dec * (0.8 + Math.min(d.tps, 120) / 200);
    rate = mix(rate * (1 - d.sleep), 0.3, d.fault);
    if (d.stepsSeen !== this.seen) {
      if (d.steps > 0) this.pending += (d.stepsSeen - this.seen) / d.steps;
      this.seen = d.stepsSeen;
    }
    const take = this.pending * (1 - Math.exp(-dt * 4));
    this.pending -= take;
    // One loop = a quarter turn in two planes of R⁴ at once, which maps the torus onto itself (4 s at rest).
    this.loop += (dt * rate * (1 - 0.85 * d.gen)) / 4 + take;
    this.loop -= Math.floor(this.loop);

    const a = (this.loop * -Math.PI) / 2;
    const c = Math.cos(a), s = Math.sin(a);
    const m = this.turn; // column-major: the z-y and x-w planes turned by the same angle
    m.fill(0);
    m[0] = c; m[3] = -s;
    m[5] = c; m[6] = s;
    m[9] = -s; m[10] = c;
    m[12] = s; m[15] = c;

    const z = this.zoom > 0 ? this.zoom : 1;
    const q = this.q;
    q.m4('uTurn', m);
    q.v2('uPlace', 2 * this.shift, -2 * this.lift);
    q.f('uZoom', z);
    q.f('uBall', 0.35 + 1.5 * d.form);
    q.f('uWall', 0.2 + 0.06 * d.ctx);
    q.f('uGlow', (0.5 + 0.5 * d.form) * (1 + 0.6 * d.pre));
    q.f('uCore', 1 + 0.8 * d.breath);
    q.f('uBeadPhase', d.wave);
    q.f('uBeadDepth', 0.035 * d.dec * (1 - d.sleep));
    q.f('uGain', 1 - 0.5 * d.sleep);
    q.v3('uTint', mix3(ONE, RED, d.fault));
    q.f('uSat', 1 - 0.8 * d.sleep);
    q.draw();
  }

  dispose() {
    this.q.dispose();
  }
}
