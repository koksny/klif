# klif-cli

`klif-cli.exe` is KLIF without the window: the same engine, from a terminal, a script or a coding agent. It reads
the same `klif.toml`, edits it through the same comment-preserving code, and shows the same commands the window
shows. This page is checked against `klif-cli --help` and the code of version 0.3.0.

```
klif-cli [--json] <command> ...
```

## How it connects

- If `klif.exe` (or `klif-cli serve`) is running for the same configuration, `klif-cli` connects to its engine over
  the local control channel (`127.0.0.1`, token from `control.json`, challenge and HMAC on every request).
- Otherwise it starts an engine inside the process, holding `engine.lock`, waits up to 3 seconds for the first
  health results, runs the command and exits. Model servers it started **keep running** after it exits; the next
  engine adopts them. For many commands in a row, run `klif-cli serve` (or keep the window open): every command
  then answers at once.
- Only one engine runs per state folder. `klif-cli serve` refuses to start when one already runs (`engine_busy`).
- Configuration comes from `KLIF_CONFIG`, else `.local\klif.toml` above the exe or the working folder, else
  `%APPDATA%\KLIF\klif.toml`. **Set `KLIF_CONFIG` when you experiment**: even `status` starts an engine, which
  creates `engine.lock`, `state.v3.json` and other files in the state folder.
- `KLIF_LOG=info|debug` (also `trace`, `error`, `off`) prints engine log lines on stderr. Warnings are on by
  default. Bench and download progress always go to stderr. `trace` covers KLIF's own records only: dependencies
  are capped at debug, so request bytes and `Authorization` headers are never printed (the window's log follows
  the same rule).

## Output contract

- **Text** by default: tables and sentences on stdout, errors on stderr as `klif-cli: <sentence>`.
- **`--json`** (anywhere on the line) prints **exactly one JSON document** on stdout and nothing else (progress and
  notes stay on stderr). Every document has `"schemaVersion": 1`. `schemaVersion` changes only for a breaking
  change; new keys may appear in a version, so ignore keys you do not know. Keys are printed in alphabetical order.
- **Errors** under `--json`:

  ```json
  { "schemaVersion": 1, "error": { "code": "not_found", "message": "There is no System \"nope\". Systems: s1 (System 1), s2 (System 2)." } }
  ```

  `message` is one sentence written for a person.
- **Exit codes:** `0` success; `1` the command failed (any error code but the two below); `2` a usage mistake or a
  missing `--yes` (`usage`, `needs_yes`).
- **Error codes:** `usage`, `needs_yes`, `not_found`, `ambiguous` (an argument names several Systems), `refused`
  (the engine said no, and says why), `engine_busy`, `engine` (could not start), `control` (the running engine did
  not answer), `fault` (a launched System faulted; the message has the log tail), `stopped` (it stopped before it was
  ready), `timeout`, `unsupported`, `invalid` (a file or preset does not parse), `io`, `bench`, `download`,
  `cancelled`, `error`.
- **Nothing happens without `--yes`** when it starts, stops, restarts, removes, deletes, downloads or replaces a
  token: `launch`, `stop`, `restart`, `systems remove`, `presets delete`, `models download`, `key clear`, `node
  token --create` when a token exists, and `bench` when it has to launch the System.

## System arguments

Wherever a command takes `<system>`:

1. an id: `s1`, `cgi`, or for another machine `render-box/s1`;
2. a local label, ignoring case and spaces: `system1`, `"System CGI"`;
3. the 0.2 names `low`, `medium`, `high`, `krea` for the local `s1`, `s2`, `s3`, `cgi`;
4. a label that is unique across the remote nodes, or `<node>/<label>`.

If none matches, the error lists the Systems; if several do, it is `ambiguous` and lists their ids. Use ids in
scripts.

## Commands

The program's own help:

```
klif-cli - KLIF (Koksny.com LOCAL INFERENCE FORNICATOR) from a terminal or a coding agent

usage: klif-cli [--json] <command> ...

  status [<system>]                       Systems, GPUs and nodes (one System with <system>)
  diag                                    diagnostics: config location, engine, GPUs, versions (no secrets)
  plan <system>                           the exact command a System would launch now (secrets masked)
  select <system>                         select a tab
  launch <system> --yes [--stop-others] [--wait]
  stop <system> --yes | stop --all --yes
  restart <system> --yes [--wait]
  dismiss <system>                        leave a fault (back to offline)

  systems list
  systems add --kind llm|image|tts|stt|video [--class fast|deep|max] [--label L] [--id ID] [--preset P] [--node N]
  systems remove <system> --yes
  systems rename <system> <label>
  systems move <system> <index>           0-based tab index among the local Systems
  systems exclusive <system> on|off

  presets list [--node N]
  presets show <id> [--node N]
  presets use <system> <id>
  presets param <system> <name> <value>
  presets save <id> --file F.toml [--use <system>] [--node N]
  presets set <id> key=value... [--node N]
  presets delete <id> --yes [--node N]

  bench <system> [--runs N] [--prompt N] [--gen N] [--keep-running] [--allow-shared] [--yes]
  bench list [--preset ID]

  models list [--kind K]
  models download <rec-id> --yes
  models adopt <rec-id> [--system S]

  key status | key set (reads one line from stdin) | key clear --yes
  node status | node token [--create [--yes]]
  nodes list
  serve                                   run the engine headless (+ the network listener when [node] listen is set)

<system>: an id (s1, render-box/s1), a label ignoring case and spaces (system1, "System 1"),
          or the 0.2 names low|medium|high|krea (local s1/s2/s3/cgi).
presets set keys: command  args=["json","array"]  args+=TOKEN  args-=TOKEN  env.NAME=VALUE  env.NAME-
          env_remove+=NAME  env_remove-=NAME  cwd  port  host  endpoint  health  model  mmproj  ctx  gpu  kind
          adapter  name  managed  api_key  model_name  quant  backend  device  notes   (empty value = unset)
config:   KLIF_CONFIG=<klif.toml>, else .local\klif.toml above the exe or the current folder, else %APPDATA%\KLIF\klif.toml.
logs:     KLIF_LOG=info|debug (stderr).
```

Also: `klif-cli --version` (`klif-cli 0.3.0`; JSON `{"version": "0.3.0"}`) and `klif-cli --help`.

### Looking

**`status [<system>]`** is what to poll. It prints the Systems in tab order (`*` marks the selected one), the GPUs
KLIF measures and the nodes:

```
KLIF 0.3.0 · engine: in-process (this klif-cli holds the engine)

   ID        LABEL       KIND   STATUS   PRESET       MODEL                       ENDPOINT                    TOK/S  VRAM
*  s1        System 1    llm    offline  fast-8b      Fast 8B                     http://127.0.0.1:7030/v1    -      -
   cgi       System CGI  image  offline  image-turbo  Image turbo                 http://127.0.0.1:1234       -      -
   lan-chat  Chat box    llm    offline  chat-box     Some model on that machine  http://192.0.2.20:11434/v1  -      -
  lan-chat: Not answering at http://192.0.2.20:11434/v1.
```

JSON (`StatusJson`):

```json
{
  "schemaVersion": 1,
  "selected": "s1",
  "systems": [
    {
      "id": "s1", "label": "System 1", "kind": "llm", "class": "fast", "status": "online",
      "preset": "fast-8b", "baseUrl": "http://127.0.0.1:7030/v1", "model": "Fast 8B",
      "decodeTps": 62.5, "vramGiB": 9.4
    }
  ],
  "gpus": [ { "id": "1002:7550", "name": "RX 9070 XT", "usedGiB": 11.2, "totalGiB": 15.9 } ],
  "nodes": [ { "id": "render-box", "state": "online", "latencyMs": 1.2 } ]
}
```

| Field | Notes |
| --- | --- |
| `selected` | The selected tab's id; absent with no Systems |
| `systems[].id`, `label`, `kind`, `status` | Always present. `kind`: `llm image tts stt video`. `status`: `not-set invalid offline starting online busy stopping fault unreachable` |
| `class`, `node`, `reason`, `preset`, `baseUrl`, `model`, `decodeTps`, `vramGiB`, `fault` | Present when they apply. `node` and an id like `render-box/s1` mark a System on another machine. `reason` explains `invalid`, `offline` (external), `unreachable`. `fault` is the fault title. `baseUrl` is what clients use; for an LLM it ends in `/v1`. `decodeTps` is the current decode speed, `vramGiB` the session's VRAM |
| `gpus[]` | `id` (`VEN:DEV`, `VEN:DEV#1`), `name`, `usedGiB`, `totalGiB` |
| `nodes[].state` | `connecting online offline unauthorized incompatible` |

`status <system>` returns that one entry (the same keys, flat, plus `schemaVersion`).

**`plan <system>`** is the most useful command before a launch and after every edit: the exact program, arguments,
working folder and environment, the effective address, the hash, and the issues (errors block Launch). Secrets are
masked. An external System shows its endpoint instead. A `{stamp}` in the preset is shown as the text `{stamp}`;
it gets its value when the System launches.

`plan` also answers for a System that cannot launch yet (the program or the model file is missing, the preset has
errors, the port is held by another program). It still prints the command KLIF resolved, with its issues
(`error: [field] text` and `warn: ...` lines under the command), and ends with the verdict: `launchable: yes`, or
`launchable: NO (<the first reason>)`. Under `--json` the document has `"launchable": true|false` and, when it is
false, `"refused"` with the same reason. An external server is `launchable: NO (an external server; KLIF only
watches it)`. The exit code is 0 either way, because the command is what you came to see. `plan` fails only when
there is no command to show: no preset selected, or a preset that does not exist or cannot be read. Check
`launchable` before you rely on the output; Launch is refused until the reason is gone.

```
System 1 (s1) · preset fast-8b

program  D:\llama.cpp\llama-server.exe
args     -m D:\models\fast-8b-Q4_K_M.gguf -c 16384 -ngl 99 --jinja --host 127.0.0.1 --port 7030 --reasoning on
cwd      D:\llama.cpp
adapter  llama.cpp
listen   127.0.0.1:7030 (health: adapter default)
env      HIP_VISIBLE_DEVICES=0
         PATH=D:\ROCm\bin;%PATH%
         LLAMA_ARG_LOG_VERBOSITY=4 (set by KLIF)
         LLAMA_ARG_LOG_PREFIX=1 (set by KLIF)
         LLAMA_ARG_LOG_TIMESTAMPS=1 (set by KLIF)
         LLAMA_API_KEY (removed by KLIF)
         VLLM_API_KEY (removed by KLIF)
hash     0cdabea1ae42b4d7

"D:\llama.cpp\llama-server.exe" -m D:\models\fast-8b-Q4_K_M.gguf -c 16384 -ngl 99 --jinja --host 127.0.0.1 --port 7030 --reasoning on

launchable: yes
```

JSON: `{ "system", "label", "preset", "launchable", "refused"?, "command": { "program", "args": [..], "cwd", "display",
"adapter", "host", "port", "health": {"type": "auto"|"http"|"tcp", "path"?}, "env": [ { "name", "value"?, "managed",
"overridden", "removed", "secret" } ], "hash", "issues": [ { "level": "error"|"warn", "field"?, "text" } ],
"external"? } }`. `display` is the one-line command string. (The paths above were fictionalised for this page;
everything else is real output.)

**`diag`** prints a document for bug reports: `cli` (version, exe, `engine` = `in-process` or `connected`, config
location and why it was chosen, config issues, the RAM type read from firmware, which only `klif-cli` can do) and
`engine` (state and data folders, the control port, GPUs with their ids, dedicated VRAM and power state, ports each
System expects and who holds them, sessions, issues). It contains no secrets. Use it to find the GPU ids for
`[gpu]`: `engine.gpus[].id`.

**`systems list`** shows every System with its class, status, preset, GPU and flags (`exclusive`, `external`,
`read-only`, `conflicts`); under `--json` the full objects (`systems[]` with `availability`, `params`, `command`,
`conflicts`, `endpoint`, `session`, `controllable`, `editable`, ...).

**`nodes list`** shows `[nodes.<id>]` with state, latency, version, the rights that node grants and the reason when
it is not online. **`models list [--kind K]`** lists recommendations and prints the disclaimer; **`bench list
[--preset ID]`** lists bench records; **`presets list`** and **`presets show <id>`** show presets (show prints the
preset as a TOML block, secrets masked, and its command); **`key status`** and **`node status`** report the API key
and this machine's node settings without revealing them.

### Acting

- **`select <system>`** selects a tab (it never stops anything).
- **`launch <system> --yes [--stop-others] [--wait]`** starts the System. With conflicts it is refused and names
  them; `--stop-others` stops them first (one at a time, waiting for each), as the "Stop X & launch" button does.
  `--wait` blocks until the server is ready (up to 15 minutes), failing with `fault` (and the log tail) or
  `stopped` if it never gets there. Without `--wait` it returns once the session exists.
- **`stop <system> --yes`**, **`stop --all --yes`** (every local session, never an external System),
  **`restart <system> --yes [--wait]`**, **`dismiss <system>`** (leave a fault). `restart` is refused before
  anything stops when the relaunch would conflict ("System 1 cannot restart while System 2 is running (exclusive
  GPU). Stop System 2 first."); the running session stays as it is.
- **`systems add --kind K [--class C] [--label L] [--id ID] [--preset P] [--node N]`** (without `--label` the default
  label of that kind and class, with ` (2)`, ` (3)` added when it is taken: "System 1 (2)"; see
  [systems.md](systems.md#kinds)), **`remove`** (refused while it runs), **`rename <system> <label>`**,
  **`move <system> <index>`** (0-based among local Systems), **`exclusive <system> on|off`**.
- **`presets use <system> <id>`** and **`presets param <system> <name> <value>`** choose a preset and a param
  choice; both apply on the next launch. `presets use` is refused (nothing is written) for a preset that does not
  exist or has another kind than the System, and it removes the System's param selections that the new preset
  cannot honour (no such param, or a value that is not one of its choices). Selections it can honour stay.
- **`presets set <id> key=value...`** edits fields of a preset (it creates the preset when it does not exist):
  `ctx=32768`, `port=7040`, `host=127.0.0.1`, `model=...`, `gpu=cpu`, `endpoint=...`, `managed=false`; `args=["-m",
  "{model}"]` replaces the list, `args+=--no-mmap` appends one token, `args-=--no-mmap` removes one;
  `env.NAME=VALUE` sets, `env.NAME-` removes; `env_remove+=NAME` and `env_remove-=NAME`. An empty value unsets a
  field. `params` and `recommended` cannot be set this way. Stored secrets stay as they are unless you set them.
  The write is refused ("changed on disk") when the preset changed between reading and writing it (Tune or another
  agent saved it meanwhile); run the command again.
- Changing a preset's `kind` (or `adapter`, which can imply it) is refused while another System uses the preset as
  a different kind: "Preset "fl1" would become an Image preset, but System 1 is using it as an LLM System; pick
  another preset there first."
- **`presets save <id> --file F.toml [--use <system>]`** replaces a preset from a file: either a `[presets.<id>]`
  table or the preset's keys at the top level (params included). Prefer this to `set` for lists and anything with
  quotes: Windows PowerShell 5.1 mangles double quotes inside arguments to native programs.
- **`presets delete <id> --yes`** is refused while a System uses the preset.
- **`--node N`** on `presets list|show|save|set|delete` and `systems add` aims the command at a remote node
  (`[nodes.<N>]`); it works only if that node grants the right (`edit` for changes). That is arbitrary command
  execution on the other machine, see [nodes.md](nodes.md).
- **`key set`** reads one line from stdin (never echoed, never printed); **`key clear --yes`**.
- **`node token`** says whether a token exists; **`node token --create`** makes one and **prints it once** (add
  `--yes` to replace an existing one: every machine using the old token needs the new one).
- **`serve`** runs the engine headless until you stop the process, and serves the network listener when `[node]
  listen` is set. Model servers keep running when it ends.

### Bench

`bench <system> [--runs N] [--prompt N] [--gen N] [--keep-running] [--allow-shared] [--yes]`

Launches the System if it is not running (needs `--yes`; an already-running System or an online external one is
benched without), waits until it is ready, then does `--runs` measured requests (default 3) and stops what it
started unless `--keep-running`.

- **LLM:** streaming `POST /v1/chat/completions` with a prompt of about `--prompt` tokens (default 512; calibrated
  with the server's tokenizer on llama.cpp) and `--gen` generated tokens (default 128), each request with a unique
  prefix and, for llama.cpp, without prompt caching and with the end-of-text token ignored, after waiting for idle
  slots. KLIF records time to first token, prefill and decode speed (the server's own timings when it reports
  them), plus load time, peak VRAM and spill.
- **Image:** `POST /v1/images/generations` on an `sd.cpp` or `openai` server; the image uses the server's launch
  defaults. Seconds per image.
- `tts`, `stt` and `video` print "not supported yet". A remote System must be benched on its own machine.
- It refuses while another local System runs on the same GPU (an unknown GPU counts as the same), because the
  numbers would be shared-GPU numbers. `--allow-shared` overrides that.
- Records are appended to `<data folder>\bench\<preset id>.json` (50 kept). A record carries the command hash; a
  later change to the command marks older results `stale` in `presets list` and the Tune drawer.

The JSON result is `{ "file", "summary", "record" }`; `record` has `at`, `presetId`, `presetHash`, `system`, `kind`,
`adapter`, `model`, `hardware` (`gpu`, `vramGiB`, `driver`), `backendBuild`, `promptTokens`, `genTokens`, `loadS`,
`peakVramGiB`, `spillMiB`, `layers` and `runs[]` (`ttftS`, `prefillTps`, `decodeTps`, `secondsPerImage`,
`promptTokens`, `genTokens`).

### Models

`models list` prints the recommendations (id, kind, class, name, quant, size, license, measured speed, state) and
the disclaimer. `models download <id> --yes` follows the download until every file is done, failed or cancelled
(without `--yes` it names every Hugging Face repo the files come from, e.g. "from huggingface.co/a/b and
huggingface.co/c/d"); it
needs `[paths] models_dir`, runs in the engine (so keep a short-lived `klif-cli` open, or use `serve` or the window)
and an interrupted download resumes next time. `models adopt <id> [--system S]` turns a recommendation into a preset
and, with `--system`, selects it. Rules and limits: [presets.md](presets.md#recommendations-and-downloads).

## Recipes for agents

Wait until a System is ready, with the status in JSON (PowerShell):

```powershell
klif-cli launch s1 --yes --wait          # blocks; exit 1 with code fault/stopped/timeout on failure
$s = klif-cli --json status s1 | ConvertFrom-Json
$s.status; $s.baseUrl                    # online, http://127.0.0.1:7030/v1
```

Change one thing and compare:

```powershell
klif-cli plan s1                                   # note the hash
klif-cli bench s1 --yes                            # baseline
klif-cli presets set fast-8b ctx=32768             # one change
klif-cli plan s1                                   # new hash, new issues?
klif-cli restart s1 --yes --wait
klif-cli bench s1 --yes                            # compare with `klif-cli bench list --preset fast-8b`
```

Add a System with its preset from a file:

```powershell
klif-cli presets save stt-local --file .\stt-local.toml
klif-cli systems add --kind stt --label "System STT" --preset stt-local
klif-cli plan stt
```

Never print `api-key.txt`, `node-token.txt`, `control.json` or the output of `node token --create` where others can
read it. See [AGENTS.md](../AGENTS.md).
