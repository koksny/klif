// What the mock reports about the window it lives in. In the browser the native frame is always present
// unless ?frameless=1 asks the skins to draw their own window controls (to design the desktop chrome).
import type { HostInfo } from '../model/types';

export const APP_VERSION = '0.2.0';

export function browserHost(frameless: boolean): HostInfo {
  return { kind: 'browser', frameless, maximized: false, appVersion: APP_VERSION };
}
