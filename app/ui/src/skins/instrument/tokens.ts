// Instrument's design tokens and fonts: the shell's drawers and klif-webui theme themselves with them, without the skin.
import '@fontsource/barlow/400.css';
import '@fontsource/barlow/500.css';
import '@fontsource/barlow/600.css';
import '@fontsource/barlow-condensed/400.css';
import '@fontsource/barlow-condensed/500.css';
import '@fontsource/barlow-condensed/600.css';
import '@fontsource/barlow-condensed/700.css';
import '@fontsource-variable/archivo/wdth.css';
import '@fontsource-variable/oswald/wght.css';
import '@fontsource/share-tech-mono/400.css';
import type { SkinTokens } from '../contract';
import { FONT, PAL } from './theme';

export const tokens: SkinTokens = {
  bg: PAL.graphite,
  surface: '#202124',
  surfaceRaised: PAL.panel,
  line: '#3A3B3F',
  ink: PAL.cream,
  muted: PAL.muted,
  accent: PAL.cyan,
  accentInk: PAL.cyanInk,
  warn: '#F2A33A',
  danger: PAL.orange,
  // The drawers read as the panel: Barlow text, engraved Barlow Condensed headings, mono for the console.
  fontUi: FONT.text,
  fontData: FONT.mono,
  fontDisplay: FONT.label,
  radius: '5px',
};
