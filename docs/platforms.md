# Platforms

KLIF is a front-end built for AMD and Windows. That is the build that is tested, and the one every decision here
favours. Other GPUs and other operating systems are expected to arrive as pull requests, ideally written by your
coding agent for your hardware. This page says what works today, which parts are platform-specific, and what a good
port looks like.

## Status

| | Status |
| --- | --- |
| Windows 10/11 x64, AMD RDNA GPU, HIP and Vulkan builds of the servers | Tested. The reference build |
| Windows, NVIDIA or Intel GPU | Untested. Presets are plain commands, so a CUDA or SYCL build of llama.cpp can be launched; GPU memory is read through DXGI and Windows counters, which are not AMD-specific, but nobody has checked them. The dormant-GPU detection is AMD-only |
| macOS 13+ on Apple silicon, Metal build of llama.cpp | Tested on an M4 (8-core GPU, 16 GB, macOS 27) with llama.cpp b11399 (Metal) and Qwen3.5 4B Q4_K_M: `KLIF.app` and `klif-cli` from `scripts/build-release.sh`, the nine skins in WKWebView, the server launched, stopped (SIGTERM; SIGKILL after 5 s for one that ignores it) and adopted after the app was killed and restarted, `klif-cli bench` (28.1 tok/s decode, 265 tok/s prefill, 2.0 s to the first token of a 512-token prompt, 1.6 s load, 3.8 GiB peak), records, hardware facts and suggestions; a build signed with a Developer ID and notarized by `--notarize` (Gatekeeper: "Notarized Developer ID", also for a quarantined download). Untested: other M-series chips (their GPU facts are read the same way), panel mode on a real second screen. Intel Macs are not a target |
| Linux | Not supported. The pure crates (`klif-common`, `klif-catalog`, `klif-supervisor`, `klif-telemetry`) compile for `x86_64-unknown-linux-gnu`; the process host has a basic process-group implementation that has never run; GPU and host telemetry return nothing; `klif-core` and `klif-cli` do not build there yet (they need OpenSSL for `ureq`'s `native-tls`, see below); the shell was never built |

"Not supported" means: not tested, not promised, welcome as a PR.

## Where the platform shows

| Layer | Windows | macOS | Linux |
| --- | --- | --- | --- |
| **Process host** (`klif-supervisor`) | `CreateProcessW` with a named job object (`Local\KLIF-<session>`) and an explicit handle list; output appended to log files; stop = terminate the job and wait; adoption by job name | `unix.rs`: each server leads its own process group (`pgid:<n>`), run with `execve` (never a shell), output appended to log files. Stop = SIGTERM to the group, SIGKILL after 5 s, 20 s in all. Adoption: the group, only when the root's start time (`proc_pidinfo`) matches the record; a group whose root has exited only when a member still writes to the session's log. Port owners from `proc_pidfdinfo` (the user's own processes). Tested | The same process groups with `/proc`; never run |
| **GPU and host facts** (`klif-telemetry/src/platform.rs`: `GpuPlatform`, `HostPlatform`) | DXGI adapters and memory, PDH counters for per-process VRAM (dedicated, shared, committed), SetupDi power state, the AMD ULPS setting (read-only), CPUID for the CPU name, `GlobalMemoryStatusEx` | `mac.rs`: Metal's default device (name, `recommendedMaxWorkingSetSize` as the unified pool), the `AGXAccelerator` IORegistry entry (core count; "In use system memory" live), the device tree (SoC id, GPU clock table), `sysctl` for the CPU and RAM, `host_statistics64` for free memory, `proc_pid_rusage` (physical footprint) per process. No root, no `powermetrics` | A stub that returns nothing. Where to read: AMD `/sys/class/drm/card*/device/mem_info_vram_used` and per-process `/proc/<pid>/fdinfo` (`drm-memory-vram`); NVIDIA through NVML |
| **Adapters** (`klif-telemetry`, `klif-catalog/src/facts.rs`) | llama.cpp, sd.cpp, vllm, openai, audiocpp, generic | OS-neutral: they parse logs and call HTTP | OS-neutral |
| **Shell** (`app/src-tauri`) | Windows-only code behind `cfg(windows)`: clipboard (secret formats), GPU pinning for the WebView, WebView2 setup; Tauri for window and tray | `macos.rs`: clipboard (NSPasteboard; a secret stays on this Mac and carries the concealed and transient markers), native Yes / No alerts; a template icon in the menu bar; WKWebView keeps its own data (`~/Library/WebKit/<bundle id>`); no GPU pinning | `other_os.rs` stand-ins; never built |
| **Downloads and bench TLS** | `ureq` with the system trust store (SChannel) | `native-tls` is Security.framework: nothing to install | `native-tls` is OpenSSL: the build needs the OpenSSL development files and `pkg-config` (`libssl-dev` and `pkg-config` on Debian and Ubuntu, `openssl-devel` on Fedora), and `cargo check` of `klif-core` and `klif-cli` stops in `openssl-sys` without them. The pure crates are unaffected (`klif-telemetry` probes over plain HTTP and uses no TLS) |
| **Config and state locations** | `%APPDATA%\KLIF`, `%LOCALAPPDATA%\KLIF` | `~/Library/Application Support/KLIF` (config, state, secrets), its `data` subfolder (records, bench, node cache), logs in `~/Library/Logs/KLIF` | `$XDG_CONFIG_HOME/klif` and `$XDG_DATA_HOME/klif` (untested) |
| **Command building** | Windows command-line quoting; `.exe`/`.com` only; `.bat` explained | POSIX quoting in previews; any file with the execute bit, run directly; a missing bit, a folder or an `.app` bundle is explained | As macOS |
| **Release** | `scripts/Build-Release.ps1` | `scripts/build-release.sh`: `KLIF.app`, `klif-cli`, a zip and its SHA-256; ad-hoc, Developer ID or notarized | none |

The engine, the catalog, the view model, the UI and `klif-cli` are the same everywhere. The seams above are small
on purpose.

## AMD specifics

- **HIP (ROCm) and Vulkan** are choices of the server build, not of KLIF. A preset launches whichever
  `llama-server` or `sd-server` you point it at; `backend = "HIP"` only labels it in the window.
- **Dormant GPU.** On some AMD cards Windows lets the card idle into a low-power state (D3, with ULPS enabled) and
  pages VRAM contents out. KLIF reads the device power state and the ULPS setting (never writes either), tells
  committed from resident memory per process, and draws paged-out VRAM as ghosts instead of reporting it as free.
  The first request after a sleep pays a wake-up delay; changing the ULPS setting is your decision and outside KLIF.
- **Device selection is in the command.** See below.

### The static HIP device ordinal trade-off

Which GPU a ROCm program uses is chosen by the program's own environment or flags: `HIP_VISIBLE_DEVICES=1`,
`ROCR_VISIBLE_DEVICES`, or a flag such as llama.cpp's `--device`. Those take an **ordinal** (0, 1, 2), which is the
order the runtime enumerates the cards in, not a PCI id. KLIF's `gpu = "VEN:DEV"` is a different thing: it says which
GPU to *measure and plan for* (memory bars, fit, conflicts) and never changes the command.

KLIF could translate one into the other, but it deliberately does not. The trade-off:

- **Static (what KLIF does).** The ordinal lives in the preset's `env` or `args`, visible in `plan`. The command is
  the whole truth, it works with any server and any runtime, and nothing happens behind your back. The price is that
  an ordinal belongs to one machine in one configuration: adding a card, moving one to another slot, a driver
  update or a different runtime can renumber them. Such a preset is not portable, which is why committed examples
  and recommendations carry no ordinal, and why `preset_from_recommendation` copies the program, working folder and
  environment from *your* existing preset.
- **Dynamic (what KLIF avoids).** Resolve `gpu = "VEN:DEV"` to an ordinal at launch by asking the runtime. It would
  survive renumbering, but only by reimplementing each runtime's enumeration, guessing which variable each server
  honours, and hiding part of the command.

Your safeguards: set both `gpu` and `HIP_VISIBLE_DEVICES` and check them against `klif-cli --json diag`
(`engine.gpus[].id` in DXGI order); read the `device` line in the server log (KLIF parses it for llama.cpp and
sd.cpp). If the server reports a device that does not match the preset's `device` (default: the GPU's name), the
System shows a warning "The server reports X, not Y: check the preset's device selection (env or args)". The same
applies to CUDA (`CUDA_VISIBLE_DEVICES`) and Vulkan (`GGML_VK_VISIBLE_DEVICES`) builds.

Do not commit an ordinal from your machine as if it were universal.

## macOS specifics

- **GPU ids.** Apple GPUs have no PCI device id. KLIF uses Apple's PCI vendor id `106B` and the SoC id from the
  device tree as the device: an M4 (`t8132`) is `106B:8132`, an M1 (`t8103`) `106B:8103`. Use that id in `[gpu]
  inference` (the GPU is measured only when `[gpu]` or a preset names it, as on Windows), in a preset's `gpu` and in
  `[hardware]`; `klif-cli --json diag` lists it.
- **Unified memory.** The GPU's memory is the working set Metal recommends (`recommendedMaxWorkingSetSize`: about
  three quarters of RAM on a 16 GB Mac, read from the system, changed by `iogpu.wired_limit_mb`). The inventory
  counts it as an integrated GPU's pool, and `klif-cli suggest` leaves the rest of the RAM for the system. Live
  memory is the accelerator's "In use system memory"; a session's share is its processes' physical footprint.
- **FP32 TFLOPS.** The GPU table lists the figures Apple published (only the M1's 2.6 TFLOPS); every other Apple GPU
  is computed from the machine: cores x 128 lanes x 2 x the top state of its clock table, shown as an estimate. The
  CPU estimate uses third-party clock measurements per chip (Apple publishes none), listed with their sources in
  `klif-telemetry/src/cpu.rs`; a chip missing there shows "?".
- **One GPU for the model and the window.** The skins animate on the same GPU the model runs on: with the KLIF
  window open in front, the M4 above decoded 24.5 tok/s instead of 28.1. A minimized or fully covered window stops
  animating (WebKit pauses hidden pages), so bench with KLIF in the background.
- **A server's memory** is its resident memory: llama.cpp maps the model file into the GPU's working set
  (`MTL0_Mapped`), which the process's footprint does not count, so KLIF uses `ri_resident_size` (it matches the
  server's own memory breakdown).
- **Servers keep running** when KLIF quits: each one leads its own process group, outside KLIF's. Quitting from the
  menu bar or with Cmd+Q never stops a server.
- **Program names.** An app started from Finder or the Dock gets a short PATH (`/usr/bin:/bin:/usr/sbin:/sbin`);
  a bare `command` is also looked up in `/opt/homebrew/bin` and `/usr/local/bin`. Anywhere else, write the absolute
  path. KLIF does not expand `~` in paths.
- **Metal is the backend.** Use the macOS arm64 build of llama.cpp (Metal is built in). Vulkan on a Mac runs
  through MoltenVK and is not what KLIF is tested with.
- **Shortcuts.** Cmd+1..9 switch skins too. F2 and F3 need Fn on a MacBook keyboard.

### Gatekeeper

A build that is not notarized by Apple (the default of `scripts/build-release.sh`, or a zip from someone else) is
stopped the first time it is opened: "Apple could not verify KLIF is free of malware". To run it anyway, once:
open it, choose Done, then System Settings > Privacy & Security > "KLIF was blocked" > Open Anyway (or Control-click
`KLIF.app` > Open on older macOS). For `klif-cli` the same applies the first time it runs from a quarantined zip. A
build made on the same Mac carries no quarantine flag and simply runs. Check what you got with
`shasum -a 256 KLIF-<version>-macos-arm64.zip` against the `.sha256` next to it.

### Signing and notarization

`scripts/build-release.sh --sign` signs with a Developer ID Application identity (hardened runtime, secure
timestamp); `--notarize` also sends the zip to Apple's notary service, waits and staples the ticket. One-time setup,
done by the owner of the Apple Developer account (the script never handles a password or a key):

1. In the Apple Developer account, create a **Developer ID Application** certificate (Xcode > Settings >
   Accounts > Manage Certificates > +, or the developer website with a certificate signing request) and keep it in
   the login keychain. `security find-identity -v -p codesigning` then lists it. With several, set
   `KLIF_SIGN_IDENTITY` to the one to use.
2. Store notary credentials in the keychain under a profile name:
   `xcrun notarytool store-credentials klif-notary --apple-id <your Apple ID> --team-id <team id>` (it asks for an
   app-specific password created at account.apple.com), or with an App Store Connect API key (`--key`, `--key-id`,
   `--issuer`). `KLIF_NOTARY_PROFILE` overrides the profile name `klif-notary`.

A stable Developer ID signature also lets macOS keep the user's "allow incoming connections" answer across updates,
which matters for a node listener (`[node] listen`). An ad-hoc build asks again after every rebuild.

### Firewall

With `[node] listen` set, the first time KLIF listens macOS may ask whether KLIF may accept incoming network
connections (when the firewall is on: System Settings > Network > Firewall). Allow it for a node that other machines
use; KLIF never changes firewall settings itself.

## Writing a port

A useful port is small and says what it was tested on. Typical pieces:

1. **GPU facts for another vendor on Windows.** Usually nothing to write: check that `klif-cli --json diag` lists
   your card with the right `id`, that `status` shows used and total VRAM, and that per-process VRAM
   (`vramGiB` while a System runs) is non-zero. If a counter is missing, fix it in `klif-telemetry/src/win.rs`.
2. **A server adapter for a CUDA/SYCL/Metal build.** Usually only a preset (`backend = "CUDA"`, the right environment
   variable). If the server's log format differs, extend the parser behind the existing adapter instead of adding a
   new one.
3. **Linux or macOS.** In order:
   - make `klif-core` and `klif-cli` build (install the OpenSSL development files and `pkg-config` for
     `native-tls`, or pick a TLS provider per platform; unix paths);
   - implement `GpuPlatform` and `HostPlatform` for the platform;
   - harden the unix `ProcessHost` (the process group needs a reliable adoption and stop story);
   - compile `app/src-tauri` and replace the Windows-only shell code behind `cfg`;
   - `config` already has XDG locations; check them.
4. **Keep Windows intact.** The Windows and AMD build is the reference. Use `cfg` or the platform traits; do not
   change behaviour there without saying so.

Checks to run and report in the PR: [CONTRIBUTING.md](../CONTRIBUTING.md) lists them, including the Linux
`cargo check` of the four pure crates. Include `klif-cli --json status` and a `klif-cli bench` from the real
hardware, with the machine details made fictional where they identify you.
