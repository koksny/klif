// Which core drives the UI: the native one inside the Tauri shell, the mock everywhere else.
export const IN_TAURI: boolean = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Load the native transport lazily so the browser build never touches the Tauri API. */
export function loadNative() {
  return import('./tauri');
}

export type { EngineAction, GpuReport, NativeHandlers, NativeLink } from './tauri';
