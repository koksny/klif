# Presets

A preset is a launch command with a name. Nothing is hidden: the program, the argument list, the working folder and
the environment in `klif.toml` are exactly what KLIF starts, and the Tune drawer, `klif-cli plan <system>` and the
file show the same final command line. A System points at one preset.

```toml
[presets.fast-8b]
name = "Fast 8B"
adapter = "llama.cpp"
command = 'D:\llama.cpp\llama-server.exe'
args = ["-m", "{model}", "-c", "{ctx}", "-ngl", "99", "--host", "{host}", "--port", "{port}", "{p.reasoning}"]
env = { HIP_VISIBLE_DEVICES = "0", PATH = 'D:\ROCm\bin;{env:PATH}' }
port = 7030
model = 'D:\models\fast-8b-Q4_K_M.gguf'
ctx = 16384

[presets.fast-8b.params.reasoning]
label = "Reasoning"
default = "on"
choices.on = { args = ["--reasoning", "on"] }
choices.off = { label = "Off", args = ["--reasoning", "off"] }
```

`config/klif.example.toml` has a complete set (llama.cpp, sd.cpp with a size param, a generic TTS server, an
external server). The id is `[a-z0-9][a-z0-9_-]{0,63}` and not a Windows device name (`con`, `nul`, `com1`, ...)
because bench records are named after it. An id with a dot is read as a sub-table, so use `-`.

## Fields

| Key | Meaning |
| --- | --- |
| `name` | Display name (default: the id) |
| `adapter` | `llama.cpp` (default), `sd.cpp`, `vllm`, `openai`, `audiocpp`, `generic` |
| `kind` | `llm`, `image`, `tts`, `stt`, `video`. Default: `image` for sd.cpp, `llm` for llama.cpp, vllm and openai. **Required for `generic`** |
| `command` | The program: an absolute path, or a name on PATH. `.exe` and `.com` only. Empty when `endpoint` is set |
| `args` | The argument list, one token per entry. KLIF never re-splits or merges tokens |
| `cwd` | Working folder (absolute, must exist). Default: the program's folder |
| `env` | Environment variables to set; values may use placeholders |
| `env_remove` | Inherited variables to remove from the child's environment |
| `port`, `host` | Effective port and bind host (rules below). Default host: `[net]` by kind |
| `endpoint` | An **external** server's URL; KLIF never starts or stops it |
| `health` | `"/path"` = HTTP GET, `"tcp"` = a listening port. Absent = the adapter's default chain |
| `model`, `mmproj`, `ctx` | The model file, its multimodal projector, the context size. `{model}`, `{mmproj}`, `{ctx}` |
| `gpu` | Which GPU it runs on: `"VEN:DEV"`, `"VEN:DEV#1"`, `"cpu"`, or a comma list. For display, fit and conflicts only |
| `managed` | Default `true`: KLIF adds the adapter's telemetry environment |
| `api_key` | Default `true`: KLIF puts its API key into the adapter's key variable |
| `model_name`, `quant`, `backend`, `device`, `notes` | Display. `backend`: HIP, Vulkan, CUDA, Metal, CPU or free text. `device` defaults to the GPU's name |
| `recommended` | The recommendation id this was made from |
| `params` | Independent options (below) |

An unknown key is a warning and is ignored. A preset that does not parse (or whose id is not allowed) is listed as
`invalid` with the reason and does not affect the others.

## Adapters

| Adapter | Default kind | Default port | Default health | Key variable | What KLIF reads |
| --- | --- | --- | --- | --- | --- |
| `llama.cpp` | llm | 7030 | HTTP `/health` | `LLAMA_API_KEY` | Log (load steps, VRAM buffers, device, timings), `/health`, `/slots`, `/v1/models`, `/metrics` |
| `sd.cpp` | image (or `kind = "video"`) | 1234 | TCP; video: HTTP `/sdcpp/v1/capabilities` | none | Log (load steps, sampling steps, device, image and video jobs), TCP |
| `vllm` | llm | 8000 | HTTP `/health` | `VLLM_API_KEY` | `/health`, `/metrics`, `/v1/models` |
| `openai` | llm | 8080 | `/health`, then `/v1/models`, then TCP | none | Probes only |
| `audiocpp` | tts | 8080 | HTTP `/health` | none | `/health`, `/v1/models` every 3 s (which model is loaded), the listening / failed lines, with `--log` one line per request |
| `generic` | required | required (or a `health` check) | TCP, or `health` | none | Process, health, per-process VRAM, log activity, and `/metrics` if it answers Prometheus text |

TTS, STT and video servers KLIF has no adapter for (a whisper.cpp server, koboldcpp, a Kokoro server, ComfyUI, ...)
run through `generic` or `openai` with the right `kind`. A `generic` System shows `busy` while the server logs
activity, so a chatty server looks busy.

- **audio.cpp** (`audiocpp_server.exe`, text to speech): `adapter = "audiocpp"`. Its models are listed in the JSON
  file `--config` names; set the preset's `model` to the same GGUF so KLIF knows the file (name, size, records).
  Pass `--host {host} --port {port}`; `--backend vulkan|hip|...` is also what the records call the backend. Add
  `--log` if you want the request count and activity: the server then prints one line per request to its output,
  which KLIF reads (`--log-file` writes a file KLIF does not read). `--idle-unload-ms` frees VRAM after quiet time;
  the live view shows whether the model is loaded. TTS records come from `klif-cli bench`.
- **whisper.cpp** (`whisper-server.exe`, speech to text): `adapter = "generic"`, `kind = "stt"`, `health =
  "/health"` (it answers 503 while the model loads), `model` plus `-m {model}`. It logs nothing per request beyond
  the file name, so the live view is activity only and records come from `klif-cli bench <system> --audio
  <file.wav>`. `--inference-path /v1/audio/transcriptions` makes it OpenAI-compatible.
- **sd.cpp video** (`sd-server.exe` with a `vid_gen` model such as MiniMax H3): `adapter = "sd.cpp"` with `kind =
  "video"`. KLIF reads `generate_video WxHxT`, the sampling bar and `generate_video completed in X s`: the live view
  shows the job in flight and every finished job becomes a `videoS` record with its size, frames and steps.
  Examples of all three are in `config\klif.example.toml`.

## How the command is built

1. **Params** are applied (below).
2. **Placeholders** are expanded in `command`, `args`, `cwd` and `env` values:

   | Placeholder | Value |
   | --- | --- |
   | `{model}` `{mmproj}` `{ctx}` | the preset's fields (a missing field is an error) |
   | `{host}` `{port}` | the effective bind host and port |
   | `{models_dir}` `{state_dir}` `{data_dir}` | KLIF's folders (`models_dir` must be set) |
   | `{stamp}` | The launch's time stamp, local time as `yyyyMMdd-HHmmss-fff` (for example `20261003-141502-087`). It is the stamp in the session name and in the log file names of that launch, so use it for file names that must differ per launch (a server's `--log-file`). Previews show the text `{stamp}` |
   | `{env:NAME}` | KLIF's own environment variable `NAME`, read when the command is built. Missing: empty plus a warning |
   | `{p.NAME}` | the selected choice's `args` (zero or more tokens); must be a whole argument |
   | `{p.NAME.VAR}` | the selected choice's variable `VAR` (one value) |

   Only `{identifier}` patterns count, so JSON braces in an argument stay as they are. An unknown placeholder or
   a missing value is an error. `{env:NAME}` is shown as `%NAME%` and hashed as that text, not as its value, so
   `klif.exe` and `klif-cli` (which may have different environments) agree on the hash. `{stamp}` is shown and
   hashed as the text `{stamp}` in the Tune drawer and in `klif-cli plan`: it has no value until a launch starts,
   and a hash that changed with every launch would mark every bench result stale.
3. **Program:** an absolute `.exe`/`.com`, or a bare name searched on the preset's own `PATH` first, then on KLIF's.
   A `.bat`/`.cmd` file is an error with the way out (`command = "cmd.exe"`, `args = ["/c", "<the .bat file>", ...]`);
   `.ps1`, `.py`, `.sh`, `.js` and similar are errors too: set `command` to the interpreter and put the script in `args`.
4. **Working folder:** `cwd`, else the program's folder.
5. **Environment:** KLIF's own environment, minus `env_remove` and the key variables, plus the managed rows, the
   API key and your `env` (yours wins per key and is shown as "overridden").
6. **Command line:** rendered with Windows quoting rules; that string is what the Tune drawer's header shows.

### Port and host

The effective port is `port` if set, else a literal in the arguments or environment (`--port` or `LLAMA_ARG_PORT`
for llama.cpp, `--listen-port` for sd.cpp, `--port` for vllm, openai, audiocpp and generic), else the adapter's default.
`port` set **and** a different literal in the arguments is an error: keep one, preferably `port = 7030` plus
`"--port", "{port}"`. Host works the same way (`--host`, `--listen-ip` for sd.cpp). For external presets both come
from `endpoint`. Pass `{host}` and `{port}` through: if the arguments never say so, llama-server listens on its own
default (8080) and binds `127.0.0.1` while KLIF watches something else, and KLIF warns about it.

### Environment and the API key

- `managed = true` (default) adds, for llama.cpp, `LLAMA_ARG_LOG_VERBOSITY=4` (when `[telemetry]
  verbose_llama_logs` is on), `LLAMA_ARG_LOG_PREFIX=1` and `LLAMA_ARG_LOG_TIMESTAMPS=1`. They are what the log
  parser reads. Rows KLIF sets are marked "set by KLIF" in the window and `klif-cli plan`. Set `managed = false`
  to opt out, or define the same name yourself to override one.
- The KLIF API key is put into `LLAMA_API_KEY` (llama.cpp) or `VLLM_API_KEY` (vllm), only when the preset has
  `api_key = true`, a key is set and KLIF starts the server. Both variables are removed from the inherited
  environment first, so a key in KLIF's own environment never leaks into a child. External presets and the other
  adapters never receive it. The key never goes into the argument list.
- Variables whose names contain KEY, TOKEN, SECRET, PASS or AUTH, and the value after `--api-key` or `--hf-token`,
  are masked as `••••` in the window, in `klif-cli` output and in logs. Sending `••••` back when saving means "keep
  the stored value". A param var that feeds such a value (`--api-key={p.auth.key}`, a secret variable set from
  `{p.NAME.VAR}`) is masked the same way in every choice, as is a var whose own name looks secret; it is kept on save
  too. (A harmless var whose name contains one of those words, such as `max_tokens`, shows as `••••` as well.) A
  secret in `args` is a warning, because every local process can read the command line.
- A server bound to anything but loopback needs the API key for llama.cpp and vllm (an error otherwise; `[security]
  api_key = "none"` turns the check off). For sd.cpp, openai, audiocpp and generic, which have no key mechanism, it
  is a warning.
- A `LLAMA_ARG_*` variable in KLIF's own environment is inherited by llama-server. KLIF warns about it and
  suggests `env_remove`.

## Params

A preset can declare independent options so that one preset covers every combination, instead of a preset per
combination:

```toml
[presets.image-turbo.params.size]
label = "Size"
default = "1024"
choices.512 = { label = "512 x 512", args = ["-W", "512", "-H", "512"] }
choices.1024 = { label = "1024 x 1024", args = ["-W", "1024", "-H", "1024"] }
```

A choice has `label`, `vars = { k = "v" }` (read with `{p.NAME.k}`), `args = [..]` (what `{p.NAME}` expands to) and
`env = { K = "V" }` (merged into the preset's environment). The System's selection lives in
`[systems.<id>] params = { size = "512" }` and changes with the Params block in Tune or `klif-cli presets param
<system> <name> <value>`. A selection that is not a choice falls back to the default with a warning. Without
`default`, the first choice in the file is the default. Choices keep their file order everywhere (Tune, `klif-cli`,
and when KLIF writes the preset). Changes apply on the next launch.

Choosing another preset for a System (Use in Tune, `klif-cli presets use`, Save as new with the System selected,
Use on a recommendation) keeps only the selections the new preset can honour: the param must exist in the new
preset and the value must be one of its choices. Every other entry is removed from `[systems.<id>] params` in the
same edit, so a stale `reasoning = "off"` does not stay in the file. Params the new preset declares that the
System had no selection for start at their defaults.

## Validation

Errors block Launch (the System is `invalid`); warnings do not. The command preview shows both, with the field.

Errors include: unknown placeholder or missing value; `{p.X}` not a whole argument or no such param; a program
that does not exist, is a script or a batch file; a model, mmproj or working folder that does not exist; `port` and
the arguments disagreeing; no port for a `generic` preset without `health`; `kind` missing for `generic`; a System's
kind not matching its preset's; an external preset with a command or without a usable endpoint (an `https://`
endpoint is refused for now: KLIF probes external servers over plain `http://`); a non-loopback host without the
API key (llama.cpp and vllm).

Warnings include: another System with the same host and port ("they cannot run at the same time"; not when either is
`exclusive` on a GPU they share, because those two can never run together anyway); a port held by
a program KLIF does not own; llama-server arguments that never pass `--port` or `--host`; arguments that break
telemetry (log verbosity below 4, no log prefix or timestamps, the log switched off or to JSON lines);
`LLAMA_ARG_*` inherited from KLIF's environment;
`vllm` through `wsl.exe` (Stop ends `wsl.exe` only); a secret written in `args`; an absolute path in the arguments
that does not exist; `port` or `args` on an external preset (ignored); an endpoint path other than `/v1` (KLIF
probes the server root); a `generic` preset with `health` but no port (KLIF then treats the server as online while
its process runs, and the health check is ignored).

## External presets

`endpoint = "http://192.0.2.20:11434"` and an empty `command` make an external preset: KLIF probes it (the adapter's
health chain), shows its state and model, and never starts, stops or restarts it. `port` and `host` come from the
endpoint. For an LLM, clients use `<endpoint>/v1`.

## The hash

`command.hash` (16 hex digits) identifies the resolved command: adapter, endpoint, program, arguments (secrets
masked), working folder, environment rows (secret values as `<secret>`), your `env_remove`, port and host. It is the
same in the window, in `klif-cli plan` and in the launch plan. It decides when a bench result is `stale` and when a
running session "differs from" its System's current command. The preset's `api_key = false` opt-out counts (it
changes the server's environment), so turning it off on a running System shows "differs from the running session".

A second hash, `specHash` (in `klif-cli --json presets show` and in Tune), covers every stored field of the preset,
also name, notes, health and gpu. Apply in Tune and `klif-cli presets set` send it back as `baseHash`: when the
preset changed on disk meanwhile, the write is refused ("changed on disk") and Tune offers Reload.

## Editing presets

| Where | How |
| --- | --- |
| Tune drawer | Preset row (Use, Save as new, Delete) and the Command section: adapter, program, working folder, endpoint toggle, one input per argument (never re-tokenized), environment rows, grid for port, host, health, model, mmproj, ctx, GPU, managed, API key. Live preview 250 ms after the last edit. Revert / Save as new / Apply. "Used by System 1, System 3" is shown before you change a shared preset |
| `klif-cli` | `presets list`, `show <id>`, `use <system> <id>`, `param`, `save <id> --file F.toml [--use <system>]`, `set <id> key=value...`, `delete <id> --yes`. [cli.md](cli.md) |
| The file | Any editor. KLIF reloads it |

Edits made by KLIF re-read the file first, refuse to write if the file on disk does not parse, write atomically,
keep CRLF line ends and a BOM, omit default-valued keys and always write `adapter`. They keep your comments:
unchanged values keep their exact text, changed values keep their trailing comment, and a comment directly above a
`[table]` header moves or goes with that table. A blank line between a section comment and the table below it keeps
the comment where it is. Comments inside an array that KLIF rewrites (an edited `args`) are not kept, so put
explanations above the array or in `notes`. Delete is refused while a System uses the preset. Moving a System
written inline (`[systems] a = { ... }`) is refused rather than rewritten.

## Recommendations and downloads

> Recommendations are starting points; their memory numbers are estimates. Models, drivers and backends change:
> your agent should benchmark and calibrate (`klif-cli bench <system>`). Each model keeps its own license. KLIF
> downloads only from huggingface.co, and only when you ask.

The model pool is embedded in the program (`crates/klif-catalog/data/recommendations.toml`, schema 2; the header of
that file has the rules). Each `[[model]]` names a Hugging Face repo, a 40-character commit, its license, its size
and architecture facts (total and active parameters, MoE, vision, `ctx_max`), the KV cache cost per 1024 tokens, an
argument template, and a ladder of `[[model.quant]]` rungs: each rung has a `quality` (about its effective bits), its
files with SHA-256 and size and, when it was measured, the numbers with the hardware and backend. Every (model, rung)
is one recommendation with the id `<model>.<rung>`, for example `qwen3.8-27b.ud-q4-k-xl`. A model or rung with a
network or model-fetch flag, a secret flag, an `env` template, a bad revision or an unsafe file name is rejected when
KLIF loads. The default quant source is unsloth; where unsloth has none (Krea 2, SDXL, text encoders, VAEs) the files
come from the repos sd.cpp's documentation uses.

A model whose files live in several Hugging Face repos (a language model and its vision projector, an image model
and its VAE) gives each file that comes from elsewhere its own `repo` and `revision`:

```toml
files = [
  { name = "model-Q4_K_M.gguf", role = "model", sha256 = "<64 hex>", size = 0 },
  { name = "vae/ae.safetensors", role = "other", repo = "org/other-repo", revision = "<40-hex commit of that repo>", key = "vae", sha256 = "<64 hex>", size = 0 },
]
```

A file without them comes from the model's `hf_repo` at the model's `revision` (or the rung's own, which Gemma's QAT
rungs use). A file that names a `repo` must also name its `revision`, because a commit belongs to one repo; a
`revision` alone pins that file to another commit of the entry's repo. Both are checked like the entry's own (an
`owner/name` repo, a 40-character commit) when KLIF loads, and an entry that fails is skipped. The download asks the
hub about that file's repo and commit, for its size and LFS hash, and stores the file under
`<models_dir>\<owner>--<repo>\<file>` of its own repo, or under its `save_as` path there (GLM-5.3-Flash's rewritten
first shard replaces the original one this way). The argument template names other files as `{file:<key>}`.

- **List:** the Recommended section of Tune (the System's kind and class first), or `klif-cli models list [--kind K]`.
- **Download:** the Download button or `klif-cli models download <id> --yes`, only after you ask and only with
  `[paths] models_dir` set. Files go to `<models_dir>\<owner>--<repo>\<file>`. KLIF reads the file list and LFS
  hashes from the hub's API for that commit, fetches over https from `huggingface.co` and its own CDN hosts
  (`*.huggingface.co`, `*.hf.co`) only, in 128 MiB ranged requests so a stalled transfer resumes, writes a `.part`
  file, checks the SHA-256 and free disk space, and only then renames it. Cancel keeps the `.part`. An existing
  file is never overwritten. A `HF_TOKEN` in KLIF's environment (for gated models) is sent to `huggingface.co`
  only.
- **Use:** `klif-cli models adopt <id> [--system S] [--ctx N] [--kv TYPE]` or Use in Tune makes a preset from the
  recommendation. It does not know where your server program is: the program, working folder and environment are
  copied from the target System's current preset with the same adapter, else from another preset with that adapter
  whose program exists, and the target System's preset also keeps its `port`. With none, the new preset is empty
  and `invalid` until you fill in the program. `model` and `mmproj` come from the entry's file roles: the first
  `role = "model"` file and the `role = "mmproj"` file, at the place the download puts each (the path of a file that
  an existing preset already references is reused). Other files are named by the arguments (`{file:...}`) or only
  downloaded (the later shards of a split GGUF). The context and the `-ctk` / `-ctv` KV cache type are the ones given,
  else this machine's suggestion of that recommendation, else its tier's floor and q8_0; a suggestion also sets
  `-fitt` (System 1: a third of the inference GPU, at least 1024 MiB; System 2 / 3: 1024) and `--offload-to-cpu`
  for an image model that needs it. The System keeps only the param selections the new preset declares (see
  Params).

## Suggestions

> Every number in a suggestion is an estimate.

`klif-cli suggest` (and the window) shows, for this machine, the model of the pool suggested for each slot: System 1
(fast), System 2 (deep), System 3 (max) and image. The memory KLIF counts:

- **The VRAM pool** is every counted GPU of any vendor (`klif-cli hardware` lists them; an integrated GPU counts
  only when there is no discrete one, so on a Strix Halo or a Mac the unified memory is the pool). 1 GiB stays free
  on each counted GPU.
- **System 1 (fast)** leaves more than a third of the pool and a quarter of the RAM free. A dense model stays
  entirely in VRAM; a MoE may keep experts in RAM.
- **System 2 (deep)** may use the whole pool, nothing in RAM.
- **System 3 (max)** may use the pool and the RAM minus the larger of 8 GiB and 10 % of the RAM; anything may
  spill to RAM.
- **Image** models run on one GPU: the largest counted one, else with `--offload-to-cpu` and RAM.
- A machine without a counted GPU runs everything from RAM (System 1 keeps a third of it free, the others the OS
  reserve).

An LLM option is the rung's file sizes plus the KV cache at a context and KV type plus the model's overhead. Per
model, KLIF takes the best rung up to the Q5 class that fits at the tier's floor context (System 1: 32k, System 2:
64k, System 3: 128k, at most the model's own maximum) with a q8_0 KV cache, then the longest context up to the
model's maximum, then the best rung again at that context, then an f16 KV cache if it still fits. Dense models are
not suggested below quality 3 (UD-Q3_K_XL), MoE models not below 2 (UD-IQ2_XXS). The first model of the tier's list
with a fitting option is suggested, preferring one that a lower tier did not already take. Nothing fits: the slot
says why. Adopting a suggestion writes its context, KV type and fit margin into the new preset (see Use above).

## More examples

A vLLM server on another machine, watched only:

```toml
[presets.vllm-box]
adapter = "vllm"
endpoint = "http://192.0.2.30:8000"
```

A log file per launch: the server writes `fast-8b-<stamp>.log` itself, and `{stamp}` is the launch's stamp (the
folder must exist). KLIF keeps reading its own captured output in `logs\`, not this file.

```toml
[presets.fast-8b-logged]
adapter = "llama.cpp"
command = 'D:\llama.cpp\llama-server.exe'
args = ["-m", "{model}", "-c", "{ctx}", "--host", "{host}", "--port", "{port}",
        "--log-file", 'D:\llama.cpp\logs\fast-8b-{stamp}.log']
port = 7030
model = 'D:\models\fast-8b-Q4_K_M.gguf'
ctx = 16384
```

A script: use its interpreter as the command and put the script in `args`.

```toml
[presets.tts-script]
adapter = "generic"
kind = "tts"
command = 'D:\python\python.exe'
args = ["D:\\tts\\serve.py", "--host", "{host}", "--port", "{port}"]
port = 8880
health = "/health"
```
