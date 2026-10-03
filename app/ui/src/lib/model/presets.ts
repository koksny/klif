// Scoped presets (SPEC 16.15): a System's preset is (system.node, system.preset). vm.presets are the LOCAL
// presets; a remote node's presets are vm.nodes[n].presets (masked, node = that node's id).
import type { PresetInfo, System, ViewModel } from './types';

/** The presets a System can choose from: its node's presets when it is remote, else the local ones. Same kind only. */
export function presetsFor(vm: ViewModel, system: System | null | undefined): PresetInfo[] {
  if (!system) return [];
  const pool = system.node ? (vm.nodes.find((n) => n.id === system.node)?.presets ?? []) : vm.presets;
  return pool.filter((p) => p.kind === system.kind);
}

/** The preset a System runs (undefined when it has none or the preset is gone). */
export function activePreset(vm: ViewModel, system: System | null | undefined): PresetInfo | undefined {
  if (!system?.preset) return undefined;
  return presetsFor(vm, system).find((p) => p.id === system.preset);
}

/** Port must fit a u16 (1..65535) and ctx a u32, or the core cannot even read the preset: one sentence each. */
export function portProblem(port: unknown): string {
  if (port === undefined || port === null) return '';
  return typeof port === 'number' && Number.isInteger(port) && port >= 1 && port <= 65535 ? '' : 'Port must be a whole number from 1 to 65535.';
}

export function ctxProblem(ctx: unknown): string {
  if (ctx === undefined || ctx === null) return '';
  return typeof ctx === 'number' && Number.isInteger(ctx) && ctx >= 0 && ctx <= 4294967295 ? '' : 'Ctx must be a whole number from 0 to 4294967295.';
}

/** The first number of a preset spec the core would refuse (port, then ctx), or ''. */
export function specNumberProblem(spec: { port?: unknown; ctx?: unknown }): string {
  return portProblem(spec.port) || ctxProblem(spec.ctx);
}
