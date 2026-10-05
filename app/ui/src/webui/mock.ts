// Development only (the Vite dev server, `/webui.html?mock`): answers the page's API in the browser with a made-up
// stack, so the page can be built and checked without a running KLIF. Pair with the code 123456.
import type { WebState, WebSystem } from './types';

const llmPresets = [
  { id: 'gemma4-26b', name: 'Gemma 4 26B-A4B', ready: true },
  { id: 'qwen38-27b', name: 'Qwen 3.8 27B', ready: true },
  { id: 'glm5-flash', name: 'GLM-5 Flash', ready: true },
];

function sys(p: Partial<WebSystem> & Pick<WebSystem, 'id' | 'label' | 'kind' | 'status'>): WebSystem {
  return { machine: 'local', presets: [], params: [], gpus: [], gpuNames: [], conflicts: [], controllable: true, external: false, ...p };
}

function initial(): WebState {
  return {
    v: 1,
    now: Date.now() / 1000,
    skin: new URLSearchParams(location.search).get('skin') ?? 'cliff',
    onConflict: 'ask',
    device: { id: '3fa1c2d4', name: 'Android phone' },
    machines: [
      {
        id: 'local',
        name: 'Desktop',
        local: true,
        os: 'Windows',
        state: 'online',
        gpus: [
          { id: '1002:7550', name: 'RX 9070 XT', totalGiB: 15.9, usedGiB: 13.3, otherGiB: 0.4, unified: false, parts: [{ system: 's1', label: 'System 1', gib: 12.9 }] },
          { id: '1002:731f', name: 'RX 5700 XT', totalGiB: 8, usedGiB: 2.7, otherGiB: 0.6, unified: false, parts: [{ system: 'tts', label: 'System TTS', gib: 2.1 }] },
        ],
      },
      {
        id: 'macbook',
        name: 'MacBook',
        local: false,
        state: 'online',
        latencyMs: 4,
        gpus: [{ id: '106b:m4', name: 'Apple M4', totalGiB: 10.7, usedGiB: 3.4, otherGiB: 3.4, unified: true, parts: [] }],
      },
      { id: 'laptop', name: 'Laptop', local: false, state: 'offline', error: 'Could not connect (timed out).', gpus: [] },
    ],
    systems: [
      sys({
        id: 's1', label: 'System 1', kind: 'llm', class: 'fast', status: 'online', model: 'Gemma 4 26B-A4B', quant: 'UD-Q4_K_XL',
        preset: 'gemma4-26b', presets: llmPresets, gpus: ['1002:7550'], gpuNames: ['RX 9070 XT'], vramGiB: 12.9,
        metric: { v: 93.1, u: 'tok/s' }, uptimeS: 8040,
        params: [{ name: 'ctx', label: 'Context', value: '65536', choices: [{ value: '32768', label: '32k' }, { value: '65536', label: '64k' }, { value: '131072', label: '128k' }] }],
      }),
      sys({
        id: 's2', label: 'System 2', kind: 'llm', class: 'deep', status: 'offline', model: 'Qwen 3.8 27B', quant: 'UD-Q4_K_XL',
        preset: 'qwen38-27b', presets: llmPresets, gpus: ['1002:7550'], gpuNames: ['RX 9070 XT'], vramGiB: 15.2, conflicts: ['System 1'],
        last: '41.3 tok/s · 20 h ago',
      }),
      sys({
        id: 's3', label: 'System 3', kind: 'llm', class: 'max', status: 'fault', model: 'GLM-5 Flash', quant: 'UD-IQ2_XXS',
        preset: 'glm5-flash', presets: llmPresets, gpus: ['1002:7550', '1002:731f'], gpuNames: ['RX 9070 XT', 'RX 5700 XT'], vramGiB: 21.6,
        conflicts: ['System 1', 'System TTS'], reason: 'The server exited while loading weights: out of memory (exit code 1).',
      }),
      sys({
        id: 'cgi', label: 'System CGI', kind: 'image', status: 'offline', model: 'Krea 2 Turbo', quant: 'Q4_0', preset: 'krea2-turbo',
        presets: [{ id: 'krea2-turbo', name: 'Krea 2 Turbo', ready: true }, { id: 'qwen-image-21', name: 'Qwen Image 2.1', ready: false }],
        gpus: ['1002:7550'], gpuNames: ['RX 9070 XT'], vramGiB: 11.4, conflicts: ['System 1'], last: '6.8 s per image · 2 h ago',
        params: [
          { name: 'precision', label: 'Precision', value: 'low', choices: [{ value: 'low', label: 'Low' }, { value: 'medium', label: 'Medium' }, { value: 'high', label: 'High' }] },
          { name: 'mode', label: 'Mode', value: 'edit', choices: [{ value: 'gen', label: 'Generate' }, { value: 'edit', label: 'Edit' }] },
        ],
      }),
      sys({
        id: 'tts', label: 'System TTS', kind: 'tts', status: 'busy', model: 'VoxCPM2', quant: 'BF16', preset: 'voxcpm2',
        presets: [{ id: 'voxcpm2', name: 'VoxCPM2', ready: true }, { id: 'kokoro', name: 'Kokoro 82M', ready: true }],
        gpus: ['1002:731f'], gpuNames: ['RX 5700 XT'], vramGiB: 2.1, uptimeS: 2460,
      }),
      sys({
        id: 'stt', label: 'System STT', kind: 'stt', status: 'starting', model: 'Whisper large-v3 turbo', quant: 'Q8_0', preset: 'whisper-turbo',
        presets: [{ id: 'whisper-turbo', name: 'Whisper large-v3 turbo', ready: true }], gpus: ['cpu'], gpuNames: ['CPU'],
        load: { step: 'Load weights', pct: 62 }, uptimeS: 9,
      }),
      sys({
        id: 'macbook/s1', label: 'System 1', machine: 'macbook', kind: 'llm', class: 'fast', status: 'online', model: 'Qwen3 4B', quant: 'Q4_K_M',
        preset: 'qwen3-4b', presets: [{ id: 'qwen3-4b', name: 'Qwen3 4B', ready: true }], gpus: ['106b:m4'], gpuNames: ['Apple M4'],
      }),
      sys({ id: 'macbook/cgi', label: 'System CGI', machine: 'macbook', kind: 'image', status: 'not-set', reason: 'No preset yet. Choose one for System CGI.' }),
      sys({ id: 'laptop/s1', label: 'System 1', machine: 'laptop', kind: 'llm', status: 'unreachable', model: 'Qwen3 8B', controllable: false }),
      sys({ id: 'laptop/tts', label: 'System TTS', machine: 'laptop', kind: 'tts', status: 'unreachable', model: 'Kokoro 82M', controllable: false }),
    ],
  };
}

const json = (status: number, v: unknown) => new Response(JSON.stringify(v), { status, headers: { 'Content-Type': 'application/json' } });
const fail = (status: number, message: string) => json(status, { error: { code: 'mock', message } });

/** Replace `fetch` for /api/* with the made-up stack. */
export function installMock() {
  const state = initial();
  const real = window.fetch.bind(window);
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, location.href);
    if (!url.pathname.startsWith('/api/')) return real(input, init);
    await new Promise((r) => setTimeout(r, 120));
    const body = init?.body ? JSON.parse(String(init.body)) : {};
    const authed = new Headers(init?.headers).get('Authorization') === 'Bearer mock';
    if (url.pathname === '/api/pair') {
      return body.secret === '123456' || /^[0-9a-f]{64}$/.test(body.secret) ? json(200, { token: 'mock', device: { id: '3fa1c2d4', name: body.name } }) : fail(403, 'That code is wrong or has expired. Check it in KLIF (Tune, Web UI).');
    }
    if (!authed) return fail(401, 'This browser is not paired with KLIF.');
    if (url.pathname === '/api/state') return json(200, { ...state, now: Date.now() / 1000 });
    if (url.pathname === '/api/forget') return json(200, { ok: true });
    if (url.pathname === '/api/console') return json(200, { lines: ['srv  load_model: loaded', 'main: server is listening on 127.0.0.1:7030', 'slot 0 | prompt 2311 tokens', 'eval time = 4371.05 ms / 407 tokens (93.1 t/s)'] });
    if (url.pathname === '/api/act') {
      const s = state.systems.find((x) => x.id === body.system);
      if (!s) return fail(404, 'That System is not in KLIF (any more).');
      if (body.type === 'launch') {
        if (s.conflicts.length && !body.stopOthers) return fail(409, `${s.label} needs ${s.conflicts.join(', ')} stopped first.`);
        for (const c of state.systems) if (s.conflicts.includes(c.label) && c.machine === s.machine) c.status = 'offline';
        s.status = 'starting';
        s.load = { step: 'Load weights', pct: 10 };
        s.reason = undefined;
        setTimeout(() => {
          s.status = 'online';
          s.load = undefined;
        }, 3000);
      } else if (body.type === 'stop') {
        s.status = 'offline';
        s.metric = undefined;
      } else if (body.type === 'restart') {
        s.status = 'starting';
        setTimeout(() => (s.status = 'online'), 2000);
      } else if (body.type === 'dismiss') {
        s.status = 'offline';
        s.reason = undefined;
      } else if (body.type === 'usePreset') {
        s.preset = body.preset;
        s.model = s.presets.find((p) => p.id === body.preset)?.name ?? s.model;
      } else if (body.type === 'setParam') {
        const p = s.params.find((x) => x.name === body.name);
        if (p) p.value = body.value;
      }
      return json(200, { ok: true });
    }
    return fail(405, 'This request is not served.');
  };
}
