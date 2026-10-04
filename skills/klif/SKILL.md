---
name: klif
description: Drive KLIF, a Windows manager for local AI model servers (llama.cpp, stable-diffusion.cpp, vLLM, speech and video servers), through its klif-cli command line. Use it to start, stop, restart, tune, benchmark and monitor those servers, to pick and download models that fit the machine, to read server logs and the best speeds KLIF has recorded, and to write or edit presets and Systems in klif.toml. Use it whenever a task mentions KLIF, klif-cli, klif.toml, Systems or presets, or local model servers on a machine where KLIF is installed.
---

# Driving KLIF with klif-cli

KLIF runs model servers as **Systems** (tabs). A System has a kind (`llm`, `image`, `tts`, `stt`, `video`, `music`) and uses a
**preset**: the exact command line, environment and port of one server, stored in `klif.toml`. `klif-cli` is KLIF
without the window: same engine, same configuration, no MCP. Everything below is `klif-cli`; the contract is in
`AGENTS.md` and `docs/cli.md` of the KLIF repository. Prefer the self-description to guessing:

```powershell
klif-cli --json help                 # every command: arguments, flags, needsYes, mayStartEngine, outputs
klif-cli help records history        # one command in text
klif-cli schema                      # the documents; klif-cli schema <name> prints a JSON Schema
```

## Before the first command

1. Find `klif-cli.exe` (next to `klif.exe`, or on `PATH`); `klif-cli --version` proves it runs.
2. **Experiments use a scratch configuration.** Copy `config\klif.example.toml` to a scratch folder and set
   `KLIF_CONFIG` to that copy. Without it `klif-cli` finds the user's live `klif.toml` and, if KLIF is open, its
   running engine. Work on the live configuration or its servers only when the user asked for exactly that, and
   never start or stop the user's model servers otherwise.
3. Without `KLIF_CONFIG` pointing at a running KLIF, the first command starts an engine inside `klif-cli`
   (`engine.lock`, `state.v3.json` appear in the state folder). Model servers it launched keep running after it exits;
   the next engine adopts them. For many commands in a row, run `klif-cli serve` in another window.

## Output, errors, `--yes`

- `--json` (anywhere on the line) prints **one JSON document** on stdout with `"schemaVersion": 1`; progress and
  notes go to stderr. Keys are in alphabetical order; ignore keys you do not know.
- A failure is `{"schemaVersion": 1, "error": {"code", "message"}}`, exit code 1; usage mistakes and a missing `--yes`
  exit 2. Codes: `usage needs_yes not_found ambiguous refused engine_busy engine control fault stopped timeout
  unsupported invalid io bench download cancelled error`. `message` is one sentence for the user; show it.
- `watch` and `logs --follow` print JSON **lines** instead (`schemaVersion`, `type`, `at`); `models download --json`
  prints its progress as JSON lines on stderr.
- **`--yes` is the user's decision.** It is required to launch, stop, restart, remove a System, delete a preset,
  download a model, clear the key, forget a record, replace a node token, and for `bench` when it must launch the
  System. Never add it to make an error go away; ask the user, then add it.
- `<system>` is an id (`s1`, `render-box/s1`), a label ignoring case and spaces (`system1`) or `low|medium|high|krea`
  (`s1|s2|s3|cgi`). Use ids in scripts.

## Looking

```powershell
klif-cli --json status               # Systems, GPUs, nodes: status, baseUrl, decodeTps, vramGiB
klif-cli --json status s1            # one System
klif-cli --json plan s1              # the exact command; read "launchable" and "issues" before acting
klif-cli --json presets list         # presets with availability and last bench
klif-cli --json hardware             # GPUs, CPU, RAM, peak FP32 TFLOPS, the VRAM pool (starts no engine)
klif-cli --json diag                 # config location, GPU ids, ports and who holds them (no secrets)
```

`status` is one of `not-set invalid offline starting online busy stopping fault unreachable`. `baseUrl` is what clients
use (an LLM's ends in `/v1`). `plan` exits 0 even when the System cannot launch; `launchable: false` carries
`refused`. The program path, model path or port named there is what to fix (`presets set`).

## Running a System

```powershell
klif-cli launch s2 --yes --wait                          # blocks until online; exit 1: fault / stopped / timeout
klif-cli launch s2 --yes --stop-others --wait            # also stops the Systems that conflict (port, GPU, VRAM)
klif-cli watch --until s2=online --timeout 900           # wait for a status without polling
klif-cli stop s2 --yes                                   # or: stop --all --yes
klif-cli restart s2 --yes --wait                         # after a preset change
klif-cli dismiss s2                                      # leave a fault (back to offline)
```

- A launch with conflicts is refused and names them. Stopping another System is the user's call (`--stop-others`).
- **Never sleep and poll.** `launch --wait` for one System; `watch --until <system>=<status> --timeout N` for any
  status (it ends with `fault` or `stopped` when a System that should come online does not; `online` also accepts
  `busy`); plain `watch --types status,fault,record,download` streams events as JSON lines. With no KLIF window or
  `klif-cli serve` running, a fault that happened before `watch` started is gone (a faulted session is not kept):
  use `launch --wait`, or keep an engine up.
- A `fault` error carries the log tail. For more: `klif-cli --json logs s2 --tail 100` (the engine keeps the last 200
  console lines per System, secrets redacted; `--follow` keeps reading until the System stops).
- External Systems (a preset with `endpoint`) are only watched: launch and stop are refused.

## Presets and params

`plan` after every edit; change **one** thing at a time.

```powershell
klif-cli presets show fast-8b                            # TOML, secrets masked, plus the command
klif-cli presets set fast-8b ctx=32768                   # key=value; empty value unsets
klif-cli presets set fast-8b args+=--no-mmap             # append / remove one token: args-=TOKEN
klif-cli presets set fast-8b env.HIP_VISIBLE_DEVICES=0   # env.NAME=VALUE, env.NAME- removes
klif-cli presets save my-tts --file .\my-tts.toml        # whole preset from a file (lists, quotes)
klif-cli presets use s1 my-tts                           # a System uses a preset (next launch)
klif-cli presets param s1 reasoning off                  # a param choice (next launch)
klif-cli systems add --kind stt --label "System STT" --preset stt-local
```

- Keys: `command args cwd env env_remove port host endpoint health model mmproj ctx gpu kind adapter name managed
  api_key model_name quant backend device notes`. `params` and `recommended` need `presets save`.
- Placeholders in `command`, `args`, `cwd`, `env`: `{model} {mmproj} {ctx} {host} {port} {models_dir} {state_dir}
  {data_dir} {stamp} {env:NAME} {p.NAME} {p.NAME.VAR}`. Pass `--host {host} --port {port}` through (`--listen-ip` /
  `--listen-port` for sd.cpp) or the server listens where KLIF is not looking. `command` is an `.exe` (absolute or on
  `PATH`), never a `.bat` or `.ps1`. `gpu` is for display and fit only: the command selects the device itself.
- PowerShell 5.1 mangles double quotes in native arguments: put lists and anything with quotes in a file and use
  `presets save --file`.
- No secrets in `args`: use `api_key = true` or an `env` variable. Stored secrets show as `••••`; sending that
  back keeps the stored value.
- `--node N` on `presets` / `systems add` edits another machine and runs arbitrary commands there. Only on explicit
  instruction.

## Models: suggest, download, adopt

```powershell
klif-cli --json suggest                                  # per slot (System 1/2/3, image...): model, quant, ctx, KV, estimated VRAM / RAM
klif-cli models list --kind llm                          # the embedded pool, installed or not
klif-cli models download <rec-id> --yes                  # huggingface.co only; needs [paths] models_dir
klif-cli models adopt <rec-id> --system s2               # makes the preset; ctx / KV from the suggestion
klif-cli models adopt <rec-id> --ctx 65536 --kv q8_0     # or set them
klif-cli plan s2 ; klif-cli launch s2 --yes --wait ; klif-cli bench s2 --yes
```

- Every number in `suggest` is an estimate from weights, KV cache and overhead against the tier's budget; bench before
  relying on it. System 1 keeps a third of the VRAM and a quarter of the RAM free, System 2 uses the VRAM pool, System
  3 may spill into RAM.
- **Downloads are the user's call:** tell them the model, the source (the repository `models download` names without
  `--yes`) and the size, then wait for a yes. Models are tens of gigabytes. With `--json` the progress is JSON lines on
  stderr (`start`, `progress`, `note`); an interrupted download resumes.
- Never put `-hf`, `--hf-repo`, `-mu`, `--model-url`, `--hf-token`, `--api-key` or similar flags into a preset.

## Bench, records, calibrate

```powershell
klif-cli bench s1 --yes                                  # 3 runs; launches the System if needed and stops it again
klif-cli bench s1 --runs 5 --prompt 2048 --gen 256 --yes
klif-cli --json bench list --preset fast-8b              # history; a result is "stale" after the command changed
klif-cli --json records --metric decodeTps               # best values per model file and backend, with conditions
klif-cli --json records history <key> --metric decode    # how a record climbed
```

1. Know what runs now (`status`, `plan`). Nothing else may share the GPU: bench refuses (`--allow-shared` gives
   shared-GPU numbers).
2. Bench, change **one** thing, `plan`, `restart --wait`, bench again, compare. Keep it if it is better and the output
   is still right; otherwise revert.
3. Calibrate for the user's hardware and workload, not for a benchmark number. Write what changed and why into the
   preset's `notes` (`presets set <id> notes="..."`).
4. Records are the best values KLIF ever saw per exact model file (SHA-256) and backend (HIP, Vulkan, CUDA, CPU...):
   `decodeTps`, `prefillTps`, `ttftS`, `imageS`, `ttsRtf`, `sttRtf`, `videoS`, `musicRtf`, each with its context, prompt size, KV
   type and GPUs. They come from everyday use and from `bench`. `records forget <key> --yes` removes a junk entry.
   Do not edit `records.json`, `records-history.jsonl` or `hashes.json` by hand.
5. Bench covers llm, image, tts, stt (stt needs `--audio file.wav`) and music (audio.cpp servers only; a run lasts as
   long as the song takes, so try `--runs 1`); a System on another node is benched on that machine.

## Safety

- **Secrets:** never print, log, commit or paste the API key (`api-key.txt`), node tokens (`node-token.txt`,
  `*.token`), `control.json`, Hugging Face tokens, or any env value whose name holds KEY, TOKEN, SECRET, PASS or
  AUTH. Do not `cat` those files. `klif-cli key set` reads the key from stdin; `node token --create` prints a token
  once: have the user run it, or redirect it to a file without echoing it.
- **Do not change what is not yours.** Never kill a process by name; stop Systems through KLIF. A port held by a
  foreign process is reported, not cleared.
- **Keep servers on loopback** unless the user wants LAN access, and then keep the API key on. Do not enable
  `[node] listen` or `allow = ["edit"]` on your own initiative (`docs/nodes.md`).
- **No personal data in anything that is committed:** fictional paths (`D:\models`), documentation addresses
  (`192.0.2.x`), no hostnames, user names or machine names in code, docs, bench output or screenshots.
- **Never run `git commit` or `git push`** unless the user asked for that commit. A pull request is for a bug fix or
  something that helps other users (a preset, a recommendation entry, a platform port), never a configuration file;
  read `CONTRIBUTING.md` first.

## Quick recipes

```powershell
# bring up a System and use it
klif-cli launch s1 --yes --wait ; (klif-cli --json status s1 | ConvertFrom-Json).baseUrl

# a launch failed
klif-cli --json logs s1 --tail 80 ; klif-cli plan s1 ; klif-cli dismiss s1

# do other work while a model loads (an engine must stay up: the KLIF window, or klif-cli serve)
klif-cli launch s2 --yes ; klif-cli watch --until s2=online --timeout 900
```
