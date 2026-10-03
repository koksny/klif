import type { SkinTokens } from '../contract';
// Fonts are imported here so they code-split with the skin: Iosevka, a narrow monospace, for everything (the
// terminal glyph atlas, the UI and the shell drawers' log lines alike).
import '@fontsource/iosevka/300.css';
import '@fontsource/iosevka/500.css';
import '@fontsource/iosevka/700.css';

export { default } from './Skin.svelte';

const IOSEVKA = '"Iosevka", "JetBrains Mono", Consolas, ui-monospace, monospace';

export const tokens: SkinTokens = {
  bg: '#010409',
  surface: '#030B16',
  surfaceRaised: '#061A33',
  line: '#12345E',
  ink: '#CFE3F5',
  muted: '#6D8FB3',
  accent: '#2E8BFF',
  accentInk: '#010915',
  warn: '#FFB547',
  danger: '#FF4D6D',
  fontUi: IOSEVKA,
  // must stay a monospace face: the console drawer prints log lines with it
  fontData: IOSEVKA,
  fontDisplay: IOSEVKA,
  radius: '0px',
};
