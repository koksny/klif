// Phosphor palette: a vector oscilloscope behind dark glass.
// Canvas code reads these constants; CSS reads the same values as custom properties (Skin.svelte).
export const P = {
  glass: '#05090B',
  grat: '#12303A',
  /** Panel rules: a touch brighter than the graticule so frames read as structure. */
  rule: '#174F5C',
  cyan: '#7FE3FF',
  brand: '#5AB6EB',
  amber: '#E8B04A',
  hot: '#F0FBFF',
  /** Secondary text: brand cyan pushed back into the glass. */
  muted: '#4F98B4',
  danger: '#E5615C',
} as const;

export const FONT_UI = "'Share Tech Mono', ui-monospace, monospace";
export const FONT_DISPLAY = "'Saira Variable', 'Saira', system-ui, sans-serif";

export function prefersReducedMotion(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

let uidSeq = 0;
/** Unique suffix for SVG defs ids (several instances can be mounted at once). */
export function nextUid(): string {
  return `ph${++uidSeq}`;
}
