import type { SkinTokens } from '../contract';
// Fonts are imported here so they code-split with the skin.
import '@fontsource-variable/jetbrains-mono';
import '@fontsource/iosevka/500.css';
import '@fontsource/iosevka/700.css';
import '@fontsource/barlow-condensed/500.css';
import '@fontsource/barlow-condensed/600.css';
import '@fontsource/barlow-condensed/700.css';

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
  fontUi: '"JetBrains Mono Variable", "JetBrains Mono", ui-monospace, monospace',
  fontData: '"JetBrains Mono Variable", "JetBrains Mono", ui-monospace, monospace',
  fontDisplay: '"Iosevka", "JetBrains Mono Variable", ui-monospace, monospace',
  radius: '3px',
};
