# klif-cli

`klif-cli.exe` is KLIF without the window: the same engine, from a terminal, a script or a coding agent. It reads
the same `klif.toml`, edits it through the same comment-preserving code, and shows the same commands the window
shows. This page is checked against `klif-cli --help` and the code of version 0.3.1.

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
- **Streams.** A command that runs until something happens prints one compact JSON object per line instead:
  `watch` and `logs --follow` on stdout, the progress of `models download` on stderr. Every line has `schemaVersion`,
  `type` and `at` (epoch seconds); a failure ends the stream with the error document on one line. Other stderr lines
  (engine warnings, `KLIF_LOG`) are plain text: skip lines that do not start with `{`.
- **Self-description.** `klif-cli --json help` lists every command with its arguments, flags, whether it needs
  `--yes`, whether it may start an engine and which documents it prints; `klif-cli schema <name>` prints the JSON
  Schema of a document. See [Help and schemas](#help-and-schemas).
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
  token --create` when a token exists, `records forget`, and `bench` when it has to launch the System.

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

  status [<system>]                        Systems, GPUs and nodes (one System with <system>)
  diag                                     Diagnostics: config location, engine, GPUs, versions (no secrets)
  hardware [--node N]                      GPUs, CPU and RAM with their peak FP32 TFLOPS (reads this machine directly)
  plan <system>                            The exact command a System would launch now (secrets masked)
  select <system>                          Select a tab
  launch <system> --yes [--stop-others] [--wait]
                                           Start a model server
  stop <system> --yes | stop --all --yes   Stop a model server, or every running System
  restart <system> --yes [--wait]          Stop and start a System again with its preset
  dismiss <system>                         Leave a fault (back to offline)
  logs <system> [--tail N] [--follow]      The System's server log lines (the engine's console, the last 200)
  watch [--system S] [--types LIST] [--until SYSTEM=STATUS] [--timeout SECONDS]
                                           Events as they happen, one line each; use it instead of sleep-and-poll loops

  systems list                             Every System with its status, preset and flags
  systems add --kind llm|image|tts|stt|video|music [--class fast|deep|max] [--label L] [--id ID] [--preset P] [--node N]
                                           Add a System (a tab)
  systems remove <system> --yes            Remove [systems.<id>] from klif.toml
  systems rename <system> <label>          Change a System's tab label
  systems move <system> <index>            Move a System to a tab position (0-based, among the local Systems)
  systems exclusive <system> on|off        Whether the System needs the whole GPU

  presets list [--node N]                  Presets with their availability and last bench
  presets show <id> [--node N]             One preset in full with the command it builds (secrets masked)
  presets use <system> <id>                Make a System use a preset (applies on the next launch)
  presets param <system> <name> <value>    Choose a param value of the System's preset (applies on the next launch)
  presets save <id> --file F.toml [--use <system>] [--node N]
                                           Create or replace a preset from a TOML file
  presets set <id> key=value... [--node N] Change keys of a preset (see the keys below); creates it when missing
  presets delete <id> --yes [--node N]     Remove [presets.<id>] from klif.toml

  bench <system> [--runs N] [--prompt N] [--gen N] [--audio F.wav] [--keep-running] [--allow-shared] [--yes]
                                           Measure a System with fixed requests (launches it if needed, stops it afterwards)
  bench list [--preset ID]                 Stored bench results

  records [--kind K] [--metric M] [--backend B] [--node N]
                                           Best values per model file and backend (N: a node, or local)
  records forget <key> --yes               Remove a junk record entry (a unique part of its key is enough)
  records history <key> [--metric M]       How a record climbed: every broken record of an entry, oldest first

  suggest [--kind K] [--class C]           The model the pool suggests per slot for this machine (estimates)
  models list [--kind K]                   The recommended models (embedded list) and whether they are installed
  models download <rec-id> --yes           Download a recommendation's files from huggingface.co into [paths] models_dir
  models adopt <rec-id> [--system S] [--ctx N] [--kv TYPE]
                                           Make a preset from a downloaded model (ctx / KV: this machine's suggestion)

  settings [record-moment on|off]          This machine's display settings ([ui] in klif.toml): show, or set one
  key status                               Whether an API key is set (never printed)
  key set                                  Store the API key (reads one line from stdin)
  key clear --yes                          Remove the stored API key
  node status                              This machine as a node: listener, rights, token
  node token [--create [--yes]]            Whether a node token exists; --create makes one and prints it once
  nodes                                    The remote nodes and their state
  serve                                    Run the engine headless (+ the network listener when [node] listen is set)

  help [<command>]                         This list; with --json the machine-readable catalog of every command
  schema [<name>]                          JSON Schema of every document klif-cli prints (a name or a command)
  version                                  The klif-cli version (also --version)

<system>: an id (s1, render-box/s1), a label ignoring case and spaces (system1, "System 1"),
          or the 0.2 names low|medium|high|krea (local s1/s2/s3/cgi).
presets set keys: command  args=["json","array"]  args+=TOKEN  args-=TOKEN  env.NAME=VALUE  env.NAME-
          env_remove+=NAME  env_remove-=NAME  cwd  port  host  endpoint  health  model  mmproj  ctx  gpu  kind
          adapter  name  managed  api_key  model_name  quant  backend  device  notes   (empty value = unset)
config:   KLIF_CONFIG=<klif.toml>, else .local\klif.toml above the exe or the current folder, else %APPDATA%\KLIF\klif.toml.
logs:     KLIF_LOG=info|debug (stderr; trace = KLIF's own records only, dependencies capped at debug).
agents:   klif-cli --json help (every command), klif-cli schema (JSON Schema of every document), klif-cli watch (events
          instead of sleep-and-poll loops). Commands that change something need --yes.
```

Also: `klif-cli --version` (`klif-cli 0.3.1`; JSON `{"version": "0.3.1"}`) and `klif-cli --help`.

### Looking

**`status [<system>]`** is what to poll. It prints the Systems in tab order (`*` marks the selected one), the GPUs
KLIF measures and the nodes:

```
KLIF 0.3.1 · engine: in-process (this klif-cli holds the engine)

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

**`hardware [--node N]`** lists the machine's GPUs and CPU with their theoretical peak FP32 throughput, and the
totals the [suggestions](#suggestions) and the Records screen use: the VRAM pool, the largest single GPU and the RAM.
It reads this machine directly (the adapters, the CPU, the RAM: no engine is started and no GPU is woken); with
`--node N` it prints that node's inventory, which the engine asks of the node. Every discrete GPU counts, whatever the
vendor; an integrated GPU counts only on a machine that has no discrete one (an APU with unified memory then counts
as the pool). `[hardware]` in klif.toml corrects it: `exclude` and `include` (ids `VEN:DEV`, `VEN:DEV#1`, `cpu`) and
`tflops` for a card the built-in table lacks. FP32 numbers are the vendor's published peak for a GPU and cores x FLOP
per cycle x base clock for the CPU (marked `estimate`); an unknown card shows `?` and is counted in `tflopsUnknown`.

```
ID         DEVICE            MEMORY           FP32 TFLOPS     COUNTED  DETAIL
1002:7550  RX 9070 XT        15.87 GiB GDDR6  48.7            yes      64 CU, 2970 MHz
cpu        Example CPU 16C   -                4.4 (estimate)  yes      16 cores, AVX-512, 4.3 GHz

Total   53.1 TFLOPS FP32 peak
Memory  15.87 GiB VRAM pool (largest GPU 15.87 GiB), 64 GiB RAM
```

JSON: `{ "gpus": [ComputeDevice], "cpu": ComputeDevice, "vramPoolGiB", "largestGpuGiB", "ramTotalGiB", "unified",
"tflopsFp32", "tflopsUnknown", "node"? }`; a `ComputeDevice` has `id`, `name`, `kind` (`gpu`, `cpu`), `integrated`,
`counted`, `vramGiB`, `memoryType`, `tflopsFp32`, `tflopsSource` (`table`, `config`, `computed`, `unknown`) and
`detail`.

**`nodes list`** shows `[nodes.<id>]` with state, latency, version, the rights that node grants and the reason when
it is not online. **`models list [--kind K]`** lists recommendations and prints the disclaimer; **`bench list
[--preset ID]`** lists bench records; **`presets list`** and **`presets show <id>`** show presets (show prints the
preset as a TOML block, secrets masked, and its command); **`key status`** and **`node status`** report the API key
and this machine's node settings without revealing them.

### Logs and events

**`logs <system> [--tail N] [--follow]`** prints the System's console, the engine's own view of the server: KLIF's
lines (the launch command, the paths of the two log files, notes about a dormant GPU or a fault), then the server's
stdout and stderr merged in arrival order, with ANSI codes removed and lines that mention an API key replaced by a
marker. KLIF keeps the last 200 lines per System, so `--tail` (default 100) cannot reach further back; the whole
files are at the `[KLIF] stdout:` and `stderr:` paths near the top of the console. It works for a System that is not
running (the console of its last session) and for a System on another node (that node serves its console).

```json
{ "schemaVersion": 1, "system": "s1", "label": "System 1", "status": "online",
  "lines": [ "0.00.008.793 I srv    load_model: loading model '...'", "0.00.008.818 I srv    llama_server: listening on http://127.0.0.1:7030" ] }
```

`--follow` keeps printing new lines (looking 2.5 times a second) until Ctrl-C or until the System stops: the last
line is then `-- System 1 is offline --`, and with `--json` an `end` event, and the exit code is 0. On a System that
is not running it prints the stored console and ends at once. `--tail 0 --follow` prints only what is new. With
`--json` every line is an event (`klif-cli schema log-event`):

```
{"at":1791116350.75,"line":"0.11.777.835 I slot launch_slot_: id  0 | task 1 | processing task, is_child = 0","schemaVersion":1,"system":"s1","type":"log"}
{"at":1791116414.68,"schemaVersion":1,"status":"offline","system":"s1","type":"end"}
```

`at` is when klif-cli saw the line (console lines carry no time). A server that logs more than 200 lines between two
looks loses the lines in between.

**`watch [--system S] [--types LIST] [--until SYSTEM=STATUS] [--timeout SECONDS]`** prints what changes in the engine,
one event per line, until Ctrl-C. **It replaces sleep-and-poll loops**: instead of `status` every few seconds, run
`klif-cli watch --until s2=online --timeout 900` and get the answer the moment it is true. It looks at the engine
twice a second and tells the differences, against the window's engine, `serve` or a private in-process engine alike.

| Type | Fields | When |
| --- | --- | --- |
| `status` | `system`, `label`, `from` (`null`: the System appeared), `to`, `reason` | a System's status changed |
| `launch` | `system`, `label`, `preset`, `model`, `endpoint` | a session started (a launch, a restart, an adopted session) |
| `stop` | `system`, `label`, `ended` (`stopped`, `fault`), `uptimeS` | a session ended |
| `fault` | `system`, `label`, `title`, `exitCode`, `logTail` (the last 12 lines) | a session faulted |
| `record` | `key`, `metric`, `old`, `new`, `model`, `quant`, `backend`, `machine`; `at` = when it was broken | a [record](#records) was broken |
| `download` | `id` (the recommendation), `file`, `state` (`running verifying done failed cancelled`), `doneBytes`, `totalBytes`, `error` | a model file download changed state, or moved on (at most once a second per file) |

`--types` takes a comma-separated subset; `--system S` limits `status`, `launch`, `stop` and `fault` to one System.
Two lines are always printed: the first, `watching` (every System with its status, the types, `until`, `timeoutS`), and,
when `--until` was reached, the last, `until`.

```
$ klif-cli watch --until s2=online --timeout 900
watching: s1 offline, s2 offline, waiting for s2=online (Ctrl-C ends it)
[+   2.5s] s2 (System 2): offline -> starting
[+   2.5s] s2 started (preset deep-27b, Deep 27B, http://127.0.0.1:7031/v1)
[+  41.0s] s2 (System 2): starting -> online
[+  41.0s] s2 is online
```

```
{"at":1791116380.50,"schemaVersion":1,"systems":[{"id":"s1","label":"System 1","status":"offline"},{"id":"s2","label":"System 2","status":"offline"}],"timeoutS":900.0,"type":"watching","types":["status","fault","launch","stop","record","download"],"until":"s2=online"}
{"at":1791116384.04,"from":"offline","label":"System 2","schemaVersion":1,"system":"s2","to":"online","type":"status"}
{"at":1791116384.04,"elapsedS":3.5,"schemaVersion":1,"status":"online","system":"s2","type":"until"}
```

- **`--until <system>=<status>`** ends the command with exit code 0 when the System has that status (`not-set invalid
  offline starting online busy stopping fault unreachable`). `online` means a loaded, answering server, so a busy one
  counts too (as for `launch --wait`). With a target of `online` or `busy`, a System that is in fault, or faults while
  watched, ends the command with the `fault` error (and its log tail), and a System that stops after a start was seen
  ends it with `stopped`: dismiss a fault first. `--timeout` then ends it with the `timeout` error (exit code 1) if the
  status was not reached in time; the server keeps whatever it was doing.
- **`--timeout SECONDS`** without `--until` just stops watching after that long (exit code 0).
- With no KLIF running, `watch` holds the engine while it runs, as `serve` does: a `launch` from another terminal
  connects to it, and the window cannot start until it ends. A session that faulted before an engine started is not
  kept, so `watch` after a failed `launch` (no `--wait`) sees an offline System; use `launch --wait`, or start `watch`
  first.

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

`bench <system> [--runs N] [--prompt N] [--gen N] [--audio F.wav] [--keep-running] [--allow-shared] [--yes]`

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
- **TTS:** fixed English texts (about ten seconds of speech each) to `POST /v1/audio/speech`. An `audiocpp` server
  is asked for `response_format: "json"` and reports its own timing; any other server gets `response_format:
  "wav"` and the voice `alloy`, the wall time is measured by KLIF and the audio length read from the WAV header.
  Records `ttsRtf` = seconds of audio per wall second (audio.cpp's own `rtf` is the inverse).
- **STT:** needs real speech: `--audio <file.wav>` (required; a WAV recording, 16 kHz mono is safest for
  whisper-server). Each run posts the file as multipart `file` to the server's transcription route
  (`/v1/audio/transcriptions` on audio.cpp; on other servers the `--inference-path` the command passes, else
  `/v1/audio/transcriptions`, then `/inference`). Records `sttRtf` = seconds of audio per wall second.
- **Music:** an `audiocpp` server only (any other adapter answers `unsupported`). Each run asks for the same 30 s
  instrumental (a fixed prompt and seed; lyrics `[Instrumental]` for ACE-Step, tags for HeartMuLa, a prompt only for
  Stable Audio; steps and guidance stay at the model's defaults) with `POST /v1/tasks/run` and the model id
  `/v1/models` lists. The request is synchronous, so a run lasts as long as the song takes to compose (use
  `--runs 1` on a big model); the first run can include loading the weights, which the median over three runs hides.
  The answer's `timing {wall_ms, audio_duration_ms, rtf}` gives the numbers; audio.cpp's `rtf` is wall / audio and
  KLIF records the inverse as `musicRtf` = seconds of music per wall second.
- `video` prints "not supported yet" (video records come from the jobs an sd.cpp server logs). A remote System
  must be benched on its own machine.
- It refuses while another local System runs on the same GPU (an unknown GPU counts as the same), because the
  numbers would be shared-GPU numbers. `--allow-shared` overrides that.
- Records are appended to `<data folder>\bench\<preset id>.json` (50 kept). A record carries the command hash; a
  later change to the command marks older results `stale` in `presets list` and the Tune drawer.

The JSON result is `{ "file", "summary", "record" }`; `record` has `at`, `presetId`, `presetHash`, `system`, `kind`,
`adapter`, `model`, `hardware` (`gpu`, `vramGiB`, `driver`), `backendBuild`, `promptTokens`, `genTokens`, `loadS`,
`peakVramGiB`, `spillMiB`, `layers` and `runs[]` (`ttftS`, `prefillTps`, `decodeTps`, `secondsPerImage`,
`promptTokens`, `genTokens`; TTS / STT / music: `audioS`, `wallS`, `ttsRtf` / `sttRtf` / `musicRtf`).

The runs also count for the [records](#records), marked `bench`.

### Records

`records [--kind K] [--metric M] [--backend B] [--node N]`, `records forget <key> --yes`, `records history <key> [--metric M]`

KLIF keeps, per exact model file (its SHA-256) and per backend (HIP, Vulkan, CUDA, CPU, Metal, from the server's
log, else the preset's `backend`), the best values ever reached on each machine: `decodeTps` and `prefillTps`
(highest), `ttftS` (lowest), `imageS` (seconds per image, lowest), `ttsRtf`, `sttRtf` and `musicRtf` (audio seconds
per wall second, highest; from `bench` only, since TTS / STT / music servers log no per-request timing) and `videoS`
(seconds per video job, lowest, with its size, frames and steps, from an sd.cpp server's `generate_video` log lines).
They are not a certified benchmark: values come from everyday use and from `bench`. A value counts when it is a
real measurement: decode with at least 128 generated tokens, prefill with at least 1024 prompt tokens that did not
come from the cache, time to first token from any request (its prompt and cached tokens are kept), an image when its
generation finished. Per-request timings come from the llama.cpp and sd.cpp logs; vLLM and OpenAI-compatible
servers only get records from `bench`, and external servers none.

Next to each value: when, `live` or `bench`, the context the server was launched with, prompt / cached / generated
tokens or image size and steps, KV type, GPUs, backend build, preset, KLIF version and the machine's FP32 TFLOPS.

- `--metric` takes `decodeTps`, `prefillTps`, `ttftS`, `imageS`, `ttsRtf`, `sttRtf`, `videoS`, `musicRtf` (or `decode`,
  `prefill`, `ttft`, `image`...) and sorts by it, best first. `--node N` keeps one node's entries; `--node local`
  this machine's. `--backend` compares without case.
- Text output: one ranking per metric. `--json`: `{ "records": [RecordEntry...] }`, each with `key`
  (`"<machine>|<sha256 or file:size>|<backend>"`; a node's entries `"<node>/<key>"`), `node`, `machine` (`[node]
  name`, else "This machine"), `kind`, `model` (`sha256`, `file`, `name`, `quant`, `source`, `sizeBytes`), `backend`
  and `best` (`<metric>`: `value`, `at`, `source`, `ctx`, `promptTokens`, `cachedTokens`, `genTokens`, `kv`, `width`,
  `height`, `steps`, `gpus`, `backendBuild`, `preset`, `klifVersion`, `tflopsFp32`).
- `records forget <key> --yes` removes an entry (and its history); any part of the key that names one entry is
  enough, several matches are `ambiguous`. A node's entry is forgotten by that node, which needs `edit` in its
  `[node] allow`.
- `records history <key> [--metric M]` prints how an entry climbed: every record that was broken, oldest first, with the
  value it replaced (`old`; absent for the first value), grouped by metric in the text output. The key is the whole key
  or a part of it that names one entry (several matches are `ambiguous`; a node's entries start with `<node>/`). The
  engine reads `records-history.jsonl`; for a node's entry it asks that node. The window's Records screen draws its
  progress chart from the same data (the control method `records_history`, view right; IPC command `klif_records_history`). JSON:
  `{ "key", "model", "quant", "backend", "machine", "node"?, "metric"?, "points": [ { "key", "metric", "old"?, "new",
  "at" } ] }`, at most the last 5000 points.
- Files (data folder): `records.json` (the best values), `records-history.jsonl` (one line per broken record: `key`,
  `metric`, `old`, `new`, `at`) and `hashes.json` (the model files' SHA-256 by path, size and modification time;
  hashing runs once per file, in the background at low IO priority).

### Models

`models list` prints the recommendations (id, kind, class, name, quant, size, license, measured speed, state) and
the disclaimer. `models download <id> --yes` follows the download until every file is done, failed or cancelled
(without `--yes` it names every Hugging Face repo the files come from, e.g. "from huggingface.co/a/b and
huggingface.co/c/d"); it
needs `[paths] models_dir`, runs in the engine (so keep a short-lived `klif-cli` open, or use `serve` or the window)
and an interrupted download resumes next time. `models adopt <id> [--system S] [--ctx N] [--kv TYPE]` turns a
recommendation into a preset and, with `--system`, selects it. Without `--ctx` / `--kv` the context and KV cache type
come from this machine's suggestion of that recommendation (`suggest`), else from the recommendation (its tier's
floor context, q8_0). Adopting a suggestion also sets llama.cpp's `-fitt` margin for its tier (System 1 keeps a third
of the inference GPU free) and adds `--offload-to-cpu` to an image preset whose suggestion needs it. Rules and limits:
[presets.md](presets.md#recommendations-and-downloads).

With `--json`, `models download` prints its progress as JSON lines on **stderr** and the final document on stdout as
usual: a `start` line (`id`, `name`, `source`, `into`, `totalBytes`), then a `progress` line per file at every state
change and at most once a second while it moves (`id`, `file`, `doneBytes`, `totalBytes`, `state`: `running verifying
done failed cancelled`, `error`), and `note` lines for remarks (`klif-cli schema download-event`):

```
{"at":1791117000.1,"doneBytes":1048576,"file":"Model-Q4_K_M.gguf","id":"example.q4-k-m","schemaVersion":1,"state":"running","totalBytes":16106127360,"type":"progress"}
```

### Suggestions

`suggest [--kind K] [--class C]` prints the model the embedded pool suggests for each slot of this machine: System 1
(fast), 2 (deep), 3 (max), image, tts, stt, video and music (a slot appears once the pool has models for it). It reads this machine
directly (GPUs, RAM; no engine is started) and shows, per slot, the model, quant, context, KV type, the estimated VRAM
and RAM against the tier's budget, where the weights go (`experts in RAM`, `layers in RAM`, `weights in RAM
(--offload-to-cpu)`, `CPU only (RAM)`) and the recommendation id to download and adopt. Every number is an estimate;
how it is made: [presets.md](presets.md#suggestions).

```
This machine: RX 9070 XT 15.9 GiB (pool 15.9 GiB, largest 15.9 GiB), 64.0 GiB RAM

SLOT             MODEL            QUANT                        CTX   KV    VRAM EST/BUDGET  RAM EST/BUDGET  PLACEMENT       REC
fast (System 1)  Gemma 4 26B-A4B  Q8_0                         256k  q8_0  9.6 / 9.6        20.3 / 48.0     experts in RAM  gemma-4-26b-a4b.q8-0
deep (System 2)  Gemma 4 12B      UD-Q4_K_XL (QAT)             256k  f16   11.9 / 14.9      0.0 / 0.0       -               gemma-4-12b.qat-ud-q4-k-xl
max (System 3)   Qwen 3.8 27B     Q8_0                         256k  f16   14.9 / 14.9      30.2 / 56.0     layers in RAM   qwen3.8-27b.q8-0
image            Krea 2 Turbo     Q4_K_M + Qwen3-VL-4B Q4_K_M  -     -     14.3 / 14.9      0.0 / 56.0      -               krea-2-turbo.q4-k-m
```

JSON: `{ "hardware": { "gpus": [ { "id", "name", "vramGiB", "integrated" } ], "cpu", "vramPoolGiB", "largestGpuGiB",
"ramTotalGiB", "unified" }, "suggestions": [ Suggestion ], "disclaimer" }`. A `Suggestion` has `slot` (`fast deep max
image tts stt video`), `kind`, `class` (LLM slots), `rec` (absent when nothing fits), `model`, `quant`, `ctx`, `kv`,
`estVramGiB`, `estRamGiB`, `budgetVramGiB`, `budgetRamGiB`, `placement` and `note` (why nothing fits, or what to
watch). The window shows the same list (the view model's `suggestions`).

### Help and schemas

**`help [<command>]`** prints the command list above; with a command (`help records history`, `help systems`) its
arguments, flags, whether it needs `--yes` and which documents it prints. **`klif-cli --json help`** prints the same as
a catalog an agent can read once instead of parsing text:

```json
{ "schemaVersion": 1, "usage": "...", "systemArgument": "...", "globalFlags": [...], "errorCodes": [ { "code": "needs_yes", "exitCode": 2, "meaning": "..." } ],
  "commands": [ { "name": "launch", "group": "run", "synopsis": "launch <system> --yes [--stop-others] [--wait]", "summary": "Start a model server",
      "args": [ { "name": "system", "required": true, "repeats": false, "description": "..." } ],
      "flags": [ { "name": "--wait", "description": "..." } ],
      "needsYes": true, "mayStartEngine": true, "outputs": [ { "schema": "launch" } ], "streams": false } ] }
```

`needsYes` is true when the command refuses without `--yes`; `yesWhen` says when a command needs it only sometimes
(`bench` when it has to launch the System). `mayStartEngine` is true when the command talks to the engine, so with no
KLIF running klif-cli starts one for the command (`engineWhen` names the part that needs it, e.g. `hardware --node`).
`streams` marks the commands that print JSON lines. The catalog is the table the text help is generated from.

**`schema [<name>]`** without a name lists the documents; with a name it prints the JSON Schema (draft 2020-12) of
that document, derived from the types the program prints with, so it follows the field names exactly. A command name
works too (`schema launch`, `schema "records history"`). The schema describes what is printed (the serialize side:
optional fields are not required), has `schemaVersion` as a required property, and is itself a JSON document with a
`schemaVersion` keyword. The names: `status`, `status-system`, `diag`, `hardware`, `suggest`, `plan`, `select`,
`launch`, `restart`, `stop`, `dismiss`, `logs`, `log-event`, `watch-event`, `systems-list`, `systems-add`,
`systems-remove`, `systems-update`, `presets-list`, `presets-show`, `presets-use`, `presets-param`, `presets-save`,
`presets-delete`, `bench`, `bench-list`, `records`, `records-forget`, `records-history`, `models-list`,
`models-download`, `models-adopt`, `download-event`, `key-status`, `key-update`, `node-status`, `node-token`,
`node-token-created`, `nodes-list`, `serve`, `help`, `schema-list`, `version` and `error` (the failure document, with
the error codes as an enum). `klif-cli schema` prints the list with the commands that print each one.

### Settings

**`settings [record-moment on|off]`** shows or sets this machine's display settings in `[ui]`: `record-moment` is the
"new record" moment over the skin (on by default; records are kept either way). Local only, never over the network.
JSON: `{schemaVersion, recordMoment}`.

## Recipes for agents

Wait until a System is ready, with the status in JSON (PowerShell):

```powershell
klif-cli launch s1 --yes --wait          # blocks; exit 1 with code fault/stopped/timeout on failure
$s = klif-cli --json status s1 | ConvertFrom-Json
$s.status; $s.baseUrl                    # online, http://127.0.0.1:7030/v1
```

Wait for an event instead of polling:

```powershell
klif-cli launch s2 --yes                              # returns once the session exists
klif-cli watch --until s2=online --timeout 900        # exit 0: online; fault / stopped / timeout: exit 1 with the reason
klif-cli --json logs s2 --tail 40                     # why it failed, or what the server printed while loading
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
