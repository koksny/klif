// What the mock reports about the window it lives in. In the browser the native frame is always present
// unless ?frameless=1 asks the skins to draw their own window controls (to design the desktop chrome).
// Panel mode is always "available" here: it is the mini layout override (there is no window to move), and
// `active` is filled in by the player from the layout actually shown (state/player.svelte.ts).
import type { HostInfo } from '../model/types';

export const APP_VERSION = '0.3.1';

export function browserHost(frameless: boolean): HostInfo {
  return { kind: 'browser', frameless, maximized: false, appVersion: APP_VERSION, panel: { available: true, active: false, target: '960x640' } };
}
