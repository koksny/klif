// Boot sequence planner: process -> device -> weights -> kv -> warmup -> ready, with the VRAM layers
// building up as the real loader fills memory. Durations come from real log timestamps where logs
// exist (weights, kv/projector/draft) and from estimates where they do not (process, device, warmup).
import type { LoadStep, LoadStepId, ModelRef, VramLayer } from '../model/types';
import { clamp } from './rng';

export interface BootDurations {
  processS: number;
  deviceS: number;
  weightsS: number;
  kvS: number;
  warmupS: number;
}

export interface BootPlan {
  d: BootDurations;
  total: number;
  kind: 'llm' | 'image';
  model: ModelRef;
  /** Layers the slot will hold once live. */
  target: VramLayer[];
  deviceDetail: string;
  port: number;
}

export function planBoot(
  d: BootDurations,
  kind: 'llm' | 'image',
  model: ModelRef,
  target: VramLayer[],
  deviceDetail: string,
  port: number,
): BootPlan {
  return { d, total: d.processS + d.deviceS + d.weightsS + d.kvS + d.warmupS, kind, model, target, deviceDetail, port };
}

export interface BootView {
  phase: 'starting' | 'loading' | 'live';
  steps: LoadStep[];
  fraction: number;
  layers: VramLayer[];
}

const easeOut = (u: number) => 1 - Math.pow(1 - clamp(u, 0, 1), 1.3);

function stepLabel(id: LoadStepId, kind: 'llm' | 'image'): string {
  switch (id) {
    case 'process':
      return 'Start process';
    case 'device':
      return 'Find device';
    case 'weights':
      return kind === 'llm' ? 'Load weights' : 'Load diffusion weights';
    case 'kv':
      return kind === 'llm' ? 'Allocate KV cache' : 'Load text encoder and VAE';
    case 'warmup':
      return 'Warm up';
    case 'ready':
      return 'Ready';
  }
}

export function bootView(plan: BootPlan, elapsed: number): BootView {
  const { d } = plan;
  const order: [LoadStepId, number][] = [
    ['process', d.processS],
    ['device', d.deviceS],
    ['weights', d.weightsS],
    ['kv', d.kvS],
    ['warmup', d.warmupS],
  ];
  let t0 = 0;
  const prog: Record<string, number> = {};
  const steps: LoadStep[] = [];
  let doneDur = 0;
  for (const [id, dur] of order) {
    const u = clamp((elapsed - t0) / Math.max(1e-6, dur), 0, 1);
    prog[id] = id === 'weights' ? easeOut(u) : u;
    const state: LoadStep['state'] = elapsed >= t0 + dur ? 'done' : elapsed >= t0 ? 'active' : 'pending';
    if (state === 'done') doneDur += dur;
    else if (state === 'active') doneDur += dur * prog[id];
    steps.push({ id, label: stepLabel(id, plan.kind), state, detail: detail(plan, id, state, prog[id]) });
    t0 += dur;
  }
  const allDone = elapsed >= plan.total;
  steps.push({
    id: 'ready',
    label: 'Ready',
    state: allDone ? 'done' : 'pending',
    detail: allDone ? `127.0.0.1:${plan.port}` : undefined,
  });

  const phase: BootView['phase'] = allDone ? 'live' : elapsed < d.processS + d.deviceS ? 'starting' : 'loading';
  // Layers: weights follow the weights step; kv/draft/projector/VAE follow the kv step; buffers follow warm-up.
  const layers: VramLayer[] = [];
  for (const l of plan.target) {
    let p: number;
    if (l.id === 'weights') p = prog.weights;
    else if (l.id === 'buffers') p = prog.warmup;
    else p = prog.kv;
    const gib = Math.round(l.gib * p * 100) / 100;
    if (gib > 0.004) layers.push({ ...l, gib });
  }
  return { phase, steps, fraction: clamp(doneDur / plan.total, 0, 1), layers };
}

function detail(plan: BootPlan, id: LoadStepId, state: LoadStep['state'], p: number): string | undefined {
  const m = plan.model;
  switch (id) {
    case 'process':
      return `${m.engine} · ${m.backend}`;
    case 'device':
      return state === 'pending' ? undefined : plan.deviceDetail;
    case 'weights': {
      const total = m.weightsGiB ?? plan.target.find((l) => l.id === 'weights')?.gib ?? 0;
      return state === 'pending' ? undefined : `${(total * p).toFixed(1)} / ${total.toFixed(1)} GiB`;
    }
    case 'kv':
      if (state === 'pending') return undefined;
      if (plan.kind === 'image') return 'text encoder + VAE';
      return `${Math.round((m.ctxTokens ?? 0) / 1024)}k · ${m.kvType ?? 'q8_0'}`;
    default:
      return undefined;
  }
}

/** Console lines of the real servers, with the time (seconds from launch) at which each appears. */
export interface BootLine {
  at: number;
  /** Seconds since the server process started (llama.cpp timestamps); -1 for untimed lines. */
  proc: number;
  text: string;
  level: 'I' | 'W';
}

export function bootLines(plan: BootPlan): BootLine[] {
  const { d, model: m } = plan;
  const start = d.processS + d.deviceS;
  const wEnd = start + d.weightsS;
  const kEnd = wEnd + d.kvS;
  const out: BootLine[] = [];
  const llama = (at: number, text: string) => out.push({ at, proc: at - start, text, level: 'I' });
  if (plan.kind === 'llm') {
    llama(start + 0.03, 'cmn  common_param: common_params_print_info: verbosity = 3 (adjust with the `-lv N` CLI arg)');
    llama(start + 0.04, `srv    load_model: loading model ${m.name} ${m.quant}`);
    llama(wEnd, 'cmn          init: llama threadpool init, n_threads = 16');
    if (m.specMode && /MTP/i.test(m.specMode)) {
      llama(wEnd + 0.2, 'common_speculative_init_result: creating MTP draft context against the target model');
    }
    if (m.vision) llama(kEnd - 0.12, 'srv    load_model: loaded multimodal model, projector on RAM');
    llama(kEnd - 0.06, `srv    load_model: initializing, n_slots = 1, n_ctx_slot = ${m.ctxTokens ?? 0}, kv_unified = 'false'`);
    llama(kEnd, 'srv  llama_server: model loaded');
    llama(kEnd + 0.01, `srv  llama_server: listening on http://127.0.0.1:${plan.port}`);
  } else {
    const size = m.imageSize ?? '512x768';
    const ram = ((m.weightsGiB ?? 7.4) * 1024 * 1.58).toFixed(2);
    const raw = (at: number, text: string) => out.push({ at, proc: -1, text, level: 'I' });
    raw(d.processS * 0.5, `Launching ${m.name} at ${size} via ${m.backend}`);
    raw(d.processS, `Image server: http://127.0.0.1:${plan.port}/`);
    raw(d.processS + d.deviceS, `GPU: ${m.device} (${plan.deviceDetail.replace(' · ', ' / ')})`);
    raw(start + 0.05, '[INFO ] stable-diffusion.cpp:717  - loading diffusion model');
    raw(start + d.weightsS * 0.6, '[INFO ] stable-diffusion.cpp:774  - loading llm (text encoder)');
    raw(wEnd, '[INFO ] stable-diffusion.cpp:788  - loading vae');
    raw(wEnd + d.kvS * 0.5, '[INFO ] stable-diffusion.cpp:905  - Version: Krea2');
    raw(wEnd + d.kvS * 0.7, '[INFO ] stable-diffusion.cpp:1605 - Using flash attention');
    raw(kEnd, `[INFO ] stable-diffusion.cpp:1742 - total params memory size = ${ram}MB (VRAM 0.00MB, RAM ${ram}MB)`);
    raw(kEnd + 0.02, '[INFO ] stable-diffusion.cpp:1852 - running in Flux FLOW mode');
    raw(kEnd + 0.04, `[INFO ] main.cpp:148  - listening on: http://127.0.0.1:${plan.port}`);
  }
  return out;
}
