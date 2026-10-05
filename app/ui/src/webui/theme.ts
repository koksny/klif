// klif-webui wears the skin KLIF's window shows: only its tokens and fonts (skins/<id>/tokens.ts), never the skin
// itself. The last one is remembered so the next visit starts in it.
import { applyTokens } from '../lib/shell/tokens';
import type { SkinTokens } from '../skins/contract';

const SKIN_KEY = 'klif.webui.skin';
const DEFAULT = 'cliff';
const tokenModules = import.meta.glob<{ tokens: SkinTokens }>('../skins/*/tokens.ts');

let applied: string | null = null;

function remembered(): string {
  try {
    return localStorage.getItem(SKIN_KEY) ?? DEFAULT;
  } catch {
    return DEFAULT;
  }
}

/** Apply the skin `id` (unknown: the remembered one, else Cliff). */
export async function applySkin(id?: string | null): Promise<void> {
  const want = id && tokenModules[`../skins/${id}/tokens.ts`] ? id : applied ?? remembered();
  if (want === applied) return;
  const load = tokenModules[`../skins/${want}/tokens.ts`] ?? tokenModules[`../skins/${DEFAULT}/tokens.ts`];
  const { tokens } = await load();
  applyTokens(tokens);
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', tokens.bg);
  applied = want;
  try {
    localStorage.setItem(SKIN_KEY, want);
  } catch {
    /* not remembered: the next visit starts in Cliff until the state arrives */
  }
}
