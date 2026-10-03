// The tower's shape: the model's architecture as the server reported it at load (ModelRef.arch), or a
// fallback drawing when it has not been reported yet. Labels say which one is on screen.
import type { ModelArch, ViewModel } from '../../lib/model/types';
import { fmtInt } from '../../lib/model/format';

export type ShapeKind = 'moe' | 'dense' | 'dit';

export interface Shape {
  kind: ShapeKind;
  layers: number;
  /** Routed experts per layer (MoE). */
  experts: number;
  /** Experts each token is routed to (MoE). */
  active: number;
  /** Always-on shared experts (MoE). */
  shared: number;
  /** False while the layer count is a stand-in drawing, not a reported figure. */
  known: boolean;
  arch: ModelArch | null;
}

/** An LLM never launched yet: a generic dense stack until llama-server prints its shape. */
const GENERIC_LAYERS = 32;
/** sd-server prints no block count: the DiT tower is drawn with the prototype's 28 slabs. */
const DIT_LAYERS = 28;

export function shapeOf(vm: ViewModel): Shape {
  const s = vm.session;
  const slot = vm.slots.find((x) => x.id === (s?.slot ?? vm.selected));
  const kind = slot?.kind ?? (s?.image ? 'image' : 'llm');
  const arch = s?.model.arch ?? slot?.model.arch ?? null;
  const layers = arch && arch.layers >= 2 ? Math.min(256, Math.round(arch.layers)) : 0;
  if (kind === 'image') return { kind: 'dit', layers: layers || DIT_LAYERS, experts: 0, active: 0, shared: 0, known: layers > 0, arch };
  if (!layers || !arch) return { kind: 'dense', layers: GENERIC_LAYERS, experts: 0, active: 0, shared: 0, known: false, arch: null };
  if (arch.experts > 0) {
    const experts = Math.min(4096, Math.round(arch.experts));
    return {
      kind: 'moe',
      layers,
      experts,
      active: Math.max(1, Math.min(experts, Math.round(arch.expertsUsed))),
      shared: Math.max(0, Math.min(8, Math.round(arch.sharedExperts))),
      known: true,
      arch,
    };
  }
  return { kind: 'dense', layers, experts: 0, active: 0, shared: 0, known: true, arch };
}

/** Rebuild key: the tower is rebuilt only when this changes. */
export function shapeKey(s: Shape): string {
  return `${s.kind}:${s.layers}:${s.experts}:${s.active}:${s.shared}`;
}

/** "10 + 1 of 512 experts" / "8 of 128 experts" */
export function expertsText(s: Shape): string {
  return `${s.active}${s.shared ? ` + ${s.shared}` : ''} of ${fmtInt(s.experts)} experts`;
}

/** The middle-layer label in the scene. */
export function layerLabel(s: Shape): string {
  if (!s.known) return s.kind === 'dit' ? 'dit blocks · count not reported' : 'shape unknown until first load';
  const mid = `layer ${Math.floor(s.layers / 2) + 1} / ${s.layers}`;
  if (s.kind === 'moe') return `${mid} · ${expertsText(s)}`;
  return `${mid} · ${s.kind === 'dit' ? 'dit' : 'dense'}`;
}

/** One line for the mini caption: "48 layers · 10 + 1 of 512". */
export function shapeLine(s: Shape): string {
  if (!s.known) return s.kind === 'dit' ? 'dit · blocks not reported' : 'shape unknown until first load';
  if (s.kind === 'moe') return `${s.layers} layers · ${s.active}${s.shared ? ` + ${s.shared}` : ''} of ${fmtInt(s.experts)}`;
  return `${s.layers} layers · ${s.kind}`;
}
