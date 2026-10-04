// Mock machine facts for the browser engine and the sample view model: a desktop with two discrete GPUs, an
// integrated one (not counted), a 16-core CPU and 96 GB of RAM, with what the core's suggester says about it and
// the records the machine holds (built in ./records.ts; the mock engine keeps its own, changing copy).
import type { HardwareInfo, RecordEntry, RecordEvent, Suggestion } from '../model/types';
import { mockRecordSet } from './records';

export const MOCK_HARDWARE: HardwareInfo = {
  gpus: [
    { id: '1002:7550', name: 'RX 9070 XT', kind: 'gpu', integrated: false, counted: true, vramGiB: 15.92, memoryType: 'GDDR6', tflopsFp32: 48.7, tflopsSource: 'table', detail: '64 CU, 2970 MHz' },
    { id: '1002:731F', name: 'RX 5700 XT', kind: 'gpu', integrated: false, counted: true, vramGiB: 7.98, memoryType: 'GDDR6', tflopsFp32: 9.75, tflopsSource: 'table', detail: '40 CU, 1905 MHz' },
    { id: '1002:13C0', name: 'Radeon Graphics', kind: 'gpu', integrated: true, counted: false, vramGiB: 2.0, tflopsSource: 'unknown' },
  ],
  cpu: { id: 'cpu', name: 'Ryzen 9 9950X3D', kind: 'cpu', integrated: false, counted: true, tflopsFp32: 4.4, tflopsSource: 'computed', detail: '16 cores, AVX-512, 4.3 GHz' },
  ramTotalGiB: 93.6,
  vramPoolGiB: 23.9,
  largestGpuGiB: 15.92,
  unified: false,
  tflopsFp32: 62.85,
  tflopsUnknown: 0,
};

/**
 * What `klif-cli --json suggest` returns for this hardware (checked against the embedded pool, KLIF 0.3.1).
 * Estimates; the rec ids are pool ids, mirrored at the top of the mock catalog (REC_DEFS).
 */
export const MOCK_SUGGESTIONS: Suggestion[] = [
  {
    slot: 'fast', kind: 'llm', class: 'fast', rec: 'gemma-4-12b.qat-ud-q4-k-xl', model: 'Gemma 4 12B', quant: 'UD-Q4_K_XL (QAT)', ctx: 262144, kv: 'f16',
    estVramGiB: 11.9, estRamGiB: 0, budgetVramGiB: 13.9, budgetRamGiB: 70.2,
  },
  {
    slot: 'deep', kind: 'llm', class: 'deep', rec: 'qwen3.8-27b.ud-q4-k-xl', model: 'Qwen 3.8 27B', quant: 'UD-Q4_K_XL', ctx: 98304, kv: 'q8_0',
    estVramGiB: 21.5, estRamGiB: 0, budgetVramGiB: 21.8, budgetRamGiB: 0,
  },
  {
    slot: 'max', kind: 'llm', class: 'max', rec: 'glm-5.3-flash.ud-q2-k-xl', model: 'GLM-5.3-Flash', quant: 'UD-Q2_K_XL', ctx: 131072, kv: 'q8_0',
    estVramGiB: 21.8, estRamGiB: 83.9, budgetVramGiB: 21.8, budgetRamGiB: 84.2, placement: 'experts in RAM',
  },
  {
    slot: 'image', kind: 'image', rec: 'krea-2-turbo.q4-k-m', model: 'Krea 2 Turbo', quant: 'Q4_K_M + Qwen3-VL-4B Q4_K_M',
    estVramGiB: 14.3, estRamGiB: 0, budgetVramGiB: 14.9, budgetRamGiB: 84.2,
  },
  {
    slot: 'tts', kind: 'tts', rec: 'voxcpm2.bf16', model: 'VoxCPM2', quant: 'BF16',
    estVramGiB: 5.4, estRamGiB: 0, budgetVramGiB: 14.9, budgetRamGiB: 84.2,
  },
  {
    slot: 'stt', kind: 'stt', rec: 'whisper-large-v3-turbo.f16', model: 'Whisper large-v3 turbo', quant: 'F16',
    estVramGiB: 2.1, estRamGiB: 0, budgetVramGiB: 14.9, budgetRamGiB: 84.2,
  },
  {
    slot: 'video', kind: 'video', rec: 'minimax-h3-fl2va.q4-k', model: 'MiniMax H3 (FL2VA)', quant: 'Q4_K + Qwen3-VL Q4_K_M',
    estVramGiB: 13.6, estRamGiB: 33.0, budgetVramGiB: 14.9, budgetRamGiB: 84.2, placement: 'weights in RAM (--offload-to-cpu)',
  },
];

/**
 * The static sample's records: the mock machine's, on the sample's clock (SAMPLE_NOW in model/sample.ts, which
 * imports this file: the value is repeated here instead of importing it back).
 */
const SAMPLE_CLOCK = 1_790_000_000;
const SAMPLE_RECORDS = mockRecordSet(SAMPLE_CLOCK);
export const MOCK_RECORDS: RecordEntry[] = SAMPLE_RECORDS.entries;
export const MOCK_RECORD_EVENTS: RecordEvent[] = SAMPLE_RECORDS.events;
