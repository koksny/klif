// Rings's design tokens and fonts: the shell's drawers and klif-webui theme themselves with them, without the skin.
import type { SkinTokens } from '../contract';
// Fonts are imported here so they code-split with the skin: Barlow for the UI and the hero figures, Barlow
// Condensed for caps labels, Share Tech Mono for log lines (the shell's console drawer prints with fontData).
import '@fontsource/barlow/200.css';
import '@fontsource/barlow/300.css';
import '@fontsource/barlow/400.css';
import '@fontsource/barlow/500.css';
import '@fontsource/barlow/600.css';
import '@fontsource/barlow-condensed/500.css';
import '@fontsource/barlow-condensed/600.css';
import '@fontsource/share-tech-mono/400.css';

const BARLOW = '"Barlow", "Segoe UI", system-ui, sans-serif';

export const tokens: SkinTokens = {
  bg: '#191A26',
  surface: '#1E2030',
  surfaceRaised: '#252839',
  line: '#33405A',
  ink: '#E3F7FF',
  muted: '#8FA9B8',
  accent: '#39E1FF',
  accentInk: '#04202A',
  warn: '#FFB347',
  danger: '#FF5C7A',
  fontUi: BARLOW,
  // must stay monospace: the console drawer prints log lines with it
  fontData: '"Share Tech Mono", ui-monospace, monospace',
  fontDisplay: BARLOW,
  radius: '14px',
};
