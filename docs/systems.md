# Systems

A **System** is one tab: one server at a time, started from one preset. You can have as many as you have use for,
and they run side by side. Only configured Systems get a tab; a fresh install with none shows a card that says
"No Systems yet" and offers **Add a System**.

```toml
[systems.s1]
label = "System 1"            # default derived from kind and class
kind = "llm"                  # llm | image | tts | stt | video   (required)
class = "fast"                # llm only, optional: fast | deep | max
preset = "fast-8b"            # absent = "not set"
params = { reasoning = "on" } # choices for the params the preset declares
exclusive = false             # true = needs the whole GPU
```

The id (`s1`) is `[a-z0-9][a-z0-9_-]{0,31}` and never contains `/`. Tab order is the order of the tables in
`klif.toml`. The window, `klif-cli` and the file all edit the same thing; changes you make in an editor are picked
up while KLIF runs.

## Kinds

| Kind | For | Typical adapter | What KLIF shows | Bench |
| --- | --- | --- | --- | --- |
| `llm` | Language models | `llama.cpp`, `vllm`, `openai` | Load steps, context, prefill and decode progress, tokens per second, VRAM composition | yes |
| `image` | Image generation | `sd.cpp` (or `openai` / `generic`) | Load steps, sampling steps, seconds per image | `sd.cpp` and `openai` servers that answer `POST /v1/images/generations` |
| `tts` | Text to speech | `generic` or `openai` | Process, health, requests in flight and total when the server exposes metrics, per-process VRAM | not yet |
| `stt` | Speech to text (audio in) | `generic` or `openai` | as `tts` | not yet |
| `video` | Video generation | `generic` or `openai` | as `tts` | not yet |

A preset has a kind too (by default `image` for `sd.cpp` and `llm` for the other named adapters; `generic` must say).
A System can use only presets of its own kind; mixing them is refused when you choose the preset and shown as
`invalid` if the file says it anyway.

**Class** (LLM only) is a grouping, not a limit: `fast`, `deep` and `max` label System 1, System 2 and System 3 by
default and decide which recommendations are listed first. A System without a class works the same.

**Default labels and ids** when you add a System without naming it: LLM `fast`/`deep`/`max` get "System 1"/"System 2"/
"System 3" (ids `s1`/`s2`/`s3`), an LLM without class the next free "System N", `image` "System CGI" (`cgi`), `tts`
"System TTS" (`tts`), `stt` "System STT" (`stt`), `video` "System Video" (`video`). A label that is already taken
(ignoring case) gets a number in brackets: a second `fast` LLM System is "System 1 (2)", a third "System 1 (3)", a
second TTS System "System TTS (2)". A taken id gets "-2", "-3" (`tts-2`); a taken `s1`/`s2`/`s3` falls back to the
next free `sN`.

## Status

Every tab shows its status as a small dot before the label, and the same word is in `klif-cli --json status`.

| Status | Meaning | Dot |
| --- | --- | --- |
| `not-set` | No preset selected | none, label muted |
| `invalid` | The preset has errors, the program or the model file is missing, or the port is held by a program KLIF does not own. The `reason` says which | none, label muted, `!` after it |
| `offline` | Ready to launch and not running. For an external server: not answering | ring, muted |
| `starting` | The process is starting or loading the model | ring, pulsing |
| `online` | Live and idle | solid |
| `busy` | Live and working (prefill, decode, sampling steps, requests in flight) | solid, pulsing |
| `stopping` | Stop was requested and KLIF is waiting for the process to end | ring, pulsing |
| `fault` | The process died, or the server logged a fatal error and is not ready. The window shows the exit code and the log tail | solid, warning colour |
| `unreachable` | The System's remote node is down, unauthorized or incompatible; last known state only | dashed ring |

A fault stays on the tab until you **Dismiss** it (back to offline) or **Restart**. Hover a dot for the status
and the reason. A System of another machine carries a muted ` · <node name>` after its label.

## Running, stopping, adopting

- **Launch** builds the command from the preset (`klif-cli plan <system>` shows it), starts the program hidden
  with output appended to log files, and watches it until the server reports ready. If the System has conflicts
  (below), Launch says so, or reads "Stop <names> & launch".
- **Stop** ends the System's own process tree (the named job object KLIF made for it), waits for the processes to
  exit and for the port to be released. It never touches a process KLIF did not start.
- **Restart** is stop and launch. Changing the preset, a param or the preset's command while a server runs only
  shows "differs from the running session"; it applies on the next launch.
- **Selecting a tab never stops anything.** Launch and Restart select the System they act on when the selected tab
  has no session.
- **Quitting KLIF leaves servers running.** The next start adopts every session it recorded (found again by job
  name), so a restart of the window costs nothing. A server that died meanwhile shows as a fault.
- **Stop all Systems** (tray, `klif-cli stop --all --yes`) stops every local session. External Systems are never
  affected.

## Conflicts and `exclusive`

Before a launch KLIF computes, for every local System that is not running, which running Systems would have to
stop first. The list is `conflicts` on the System, and it is why the button may say "Stop System 1 & launch".

| Rule | A running System conflicts when |
| --- | --- |
| Port | It listens on the same port and an overlapping host (`0.0.0.0` and `::` overlap any host) |
| Exclusive GPU | It shares a GPU and either side has `exclusive = true` |
| VRAM | The System's expected VRAM does not fit in the GPU's free memory next to what is running. Running Systems on that GPU are added to the list, largest first, until it fits |

The expected VRAM is what an earlier session of the same command measured, else (when nothing is offloaded
to the CPU) the size of the model files. While another System is starting or about to launch, KLIF reserves the larger of what it
has measured so far and what it expects. If a System would not fit even with everything else stopped, KLIF warns "Does not
fit even with everything stopped" and lists no VRAM conflicts. A System whose preset names no `gpu`, with no
`[gpu] inference` set, counts as sharing an unknown GPU with every other such System, which is the cautious
reading.

What Launch does with conflicts:

- `[launch] on_conflict = "ask"` (default): refuse and name them ("System 2 cannot launch while System 1 is running
  (exclusive GPU). Use "Stop System 1 & launch"."). The window's button then reads "Stop System 1 & launch" and
  sends `stopOthers`; `klif-cli launch s2 --stop-others --yes` does the same.
- `"stop"`: stop them first, then launch. A System that is `busy` is never stopped this way; that needs the explicit
  choice above. If a System still waiting to be stopped turns `busy` before its turn, the launch is cancelled
  ("launch of System CGI cancelled: System 5 became busy; use "Stop System 5 & launch" to stop it anyway") and that
  System keeps running.
- Stopping happens one System at a time, each waited for. A System that is waiting to launch counts as a holder:
  asking for something that conflicts with it answers "System 2 is about to launch."
- Restart never stops other Systems: when the relaunch would conflict, it is refused before anything stops
  ("System 1 cannot restart while System 2 is running (exclusive GPU). Stop System 2 first.").

A port held by a program KLIF does not own makes the System `invalid` ("held by ..."); KLIF reports it and does not
kill it. Two Systems whose presets resolve to the same host and port get a warning on both, except when either is
`exclusive` on a GPU they share: those two can never run together, so the shared port is not a problem.

## External Systems

A preset with `endpoint` and no `command` is an external server: another machine's llama-server, an Ollama, a TTS
box. KLIF probes it all the time, shows `online`, `busy` or `offline`, and its endpoint and model, and nothing else:
Launch, Stop and Restart are refused with a sentence ("... is an external server; start it where it runs."), and the
window shows no Launch or Stop control for them, only a quiet "External server" note in its place. External
Systems are never in a conflict list, never `invalid` because of a port, and never receive KLIF's API key. A local
System whose address equals an online external one is `invalid` ("port is used by the external System ...").

## Removed from the file while running

If you delete a running System's table (or it stops parsing), the server is not orphaned: it stays on the tab bar
as "<id> (not in klif.toml)", read-only, with Stop and Dismiss only, and it still counts in conflicts. It uses the
kind, adapter, port and GPU it was started with. Put the table back and it is a normal System again; it disappears
when its process is gone.

## Changing Systems

| To | Window | Command | File |
| --- | --- | --- | --- |
| Add | `+` at the end of the tabs: kind, class, label, then Recommended / existing preset / blank | `systems add --kind llm --class fast --label "System 1"` | a `[systems.<id>]` table |
| Rename | Inline in the Tune drawer | `systems rename s1 "Daily"` | `label` |
| Reorder | Move left / right | `systems move s1 0` (0-based index among local Systems) | table order |
| Exclusive | Exclusive GPU | `systems exclusive s3 on` | `exclusive` |
| Choose preset | Preset row, **Use** | `presets use s1 fast-8b` | `preset` |
| Choose a param | Params block | `presets param s1 reasoning off` | `params` |
| Remove | Remove System... | `systems remove s1 --yes` | delete the table |

Choosing a preset is refused (and nothing is written) when the preset does not exist or its kind is not the System's
kind. When it goes through, the System's `params` keep only the selections the new preset can honour (a param it
declares, set to one of its choices); the rest are removed from the file in the same edit. Params the new preset
declares that the System had no selection for start at the preset's default.

Removing a System that is running is refused; removing one does not delete its preset. `systems rename` accepts
the same arguments as every other command (id or label). Everything that changes `klif.toml` goes through a
comment-preserving editor that re-reads the file first and refuses to write if the file on disk does not parse.

## Several GPUs

A preset says which GPU it runs on with `gpu = "VEN:DEV"` (PCI vendor and device, hex), `"cpu"`, or a comma list for
a model split across cards; without `gpu` it uses `[gpu] inference`. Two identical cards are `VEN:DEV` and
`VEN:DEV#1` in DXGI order. `klif-cli --json diag` lists the ids KLIF sees. The `gpu` field is what KLIF measures
and plans with; the command itself must select the device (for example `HIP_VISIBLE_DEVICES`), see
[platforms.md](platforms.md). If a server logs a device that does not match the preset's `device` (default: the
GPU's name), the System shows a warning.

## Systems on other machines

Another node's Systems appear after the local ones, with the id `<node>/<system>` (for example `render-box/cgi`), the
node's name next to the label, and `controllable` / `editable` flags that follow the rights that node grants. See
[nodes.md](nodes.md).
