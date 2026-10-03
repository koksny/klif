// KLIF fx: the view model reduced to a few smoothed signals every animated background reads the same way.
// With nothing running every signal sits at its rest value, so every visual shows its resting look.

import type { ViewModel } from '../model/types';

const approach = (v: number, to: number, k: number, dt: number) => v + (to - v) * (1 - Math.exp(-k * dt));

export class Drive {
  /** 1 = the shape is fully there (idle, live); 0 = not yet / gone (starting, stopping). Load drives it in between. */
  form = 1;
  /** Prefill pressure: 0, or 0.4..1 with the prompt's progress. */
  pre = 0;
  /** 1 while decoding. */
  dec = 0;
  /** Smoothed decode tok/s (holds while decoding, decays after). */
  tps = 0;
  /** Context fill 0..1 (0 without an LLM session). */
  ctx = 0;
  /** 1 while the GPU sleeps (paged out). */
  sleep = 0;
  /** 1 in a fault. */
  fault = 0;
  /** 1 while Krea samples. */
  gen = 0;
  /** Krea progress step/steps (0 when not sampling). */
  stepFrac = 0;
  steps = 0;
  step = 0;
  /** Token pulse, 1 on each shown token, decays fast. Shown tokens are capped at 10/s. */
  pulse = 0;
  /** Step pulse, 1 on each Krea step, decays slower. */
  stepPulse = 0;
  /** Smooth breath on each Krea step: rises in ~0.15 s, falls in ~0.5 s (never a jump). */
  breath = 0;
  /** Decode ripple phase in cycles: 1.2 Hz plus tok/s / 60 (capped), continuous. */
  wave = 0;
  /** Count of shown tokens and steps so far (visuals that need discrete events). */
  tokens = 0;
  stepsSeen = 0;

  private tokPhase = 0;
  private lastStep = -1;

  update(vm: ViewModel, dt: number) {
    const s = vm.session;
    const phase = s?.phase ?? 'idle';
    const llm = s?.llm ?? null;
    const img = s?.image ?? null;
    const dormant = phase === 'live' && !!vm.vram.dormant;

    const formTo =
      phase === 'starting' ? 0 : phase === 'loading' ? Math.max(0.02, s?.loading?.fraction ?? 0) : phase === 'stopping' ? 0.12 : 1;
    this.form = approach(this.form, formTo, formTo < this.form ? 2.2 : 3, dt);

    const preTo = llm?.activity === 'prefill' && llm.prefill ? 0.4 + 0.6 * (llm.prefill.tokens > 0 ? llm.prefill.doneTokens / llm.prefill.tokens : 0) : 0;
    this.pre = approach(this.pre, preTo, 4, dt);

    const decoding = llm?.activity === 'decode' && !dormant;
    this.dec = approach(this.dec, decoding ? 1 : 0, 3, dt);
    this.tps = approach(this.tps, decoding ? llm!.decodeTps : 0, decoding ? 2 : 0.8, dt);

    const ctxTo = llm && llm.context.totalTokens > 0 ? Math.min(1, llm.context.usedTokens / llm.context.totalTokens) : 0;
    this.ctx = approach(this.ctx, ctxTo, 1.5, dt);

    this.sleep = approach(this.sleep, dormant ? 1 : 0, dormant ? 1.4 : 3, dt);
    this.fault = approach(this.fault, phase === 'fault' ? 1 : 0, 2.5, dt);

    const sampling = img?.activity === 'generating' && img.steps > 0;
    this.gen = approach(this.gen, sampling ? 1 : 0, 3, dt);
    this.steps = sampling ? img!.steps : 0;
    this.step = sampling ? img!.step : 0;
    this.stepFrac = sampling ? img!.step / img!.steps : 0;
    if (sampling && img!.step !== this.lastStep) {
      if (this.lastStep >= 0) {
        this.stepPulse = 1;
        this.stepsSeen++;
      }
      this.lastStep = img!.step;
    } else if (!sampling) this.lastStep = -1;
    this.stepPulse *= Math.exp(-dt * 3);
    this.breath = approach(this.breath, this.stepPulse, 8, dt);
    this.wave += dt * (1.2 + Math.min(this.tps, 120) / 60);

    if (decoding) {
      this.tokPhase += dt * Math.min(10, Math.max(0.5, llm!.decodeTps));
      if (this.tokPhase >= 1) {
        this.tokPhase -= Math.floor(this.tokPhase);
        this.pulse = 1;
        this.tokens++;
      }
    }
    this.pulse *= Math.exp(-dt * 7);
  }
}

export const mix = (a: number, b: number, t: number) => a + (b - a) * t;
export const mix3 = (a: number[], b: number[], t: number) => a.map((v, i) => v + (b[i] - v) * t);
