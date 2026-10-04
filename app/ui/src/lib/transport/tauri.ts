// Native transport: the view model comes from the Rust core inside the Tauri shell, actions go back to it.
// Loaded only inside Tauri (see ./index.ts); the browser keeps using the mock core.
//
// Wire contract (app/src-tauri):
//   event   klif://vm    ViewModel, at the telemetry rate (2 Hz)
//   event   klif://skin  skin id picked from the tray menu
//   event   klif://gpu   { ok, adapter, expected?, pid? } after the WebView2 GPU process was located
//   invoke  klif_snapshot -> ViewModel
//   invoke  klif_act { action: EngineAction }            (serde shape of klif_common::vm::Action)
//   invoke  klif_open_endpoint | klif_copy_endpoint { system? } | klif_copy_api_key   (handled natively; the key
//                                                         never reaches JS)
//   invoke  klif_preset_get { id, node? } -> PresetDetail | null       (secrets masked)
//   invoke  klif_records_history { key, metric? } -> RecordHistoryLine[]   (= RecordEvent[]: the climb of one record
//                                                         key, oldest first; a node's "<node>/<key>" is asked of
//                                                         that node)
//   invoke  klif_save_image { name, data: base64 } -> string   (the Records export card: writes a PNG or GIF into
//                                                         the user's Pictures folder, subfolder KLIF, never
//                                                         overwriting ("-2", "-3"...); name is reduced to
//                                                         [A-Za-z0-9._-] and must end in .png or .gif; resolves
//                                                         with the full path of the file)
//   invoke  klif_command_preview { spec, system? } -> CommandView      (an unsaved preset; Tune's debounced preview)
//   invoke  klif_set_api_key { key: string | null }                    (native; never echoed back)
//   invoke  klif_open_config | klif_open_logs
//   invoke  klif_toggle_panel                            (panel mode: the window moves onto the small status
//                                                         screen and fills it, or comes back; the state
//                                                         arrives as ViewModel.host.panel)
//   invoke  klif_ui_log { line }                         (diagnostics into the shell log)
//   invoke  klif_skins { skins: [{ id, name }] }          (the tray's Skin submenu follows the UI registry)
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { CommandView, EngineAction, PresetDetail, PresetSpec, RecordHistoryLine, RecordMetric, SystemId, ViewModel } from '../model/types';
import { SKINS } from '../../skins/registry';

/** klif_common::vm::Action, internally tagged by "type" (declared in model/types.ts). */
export type { EngineAction };

export interface GpuReport {
  ok: boolean;
  /** The adapter the UI's GPU process renders on (display name), if it was found. */
  adapter: string | null;
  /** The adapter it was pinned to, if any. */
  expected?: string | null;
  pid?: number | null;
}

export interface NativeHandlers {
  vm(vm: ViewModel): void;
  skin(id: string): void;
  gpu(report: GpuReport): void;
}

export interface NativeLink {
  act(action: EngineAction): Promise<void>;
  openEndpoint(system?: SystemId): Promise<void>;
  copyEndpoint(system?: SystemId): Promise<void>;
  copyApiKey(system?: SystemId): Promise<void>;
  presetGet(id: string, node?: string): Promise<PresetDetail | null>;
  recordsHistory(key: string, metric?: RecordMetric): Promise<RecordHistoryLine[]>;
  /** Writes the image into Pictures\KLIF; resolves with its full path. */
  saveImage(fileName: string, data: Blob): Promise<string>;
  commandPreview(spec: PresetSpec, system?: SystemId): Promise<CommandView>;
  setApiKey(key: string | null): Promise<void>;
  openConfig(): Promise<void>;
  openLogs(): Promise<void>;
  /** Panel mode on / off. Rejects with a sentence when there is no small screen to move to. */
  togglePanel(): Promise<void>;
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  log(line: string): void;
  disconnect(): void;
}

/** Where the core start stands (`klif_engine_status`): "waiting" = another process holds the engine; the shell
 *  retries every 2 s and `message` says which process. */
export type EngineStatus = { state: 'starting' | 'waiting' | 'ready' | 'failed'; message?: string };
export const engineStatus = () => invoke<EngineStatus>('klif_engine_status');

/** Errors from commands arrive as plain strings (user-facing sentences); anything else is stringified. */
export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

/** The bytes of a Blob as standard base64 (what klif_save_image takes). Chunked: spreading a big array overflows the stack. */
async function toBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let bin = '';
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}

export async function connect(h: NativeHandlers): Promise<NativeLink> {
  const log = (line: string) => {
    void invoke('klif_ui_log', { line }).catch(() => {});
  };
  // Page errors land in the shell log too (there is no devtools console in a release build).
  const onError = (e: ErrorEvent) => log(`error: ${e.message} (${e.filename}:${e.lineno})`);
  const onRejection = (e: PromiseRejectionEvent) => log(`unhandled rejection: ${errorText(e.reason)}`);
  window.addEventListener('error', onError);
  // The tray's Skin submenu lists what this build has (built-in and local-only skins).
  void invoke('klif_skins', { skins: SKINS.map((s) => ({ id: s.id, name: s.name })) }).catch((e) => log(`skins: ${errorText(e)}`));
  window.addEventListener('unhandledrejection', onRejection);
  const consoleError = console.error;
  console.error = (...args: unknown[]) => {
    consoleError(...args);
    log(`console.error: ${args.map(errorText).join(' ')}`);
  };
  // Snapshots carry their own clock: never let an older one replace a newer one.
  let lastNow = -Infinity;
  let events = 0;
  const apply = (vm: ViewModel, via: string) => {
    if (vm.now < lastNow) return;
    lastNow = vm.now;
    h.vm(vm);
    if (via === 'event') {
      events++;
      if (events === 1) log('first klif://vm event applied');
      else if (events === 20) log('20 klif://vm events applied');
    }
  };
  const offs: UnlistenFn[] = await Promise.all([
    listen<ViewModel>('klif://vm', (e) => apply(e.payload, 'event')),
    listen<string>('klif://skin', (e) => h.skin(e.payload)),
    listen<GpuReport>('klif://gpu', (e) => h.gpu(e.payload)),
  ]);
  const first = await invoke<ViewModel>('klif_snapshot');
  apply(first, 'snapshot');
  log(`connected: snapshot with ${first.systems.length} systems, session ${first.session?.phase ?? 'none'}, ${location.href}`);

  const win = getCurrentWindow();
  return {
    act: (action) => invoke('klif_act', { action }),
    openEndpoint: (system) => invoke('klif_open_endpoint', { system: system ?? null }),
    copyEndpoint: (system) => invoke('klif_copy_endpoint', { system: system ?? null }),
    copyApiKey: (system) => invoke('klif_copy_api_key', { system: system ?? null }),
    presetGet: (id, node) => invoke<PresetDetail | null>('klif_preset_get', { id, node: node ?? null }),
    recordsHistory: (key, metric) => invoke<RecordHistoryLine[]>('klif_records_history', { key, metric: metric ?? null }),
    saveImage: async (fileName, data) => invoke<string>('klif_save_image', { name: fileName, data: await toBase64(data) }),
    commandPreview: (spec, system) => invoke<CommandView>('klif_command_preview', { spec, system: system ?? null }),
    setApiKey: (key) => invoke('klif_set_api_key', { key }),
    openConfig: () => invoke('klif_open_config'),
    openLogs: () => invoke('klif_open_logs'),
    togglePanel: () => invoke('klif_toggle_panel'),
    minimize: () => win.minimize(),
    toggleMaximize: () => win.toggleMaximize(),
    close: () => win.close(),
    log,
    disconnect: () => {
      for (const off of offs) off();
      window.removeEventListener('error', onError);
      window.removeEventListener('unhandledrejection', onRejection);
      console.error = consoleError;
    },
  };
}
