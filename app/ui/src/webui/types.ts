// What klif-webui's server sends (crates/klif-core/src/webui.rs, `web_state`): machines, their GPUs and every
// System, shaped for a phone. Change both together.
import type { ParamView, SystemKind, SystemStatus } from '../lib/model/types';

export interface WebState {
  v: 1;
  now: number;
  /** The skin KLIF's window shows (`[ui] skin`). */
  skin?: string;
  onConflict: 'ask' | 'stop';
  machines: WebMachine[];
  systems: WebSystem[];
  device: { id: string; name: string };
}

export interface WebMachine {
  /** "local" for this machine, else the node id. */
  id: string;
  name: string;
  local: boolean;
  os?: string;
  state: 'connecting' | 'online' | 'offline' | 'unauthorized' | 'incompatible';
  latencyMs?: number;
  error?: string;
  gpus: WebGpu[];
}

export interface WebGpu {
  id: string;
  name: string;
  totalGiB: number;
  usedGiB: number;
  /** In use by something other than a KLIF System on this GPU alone. */
  otherGiB: number;
  unified: boolean;
  parts: { system: string; label: string; gib: number }[];
}

export interface WebSystem {
  id: string;
  label: string;
  /** "local" or the node id. */
  machine: string;
  kind: SystemKind;
  class?: string;
  status: SystemStatus;
  reason?: string;
  model?: string;
  quant?: string;
  preset?: string;
  presets: { id: string; name: string; ready: boolean }[];
  params: ParamView[];
  /** GPU ids ("VEN:DEV", "cpu"); empty: none yet. */
  gpus: string[];
  gpuNames: string[];
  /** Held while it runs, else what it is expected to need. */
  vramGiB?: number;
  metric?: { v: number; u: string };
  /** What the last session reached: "41.3 tok/s · 2 h ago". */
  last?: string;
  load?: { step: string; pct: number };
  uptimeS?: number;
  /** Labels of the running Systems a launch stops first. */
  conflicts: string[];
  controllable: boolean;
  external: boolean;
}
