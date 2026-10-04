// Skin contract. A skin is one complete visual language for KLIF.
//
// The state space is composed, never pre-baked:
//   skin (9)  x  size class (full | mini)  x  phase (idle | loading | live | fault)  x  slot kind (llm | image)
// A skin implements ONE root component that branches on size and phase internally and shares its
// own widgets across them. Utility surfaces (console drawer, tune drawer, settings) are NOT part of
// a skin: the shell draws them and themes them with the skin's tokens.
import type { Component } from 'svelte';
import type { Actions, ViewModel } from '../lib/model/types';

export type BuiltinSkinId = 'cliff' | 'silicon' | 'instrument' | 'phosphor' | 'decode' | 'loom' | 'ether' | 'rings' | 'spirit';
/** A built-in skin, or a local-only one from skins/private/<id>/ (see registry.ts). */
export type SkinId = BuiltinSkinId | (string & {});

/**
 * full: the desktop window (typically ~1024x1152 CSS px, portrait, right half of a 1440p monitor).
 * mini: a tiny 3.5" 960x640 panel read from ~1 m. Read-only: no controls, huge type.
 */
export type SizeClass = 'full' | 'mini';

export interface SkinProps {
  vm: ViewModel;
  actions: Actions;
  size: SizeClass;
}

/** Design tokens the shell uses to theme shared utility surfaces to match the active skin. */
export interface SkinTokens {
  bg: string;
  surface: string;
  surfaceRaised: string;
  line: string;
  ink: string;
  muted: string;
  accent: string;
  /** Text colour on top of the accent. */
  accentInk: string;
  warn: string;
  danger: string;
  /** The colour of a record (a new best on the Records screen and the "new record" moment); default: warn. */
  record?: string;
  fontUi: string;
  fontData: string;
  fontDisplay: string;
  radius: string;
}

export interface SkinModule {
  default: Component<SkinProps>;
  tokens: SkinTokens;
}

export interface SkinMeta {
  id: SkinId;
  /** Display name, e.g. "Cliff". */
  name: string;
  /** One line for the switcher. */
  blurb: string;
  load: () => Promise<SkinModule>;
}
