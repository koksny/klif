import type { SkinTokens } from '../contract';
// Fonts are imported here so they code-split with the skin: Archivo with its width axis (used expanded, 125%)
// for display text and labels, JetBrains Mono for data, small readouts and console log lines.
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource-variable/jetbrains-mono/wght.css';

export { default } from './Skin.svelte';

export const tokens: SkinTokens = {
  bg: '#050302',
  surface: '#0C0703',
  surfaceRaised: '#160B02',
  line: '#4A2A0A',
  ink: '#FFD98A',
  muted: '#A87A3A',
  accent: '#FFB000',
  accentInk: '#1A0D00',
  warn: '#FF6A1A',
  danger: '#FF3B30',
  fontUi: '"Archivo Variable", "Archivo", sans-serif',
  // must stay monospace: the console drawer prints log lines with it
  fontData: '"JetBrains Mono Variable", Consolas, monospace',
  fontDisplay: '"Archivo Variable", "Archivo", sans-serif',
  radius: '0px',
};
