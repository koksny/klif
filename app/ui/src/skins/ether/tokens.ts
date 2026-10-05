// Ether's design tokens and fonts: the shell's drawers and klif-webui theme themselves with them, without the skin.
import type { SkinTokens } from '../contract';
// Iosevka (data, console log lines) code-splits with the skin; Segoe UI Variable (display, UI) is the system face.
import '@fontsource/iosevka/300.css';
import '@fontsource/iosevka/500.css';

export const tokens: SkinTokens = {
  bg: '#000000',
  surface: '#0A080E',
  surfaceRaised: '#141019',
  line: '#2A2233',
  ink: '#EDE8F5',
  muted: '#9A90A8',
  accent: '#7AB0FF',
  accentInk: '#06101F',
  warn: '#FFB347',
  danger: '#FF5470',
  fontUi: '"Segoe UI Variable Text", "Segoe UI", -apple-system, system-ui, sans-serif',
  // must stay monospace: the console drawer prints log lines with it
  fontData: '"Iosevka", ui-monospace, monospace',
  fontDisplay: '"Segoe UI Variable Display", "Segoe UI", -apple-system, system-ui, sans-serif',
  radius: '10px',
};
