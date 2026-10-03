# KLIF

<p align="center">
  <img src="docs/assets/koksny-mark.png" alt="KLIF" width="96" height="96">
</p>

<p align="center">
  <strong>Koksny.com LOCAL INFERENCE FORNICATOR</strong><br>
  AMD optimized frontend for transformers and DiT.
</p>

KLIF manages a local inference stack from one window: the language-model, image, speech, transcription and video
servers you already run, on one machine or several. Each server is a **System**, shown as a tab with a live
status. KLIF starts and stops them, shows what they do to the GPU, and keeps the exact command line of every
System visible and editable. `klif-cli` does the same from a terminal or a coding agent.

KLIF is a frontend for people who already run local models. It is not a model zoo and not an inference runtime:
llama.cpp, stable-diffusion.cpp, vLLM and the rest are separate projects you install yourself, and no weights are
shipped.

> KLIF is a front-end built for AMD and Windows. Different GPU or OS? Let your agent open a PR. Installed KLIF?
> Let your agent tune and calibrate it. Want other models than the defaults? Let your agent set them up.

## What it does

- **Systems as tabs.** You define the Systems you have: "System 1", "System 2", "System 3", "System CGI",
  "System TTS", "System STT", "System Video", or anything else. Only configured Systems get a tab. Kinds are
  `llm`, `image`, `tts`, `stt` and `video`. A tab shows online, busy, starting, offline, not set, invalid, fault or
  unreachable. [docs/systems.md](docs/systems.md)
- **Several at once.** Every System has its own server, session, logs and telemetry. Before a launch KLIF checks
  for port clashes, an `exclusive` System on the same GPU and free VRAM, and tells you which Systems would have to
  stop. It never kills a process it does not own.
- **Other machines and servers you do not start.** A System can be an external server that KLIF only watches, or
  live on another computer running KLIF (a remote node, opt-in, token-authenticated). One window then covers the
  whole stack. [docs/nodes.md](docs/nodes.md)
- **Commands you can read.** A preset is the launch command: program, argument list, working folder, environment.
  The Tune drawer, `klif-cli plan` and `klif.toml` show the same thing, and what you see is what runs. Presets can
  declare independent params (reasoning on/off, image size) instead of one preset per combination.
  [docs/presets.md](docs/presets.md)
- **Adapters, not a fixed list.** `llama.cpp`, `sd.cpp`, `vllm`, `openai` (any OpenAI-compatible server) and
  `generic` (any program that listens on a port). The adapter decides defaults and which log lines and endpoints
  KLIF reads for telemetry.
- **klif-cli.** Status, plan, launch, stop, presets, params, bench, downloads, keys, nodes, with `--json` and a
  stable `schemaVersion`. Built so your coding agent can tune, calibrate and extend KLIF without a GUI.
  [docs/cli.md](docs/cli.md)
- **Recommendations, with a disclaimer.** KLIF can suggest models per kind and class and download them. The
  suggestions are starting points measured on one machine. Models, drivers and backends change: your agent
  should benchmark and calibrate (`klif-cli bench <system>`). Each model keeps its own license. KLIF downloads
  only from huggingface.co, and only when you ask.
- **Skins.** Nine interchangeable looks over the same data, each with a full window and a 960x640 panel layout for
  a small status screen.

## Requirements

- Windows 10 or 11, 64-bit.
- The Microsoft Edge WebView2 runtime. It is part of Windows 11 and of most up-to-date Windows 10 installs; if it
  is missing, install the Evergreen runtime from Microsoft.
- An AMD GPU is the first-class target: the tested setups run HIP (ROCm) and Vulkan builds of llama.cpp and
  stable-diffusion.cpp. GPU memory is read through DXGI and Windows performance counters, which are not
  AMD-specific, but nothing else is tested. See [docs/platforms.md](docs/platforms.md).
- The servers you want to run (llama.cpp, stable-diffusion.cpp, and so on) and their model files, in folders you
  choose. KLIF starts them; it does not install them.

To build KLIF yourself: Rust 1.90 or newer (MSVC toolchain), Node.js 20.19+ or 22.12+ with npm, and PowerShell
5.1+ for the build script.

## Quickstart

1. **Build.** From a clone of this repository:

   ```powershell
   .\scripts\Build-Release.ps1
   ```

   This builds the UI, `klif.exe` and `klif-cli.exe` into `dist\KLIF\` and prints their SHA-256. Pass `-OutDir`
   to put them elsewhere. The repository contains source; check that any binary you run matches a build you made
   or a hash you trust. If PowerShell's execution policy refuses the script, read it, then run it for this
   process only: `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\Build-Release.ps1`.

2. **Configure.** Copy [config/klif.example.toml](config/klif.example.toml) to `%APPDATA%\KLIF\klif.toml` and
   replace the fictional paths (`D:\llama.cpp`, `D:\models`, `192.0.2.x`) with yours:

   ```powershell
   New-Item -ItemType Directory -Force "$env:APPDATA\KLIF" | Out-Null
   Copy-Item config\klif.example.toml "$env:APPDATA\KLIF\klif.toml"
   ```

   Or skip the file: start KLIF with no configuration and use **Add a System** in the window. `klif-cli` finds the
   same file; set `KLIF_CONFIG` to use another one.

3. **Look before you launch.**

   ```powershell
   klif-cli status
   klif-cli plan s1
   ```

   `plan` prints the program, arguments, working folder and environment that Launch would use, with secrets
   masked. Run `klif-cli` from `dist\KLIF` (or put that folder on your PATH).

4. **Launch.** Start `klif.exe`, select a tab and press Launch, or:

   ```powershell
   klif-cli launch s1 --yes --wait
   ```

   Closing KLIF does not stop the servers. They keep running, and the next KLIF adopts them. Stop them from KLIF
   or with `klif-cli stop --all --yes`.

5. **Models.** Set `[paths] models_dir`, then use **Recommended** in the Tune drawer or `klif-cli models list` and
   `klif-cli models download <id> --yes`. Downloads are off until `models_dir` is set.

## Several machines

A second computer running KLIF can offer its Systems to this one. On that machine set `[node] listen` and create a
token with `klif-cli node token --create`; on this one add a `[nodes.<id>]` table with the address and the token
file. Remote Systems appear as extra tabs with the node's name next to them. Read
[docs/nodes.md](docs/nodes.md) before you enable it: the `edit` right lets the other machine run arbitrary commands,
and the traffic is not encrypted yet. A default install opens no network port.

## Windows Defender and SmartScreen

KLIF starts other programs, reads GPU and process counters, probes local HTTP ports and can open a network
listener when you ask. An executable that does these things and has no reputation yet can be flagged by
heuristics, or shown the "Windows protected your PC" SmartScreen page. Nobody can promise this will not happen on
your machine, and this project does not claim its builds are "clean" for all time.

What the project does to avoid looking like malware:

- KLIF's programs do not start PowerShell, `cmd.exe` or any script host themselves. They start exactly the program
  in your preset.
- No network port is opened unless `[node] listen` is set. The control channel `klif-cli` uses listens on
  `127.0.0.1` only and needs a per-run token.
- The executables carry full version information and a manifest that asks for no elevation (`asInvoker`).
- UI assets are embedded uncompressed, and release builds strip the builder's local paths.
- The hardware-identification reader that `klif-cli diag` uses for the RAM type is compiled only into
  `klif-cli.exe`, never into `klif.exe`.
- `Build-Release.ps1 -Sign` signs both executables when you provide a certificate or an Azure Trusted Signing
  profile through environment variables (see the script). Signing is optional and not set up in this repository.

What you can do:

- Build from source and compare the SHA-256 the build prints with `Get-FileHash -Algorithm SHA256 <file>`.
- Scan a file without remediation:

  ```powershell
  & "$env:ProgramFiles\Windows Defender\MpCmdRun.exe" -Scan -ScanType 3 -File "C:\path\to\klif.exe" -DisableRemediation
  ```

  One scan reflects one set of definitions at one time. Treat it as a data point, not as a certificate.
- You do not need to turn Defender off or exclude a folder to run KLIF, and you should not.

**If Defender flags a build you made from this source**, report it as a false positive to Microsoft:

1. Note the detection name: Windows Security, **Protection history**, or
   `Get-MpThreatDetection | Select-Object ThreatID, Resources, InitialDetectionTime` and `Get-MpThreat`.
2. Note the definitions version: Windows Security, **Virus & threat protection updates**, or
   `(Get-MpComputerStatus).AntivirusSignatureVersion`.
3. Open the Microsoft Security Intelligence file submission page,
   <https://www.microsoft.com/en-us/wdsi/filesubmission>. Signing in with a Microsoft account is optional, but it
   lets you track the submission.
4. Choose the submitter type **Software developer** and the reason **Incorrectly detected as malware/malicious**
   (not "Malware/malicious").
5. Upload the file, enter the detection name and the definitions version, and say in English what the program is:
   an open-source process manager at <https://github.com/koksny/klif>, the build commit, the SHA-256, and the
   behaviour that likely triggered the detection (it starts child processes and reads GPU counters).
6. Wait for Microsoft's answer. A cleared detection reaches machines with a later definitions update. The form may
   change; if it does, look for "submit a file for malware analysis" on Microsoft Security Intelligence.

A SmartScreen page for a file you built yourself or whose hash you verified can be passed with **More info**, then
**Run anyway**. Do not do that for a file someone else sent you.

## Docs

- [Overview](docs/overview.md): how the pieces fit, files, ports
- [Systems](docs/systems.md): kinds, statuses, conflicts, external servers
- [Presets](docs/presets.md): launch commands, placeholders, params, adapters, recommendations
- [klif-cli](docs/cli.md): every command and the JSON contract
- [Nodes](docs/nodes.md): several machines, rights, the security model
- [Platforms](docs/platforms.md): Windows and AMD first; CUDA, Linux and macOS through PRs
- [Brand](docs/brand.md): accent and mark
- [Publishing](docs/publish.md): what may enter git, what must not
- [Changelog](CHANGELOG.md), [Contributing](CONTRIBUTING.md), [Security](SECURITY.md), [Agent notes](AGENTS.md)

## License

MIT. Model weights you load through KLIF keep their own licenses. Third-party runtimes (llama.cpp, sd.cpp, ComfyUI,
and so on) keep theirs too.
