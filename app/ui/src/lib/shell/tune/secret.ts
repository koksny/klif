// The secret rules of klif-common (crates/klif-common/src/secret.rs) as the Tune editor needs them: which env
// names and which argument values are secret, so the editor masks them and refuses to send a clear value to a
// remote node (SPEC 16.10: the payload crosses plain TCP). Keep in step with secret.rs; the core stays the
// authority (it masks everything it sends and resolves MASK on save).
import type { PresetSpec } from '../../model/types';

/** What a masked value looks like. Sent back unchanged it means "keep the stored value". */
export const MASK = '••••';

/** Flags whose following token (or `=value` part) is a secret. */
export const SECRET_ARG_FLAGS = ['--api-key', '--hf-token', '-hft'];

/** Same rule as `is_secret_env`: KEY, TOKEN, SECRET, PASS or AUTH anywhere in the name, case-insensitive. */
export function isSecretEnv(name: string): boolean {
  const n = name.trim().toUpperCase();
  return ['KEY', 'TOKEN', 'SECRET', 'PASS', 'AUTH'].some((p) => n.includes(p));
}

export function isSecretFlag(token: string): boolean {
  return SECRET_ARG_FLAGS.includes(token);
}

/** The secret flag a `--flag=value` token starts with, if any. */
export function secretFlagPrefix(token: string): string | null {
  return SECRET_ARG_FLAGS.find((f) => token.length > f.length && token.startsWith(f) && token[f.length] === '=') ?? null;
}

/** Indexes of the tokens that carry a secret value: the token after a secret flag, and `--flag=value` tokens. */
export function secretArgIndexes(args: readonly string[]): Set<number> {
  const out = new Set<number>();
  for (let i = 0; i < args.length; i++) {
    if (isSecretFlag(args[i]) && i + 1 < args.length) out.add(i + 1);
    else if (secretFlagPrefix(args[i])) out.add(i);
  }
  return out;
}

/** A value that only names a variable of the node's own environment (`{env:NAME}`) carries no secret. */
function envReference(v: string): boolean {
  return /^\{env:[^{}]+\}$/.test(v.trim());
}

/** A secret value in clear: anything but empty, MASK or a `{env:NAME}` reference (the engine's rule). */
function clearValue(v: string): boolean {
  return v !== '' && v !== MASK && !envReference(v);
}

function clearArgs(args: readonly string[] | undefined, where: string, out: string[]) {
  if (!args) return;
  for (const i of secretArgIndexes(args)) {
    const t = args[i];
    const flag = secretFlagPrefix(t);
    const value = flag ? t.slice(flag.length + 1) : t;
    if (clearValue(value)) out.push(`${where}${flag ?? args[i - 1]}`);
  }
}

function clearEnv(env: Record<string, string> | undefined, where: string, out: string[]) {
  if (!env) return;
  for (const [k, v] of Object.entries(env)) {
    if (isSecretEnv(k) && clearValue(v)) out.push(`${where}${k}`);
  }
}

/**
 * The secret values a spec would send in clear (anything but MASK), named for the message. Empty = safe to send
 * to a remote node.
 */
export function clearSecrets(spec: PresetSpec): string[] {
  const out: string[] = [];
  clearEnv(spec.env, '', out);
  clearArgs(spec.args, '', out);
  for (const [name, p] of Object.entries(spec.params ?? {})) {
    for (const [value, c] of Object.entries(p.choices ?? {})) {
      clearEnv(c.env, `${name}.${value}: `, out);
      clearArgs(c.args, `${name}.${value}: `, out);
    }
  }
  return out;
}

function maskArgs(args: string[]): string[] {
  const idx = secretArgIndexes(args);
  return args.map((t, i) => {
    if (!idx.has(i)) return t;
    const flag = secretFlagPrefix(t);
    if (flag) return clearValue(t.slice(flag.length + 1)) ? `${flag}=${MASK}` : t;
    return clearValue(t) ? MASK : t;
  });
}

function maskEnv(env: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.entries(env).map(([k, v]) => [k, isSecretEnv(k) && clearValue(v) ? MASK : v]));
}

/**
 * A copy of the spec with every value `clearSecrets` reports replaced by MASK (top-level and in every param
 * choice). For a remote node's command preview: the node masks secrets in what it returns anyway, so the preview
 * loses nothing, and a typed secret never crosses plain TCP (SPEC 16.10).
 */
export function maskClearSecrets(spec: PresetSpec): PresetSpec {
  const out: PresetSpec = JSON.parse(JSON.stringify(spec));
  if (out.env) out.env = maskEnv(out.env);
  if (out.args) out.args = maskArgs(out.args);
  for (const p of Object.values(out.params ?? {})) {
    for (const c of Object.values(p.choices ?? {})) {
      if (c.env) c.env = maskEnv(c.env);
      if (c.args) c.args = maskArgs(c.args);
    }
  }
  return out;
}

/** The sentence Tune shows when a remote preset carries a clear secret (SPEC 16.10). */
export const REMOTE_SECRET_REFUSAL = "Secrets cannot cross plain TCP; set them in that node's klif.toml";
