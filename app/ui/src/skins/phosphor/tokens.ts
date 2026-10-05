// Phosphor's design tokens and fonts: the shell's drawers and klif-webui theme themselves with them, without the skin.
import '@fontsource-variable/saira/wdth.css';
import '@fontsource/share-tech-mono/index.css';
import type { SkinTokens } from '../contract';
import { FONT_DISPLAY, FONT_MONO, FONT_UI, P } from './palette';

export const tokens: SkinTokens = {
  bg: P.glass,
  surface: '#071014',
  surfaceRaised: '#0B1A20',
  line: P.rule,
  ink: '#CDEFF8',
  muted: P.muted,
  accent: P.cyan,
  accentInk: P.glass,
  warn: P.amber,
  danger: P.danger,
  // Saira for the shell drawers too (see palette.ts); the mono face for console log lines.
  fontUi: FONT_UI,
  fontData: FONT_MONO,
  fontDisplay: FONT_DISPLAY,
  radius: '6px',
};
