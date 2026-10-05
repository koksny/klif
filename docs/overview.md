# Overview

KLIF is a Windows front end for a local inference stack. You describe each server once as a **preset** (a launch
command), put presets into **Systems** (tabs), and KLIF starts, watches and stops them, on this machine or, through
**nodes**, on others. It does not train models, it does not vendor weights, and it does not replace the servers it
runs.

## Concepts

| Term | Meaning |
| --- | --- |
| **System** | One tab, one server at a time. Has a kind (`llm`, `image`, `tts`, `stt`, `video`, `music`), an optional LLM class, an active preset and a status. [systems.md](systems.md) |
| **Preset** | A launch command: program, arguments, working folder, environment, port, model, plus optional params. Or, with `endpoint`, an external server KLIF only watches. [presets.md](presets.md) |
| **Param** | An independent option of a preset (reasoning on/off, image size) selected per System, so presets do not multiply. |
| **Adapter** | The server family a preset belongs to (`llama.cpp`, `sd.cpp`, `vllm`, `openai`, `generic`). It sets defaults and tells telemetry which logs and endpoints to read. |
| **Session** | One run of a System's server: its process, logs, measurements. |
| **Node** | A machine running KLIF. This machine can be one for others (`[node]`), and can show other nodes' Systems (`[nodes.<id>]`). [nodes.md](nodes.md) |
| **Recommendation** | A model suggestion per kind and class that can become a preset and be downloaded on request. |
| **klif-webui** | A small control page for a phone or a browser on the LAN, served by the window app, off by default. Devices pair first. [webui.md](webui.md) |

## Programs

- **`klif.exe`** is the desktop app: a Tauri 2 shell around the Svelte UI. Frameless window drawn by the skin,
  tray icon, panel mode for a small status screen, single instance.
- **`klif-cli.exe`** is the same engine from a terminal or a coding agent. [cli.md](cli.md)

Both run the same engine code. Only one engine runs per state folder (an exclusive lock on `engine.lock`).
`klif-cli` talks to the engine of a running `klif.exe` (or of `klif-cli serve`) over a local control channel; with
none running it starts one for the duration of the command. If `klif-cli` holds the engine when you start
`klif.exe`, the window says so and retries until it is free.

```
klif.exe (shell + UI) ─┐
                       ├─ engine ─┬─ catalog     presets -> commands, validation, klif.toml edits
klif-cli serve ────────┘          ├─ supervisor  start, adopt, stop (job objects)
klif-cli <command> ── control ───►├─ telemetry   GPU/CPU/RAM counters, log parsing, health probes
                      channel     ├─ nodes       other KLIF machines (opt-in listener, client hub)
                                  └─ bench, download, keys
```

Servers are not children of the window. Each runs in its own named job object without kill-on-close, so quitting
KLIF leaves them running, and the next start adopts them again. Stop is explicit (tab, tray "Stop all Systems", or
`klif-cli stop`).

## Crates

| Crate | Role |
| --- | --- |
| `klif-common` | `klif.toml` types and loader, the view model (mirrored in `app/ui/src/lib/model/types.ts`), secret masking, launch spec, Windows command-line rendering |
| `klif-catalog` | Resolves presets into commands and plans, validation issues, model facts, comment-preserving `klif.toml` edits, embedded recommendations |
| `klif-supervisor` | Process host: spawn with output to log files, named job objects, adoption, stop |
| `klif-telemetry` | GPU and host readings behind a platform layer, per-adapter log parsers and probes |
| `klif-core` | The engine, persistence, control protocol, nodes, bench, downloads, API key |
| `klif-cli` | The command-line program |
| `app/src-tauri` | The desktop shell |
| `app/ui` | The front end: skins, Tune drawer, a mock engine for browser development |

## Files and folders

KLIF reads `klif.toml` from the first of: the `KLIF_CONFIG` file; `.local\klif.toml` found by walking up from the
executable's folder; the same walking up from the working folder; `%APPDATA%\KLIF\klif.toml`. The folder of that
file is the **state folder**. The **data folder** is the state folder, except for the default `%APPDATA%\KLIF`,
whose data lives in `%LOCALAPPDATA%\KLIF`.

| In the state folder | What |
| --- | --- |
| `klif.toml` | Your configuration. KLIF edits it comment-preservingly |
| `state.v3.json` | Selected tab, running sessions (to adopt them), last sessions, cached model shapes |
| `state.json` | 0.2's state. Read once if `state.v3.json` is missing; never written |
| `engine.lock` | Held by the one running engine |
| `control.json` | Port and per-run token of the local control channel; removed on shutdown |
| `api-key.txt` | The KLIF API key (when `[security] api_key = "file"`) |
| `node-token.txt` | This node's token, when you created one |
| `webui-devices.json` | The devices paired with klif-webui: name, dates and the SHA-256 of each token |
| `instance-id` | Random id of this installation (nodes use it to spot themselves) |
| `window.json`, `panel.json` | Window geometry |

| In the data folder | What |
| --- | --- |
| `logs\` | One `.out.log` and `.err.log` per session (`klif-<stamp>-<system>-<preset>-p<port>`), and `klif-shell.log` (`[paths] logs_dir` moves it) |
| `bench\<preset>.json` | Bench records, last 50 per preset |
| `node-cache.json` | Last known Systems of remote nodes, for showing them as unreachable |
| `webview-data\` | The WebView2 profile |

Downloaded models go to `[paths] models_dir` (`<models_dir>\<owner>--<repo>\<file>`).

## Ports

| Port | What |
| --- | --- |
| 7030 | Default for llama.cpp presets without a port |
| 1234 | Default for sd.cpp presets |
| 8000 / 8080 | Defaults for vllm / openai presets |
| (none) | `generic` presets must give a port or a health check |
| random, `127.0.0.1` | The local control channel (see `control.json`) |
| 7340 | The node listener, only when `[node] listen` is set |
| 7341 | klif-webui, only when `[webui] enabled` is true and the window app runs |
| 5193, `127.0.0.1` | The UI dev server (development only) |

Everything binds to loopback unless you set a host. Committed code and examples never carry a real LAN address.

## Telemetry in one paragraph

For each running System KLIF reads the process (alive, exit code), the server's log files, health and metrics
endpoints (`/health`, `/slots`, `/v1/models`, `/metrics`, depending on the adapter), and Windows GPU counters:
adapter memory through DXGI and per-process dedicated memory through performance counters. From these it draws
VRAM composition, prefill and decode progress, requests in flight, and fits (will this System's memory need fit
next to the others?). A probe sends the KLIF API key as a Bearer header, and only to the System's own host.
Servers that print little still get a process, health and per-process VRAM view.

## Desktop shell commands

The UI reaches the engine through Tauri commands (`app/src-tauri/src/commands.rs`):

| Command | Purpose |
| --- | --- |
| `klif_snapshot` | The current view model (waits while another process holds the engine) |
| `klif_engine_status` | `starting`, `waiting`, `ready` or `failed`, with a message, for the connecting screen |
| `klif_act` | Run an engine action (launch, stop, usePreset, savePreset, addSystem, ...). Only a short summary is logged |
| `klif_preset_get` | One preset, secrets masked (`id`, optional `node`) |
| `klif_command_preview` | The exact command and issues for an unsaved preset draft |
| `klif_records_history` | The climb of one record (`key`, optional `metric`): every broken record, oldest first |
| `klif_save_image` | Save the Records export card (`name`, base64 `data`): a PNG or GIF into `Pictures\KLIF`, a safe name, never overwriting (`-2`, `-3`, ...); returns the full path |
| `klif_set_api_key` | Store or clear the KLIF API key; the value is wiped after use and never echoed |
| `klif_open_config`, `klif_open_logs` | Open `klif.toml` (creating it if needed) or the logs folder |
| `klif_open_endpoint`, `klif_copy_endpoint`, `klif_copy_api_key` | Open or copy a System's URL (LLMs with `/v1`); copy the key (local Systems only) |
| `klif_toggle_panel` | Move the window to the panel monitor and back |
| `klif_skins`, `klif_ui_log` | Tray skin list, UI log line |

The tray offers Show/Hide, Panel mode, Skin, **Stop all Systems** (asks first) and Quit (never stops a server).

## Hardware the public tree talks about

Document **classes**, not a single living-room PC:

- HIP on RDNA4-class 16 GiB (example: RX 9070 XT / `gfx1201`)
- Vulkan on the same card
- A second card as a documented option, not a required layout
- CPU and system RAM for expert or encoder offload when VRAM is tight

Do not commit HIP device ordinals from one machine (`ROCm0` vs `ROCm2`) as if they were universal; see
[platforms.md](platforms.md).

## What users supply

- Model files (GGUF, safetensors), in folders they choose.
- The server programs: a HIP or Vulkan `llama-server` / `sd-server`, a vLLM install, a TTS server, whatever their
  Systems run.
- A `klif.toml` pointing at them (see [../config/klif.example.toml](../config/klif.example.toml)); no `C:\...`
  paths are baked into this tree.

See [publish.md](publish.md) for the copy checklist when promoting a private change into this repo.
