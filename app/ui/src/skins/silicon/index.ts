import type { SkinTokens } from '../contract';
// Fonts are imported here so they code-split with the skin. The UI face is Bahnschrift (part of Windows);
// the mono face is bundled for console text, ports and keys.
import '@fontsource-variable/jetbrains-mono';

export { default } from './Skin.svelte';

export const tokens: SkinTokens = {
  bg: '#0D1115',
  surface: '#0E151B',
  surfaceRaised: '#131C24',
  line: '#33424F',
  ink: '#DCE7EF',
  muted: '#7C8F9E',
  accent: '#5AB6EB',
  accentInk: '#04121B',
  warn: '#F2B33A',
  danger: '#FF5A36',
  // DIN 1451 (Bahnschrift, part of Windows) for the shell drawers too (see palette.ts); the mono face for
  // console log lines.
  fontUi: '"Bahnschrift", "DIN Alternate", "Barlow", "Segoe UI", sans-serif',
  fontData: '"JetBrains Mono Variable", "JetBrains Mono", ui-monospace, monospace',
  fontDisplay: '"Bahnschrift", "DIN Alternate", "Barlow", "Segoe UI", sans-serif',
  radius: '3px',
};
