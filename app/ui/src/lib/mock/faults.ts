// Fault texts. Shapes follow real failures seen in the session logs; paths and addresses removed.
import type { Fault, LoadStep, LoadStepId } from '../model/types';

export type FaultKind = 'exit' | 'rocm-crash';

export const ROCM_EXIT_CODE = -1073740791;

/** Stderr lines of the real 2026-09-04 image-server crash (edit job, first sampling step). */
export const ROCM_CRASH_STDERR: string[] = [
  '  Device 2: AMD Radeon RX 9070 XT, gfx1201 (0x1201), VMM: no, Wave Size: 32, VRAM: 16304 MiB',
  '[ERROR] ggml_extend.hpp:72   - ggml_cuda_compute_forward: PAD failed',
  '[ERROR] ggml_extend.hpp:72   - ROCm error: device kernel image is invalid',
  '[ERROR] ggml_extend.hpp:72   -   current device: 2, in function ggml_cuda_compute_forward at ggml-cuda.cu:3128',
  '[ERROR] ggml_extend.hpp:72   -   err',
  'ggml-cuda.cu:104: ROCm error',
];

/** What a llama.cpp server printed when it died two seconds after launch (a bad model file). */
export const EXIT_FAIL_STDERR: string[] = [
  'llama_model_load: error loading model: unable to open file',
  'llama_model_load_from_file_impl: failed to load model',
  'common_init_from_params: failed to load model',
  'srv    load_model: failed to load model, exiting',
  'llama-server.exe exited with code 1.',
];

export function exitCodeHex(code: number): string {
  return '0x' + (code >>> 0).toString(16).toUpperCase().padStart(8, '0');
}

export function makeFault(kind: FaultKind, sinceS: number): Fault {
  if (kind === 'rocm-crash') {
    return {
      title: 'The image server crashed while sampling an edit job.',
      exitCode: ROCM_EXIT_CODE,
      exitCodeHex: exitCodeHex(ROCM_EXIT_CODE),
      logTail: [...ROCM_CRASH_STDERR, `sd-server HIP exited with code ${ROCM_EXIT_CODE}.`],
      sinceS,
    };
  }
  return {
    title: 'The server exited two seconds after launch.',
    exitCode: 1,
    exitCodeHex: exitCodeHex(1),
    logTail: [...EXIT_FAIL_STDERR],
    sinceS,
  };
}

const FAILED_DETAIL: Record<LoadStepId, string> = {
  process: 'process did not start',
  device: 'device listing failed',
  weights: 'weights not loaded',
  kv: 'allocation failed',
  warmup: 'warm-up failed',
  ready: 'never became ready',
};

/** The load steps at the moment the process died: the step that was running is marked failed. */
export function failSteps(steps: LoadStep[]): LoadStep[] {
  return steps.map((s) => (s.state === 'active' ? { ...s, state: 'failed', detail: FAILED_DETAIL[s.id] } : { ...s }));
}
