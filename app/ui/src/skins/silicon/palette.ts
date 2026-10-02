// Silicon palette: an engineering drawing on drafting charcoal.
// Cyan = live/filled, white-hot = key numbers, red-orange = limits only.
export const C = {
  bg: '#0D1115',
  panel: '#0E151B',
  dieFill: '#0F171E',
  grid: '#1A222A',
  gridFine: '#141B21',
  line: '#33424F',
  lineDim: '#25323D',
  lineBright: '#4E6779',
  lineHi: '#8FB4CC',
  cyan: '#5AB6EB',
  cyanLit: '#6FC6F5',
  cyanDeep: '#175F8F',
  cyanMid: '#2479AC',
  cyanSoft: '#3E9AD0',
  cellDim: '#20313C',
  hot: '#F4FAFF',
  ink: '#D8E4EC',
  inkDim: '#AFC2CF',
  muted: '#7C8F9E',
  label: '#86C3E6',
  red: '#FF5A36',
  amber: '#F2B33A',
} as const;

// DIN 1451 (Bahnschrift, shipped with Windows) is the lettering standard of technical drawings: labels,
// numbers and hero readouts. The mono face is left for console text, ports and keys.
export const FONT_UI = '"Bahnschrift", "DIN Alternate", "Barlow", "Segoe UI", sans-serif';
export const FONT_MONO = '"JetBrains Mono Variable", "JetBrains Mono", ui-monospace, monospace';
export const FONT_DATA = FONT_UI;
export const FONT_DISPLAY = FONT_UI;
