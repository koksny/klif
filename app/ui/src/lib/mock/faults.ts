// Fault texts. Shapes follow real failures seen in the session logs; paths and addresses removed.
import type { Fault, LoadStep, LoadStepId } from '../model/types';

export type FaultKind = 'powershell' | 'rocm-crash';

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

/** What the starter script's own error stream showed when it died two seconds after launch. */
export const POWERSHELL_FAIL_STDERR: string[] = [
  'llama-server.exe : 0.00.001.123 I srv  llama_server: initializing ...',
  'At launcher.ps1:90 char:18',
  '+ $deviceOutput = (& $server --list-devices 2>&1 | Out-String)',
  '+                  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~',
  '    + CategoryInfo          : NotSpecified: (0.00.001.123 I ...nitializing ...:String) [], RemoteException',
  '    + FullyQualifiedErrorId : NativeCommandError',
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
    title: 'The starter script failed two seconds after launch.',
    logTail: [...POWERSHELL_FAIL_STDERR],
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
