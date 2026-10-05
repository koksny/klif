// klif-webui's connection to KLIF: the device token (localStorage), pairing, the state poll and the actions.
// No cookie: the token travels only in an Authorization header this page sets, so another site cannot ride on it.
import type { WebState } from './types';

const TOKEN_KEY = 'klif.webui.token';
const POLL_MS = 1500;
const CONSOLE_MS = 2000;

function readToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

function writeToken(t: string | null) {
  try {
    if (t) localStorage.setItem(TOKEN_KEY, t);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    /* blocked storage: the device has to pair again next time */
  }
}

/** A name for this device from its browser ("iPhone", "Android phone", "Firefox on Windows"). */
export function guessDeviceName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad/.test(ua)) return 'iPad';
  if (/Android/.test(ua)) return /Mobile/.test(ua) ? 'Android phone' : 'Android tablet';
  const browser = /Firefox\//.test(ua) ? 'Firefox' : /Edg\//.test(ua) ? 'Edge' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : 'Browser';
  const os = /Windows/.test(ua) ? 'Windows' : /Mac OS X/.test(ua) ? 'macOS' : /Linux/.test(ua) ? 'Linux' : '';
  return os ? `${browser} on ${os}` : browser;
}

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

async function call<T>(path: string, init: RequestInit & { token?: string | null; body?: string } = {}): Promise<T> {
  const headers: Record<string, string> = {};
  if (init.token) headers.Authorization = `Bearer ${init.token}`;
  if (init.body !== undefined) headers['Content-Type'] = 'application/json';
  let res: Response;
  try {
    res = await fetch(path, { method: init.method ?? 'GET', headers, body: init.body, cache: 'no-store' });
  } catch {
    throw new ApiError('KLIF does not answer. Is the computer on, and this device on the same network?', 0);
  }
  const text = await res.text();
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    /* not JSON: use the status line below */
  }
  if (!res.ok) {
    const msg = (data as { error?: { message?: string } } | null)?.error?.message ?? (text.trim() || `KLIF answered ${res.status}.`);
    throw new ApiError(msg, res.status);
  }
  return data as T;
}

export type Act =
  | { type: 'launch'; system: string; stopOthers?: boolean }
  | { type: 'stop' | 'restart' | 'dismiss'; system: string }
  | { type: 'usePreset'; system: string; preset: string }
  | { type: 'setParam'; system: string; name: string; value: string };

class Client {
  token = $state<string | null>(readToken());
  state = $state<WebState | null>(null);
  /** When the last poll answered (ms). */
  okAt = $state(0);
  /** The last poll's failure, while KLIF does not answer. */
  down = $state<string | null>(null);
  private timer: ReturnType<typeof setTimeout> | null = null;
  private polling = false;

  get paired(): boolean {
    return !!this.token;
  }

  /** Pair with the QR secret or the typed code. */
  async pair(secret: string, name: string): Promise<void> {
    const r = await call<{ token: string }>('/api/pair', { method: 'POST', body: JSON.stringify({ secret, name }) });
    this.token = r.token;
    writeToken(r.token);
    this.start();
  }

  /** This device unpairs itself (KLIF forgets it). */
  async forget(): Promise<void> {
    try {
      await call('/api/forget', { method: 'POST', token: this.token, body: '{}' });
    } finally {
      this.unpair();
    }
  }

  private unpair() {
    this.token = null;
    this.state = null;
    writeToken(null);
    this.stop();
  }

  start() {
    if (this.polling) return;
    this.polling = true;
    void this.poll();
  }

  stop() {
    this.polling = false;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }

  /** Ask now (after an action) instead of waiting for the next tick. */
  refresh() {
    if (this.timer) clearTimeout(this.timer);
    void this.poll();
  }

  private async poll() {
    if (!this.polling || !this.token) return;
    if (document.visibilityState === 'hidden') {
      this.schedule(POLL_MS);
      return;
    }
    try {
      this.state = await call<WebState>('/api/state', { token: this.token });
      this.okAt = Date.now();
      this.down = null;
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        this.unpair();
        return;
      }
      this.down = e instanceof Error ? e.message : String(e);
    }
    if (this.polling) this.schedule(this.down ? POLL_MS * 2 : POLL_MS);
  }

  /** One chain only: a refresh() during a poll would otherwise leave two timers running. */
  private schedule(ms: number) {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.poll(), ms);
  }

  async act(a: Act): Promise<void> {
    await call('/api/act', { method: 'POST', token: this.token, body: JSON.stringify(a) });
    this.refresh();
  }

  async console(system: string): Promise<string[]> {
    const r = await call<{ lines: string[] }>(`/api/console?system=${encodeURIComponent(system)}`, { token: this.token });
    return r.lines;
  }
}

export const client = new Client();
export { CONSOLE_MS };
