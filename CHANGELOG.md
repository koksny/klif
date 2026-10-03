# Changelog

## 0.3.0 (unreleased, branch `0.3`)

KLIF stops being a launcher for a fixed set of model "tiers" and becomes a manager for a whole local inference
stack: any number of Systems side by side, on this machine or on others, each with a launch command you can read
and edit. Windows with an AMD GPU stays the tested build.

### Added

- **Systems as tabs.** Define as many as you like in `[systems.<id>]`; tab order is file order. Kinds: `llm`,
  `image`, `tts`, `stt`, `video`. LLM Systems may carry a class (`fast`, `deep`, `max`) that groups
  recommendations and gives the default labels System 1, System 2, System 3 (System CGI, System TTS, System STT,
  System Video for the other kinds; a label already taken becomes "System 1 (2)", "System TTS (2)", ...). Every
  tab shows its status: not set, invalid, offline, starting, online, busy, stopping, fault or unreachable.
  Selecting a tab never stops anything.
- **Concurrent Systems.** Each System has its own server, session, logs and telemetry. Launch checks for
  conflicts first (same port, an `exclusive` System on the same GPU, not enough free VRAM) and names them;
  `[launch] on_conflict` and `--stop-others` decide whether KLIF stops them for you. A running server whose System
  you remove from `klif.toml` stays visible as "<id> (not in klif.toml)" until it exits or you stop it.
- **External servers.** A preset with `endpoint` instead of `command` is watched, never started or stopped
  (plain `http://` endpoints for now).
- **Remote nodes.** `[node]` (opt-in listener, default port 7340, token, `allow = ["launch"]` and/or `"edit"`) and
  `[nodes.<id>]` show another machine's Systems in the same window and `klif-cli`. Requests are authenticated with
  a challenge and HMAC-SHA256 under a token that never crosses the wire, and the node proves it holds the token
  too (protocol 2); the traffic itself is not encrypted yet. KLIF refuses to send a clear secret to a remote node
  from any client. See [docs/nodes.md](docs/nodes.md).
- **Transparent presets.** `[presets.<id>]` is the launch command: program, argument list, working folder,
  environment. Placeholders (`{model}`, `{port}`, `{stamp}`, `{env:NAME}`, `{p.NAME}`, ...) and independent params
  (`[presets.<id>.params.<name>]`) replace a Cartesian product of presets; choosing another preset for a System
  removes the param selections the new preset does not declare. Adapters: `llama.cpp`, `sd.cpp`, `vllm`, `openai`,
  `generic`. The Tune drawer and `klif-cli plan` show the exact command line and validation issues before anything
  starts; `plan` also shows the command of an invalid System, marked `launchable: NO`. See
  [docs/presets.md](docs/presets.md).
- **Tune drawer, rebuilt.** Add a System (kind, class, label, then Recommended / existing preset / blank), rename,
  move, mark exclusive, remove; pick a preset and its params; edit the command (adapter, program, args, env, port,
  host, health, model, GPU) with a live preview; see the fit on the System's GPU and the latest bench; API key
  row; node list.
- **klif-cli** as its own program (`klif-cli.exe`): status, plan, select, launch, stop, restart, dismiss, systems,
  presets, bench, models, key, node, nodes, serve. `--json` prints one document with `schemaVersion` 1. Anything
  that starts, stops, removes or downloads needs `--yes`. See [docs/cli.md](docs/cli.md).
- **Bench.** `klif-cli bench <system>` measures load time, time to first token, prefill and decode speed (LLM) or
  seconds per image, plus peak VRAM, and keeps the last 50 records per preset. Results go stale when the command
  changes.
- **Recommendations and downloads.** Starting points per kind and class, embedded in the binaries. Models are
  fetched only on an explicit action, only from huggingface.co (https, official hosts), resumable and verified
  against the hub's SHA-256. A model whose files live in several repos names a `repo` and `revision` per file.
  Needs `[paths] models_dir`.
- **Telemetry for any kind.** Per-process VRAM, several GPUs (`gpu = "VEN:DEV"`, `"VEN:DEV#1"`, `"cpu"`), a
  `generic` adapter that reads process, health, log activity and `/metrics` when a server exposes it.
- **Config.** `klif.toml` is looked up through `KLIF_CONFIG`, `.local\klif.toml` above the exe or the working
  folder, then `%APPDATA%\KLIF\klif.toml`. Edits are picked up while KLIF runs. KLIF's own edits keep comments and
  table order. `config/klif.example.toml` is a complete, parseable template.
- **Generic display** of Systems that are not LLMs or image servers in all nine skins.
- **Windows resources.** VERSIONINFO and an application manifest (`asInvoker`, Windows 10/11, per-monitor DPI,
  long paths) for `klif.exe` and `klif-cli.exe`.
- **Build-Release.ps1:** `-OutDir`, optional `-Sign`, version consistency check, SHA-256 of both exes, refuses a
  `private` folder in the output, a font that is not woff2 in the UI, and an exe that names `ExecutionPolicy` or
  `powershell.exe`. The UI is always rebuilt from an empty `app/ui/dist`.

### Changed

- The view model says **System** where 0.2 said slot (`vm.systems`, `System`, `SystemId`, `SystemKind`) and
  `machine` where it said `system` for CPU and RAM.
- KLIF starts servers directly: a hidden process with output appended to log files, inside a named Windows job
  object (without kill-on-close) so servers survive KLIF and are adopted again on the next start. No script host is
  involved.
- Persistence is `state.v3.json`. 0.2's `state.json` is read once, read-only, and never written.
- The local control channel (used by `klif-cli`) authenticates with the same challenge and HMAC as the network
  listener.

### Removed

- The PowerShell launcher integration: `[launcher]` and `[krea]` in `klif.toml` (ignored now), the catalog
  exporter script, `catalog-parity`, the built-in recipes, fixed tiers and slot logic, adoption of the old
  launcher's runs, and the Rust stub engine.
- `[tiers]` is read only as a fallback when a file has no `[systems]` table: `low`, `medium`, `high`, `krea` become
  the Systems `s1`, `s2`, `s3`, `cgi`. `klif-cli` still accepts those four names for those ids.

### Migrating from 0.2

- 0.2 kept its model presets in the PowerShell launcher's catalog; 0.3 does not read it. Recreate each command as a
  `[presets.<id>]` table (the final command line is what `klif-cli plan` showed in 0.2), then point each System at
  its preset. One `klif.toml` can serve both versions while you move: 0.3 ignores `[launcher]` and `[krea]`.
- A 0.2 session that is still running is adopted from `state.json` once, as `s1`, `s2`, `s3` or `cgi`.

## 0.2.0 (2026-10-02 to 2026-10-03)

First version with running code.

- **UI prototype** (Svelte 5, Vite) over one view-model contract, driven by a mock engine in a browser: four
  skins (Cliff, Silicon, Instrument, Phosphor), each with a full window and a 960x640 mini panel, covering idle,
  loading, live LLM, image job, spill and fault states.
- **Native core** (Cargo workspace): shared types and `klif.toml` (`klif-common`), launch plans (`klif-catalog`),
  process supervision with named job objects and adoption (`klif-supervisor`), GPU and process telemetry through
  DXGI and PDH with log parsing for llama-server and sd-server (`klif-telemetry`), and the session engine with
  `klif-cli` (`klif-core`).
- **Desktop shell** (Tauri 2): frameless window with skin-drawn chrome, tray, single instance, WebView2 pinned to
  the configured UI adapter, embedded UI in release builds, panel mode on a small status screen.
- Dormant-GPU state: VRAM that Windows paged out while an AMD card idles is drawn as ghosts.
- Krea precision and edit options, more image sizes, a reference-image Vision toggle.
- Mini panels got a Launch / Stop control and tier picking.
- Five more skins: Decode, Loom, Ether, Rings and Spirit; tiers renamed System 1, 2, 3 and System CGI.
- The model's layers, experts and heads, read from llama-server's startup log, are exposed for the skins.

## 0.1.0 (2026-08-31)

- Public identity: name, sky-cyan mark, MIT license, docs.
- Publishing rules for what may enter git once the launcher is copied here.
- No runtime code in this tag.
