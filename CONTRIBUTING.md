# Contributing

This repo is the public tree of KLIF. Issues and PRs are welcome, and so are PRs written by your coding agent:
KLIF is a front-end built for AMD and Windows, and the expected way to get another GPU, another OS or other
models supported is a PR. [AGENTS.md](AGENTS.md) is the briefing for agents.

## Ground rules

- No model weights, no compiled HIP/Vulkan runtimes, no logs, no secrets.
- Paths in examples are relative or clearly fictional (`D:\models\...`, `D:\llama.cpp\...`), never a contributor's
  real home directory. Addresses in examples are `127.0.0.1` or the documentation range `192.0.2.x`; no real LAN
  addresses, host names, user names or e-mail addresses anywhere, including docs, bench output and screenshots.
- Bind examples to `127.0.0.1` unless the change is explicitly about LAN serving.
- Never put network or model-fetch flags (`-hf`, `--hf-repo`, `--model-url`, ...) or secrets in a recommendation or
  a preset you submit. Models come only from `klif-cli models download`.
- Read [docs/publish.md](docs/publish.md) if you are promoting code from a private tree, and run its search on your diff.
- Keep a PR to one subject. Small and reviewable beats complete.

## Development setup

You need Rust 1.90 or newer (MSVC toolchain on Windows), Node.js 20.19+ or 22.12+ with npm, and the WebView2
runtime to run the desktop app.

```powershell
# Rust: from the repository root
cargo check --workspace --all-targets
cargo build -p klif-cli            # target\debug\klif-cli.exe

# The desktop shell is its own Cargo workspace
cd app\src-tauri
cargo check

# The UI
cd app\ui
npm ci
npm run dev                        # Vite dev server, http://127.0.0.1:5193
npm run check                      # svelte-check
```

- The Vite dev server listens on `127.0.0.1:5193` with `strictPort: true`: it fails rather than picking another
  port, and so should you. If the port already answers, reuse that server. The Tauri `devUrl` points at it too.
- In a browser the UI runs against a mock engine with scenarios, so you can work on the front end without any
  model server: `http://127.0.0.1:5193/?skin=cliff&scenario=multi` (also `kinds`, `empty`, `node-down`, `many`,
  `invalid`, `downloads`; `size=mini` for the panel layout; `drawer=tune` opens the Tune drawer).
- To run the real shell against the dev server, start the dev server first, then `cargo run` in `app\src-tauri`.
  A release build needs the `custom-protocol` feature, which `scripts\Build-Release.ps1` sets; use that script.
- When you run `klif-cli` or the app, set `KLIF_CONFIG` to a scratch copy of `config\klif.example.toml`. Do not
  test against your live configuration, and do not start or stop your real model servers for a code change. A
  small fake HTTP server on `127.0.0.1` answering `/health`, `/v1/models` and a streaming chat completion is
  enough to drive most of the engine.
- `.local\` is gitignored; it is the place for your own `klif.toml`, state and scratch builds.
- There is no automated test suite yet. Say in the PR what you ran: the `cargo check` and `npm run check` lines
  above, `klif-cli` commands with their output (paths and addresses made fictional), screenshots of UI changes.

### Layout

See the repo map in [AGENTS.md](AGENTS.md). Two contracts to keep in step: the view model in
`crates/klif-common/src/vm.rs` and `app/ui/src/lib/model/types.ts`, and the `klif-cli` JSON in
[docs/cli.md](docs/cli.md) (`schemaVersion` changes only for a breaking change).

## Kinds of PR

### Code (fix or feature)

- [ ] One subject; the description says what changed and why.
- [ ] `cargo check --workspace --all-targets`, `cargo check` in `app\src-tauri`, `npm run check` in `app\ui`.
- [ ] If you touched the view model, both `vm.rs` and `types.ts` changed.
- [ ] UI change: screenshots of the affected skins, full and mini layout.
- [ ] Nothing starts a shell or a script host; nothing opens a network port by default.

### Platform (another GPU vendor or OS)

- [ ] Describes the hardware and OS you tested on, driver and backend versions, and what you did not test.
- [ ] Windows behaviour is unchanged (the Windows and AMD build stays the reference); platform code is behind
      `cfg` or a trait ([docs/platforms.md](docs/platforms.md) lists the seams).
- [ ] Pure crates still check for `x86_64-unknown-linux-gnu`
      (`cargo check -p klif-common -p klif-catalog -p klif-supervisor -p klif-telemetry --target x86_64-unknown-linux-gnu`).
      Checking `klif-core` and `klif-cli` there also needs the OpenSSL development files and `pkg-config`
      (`ureq`'s `native-tls`, see [docs/platforms.md](docs/platforms.md)).
- [ ] Evidence that it works: `klif-cli --json status` and `klif-cli bench` output from the real hardware.

### Preset (a launch command for a server or model)

- [ ] A `[presets.<id>]` snippet that `klif-cli plan` accepts with no error issues, for a server others can run.
- [ ] Fictional paths, `{models_dir}` or `{model}` instead of real ones; `--host {host} --port {port}` passed through.
- [ ] No secrets in `args`, no fetch flags, nothing that depends on one person's machine (HIP device ordinals are
      machine-specific; say so in `notes` or leave them to the user's own preset).
- [ ] The adapter, kind and `health` are right for the server, and you ran it once.
- [ ] Goes in `docs/presets.md` as an example or in a new doc, not in a user's `klif.toml`.

### Recommendation (an entry in `crates/klif-catalog/data/recommendations.toml`)

- [ ] `hf_repo`, a 40-character `revision` and every file's `sha256` and `size` were checked against the Hugging
      Face API for that revision. A file that comes from another repo names its own `repo` and `revision`, and those
      were checked the same way.
- [ ] `license` is the model's actual license; the PR notes any condition (gated, non-commercial).
- [ ] `args` use placeholders (`{model}`, `{ctx}`, `{host}`, `{port}`), no network or secret flags, no `env`.
- [ ] `measured` only if you ran `klif-cli bench` on a file whose SHA-256 equals the hub's; `hardware` and `backend`
      say what it was measured on, `date` is the day you measured.
- [ ] `kind` (and `class` for an LLM) are right; `min_vram_gib` and `hardware_class` are honest.
- [ ] The file still loads: `klif-cli models list` shows the entry and no warning about it.

### Bench (measured numbers)

- [ ] `klif-cli bench <system> --yes` output with default options (or the options stated), the preset it ran, and
      its `hash`.
- [ ] GPU, VRAM, driver, backend build and what else was running. Shared-GPU numbers (`--allow-shared`) are labelled.
- [ ] No paths, hostnames or addresses in what you paste.

### Docs

- [ ] English, plain and specific. Commands and output were run, not recalled.
- [ ] Example values are fictional.

## License

By contributing you agree the patch is MIT-licensed, same as [LICENSE](LICENSE).
