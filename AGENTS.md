# Agent notes (public)

This repository is the **public** KLIF tree: [github.com/koksny/klif](https://github.com/koksny/klif).

## Do

- Keep commits small and publishable.
- Prefer `$PSScriptRoot` and environment/config over absolute paths.
- Default HTTP bind to `127.0.0.1`. The one exception is klif-webui (`[webui] host`, default `0.0.0.0`): its purpose is
  a phone on the LAN, and it is off until the user turns it on.
- Follow [docs/publish.md](docs/publish.md) before adding files that originated in a private workshop.
- If `AGENTS.local.md` exists in the working copy, treat it as the private map (workshop root, hardware, LAN). Do not copy its contents into git.

## Do not

- Do not commit weights, HIP/Vulkan binaries, logs, launcher state, or `.env`.
- Do not put LAN IPs, home hostnames, or personal filesystem roots in tracked files.
- Do not vendor `node_modules` or fat HIP DLLs.
- Do not `git push --force` to `main`.
- Do not switch the operator's daily Cursor workspace onto this repo unless they asked.

## Product voice

Window title **KLIF**. Tagline: **Koksny.com LOCAL INFERENCE FORNICATOR**. Subline: **AMD optimized frontend for transformers and DiT**.

---

# Guide for the user's coding agent

This section is for an agent working for someone who installed KLIF or who is changing this repository. The
policy KLIF asks of agents:

> KLIF is a front-end built for AMD and Windows, and it runs on macOS with Apple silicon. Different GPU or OS? Let
> your agent open a PR. Installed KLIF? Let your agent tune and calibrate it. Want other models than the defaults?
> Let your agent set them up.

**Before you open a pull request**, read [CONTRIBUTING.md](CONTRIBUTING.md). KLIF is maintained for its author's
daily use, and the bar is the same for agents and humans:

- **Useful here.** It fixes a real bug, or it improves daily work with local models for the maintainer as well.
  Anything that only your user needs stays in their fork or their own `klif.toml`; most tuning and preset work never
  needs a PR at all.
- **Checked three times.** It builds and passes the checks below; a session or model that did not write it
  reviewed the diff and everything it found is fixed; it ran on real hardware (or, for UI work, on the mock engine
  in every affected skin), with the evidence in the PR. Fill in `.github/PULL_REQUEST_TEMPLATE.md` honestly,
  including which model wrote the change and which reviewed it.

A pull request that misses either bar is closed without discussion. Do not open one to "start a conversation":
open an issue that describes the problem.

| The user wants | You do | Where it ends up |
| --- | --- | --- |
| KLIF on another GPU vendor or OS | Implement the platform pieces ([docs/platforms.md](docs/platforms.md)), check, open a PR | this repository |
| Their installed KLIF tuned | Bench, change one thing, bench again with `klif-cli` (below) | their `klif.toml` |
| Models other than the defaults | Write presets, download only on request | their `klif.toml`; a PR only if it helps others |

Read first: [docs/overview.md](docs/overview.md), [docs/presets.md](docs/presets.md), [docs/cli.md](docs/cli.md).
Details that matter to you are repeated here so you can work from this file.

## Repo map

| Path | What it is |
| --- | --- |
| `crates/klif-common` | Shared types: `klif.toml` (`config.rs`), the view model (`vm.rs`, mirrored by `app/ui/src/lib/model/types.ts`), secret masking, launch spec, Windows command-line rendering |
| `crates/klif-catalog` | Presets to commands: placeholder and param resolution, validation issues, launch plans, model facts, comment-preserving `klif.toml` edits (`store.rs`), the embedded model pool (`data/recommendations.toml`) and the per-machine suggestions (`suggest.rs`) |
| `crates/klif-supervisor` | Starts, adopts and stops server processes (Windows job objects; basic process groups elsewhere) |
| `crates/klif-telemetry` | GPU, CPU and RAM readings (`platform.rs`), per-adapter log parsers and probes |
| `crates/klif-core` | The engine (`engine.rs`, `engine/`), `state.v3.json`, the control protocol (`wire.rs`, `control.rs`, `link.rs`), nodes (`nodes.rs`), klif-webui (`webui.rs`), `bench.rs`, `download.rs`, `keys.rs` |
| `crates/klif-cli` | `klif-cli.exe` |
| `app/src-tauri` | `klif.exe`: Tauri 2 shell, tray, panel mode, IPC commands. Its own Cargo workspace |
| `app/ui` | Svelte 5 + Vite front end: `src/skins` (nine skins), `src/lib/shell` (Tune drawer in `shell/tune`), `src/lib/mock` (the browser mock engine), `src/webui` (the klif-webui page) |
| `config/klif.example.toml` | A complete, parseable configuration template |
| `docs/`, `scripts/Build-Release.ps1` | Documentation; the release build |
| `skills/klif/SKILL.md` | The Agent Skill (Agent Skills format) that teaches a coding agent to drive KLIF with `klif-cli`; the release build copies it next to the exes |

The view model is the contract between Rust and the UI: change `vm.rs` and `types.ts` together. Never touch
`app/ui/src/skins/private/`, `app/ui/proto/lively/`, `app/ui/proto/ref/` or `app/ui/public/private/`; they are
local-only and must never ship.

## Build and check

```powershell
cargo check --workspace --all-targets      # repo root
cargo build -p klif-cli                    # target\debug\klif-cli.exe
cd app\src-tauri; cargo check              # klif.exe (separate workspace)
cd app\ui; npm ci; npm run check           # svelte-check
cd app\ui; npm run dev                     # Vite on http://127.0.0.1:5193
.\scripts\Build-Release.ps1                # dist\KLIF\klif.exe and klif-cli.exe
```

- The dev server is `127.0.0.1:5193` with `strictPort`. If something already answers there, reuse it; do not
  start another one on a different port.
- There is no automated test suite yet. Verify with the checks above and by running `klif-cli` against fake
  servers.
- When you run `klif-cli` or the app for experiments, set `KLIF_CONFIG` to a scratch copy of the config (for
  example from `config/klif.example.toml`). Do not point experiments at the user's live `klif.toml`, and never
  start or stop their model servers unless they asked for exactly that.

## klif.toml

Looked up through `KLIF_CONFIG`, then `.local\klif.toml` above the exe or the working folder, then
`%APPDATA%\KLIF\klif.toml` (macOS: `~/Library/Application Support/KLIF/klif.toml`). Edits are picked up while
KLIF runs; a file that does not parse keeps the last good configuration. Sections and entries are read one by one,
so one bad `[presets.x]` does not hide the rest. Unknown sections (`[launcher]`, `[krea]`) are ignored. Full
template: `config/klif.example.toml`.

```toml
[net]        llm_host, image_host            # default "127.0.0.1"; image presets use image_host, the others llm_host
[gpu]        inference, ui, inference_name   # PCI "VEN:DEV"; "VEN:DEV#1" = second identical card; inference is the default GPU of presets
[ui]         frameless, panel_monitor, record_moment (the "new record" moment; default true), skin   # skin = the window's skin id; the window writes it, klif-webui follows it
[telemetry]  warn_below_gib, verbose_llama_logs, ram_type
[paths]      models_dir, logs_dir            # models_dir has no default: downloads are refused until it is set
[security]   api_key = "file" | "env:NAME" | "none"
[launch]     on_conflict = "ask" | "stop"
[hardware]   exclude = ["VEN:DEV"], include = ["VEN:DEV"], tflops = { "VEN:DEV" = 48.7, cpu = 4.4 }   # corrects klif-cli hardware; ids as in [gpu], or "cpu"
[webui]      enabled (default false), host (an IP address, default "0.0.0.0"), port (default 7341)   # klif-webui, the control page for a phone on the LAN: docs/webui.md; only klif.exe serves it

[systems.<id>]       # id: [a-z0-9][a-z0-9_-]{0,31}; tab order = file order
label, kind = "llm"|"image"|"tts"|"stt"|"video" (required), class = "fast"|"deep"|"max" (llm only),
preset = "<preset id>", params = { <param> = "<choice>" }, exclusive = true|false

[presets.<id>]       # id: [a-z0-9][a-z0-9_-]{0,63}
name, adapter = "llama.cpp"|"sd.cpp"|"vllm"|"openai"|"audiocpp"|"generic", kind (required for generic),
command, args = [..], cwd, env = { K = "V" }, env_remove = [..], port, host, endpoint (external server),
health = "/path" | "tcp", model, mmproj, ctx, gpu = "VEN:DEV" | "cpu" | "a,b", managed, api_key,
model_name, quant, backend, device, notes, recommended
[presets.<id>.params.<name>]   label, default, choices.<value> = { label, vars = {k="v"}, args = [..], env = {K="V"} }

[node]               # this machine as a node: name, listen = "0.0.0.0:7340", allow = ["launch","edit"]
[nodes.<id>]         # a remote node: address = "192.0.2.10:7340", token = "file:<path>" | "env:NAME", name
```

Placeholders in `command`, `args`, `cwd` and `env` values: `{model} {mmproj} {ctx} {host} {port} {models_dir}
{state_dir} {data_dir} {stamp} {env:NAME} {p.NAME} {p.NAME.VAR}`. `{p.NAME}` must be a whole argument and expands
to the selected choice's `args`. `{stamp}` is the launch's time stamp (`yyyyMMdd-HHmmss-fff`, local time) for file
names that must differ per launch, such as a server's `--log-file`; previews and `plan` show it as written.
`command` is an absolute path or a name on PATH to an `.exe` or `.com` (macOS: to a file with the execute bit, run
directly); KLIF does not run `.bat` or `.ps1` files for you. Pass `--host {host} --port {port}` through
(`--listen-ip` / `--listen-port` for sd.cpp) or the server listens where KLIF is not looking. `gpu` is for display
and fit only: the command itself must select the device (for example `HIP_VISIBLE_DEVICES`).

Prefer `klif-cli presets set|save|param` and `klif-cli systems ...` over editing the file by hand: they validate,
keep comments and table order, and refuse a file that does not parse. Comments inside an array KLIF rewrites (an
edited `args`) are not kept. Choosing another preset (`presets use`) removes the System's param selections that the
new preset does not declare, so run `plan` afterwards and set the ones you still want.

## klif-cli contract

`klif-cli [--json] <command>`; `--json` may appear anywhere. Details and every command: [docs/cli.md](docs/cli.md).

- **One JSON document on stdout**, always with `"schemaVersion": 1`. Progress and notes go to stderr. Keys are in
  alphabetical order, so do not rely on `schemaVersion` coming first.
- **Streams:** `watch` and `logs --follow` print one compact JSON object per line on stdout (`schemaVersion`, `type`,
  `at`), and `models download --json` prints its progress the same way on stderr; the final document of a download
  still comes on stdout. Other stderr lines are plain text. An error ends a stream as the error document on one line.
- **Errors:** `{"schemaVersion": 1, "error": {"code": "...", "message": "..."}}`, exit code 1. Usage mistakes and a
  missing `--yes` exit with 2 (`usage`, `needs_yes`). Codes: `usage needs_yes not_found ambiguous refused
  engine_busy engine control fault stopped timeout unsupported invalid io bench download cancelled error`.
  `message` is one sentence meant to be shown to the user.
- **`--yes`** is required to launch, stop, restart, remove a System, delete a preset, download a model, clear the
  key, forget a record, replace a node token, open a klif-webui pairing (`webui pair`) or remove a paired device
  (`webui forget`). It is the user's decision; do not add it to make an error go away.
- **`webui`** shows klif-webui (on or off, where it listens, `error`, the paired devices, whether a pairing is open;
  never a pairing code); `webui on|off [--host IP] [--port N]` changes `[webui]`, `webui cancel` closes a pairing.
  Only the KLIF window app serves the page: with no window `klif-cli` reports `error` and `webui pair` is refused.
  See [docs/webui.md](docs/webui.md). `settings skin <id>` sets `[ui] skin`.
- **System arguments:** an id (`s1`, `render-box/s1`), a label ignoring case and spaces (`system1`), or the 0.2
  names `low|medium|high|krea` for `s1|s2|s3|cgi`.
- **`status`** returns `StatusJson`:

  ```json
  {
    "schemaVersion": 1,
    "selected": "s1",
    "systems": [
      { "id": "s1", "label": "System 1", "kind": "llm", "class": "fast", "status": "online",
        "preset": "fast-8b", "baseUrl": "http://127.0.0.1:7030/v1", "model": "Fast 8B",
        "decodeTps": 62.5, "vramGiB": 9.4 }
    ],
    "gpus": [ { "id": "1002:7550", "name": "RX 9070 XT", "usedGiB": 11.2, "totalGiB": 15.9 } ],
    "nodes": [ { "id": "render-box", "state": "online", "latencyMs": 1.2 } ]
  }
  ```

  Only `id`, `label`, `kind` and `status` are always present; `reason` (why a System is invalid, offline or
  unreachable), `fault` (the title of a fault) and, for a System on another machine, `node` appear when they apply,
  and such a System's id is `<node>/<id>` (`render-box/s1`). `status` is one of `not-set invalid offline starting
  online busy stopping fault unreachable`. `status <system>` returns that one entry. `baseUrl` is what clients
  use; for an LLM it ends in `/v1`. `adapter` says which API it speaks (`llama.cpp`, `sd.cpp`, `audiocpp`, ...) and
  `apiKey: true` that the server wants KLIF's API key, `params` the param choices it launches with; [skills/klif/SKILL.md](skills/klif/SKILL.md) has the request
  and answer of each (KLIF has no command that generates: send the work to `baseUrl`).
- Never guess sleep times. `launch --wait` blocks until a System is online. `watch --until <system>=<status>
  --timeout <seconds>` waits for any status and ends with `fault` or `stopped` when a System that should come online
  does not; `watch` alone streams status changes, faults, new records and download progress as JSON lines.
  `status` is the snapshot, `logs <system>` the server's last 200 console lines (`--follow` to keep reading).
  `plan <system>` returns
  `{system, label, preset, launchable, command}` where `command` holds `program`, `args`, `cwd`, `env[]` (secrets
  masked), `host`, `port`, `hash` and `issues[]`; nothing is started. `launchable` is `false` (and `refused` says
  why) when a launch would be refused: an error in `issues[]`, an invalid System, or an external server. `plan`
  still shows the command of such a System and exits 0, so read `launchable` before acting on the output. It
  fails only when there is no command to show (no preset selected).
- **Hardware and records:** `klif-cli hardware` lists the GPUs, CPU and RAM with their peak FP32 TFLOPS and the VRAM
  pool; it reads the machine directly (no engine, no GPU woken) and `[hardware]` corrects it. `klif-cli records` lists
  the best values per model file and backend (decode, prefill, time to first token, seconds per image...) with the
  conditions they were reached under, `records history <key>` how a value climbed, `records forget <key> --yes`
  removes a junk entry. They are kept in the data folder: `records.json`, `records-history.jsonl` (one line per
  broken record) and `hashes.json` (SHA-256 of the model files); never edit them by hand.
- **Describe yourself:** `klif-cli --json help` is the machine-readable catalog (every command with its arguments
  and flags, whether it needs `--yes`, whether it may start an engine, which documents it prints); `klif-cli schema
  [<name>]` prints the JSON Schema of a document. Read them instead of parsing the text help.
  [skills/klif/SKILL.md](skills/klif/SKILL.md) is an Agent Skill that teaches this contract in one file.
- klif-cli talks to the running GUI's or `klif-cli serve`'s engine; with none running it runs one itself for the
  duration of the command. Model servers it launched keep running after it exits.

## Bench, calibrate, PR

1. `klif-cli --json status` and `klif-cli plan <system>`: know what runs now. Read the `issues`.
2. Make sure no other System shares the GPU; bench refuses otherwise (`--allow-shared` overrides, and the numbers
   are then shared-GPU numbers).
3. `klif-cli bench <system> --yes` launches the System if needed, sends fixed requests (default 3 runs, 512 prompt
   tokens, 128 generated; image servers: seconds per image), records load time, time to first token, prefill and
   decode speed and peak VRAM, and stops what it started. `klif-cli bench list --preset <id>` shows the history.
   tts and stt are benched too (audio seconds per wall second; stt needs `--audio <file.wav>`), music on an audio.cpp
   server only (a fixed 30 s instrumental, `musicRtf`; a run lasts as long as the song takes, try `--runs 1`); video
   is not benched yet; remote Systems must be benched on their own machine.
4. Change **one** thing (`presets set <id> ctx=32768`, `args+=--no-mmap`, `presets param <system> <name>
   <value>`), run `plan` again, restart the System, bench again, compare. Keep the change if it is better and the
   output is still right; revert it if not. A record is marked stale when the command's hash changed since.
5. Calibrate for the user's hardware and workload, not for a benchmark number. Write down what you changed and why in the
   preset's `notes`.
6. Open a PR only for something that helps others (a preset, a recommendation entry, bench numbers, a platform
   port). Never include a configuration file. See [CONTRIBUTING.md](CONTRIBUTING.md) for the checklists.

A **recommendation** is one quant rung of a model in the pool, `crates/klif-catalog/data/recommendations.toml`
(schema 2; the header of that file has the shape and the estimate model), with the id `<model>.<rung>`. A model must
point at a Hugging Face repo and a 40-character commit sha, each rung at files with their SHA-256 and size, all
verified against the hub; a file that comes from another repo carries its own `repo` and `revision`, verified the
same way. An LLM model also needs its KV cache cost (`kv`, worked out from its `config.json`; write the arithmetic
in a comment). Add `measured` only for numbers you measured with `klif-cli bench`, on a file whose SHA-256 equals
the hub's, and name the hardware class and backend. Recommendations are starting points, not promises.

`klif-cli suggest [--kind K] [--class C]` shows which rung the pool suggests for each slot of this machine (System 1
/ 2 / 3, image), with estimated VRAM and RAM against each tier's budget; `--json` returns
`{hardware, suggestions[]}` (docs/cli.md). `klif-cli models adopt <rec> --system <S>` then uses the suggestion's
context and KV type (or `--ctx N --kv TYPE`). Every number there is an estimate: bench before you rely on it.

## Safety rules

- **Never print, log, commit or paste secrets:** the KLIF API key (`api-key.txt`), node tokens (`node-token.txt`,
  `*.token`), `control.json`, Hugging Face tokens, and any `env` value whose name contains KEY, TOKEN, SECRET, PASS
  or AUTH. Do not `cat` those files. `klif-cli key set` reads the key from stdin. `klif-cli node token --create`
  prints a token once: have the user run it, or redirect its output to a file without echoing it. KLIF masks
  secrets as `••••`; sending that value back means "keep what is stored".
- **No secrets in `args`.** A key in the argument list is visible to every local process. Use `api_key = true` or
  an `env` variable.
- **Never put network or model-fetch flags into a recommendation or a preset you submit:** `-hf`, `-hfr`,
  `--hf-repo`, `-hff`, `--hf-file`, `-hft`, `--hf-token`, `-mu`, `--model-url`, `-dr`, `--docker-repo`, their
  draft, vocoder and mmproj variants (`-mmu`, `--mmproj-url`, ...), and secret flags such as `--api-key`.
  Recommendations also cannot carry `env`. Models arrive only through `klif-cli models download <id> --yes`, which
  fetches from huggingface.co and nowhere else.
- **Never commit personal data.** Use fictional paths (`D:\models`, `D:\llama.cpp`), documentation addresses
  (`192.0.2.x`) and no hostnames, user names, e-mail addresses or machine names, in code, docs, examples, bench
  output and screenshots. Run the search in [docs/publish.md](docs/publish.md) on your diff.
- **Do not change what you do not own.** Never kill a process by name; stop Systems through KLIF. A port held by a
  foreign process is reported, not cleared.
- **Remote nodes:** `--node <id>` on `presets set|save|delete` or `systems add` runs arbitrary commands on another
  machine when that node grants `edit`. Do it only on explicit instruction. Do not enable `[node] listen` or
  `allow = ["edit"]` on your own initiative; read [docs/nodes.md](docs/nodes.md) first.
- **The klif-webui pairing code and address are a credential.** `klif-cli webui pair` prints a one-time code and an
  address with a secret in it; whoever has them can pair a device that then launches and stops Systems. Never run it
  unless the user asked for a pairing, show its output only to the user, and never paste it into a file, a log, an
  issue or a pull request. Remove a paired device (`webui forget`) only when the user asks.
- **Do not turn klif-webui on, or point its `host` at a LAN address, on your own initiative.** It is off by default,
  speaks plain HTTP and opens a port to the network; the user decides (read [docs/webui.md](docs/webui.md)). On a
  scratch configuration for an experiment, keep it off or on `127.0.0.1`.
- **Keep servers on loopback** unless the user wants LAN access, and then keep the API key on. KLIF refuses to
  launch llama.cpp or vLLM on a non-loopback host without one (unless `[security] api_key = "none"`).
- **Never run `git commit` or `git push` unless the user asked for that commit.**
