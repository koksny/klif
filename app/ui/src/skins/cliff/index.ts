// Cliff (Urwisko): coastal topography. VRAM as a sea cliff, system RAM as the sea.
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource-variable/ibm-plex-sans/wght.css';
// Italic: hydrography labels in the scene (the sea is System RAM).
import '@fontsource-variable/ibm-plex-sans/wght-italic.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import './cliff.css';
import type { SkinTokens } from '../contract';
export { default } from './Skin.svelte';

export const tokens: SkinTokens = {
  bg: '#0F1316',
  surface: '#1D252A',
  surfaceRaised: '#252F35',
  line: '#2C363C',
  ink: '#DCEFF8',
  muted: '#8FA3AE',
  accent: '#5AB6EB',
  accentInk: '#0F1316',
  warn: '#F2A33A',
  danger: '#E8645A',
  fontUi: "'IBM Plex Sans Variable', 'IBM Plex Sans', system-ui, sans-serif",
  fontData: "'IBM Plex Mono', ui-monospace, monospace",
  fontDisplay: "'Archivo Variable', 'Archivo', system-ui, sans-serif",
  radius: '8px',
};
