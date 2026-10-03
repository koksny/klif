// Small pure helpers of the Tune drawer: error sentences, names, ids, sizes, spec cleaning and comparison.
import type { LlmClass, PresetSpec, System, SystemKind, ViewModel } from '../../model/types';
import { systemLabel } from '../../model/systems';

/** The sentence of a rejected action (the engine rejects with a plain sentence). */
export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  if (e && typeof e === 'object' && 'message' in e) {
    const m = (e as { message: unknown }).message;
    if (typeof m === 'string') return m;
  }
  return String(e);
}

/**
 * Passed to every engine action Tune sends: the player then skips its toast, because Tune shows the refusal
 * inline next to the control (one place, not two).
 */
export const QUIET = { quiet: true } as const;

/** Run an action; resolves to its error sentence, or '' when it worked. Never rejects. */
export async function attempt(run: () => Promise<unknown>): Promise<string> {
  try {
    await run();
    return '';
  } catch (e) {
    return errorText(e) || 'That did not work.';
  }
}

/** "A", "A and B", "A, B and C". */
export function listNames(names: string[]): string {
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** Labels of System ids (the id itself when unknown). */
export function labelsOf(vm: ViewModel, ids: readonly string[]): string[] {
  return ids.map((id) => systemLabel(vm, id));
}

/** The Systems (of one machine) whose active preset is `id`, in tab order. */
export function usersOf(vm: ViewModel, node: string | undefined, id: string): System[] {
  return vm.systems.filter((s) => (s.node ?? '') === (node ?? '') && s.preset === id);
}

/** Copy text to the clipboard (with the old execCommand fallback). */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    try {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand('copy');
      ta.remove();
      return ok;
    } catch {
      return false;
    }
  }
}

/** 12.3 GiB / 640 MiB. */
export function fmtBytes(bytes: number): string {
  const gib = bytes / 1024 ** 3;
  if (gib >= 1) return `${gib >= 100 ? Math.round(gib) : gib.toFixed(1)} GiB`;
  return `${Math.max(1, Math.round(bytes / 1024 ** 2))} MiB`;
}

/** Seconds since epoch -> "2026-10-03". */
export function fmtDate(epochS: number): string {
  const d = new Date(epochS * 1000);
  const p = (n: number) => n.toString().padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// ------------------------------------------------------------------------------------------- ids, labels

const PRESET_ID = /^[a-z0-9][a-z0-9_-]{0,63}$/;
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;

/** Why a preset id cannot be used ('' when it can). Same rules as klif.toml (the core checks again). */
export function presetIdProblem(id: string, taken: readonly string[]): string {
  if (!id) return 'Give the preset an id.';
  if (!PRESET_ID.test(id)) return 'Use lower-case letters, digits, "-" and "_" (up to 64, starting with a letter or digit).';
  if (RESERVED.test(id)) return `"${id}" is a reserved Windows name.`;
  if (taken.includes(id)) return `A preset "${id}" already exists.`;
  return '';
}

/** A preset id derived from free text: "Gemma 4 26B" -> "gemma-4-26b". */
export function slugify(text: string): string {
  const s = text
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^[-_]+|[-_]+$/g, '')
    .slice(0, 64);
  return s && !RESERVED.test(s) ? s : 'preset';
}

/** `base`, else `base-2`, `base-3`... (not in `taken`). */
export function uniqueId(base: string, taken: readonly string[]): string {
  if (!taken.includes(base)) return base;
  for (let n = 2; ; n++) {
    const id = `${base.slice(0, 60)}-${n}`;
    if (!taken.includes(id)) return id;
  }
}

const CLASS_LABEL: Record<LlmClass, string> = { fast: 'System 1', deep: 'System 2', max: 'System 3' };

/**
 * The label a new System gets by default (SPEC 2.2), used to prefill the Label field. The core applies the
 * same rule when no label is sent.
 */
export function defaultSystemLabel(kind: SystemKind, cls: LlmClass | undefined, taken: readonly string[]): string {
  const isTaken = (l: string) => taken.some((t) => t.trim().toLowerCase() === l.toLowerCase());
  let base: string;
  if (kind === 'llm' && !cls) {
    for (let n = 1; ; n++) if (!isTaken(`System ${n}`)) return `System ${n}`;
  }
  if (kind === 'llm') base = CLASS_LABEL[cls!];
  else if (kind === 'image') base = 'System CGI';
  else if (kind === 'tts') base = 'System TTS';
  else if (kind === 'stt') base = 'System STT';
  else base = 'System Video';
  if (!isTaken(base)) return base;
  for (let n = 2; ; n++) if (!isTaken(`${base} (${n})`)) return `${base} (${n})`;
}

// ------------------------------------------------------------------------------------------------- specs

const OPTIONAL_TEXT: (keyof PresetSpec)[] = [
  'name',
  'command',
  'cwd',
  'host',
  'endpoint',
  'health',
  'model',
  'mmproj',
  'gpu',
  'model_name',
  'quant',
  'backend',
  'device',
  'notes',
  'recommended',
];

/**
 * The spec as it is sent: empty optional text fields, empty lists/maps and non-numbers dropped. Tokens and env
 * values are kept exactly as typed (never trimmed, never split).
 */
export function cleanSpec(spec: PresetSpec): PresetSpec {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(spec)) {
    if (v === undefined || v === null) continue;
    if (OPTIONAL_TEXT.includes(k as keyof PresetSpec) && typeof v === 'string' && v.trim() === '') continue;
    if ((k === 'port' || k === 'ctx') && (typeof v !== 'number' || !Number.isFinite(v))) continue;
    if (k === 'args' && Array.isArray(v) && v.length === 0) continue;
    if (k === 'env_remove' && Array.isArray(v)) {
      const names = v.filter((n) => typeof n === 'string' && n.trim() !== '');
      if (names.length) out[k] = names;
      continue;
    }
    if ((k === 'env' || k === 'params') && typeof v === 'object' && Object.keys(v as object).length === 0) continue;
    out[k] = v;
  }
  return out as PresetSpec;
}

/** JSON with sorted keys, so two equal specs compare equal whatever order their keys were written in. */
export function stableJson(v: unknown): string {
  return JSON.stringify(v, (_k, val: unknown) => {
    if (val && typeof val === 'object' && !Array.isArray(val)) {
      const o = val as Record<string, unknown>;
      return Object.fromEntries(Object.keys(o).sort().map((k) => [k, o[k]]));
    }
    return val;
  });
}

/** Comparable form of a spec (what Apply would send). */
export function specKey(spec: PresetSpec): string {
  return stableJson(cleanSpec(spec));
}

/** A deep, plain copy (no proxies) of JSON data. */
export function plain<T>(v: T): T {
  return v === undefined ? v : (JSON.parse(JSON.stringify(v)) as T);
}

/** The adapter a blank preset of this kind starts with. */
export function defaultAdapter(kind: SystemKind): PresetSpec['adapter'] {
  return kind === 'llm' ? 'llama.cpp' : kind === 'image' ? 'sd.cpp' : 'generic';
}
