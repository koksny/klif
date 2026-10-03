import type { SkinTokens } from '../contract';
// Fonts are imported here so they code-split with the skin: IBM Plex Mono for everything (200 for the big
// readouts, 400/500 for the OSD, and the shell drawers' log lines alike).
import '@fontsource/ibm-plex-mono/200.css';
import '@fontsource/ibm-plex-mono/300.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';

export { default } from './Skin.svelte';

const PLEX = '"IBM Plex Mono", Consolas, monospace';

export const tokens: SkinTokens = {
  bg: '#080808',
  surface: '#0E1114',
  surfaceRaised: '#151A1F',
  line: '#1E3A4A',
  ink: '#D9F3FF',
  muted: '#6F8C99',
  accent: '#2BC8FF',
  accentInk: '#00141F',
  warn: '#FFB347',
  danger: '#FF3B3B',
  fontUi: PLEX,
  // must stay a monospace face: the console drawer prints log lines with it
  fontData: PLEX,
  fontDisplay: PLEX,
  radius: '2px',
};
