# Changelog

## 0.3.2 (unreleased, branch `0.3.2`)

KLIF on a phone and on a Mac: a small page that shows every System and controls it from a device on your network,
and the window app and `klif-cli` on macOS with Apple silicon.

### klif-webui

- **klif-webui.** A control page for a phone or a browser on the network, served by the KLIF window app (`[webui]`
  in `klif.toml`: `enabled`, `host`, `port`; off by default, `0.0.0.0:7341`). It shows each machine, its GPUs as memory
  bars that name who holds what, and the Systems with their status. A sheet per System launches, stops, restarts and
  dismisses it ("Stop X & launch" when a launch has to stop others), chooses its preset and params and reads its
  console. It wears the window's skin and has no animations. See [docs/webui.md](docs/webui.md).
- **Pairing.** A device sees and controls nothing until it is paired, by a QR code or a 6-digit code that Tune opens
  for 5 minutes (one success or 5 wrong codes close it). A device holds a random token in its browser; KLIF stores
  only its SHA-256 in `webui-devices.json`. The page is plain HTTP (authenticated, not encrypted, like nodes); KLIF
  checks the Host header, accepts only JSON in a POST, limits connections and sizes, and sends a Content-Security-Policy.
  KLIF never changes firewall rules.
- **Tune > Web UI.** The switch, the address and port, **Pair a device** (QR code and code), and the paired devices
  with **Remove**.
- **`klif-cli webui`.** `webui` shows the state and the devices (never a pairing code), `webui on|off [--host IP]
  [--port N]` changes `[webui]`, `webui pair --yes` opens a pairing and prints its one-time code and address,
  `webui cancel` closes it, `webui forget <device>|--all --yes` removes devices. Documents `webui` and `webui-pair`
  in `klif-cli schema`.
- **`[ui] skin`.** The window writes the skin it shows to `klif.toml` and follows a change made elsewhere; klif-webui
  follows it. `klif-cli settings skin <id>` sets it.
- **Agents.** `AGENTS.md` and the Agent Skill treat the pairing code and address as a credential, and tell agents not
  to turn klif-webui on, or open it to the LAN, on their own.

### macOS

KLIF runs on macOS 13 or later on Apple silicon: the window app (`KLIF.app`) and `klif-cli`, launching and stopping
a llama.cpp server with Metal. Windows and AMD stay the reference build; everything below sits behind
`cfg(target_os = "macos")` (or `cfg(unix)`) or the platform traits.

- **Locations.** `~/Library/Application Support/KLIF` (klif.toml, state, secrets), its `data` subfolder (records,
  bench, node cache), logs in `~/Library/Logs/KLIF`. The lookup order is unchanged (`KLIF_CONFIG`, `.local/klif.toml`
  above the program or the working folder, then the default).
- **Commands.** `command` is an absolute path or a PATH name of a file with the execute bit, run with `execve` (never
  through a shell); a missing bit, a folder and an `.app` bundle are explained. A bare name is also looked up in
  the Homebrew folders (an app started from Finder gets launchd's short PATH). `{env:X}` shows as `$X`.
- **Process host.** Each server leads its own process group; stop sends SIGTERM to the group, then SIGKILL after 5 s.
  A session is adopted after a restart only when the root's start time matches the record (`proc_pidinfo`); a group
  whose root has exited only when a member still writes to the session's log. Port owners come from `proc_pidfdinfo`.
  Servers keep running when KLIF quits.
- **Hardware.** The GPU from Metal and the IORegistry: id `106B:<SoC>` (an M4 is `106B:8132`), Metal's recommended
  working set as the unified pool, cores x 128 lanes x 2 x the top DVFS clock as an estimated FP32 peak (the M1's
  published 2.6 TFLOPS is in the table). The CPU from `sysctl` with a sourced per-chip clock table (estimate).
  Live: GPU "In use system memory", each server's physical footprint, CPU load.
- **Window app.** Clipboard through NSPasteboard (a secret stays on this Mac and is marked concealed and transient),
  native Yes / No alerts, a template icon in the menu bar (a click opens the menu), `icon.icns`,
  `tauri.macos.conf.json`; the window stays frameless with KLIF's own controls. Ether falls back to the system face;
  Cmd+1..9 switch skins.
- **Release.** `scripts/build-release.sh`: the version check, a clean UI build, `--locked` builds with remapped paths,
  `KLIF.app` and `klif-cli`, `SKILL.md`, a zip and its SHA-256; ad-hoc signed by default, `--sign` (Developer ID,
  hardened runtime) and `--notarize` (notarytool keychain profile, stapled).
- **Docs.** `docs/platforms.md` has the macOS status, the seams per platform, Gatekeeper, signing and notarization,
  and the firewall.
- **Tested** on an M4 (8-core GPU, 16 GB) with llama.cpp b11399 (Metal) and Qwen3.5 4B Q4_K_M: 28.1 tok/s decode,
  265 tok/s prefill, 1.6 s load (`klif-cli bench`); launch, stop and adoption after the app was killed; a signed and
  notarized build accepted by Gatekeeper.

### Agents

From the first remote test: an agent on the Mac launched the image System on the PC through `klif-cli` and generated
an image, and said what was in its way.

- **`klif-cli status` names the API.** Each System carries `adapter` (`llama.cpp`, `sd.cpp`, `audiocpp`, ...) and,
  when KLIF can tell, `apiKey` (the server wants KLIF's API key).
- **Using a running System** in the Agent Skill and `docs/systems.md`: the request and the answer for each adapter
  (OpenAI chat, sd.cpp `/v1/images/generations` with `b64_json`, audio.cpp speech, transcription and music, whisper's
  `/inference`). KLIF still has no command that generates: the work goes to `baseUrl`.
- **`presets param` shows what it replaced** (`edit low -> off`; `--json`: `name`, `previous`, `value`, `applies`),
  so a change made for one task can be put back.
- **`klif-cli status` shows the params** a System launches with (`params`: name -> choice).
- **`presets show` finds a node's preset:** `presets show desktop/krea` means `--node desktop`, and a preset that only
  another machine has is named with the command that shows it.
- **The Agent Skill works in any shell:** the commands are shell-neutral, the JSON recipe has a PowerShell and a
  macOS / Linux form, and it covers macOS paths and commands.

### Fixed (all platforms)

- **"Does not fit even with everything stopped" for a server that fits.** The measured VRAM of an sd.cpp (or any
  log-less) session was its committed total, which counts weights `--offload-to-cpu` keeps in system memory (a Krea
  preset read 22 GiB on a 16 GiB card it runs on). KLIF now keeps what the session held on the card, drops the old
  measurements once, and never says "does not fit" for a preset that keeps weights in system RAM (`--cpu-moe`,
  `-ncmoe`, `-ot ...=CPU`, `-ngl 0`, `--offload-to-cpu`, `--cpu-offload-gb`; `ramOffload` on the System): it makes do
  with the VRAM it finds. Tune says so instead of "Over by".
- **A request with a float could fail its MAC.** The receiver checked the MAC against the params as it parsed them,
  and serde_json's default float parsing does not always give back the number that was sent (about one value in
  ten). `klif-cli bench` lost its result that way ("not handed to the records", silently) and, when it had launched
  the System, could not stop it ("the connection was closed"). serde_json now parses floats exactly
  (`float_roundtrip`); nothing changes on the wire.
- **Secret files on unix** (`control.json`, `node-token.txt`, `api-key.txt`) are written readable by their owner
  only (0600). Windows is unchanged (the profile's ACL protects them).

## 0.3.1 (unreleased, branch `0.3.1`)

KLIF now knows the machine it runs on: what it can compute, which models fit it, and the best each model file has
reached on it.

### Added

- **Hardware and FP32 TFLOPS.** Every GPU and the CPU with its theoretical peak FP32 throughput: the vendor's figure
  from an embedded table of 459 GPUs (AMD, NVIDIA, Intel), cores x FLOP per cycle x base clock for the CPU. Discrete
  GPUs add up to one VRAM pool; an integrated GPU counts only on a machine without a discrete one (its unified
  memory is then the pool). `[hardware] exclude`, `include` and `tflops` override what KLIF detects.
  `klif-cli hardware`.
- **Suggested models for this machine.** The embedded model pool (schema 2) lists models with their quant rungs and
  KV cost; KLIF suggests one per slot (System 1, 2, 3, image, speech, transcription, video) with the context and KV
  type it was sized for. System 1 leaves a third of the VRAM pool and a quarter of the RAM free and prefers a model
  that fits entirely on the GPUs; System 2 may use the whole VRAM pool; System 3 the VRAM and the RAM minus a
  reserve for the OS. Every number is an estimate. Pool: Gemma 4 (E2B to 31B), Qwen 3.8 27B and Flash-Next,
  GLM-5.3 and GLM-5.3-Flash, DeepSeek-V4 Flash and Pro (unsloth GGUFs by default), Krea 2 Turbo, Qwen Image 2.1,
  FLUX.2 klein, FLUX.1 schnell, SDXL, VoxCPM2, Qwen3-TTS, Chatterbox, Kokoro, Whisper large-v3 turbo and MiniMax H3
  (its license grants no rights in the EU, the UK, South Korea or the USA). `klif-cli suggest`,
  `klif-cli models adopt <rec> --ctx N --kv TYPE`, and the Tune drawer's "Models" section, suggestion first.
- **Records.** The best decode and prefill speed, time to first token, time per image or video and speech or
  transcription speed each model file reached on each backend, from everyday use and from `klif-cli bench`, with
  the conditions it was reached under (context, prompt length, image size, KV type, GPUs, backend build). Decode is
  the better of a request's average and its best full `tg_3s` window, the speed the live readout showed. A model is
  its file's SHA-256; HIP, Vulkan, CUDA and CPU keep separate records. Kept in `records.json` and
  `records-history.jsonl` in the data folder; a node's records show with its name. `klif-cli records`,
  `records history <key>`, `records forget <key> --yes`.
- **Records screen** in the main window (the Records button, or R): the machine and its TFLOPS on top, metric tabs,
  filters by machine and backend, search, cards or a ranked list, details with how each record climbed. A card
  exports as a 1200x675 PNG in the skin's colours, to the clipboard or to Pictures\KLIF.
- **"New record" moment** in every skin, in the full window and on the 960x640 panel: every record one request
  broke, together, counting up from the old best; gone 5 s later. `[ui] record_moment = false` (a switch in Tune and
  on the Records screen, or `klif-cli settings record-moment off`) turns it off; records are kept either way.
- **Music.** A `music` System kind (System Music) on audio.cpp: ACE-Step 1.5 (turbo, XL turbo), HeartMuLa 3B and
  Stable Audio 3 Small in the pool, the server config written on adopt, and `klif-cli bench` measuring seconds of
  music per second (the `musicRtf` record).
- **Speech, transcription and video runtimes.** An `audiocpp` adapter for audio.cpp's `audiocpp_server` (VoxCPM2,
  Qwen3-TTS, Chatterbox, Kokoro; adopting one writes its server config next to the model), whisper.cpp's
  `whisper-server` through `generic` with a `/health` check, and video on sd.cpp's `sd-server` (MiniMax H3).
  `klif-cli bench` measures speech (x real time) and transcription (`--audio <file.wav>`).
- **For agents.** `klif-cli logs <system> [--follow]`, `watch` (JSON lines of status changes, faults, launches, new
  records and downloads, with `--until <system>=<status>`), `help --json` (the command catalog), `schema [<name>]`
  (JSON Schema of every output), download progress as JSON lines on stderr, and an Agent Skill
  (`skills/klif/SKILL.md`) in the repository and in the release folder.

### Changed

- The Stop button takes a colour from each skin's palette instead of one shared red.
- The schema-1 recommendation list is replaced by the model pool; Tune's "Recommended" section is now "Models".

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
