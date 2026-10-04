import type { SkinTokens } from '../../skins/contract';

/** Publish the active skin's tokens as CSS variables on :root so shared surfaces match the skin. */
export function applyTokens(t: SkinTokens) {
  const s = document.documentElement.style;
  s.setProperty('--k-bg', t.bg);
  s.setProperty('--k-surface', t.surface);
  s.setProperty('--k-surface-raised', t.surfaceRaised);
  s.setProperty('--k-line', t.line);
  s.setProperty('--k-ink', t.ink);
  s.setProperty('--k-muted', t.muted);
  s.setProperty('--k-accent', t.accent);
  s.setProperty('--k-accent-ink', t.accentInk);
  s.setProperty('--k-warn', t.warn);
  s.setProperty('--k-danger', t.danger);
  s.setProperty('--k-record', t.record ?? t.warn);
  s.setProperty('--k-font-ui', t.fontUi);
  s.setProperty('--k-font-data', t.fontData);
  s.setProperty('--k-font-display', t.fontDisplay);
  s.setProperty('--k-radius', t.radius);
}
