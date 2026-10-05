//! The command catalog: every klif-cli command with its synopsis, arguments, flags, whether it needs `--yes`,
//! whether it may start an engine and which documents it prints. One table feeds `klif-cli --help` (the command
//! list below), `klif-cli help [<command>]` / `help --json` and `klif-cli schema` (which commands print which
//! schema). Add a command or a flag here when you add it to the parser; the parsers themselves are in the command
//! modules.

use crate::outputs::{CatalogArg, CatalogCommand, CatalogErrorCode, CatalogFlag, CatalogOutput, HelpDoc};

/// A positional argument.
pub struct A {
    pub name: &'static str,
    pub required: bool,
    pub repeats: bool,
    pub desc: &'static str,
}

/// A flag: `value` is the placeholder of an option that takes one.
pub struct F {
    pub name: &'static str,
    pub value: Option<&'static str>,
    pub desc: &'static str,
}

/// Whether the command refuses to run without `--yes`.
pub enum Yes {
    No,
    Always,
    /// Only in the described case.
    When(&'static str),
}

/// Whether the command talks to the engine (and so starts one in-process when no KLIF runs).
pub enum Eng {
    Never,
    May,
    /// Only for the described part of the command.
    Only(&'static str),
}

/// A document the command prints with `--json`.
pub struct O {
    pub schema: &'static str,
    pub when: Option<&'static str>,
}

pub struct Cmd {
    pub name: &'static str,
    pub group: &'static str,
    pub synopsis: &'static str,
    pub summary: &'static str,
    pub args: &'static [A],
    pub flags: &'static [F],
    pub yes: Yes,
    pub engine: Eng,
    pub outputs: &'static [O],
    /// `--json` prints JSON lines (until the command ends) instead of one document.
    pub streams: bool,
}

const fn a(name: &'static str, required: bool, desc: &'static str) -> A {
    A { name, required, repeats: false, desc }
}

const fn f(name: &'static str, value: Option<&'static str>, desc: &'static str) -> F {
    F { name, value, desc }
}

const fn o(schema: &'static str) -> O {
    O { schema, when: None }
}

const SYSTEM: A = a("system", true, "A System: an id (s1, render-box/s1), a label (system1) or low|medium|high|krea");
const NODE: F = f("--node", Some("N"), "A node (see nodes list); omit for this machine");
const YES: F = f("--yes", None, "Confirms the change; without it the command refuses (needs_yes)");
const WAIT: F = f("--wait", None, "Wait until the System is online (a fault or a stop ends the wait with an error)");
const KIND: F = f("--kind", Some("K"), "llm | image | tts | stt | video");

/// Every command, in the order `--help` prints them; a change of `group` is a blank line there.
pub static COMMANDS: &[Cmd] = &[
    // ---- run
    Cmd {
        name: "status",
        group: "run",
        synopsis: "status [<system>]",
        summary: "Systems, GPUs and nodes (one System with <system>)",
        args: &[a("system", false, "A System; without it every System, GPU and node")],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("status"), O { schema: "status-system", when: Some("with <system>") }],
        streams: false,
    },
    Cmd {
        name: "diag",
        group: "run",
        synopsis: "diag",
        summary: "Diagnostics: config location, engine, GPUs, versions (no secrets)",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("diag")],
        streams: false,
    },
    Cmd {
        name: "hardware",
        group: "run",
        synopsis: "hardware [--node N]",
        summary: "GPUs, CPU and RAM with their peak FP32 TFLOPS (reads this machine directly)",
        args: &[],
        flags: &[f("--node", Some("N"), "A node's inventory (by id or name) instead of this machine's")],
        yes: Yes::No,
        engine: Eng::Only("--node"),
        outputs: &[o("hardware")],
        streams: false,
    },
    Cmd {
        name: "plan",
        group: "run",
        synopsis: "plan <system>",
        summary: "The exact command a System would launch now (secrets masked)",
        args: &[SYSTEM],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("plan")],
        streams: false,
    },
    Cmd {
        name: "select",
        group: "run",
        synopsis: "select <system>",
        summary: "Select a tab",
        args: &[SYSTEM],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("select")],
        streams: false,
    },
    Cmd {
        name: "launch",
        group: "run",
        synopsis: "launch <system> --yes [--stop-others] [--wait]",
        summary: "Start a model server",
        args: &[SYSTEM],
        flags: &[
            YES,
            f("--stop-others", None, "Stop the Systems that conflict (port, exclusive GPU, VRAM) first"),
            f("--wait", None, "Wait until the System is online (up to 15 minutes; the server keeps loading if klif-cli stops)"),
        ],
        yes: Yes::Always,
        engine: Eng::May,
        outputs: &[o("launch")],
        streams: false,
    },
    Cmd {
        name: "stop",
        group: "run",
        synopsis: "stop <system> --yes | stop --all --yes",
        summary: "Stop a model server, or every running System",
        args: &[a("system", false, "A System (not with --all)")],
        flags: &[f("--all", None, "Stop every running local System"), YES],
        yes: Yes::Always,
        engine: Eng::May,
        outputs: &[o("stop")],
        streams: false,
    },
    Cmd {
        name: "restart",
        group: "run",
        synopsis: "restart <system> --yes [--wait]",
        summary: "Stop and start a System again with its preset",
        args: &[SYSTEM],
        flags: &[YES, WAIT],
        yes: Yes::Always,
        engine: Eng::May,
        outputs: &[o("restart")],
        streams: false,
    },
    Cmd {
        name: "dismiss",
        group: "run",
        synopsis: "dismiss <system>",
        summary: "Leave a fault (back to offline)",
        args: &[SYSTEM],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("dismiss")],
        streams: false,
    },
    Cmd {
        name: "logs",
        group: "run",
        synopsis: "logs <system> [--tail N] [--follow]",
        summary: "The System's server log lines (the engine's console, the last 200)",
        args: &[SYSTEM],
        flags: &[
            f("--tail", Some("N"), "How many of the latest lines (default 100; 0 with --follow = only new lines)"),
            f("--follow", None, "Keep printing new lines until Ctrl-C or the System stops (with --json: JSON lines)"),
        ],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("logs"), O { schema: "log-event", when: Some("with --follow (JSON lines)") }],
        streams: false,
    },
    Cmd {
        name: "watch",
        group: "run",
        synopsis: "watch [--system S] [--types LIST] [--until SYSTEM=STATUS] [--timeout SECONDS]",
        summary: "Events as they happen, one line each; use it instead of sleep-and-poll loops",
        args: &[],
        flags: &[
            f("--system", Some("S"), "Only the status / launch / stop / fault events of this System"),
            f("--types", Some("LIST"), "Comma-separated: status, fault, launch, stop, record, download (default: all)"),
            f("--until", Some("SYSTEM=STATUS"), "Exit 0 when the System reaches the status (not-set, invalid, offline, starting, online, busy, stopping, fault, unreachable)"),
            f("--timeout", Some("SECONDS"), "With --until: exit with the timeout error after this long; without: stop watching (exit 0)"),
        ],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("watch-event")],
        streams: true,
    },
    // ---- systems
    Cmd {
        name: "systems list",
        group: "systems",
        synopsis: "systems list",
        summary: "Every System with its status, preset and flags",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("systems-list")],
        streams: false,
    },
    Cmd {
        name: "systems add",
        group: "systems",
        synopsis: "systems add --kind llm|image|tts|stt|video|music [--class fast|deep|max] [--label L] [--id ID] [--preset P] [--node N]",
        summary: "Add a System (a tab)",
        args: &[],
        flags: &[
            f("--kind", Some("K"), "llm | image | tts | stt | video (required)"),
            f("--class", Some("C"), "fast | deep | max (llm only)"),
            f("--label", Some("L"), "Tab label"),
            f("--id", Some("ID"), "System id ([a-z0-9][a-z0-9_-]{0,31})"),
            f("--preset", Some("P"), "Preset the System uses"),
            NODE,
        ],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("systems-add")],
        streams: false,
    },
    Cmd {
        name: "systems remove",
        group: "systems",
        synopsis: "systems remove <system> --yes",
        summary: "Remove [systems.<id>] from klif.toml",
        args: &[SYSTEM],
        flags: &[YES],
        yes: Yes::Always,
        engine: Eng::May,
        outputs: &[o("systems-remove")],
        streams: false,
    },
    Cmd {
        name: "systems rename",
        group: "systems",
        synopsis: "systems rename <system> <label>",
        summary: "Change a System's tab label",
        args: &[SYSTEM, A { name: "label", required: true, repeats: true, desc: "The new label (words are joined)" }],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("systems-update")],
        streams: false,
    },
    Cmd {
        name: "systems move",
        group: "systems",
        synopsis: "systems move <system> <index>",
        summary: "Move a System to a tab position (0-based, among the local Systems)",
        args: &[SYSTEM, a("index", true, "0 = first tab")],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("systems-update")],
        streams: false,
    },
    Cmd {
        name: "systems exclusive",
        group: "systems",
        synopsis: "systems exclusive <system> on|off",
        summary: "Whether the System needs the whole GPU",
        args: &[SYSTEM, a("on|off", true, "on | off")],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("systems-update")],
        streams: false,
    },
    // ---- presets
    Cmd {
        name: "presets list",
        group: "presets",
        synopsis: "presets list [--node N]",
        summary: "Presets with their availability and last bench",
        args: &[],
        flags: &[NODE],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("presets-list")],
        streams: false,
    },
    Cmd {
        name: "presets show",
        group: "presets",
        synopsis: "presets show <id> [--node N]",
        summary: "One preset in full with the command it builds (secrets masked)",
        args: &[a("id", true, "Preset id")],
        flags: &[NODE],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("presets-show")],
        streams: false,
    },
    Cmd {
        name: "presets use",
        group: "presets",
        synopsis: "presets use <system> <id>",
        summary: "Make a System use a preset (applies on the next launch)",
        args: &[SYSTEM, a("id", true, "Preset id")],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("presets-use")],
        streams: false,
    },
    Cmd {
        name: "presets param",
        group: "presets",
        synopsis: "presets param <system> <name> <value>",
        summary: "Choose a param value of the System's preset (applies on the next launch)",
        args: &[SYSTEM, a("name", true, "Param name"), a("value", true, "One of the param's choices")],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("presets-param")],
        streams: false,
    },
    Cmd {
        name: "presets save",
        group: "presets",
        synopsis: "presets save <id> --file F.toml [--use <system>] [--node N]",
        summary: "Create or replace a preset from a TOML file",
        args: &[a("id", true, "Preset id")],
        flags: &[
            f("--file", Some("F.toml"), "A [presets.<id>] table, or the preset's keys at the top level (required)"),
            f("--use", Some("SYSTEM"), "Also make this System use the preset"),
            NODE,
        ],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("presets-save")],
        streams: false,
    },
    Cmd {
        name: "presets set",
        group: "presets",
        synopsis: "presets set <id> key=value... [--node N]",
        summary: "Change keys of a preset (see the keys below); creates it when missing",
        args: &[
            a("id", true, "Preset id"),
            A { name: "key=value", required: true, repeats: true, desc: "key=value, args+=TOKEN, args-=TOKEN, env.NAME=VALUE, env.NAME-, env_remove+=NAME; an empty value unsets" },
        ],
        flags: &[NODE],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("presets-save")],
        streams: false,
    },
    Cmd {
        name: "presets delete",
        group: "presets",
        synopsis: "presets delete <id> --yes [--node N]",
        summary: "Remove [presets.<id>] from klif.toml",
        args: &[a("id", true, "Preset id")],
        flags: &[YES, NODE],
        yes: Yes::Always,
        engine: Eng::May,
        outputs: &[o("presets-delete")],
        streams: false,
    },
    // ---- bench
    Cmd {
        name: "bench",
        group: "bench",
        synopsis: "bench <system> [--runs N] [--prompt N] [--gen N] [--audio F.wav] [--keep-running] [--allow-shared] [--yes]",
        summary: "Measure a System with fixed requests (launches it if needed, stops it afterwards)",
        args: &[SYSTEM],
        flags: &[
            f("--runs", Some("N"), "Number of runs (default 3)"),
            f("--prompt", Some("N"), "Prompt tokens per run (llm; default 512)"),
            f("--gen", Some("N"), "Generated tokens per run (llm; default 128)"),
            f("--audio", Some("F.wav"), "Speech to transcribe (stt; required there)"),
            f("--keep-running", None, "Leave the System running afterwards"),
            f("--allow-shared", None, "Bench although another System shares the GPU (the numbers are then shared-GPU numbers)"),
            f("--yes", None, "Allow bench to launch the System"),
        ],
        yes: Yes::When("the System is not running (bench launches it for the runs)"),
        engine: Eng::May,
        outputs: &[o("bench")],
        streams: false,
    },
    Cmd {
        name: "bench list",
        group: "bench",
        synopsis: "bench list [--preset ID]",
        summary: "Stored bench results",
        args: &[],
        flags: &[f("--preset", Some("ID"), "Only this preset's results")],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("bench-list")],
        streams: false,
    },
    // ---- records
    Cmd {
        name: "records",
        group: "records",
        synopsis: "records [--kind K] [--metric M] [--backend B] [--node N]",
        summary: "Best values per model file and backend (N: a node, or local)",
        args: &[],
        flags: &[
            KIND,
            f("--metric", Some("M"), "decodeTps | prefillTps | ttftS | imageS | ttsRtf | sttRtf | videoS | musicRtf (also decode, prefill, ttft, image, tts, stt, video, music)"),
            f("--backend", Some("B"), "HIP, Vulkan, CUDA, CPU, Metal..."),
            f("--node", Some("N"), "A node, or local for this machine"),
        ],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("records")],
        streams: false,
    },
    Cmd {
        name: "records forget",
        group: "records",
        synopsis: "records forget <key> --yes",
        summary: "Remove a junk record entry (a unique part of its key is enough)",
        args: &[a("key", true, "A record key, or a unique part of it")],
        flags: &[YES],
        yes: Yes::Always,
        engine: Eng::May,
        outputs: &[o("records-forget")],
        streams: false,
    },
    Cmd {
        name: "records history",
        group: "records",
        synopsis: "records history <key> [--metric M]",
        summary: "How a record climbed: every broken record of an entry, oldest first",
        args: &[a("key", true, "A record key, or a unique part of it")],
        flags: &[f("--metric", Some("M"), "Only this metric")],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("records-history")],
        streams: false,
    },
    // ---- models
    Cmd {
        name: "suggest",
        group: "models",
        synopsis: "suggest [--kind K] [--class C]",
        summary: "The model the pool suggests per slot for this machine (estimates)",
        args: &[],
        flags: &[KIND, f("--class", Some("C"), "fast | deep | max (llm only)")],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("suggest")],
        streams: false,
    },
    Cmd {
        name: "models list",
        group: "models",
        synopsis: "models list [--kind K]",
        summary: "The recommended models (embedded list) and whether they are installed",
        args: &[],
        flags: &[KIND],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("models-list")],
        streams: false,
    },
    Cmd {
        name: "models download",
        group: "models",
        synopsis: "models download <rec-id> --yes",
        summary: "Download a recommendation's files from huggingface.co into [paths] models_dir",
        args: &[a("rec-id", true, "A recommendation id (models list)")],
        flags: &[YES],
        yes: Yes::Always,
        engine: Eng::May,
        outputs: &[o("models-download"), O { schema: "download-event", when: Some("progress lines on stderr with --json") }],
        streams: false,
    },
    Cmd {
        name: "models adopt",
        group: "models",
        synopsis: "models adopt <rec-id> [--system S] [--ctx N] [--kv TYPE]",
        summary: "Make a preset from a downloaded model (ctx / KV: this machine's suggestion)",
        args: &[a("rec-id", true, "A recommendation id")],
        flags: &[
            f("--system", Some("S"), "Also make this System use the new preset"),
            f("--ctx", Some("N"), "Context tokens (256 or more)"),
            f("--kv", Some("TYPE"), "KV cache type: f16, q8_0, q4_0..."),
        ],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("models-adopt")],
        streams: false,
    },
    // ---- setup
    Cmd {
        name: "settings",
        group: "setup",
        synopsis: "settings [record-moment on|off]",
        summary: "This machine's display settings ([ui] in klif.toml): show, or set one",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("settings")],
        streams: false,
    },
    Cmd {
        name: "key status",
        group: "setup",
        synopsis: "key status",
        summary: "Whether an API key is set (never printed)",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("key-status")],
        streams: false,
    },
    Cmd {
        name: "key set",
        group: "setup",
        synopsis: "key set",
        summary: "Store the API key (reads one line from stdin)",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("key-update")],
        streams: false,
    },
    Cmd {
        name: "key clear",
        group: "setup",
        synopsis: "key clear --yes",
        summary: "Remove the stored API key",
        args: &[],
        flags: &[YES],
        yes: Yes::Always,
        engine: Eng::Never,
        outputs: &[o("key-update")],
        streams: false,
    },
    Cmd {
        name: "node status",
        group: "setup",
        synopsis: "node status",
        summary: "This machine as a node: listener, rights, token",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("node-status")],
        streams: false,
    },
    Cmd {
        name: "node token",
        group: "setup",
        synopsis: "node token [--create [--yes]]",
        summary: "Whether a node token exists; --create makes one and prints it once",
        args: &[],
        flags: &[
            f("--create", None, "Create the token (printed once)"),
            f("--yes", None, "Replace an existing token (every machine that uses the old one must get the new one)"),
        ],
        yes: Yes::When("--create when a token already exists"),
        engine: Eng::Never,
        outputs: &[o("node-token"), O { schema: "node-token-created", when: Some("with --create") }],
        streams: false,
    },
    Cmd {
        name: "nodes list",
        group: "setup",
        synopsis: "nodes",
        summary: "The remote nodes and their state",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("nodes-list")],
        streams: false,
    },
    Cmd {
        name: "serve",
        group: "setup",
        synopsis: "serve",
        summary: "Run the engine headless (+ the network listener when [node] listen is set)",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::May,
        outputs: &[o("serve")],
        streams: false,
    },
    // ---- agents
    Cmd {
        name: "help",
        group: "agents",
        synopsis: "help [<command>]",
        summary: "This list; with --json the machine-readable catalog of every command",
        args: &[A { name: "command", required: false, repeats: true, desc: "A command (\"records history\") for its flags and outputs" }],
        flags: &[],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("help")],
        streams: false,
    },
    Cmd {
        name: "schema",
        group: "agents",
        synopsis: "schema [<name>]",
        summary: "JSON Schema of every document klif-cli prints (a name or a command)",
        args: &[a("name", false, "A schema name from the list, or a command (\"launch\", \"records history\")")],
        flags: &[],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("schema-list"), O { schema: "json-schema", when: Some("with a name: the JSON Schema itself (plus a schemaVersion keyword)") }],
        streams: false,
    },
    Cmd {
        name: "version",
        group: "agents",
        synopsis: "version",
        summary: "The klif-cli version (also --version)",
        args: &[],
        flags: &[],
        yes: Yes::No,
        engine: Eng::Never,
        outputs: &[o("version")],
        streams: false,
    },
];

/// Flags every command accepts.
pub static GLOBAL_FLAGS: &[F] = &[f("--json", None, "One JSON document on stdout ({schemaVersion: 1, ...}); a failure is {schemaVersion: 1, error: {code, message}}")];

/// The codes of the error document, with the process exit code.
pub static ERROR_CODES: &[(&str, u8, &str)] = &[
    ("usage", 2, "The command line is wrong (unknown command, missing or unknown argument or flag)"),
    ("needs_yes", 2, "A command that changes something was given without --yes"),
    ("not_found", 1, "No such System, preset, node, recommendation or record"),
    ("ambiguous", 1, "The argument names several Systems or records; use the full id"),
    ("refused", 1, "The engine refused (its sentence says why)"),
    ("engine_busy", 1, "Another KLIF process holds the engine and does not answer"),
    ("engine", 1, "The engine could not start"),
    ("control", 1, "The running KLIF engine did not answer on its control channel"),
    ("fault", 1, "The server faulted while a command waited for it"),
    ("stopped", 1, "The server stopped before it was ready"),
    ("timeout", 1, "A wait ended without the condition (the server keeps loading / running)"),
    ("unsupported", 1, "Not available here (a remote System, or a node that is too old)"),
    ("invalid", 1, "A file or klif.toml entry does not parse or validate"),
    ("io", 1, "A file could not be read or written"),
    ("bench", 1, "The bench failed"),
    ("download", 1, "A download failed"),
    ("cancelled", 1, "A download was cancelled"),
    ("error", 1, "Any other failure"),
];

pub const SYSTEM_ARGUMENT: &str = "an id (s1, render-box/s1), a label ignoring case and spaces (system1, \"System 1\"), or the 0.2 names low|medium|high|krea (local s1/s2/s3/cgi)";

/// Where klif.toml is looked up, as `--help` says it on this platform.
#[cfg(windows)]
const CONFIG_LOOKUP: &str = ".local\\klif.toml above the exe or the current folder, else %APPDATA%\\KLIF\\klif.toml";
#[cfg(target_os = "macos")]
const CONFIG_LOOKUP: &str = ".local/klif.toml above the exe or the current folder, else ~/Library/Application Support/KLIF/klif.toml";
#[cfg(not(any(windows, target_os = "macos")))]
const CONFIG_LOOKUP: &str = ".local/klif.toml above the exe or the current folder, else $XDG_CONFIG_HOME/klif/klif.toml";

/// What follows the command list in `--help`.
const USAGE_TAIL: &str = "\
<system>: an id (s1, render-box/s1), a label ignoring case and spaces (system1, \"System 1\"),
          or the 0.2 names low|medium|high|krea (local s1/s2/s3/cgi).
presets set keys: command  args=[\"json\",\"array\"]  args+=TOKEN  args-=TOKEN  env.NAME=VALUE  env.NAME-
          env_remove+=NAME  env_remove-=NAME  cwd  port  host  endpoint  health  model  mmproj  ctx  gpu  kind
          adapter  name  managed  api_key  model_name  quant  backend  device  notes   (empty value = unset)
config:   KLIF_CONFIG=<klif.toml>, else {CONFIG_LOOKUP}.
logs:     KLIF_LOG=info|debug (stderr; trace = KLIF's own records only, dependencies capped at debug).
agents:   klif-cli --json help (every command), klif-cli schema (JSON Schema of every document), klif-cli watch (events
          instead of sleep-and-poll loops). Commands that change something need --yes.
";

/// The `--help` text: the command list from [`COMMANDS`], then the notes.
pub fn usage_text() -> String {
    let mut s = String::from(
        "klif-cli - KLIF (Koksny.com LOCAL INFERENCE FORNICATOR) from a terminal or a coding agent\n\nusage: klif-cli [--json] <command> ...\n",
    );
    let mut group = "";
    for c in COMMANDS {
        if c.group != group {
            s.push('\n');
            group = c.group;
        }
        // The summary sits beside a short synopsis, under a long one.
        let syn = c.synopsis;
        if syn.chars().count() <= 40 {
            s.push_str(&format!("  {syn:<40} {}\n", c.summary));
        } else {
            s.push_str(&format!("  {syn}\n{:43}{}\n", "", c.summary));
        }
    }
    s.push('\n');
    s.push_str(&USAGE_TAIL.replace("{CONFIG_LOOKUP}", CONFIG_LOOKUP));
    s
}

fn command_doc(c: &Cmd) -> CatalogCommand {
    CatalogCommand {
        name: c.name.into(),
        group: c.group.into(),
        synopsis: c.synopsis.into(),
        summary: c.summary.into(),
        args: c
            .args
            .iter()
            .map(|a| CatalogArg { name: a.name.into(), required: a.required, repeats: a.repeats, description: a.desc.into() })
            .collect(),
        flags: c.flags.iter().map(flag_doc).collect(),
        needs_yes: matches!(c.yes, Yes::Always),
        yes_when: match c.yes {
            Yes::When(w) => Some(w.into()),
            _ => None,
        },
        may_start_engine: !matches!(c.engine, Eng::Never),
        engine_when: match c.engine {
            Eng::Only(w) => Some(w.into()),
            _ => None,
        },
        outputs: c.outputs.iter().map(|o| CatalogOutput { schema: o.schema.into(), when: o.when.map(str::to_string) }).collect(),
        streams: c.streams,
    }
}

fn flag_doc(f: &F) -> CatalogFlag {
    CatalogFlag { name: f.name.into(), value: f.value.map(str::to_string), description: f.desc.into() }
}

/// `help --json` (all commands, or the ones named by `only`).
pub fn help_doc(only: Option<&[&Cmd]>) -> HelpDoc {
    HelpDoc {
        usage: usage_text(),
        system_argument: SYSTEM_ARGUMENT.into(),
        global_flags: GLOBAL_FLAGS.iter().map(flag_doc).collect(),
        error_codes: ERROR_CODES.iter().map(|(code, exit, meaning)| CatalogErrorCode { code: (*code).into(), exit_code: *exit, meaning: (*meaning).into() }).collect(),
        commands: match only {
            Some(list) => list.iter().map(|c| command_doc(c)).collect(),
            None => COMMANDS.iter().map(command_doc).collect(),
        },
    }
}

/// The commands a word list names: "records history" is that command, "systems" all of `systems ...`, "records"
/// the command and its subcommands.
pub fn matching(words: &[String]) -> Vec<&'static Cmd> {
    let name = words.iter().map(|w| w.trim().to_ascii_lowercase()).collect::<Vec<_>>().join(" ");
    let prefix = format!("{name} ");
    COMMANDS.iter().filter(|c| c.name == name || c.name.starts_with(&prefix)).collect()
}

/// The commands that print the schema `name`.
pub fn commands_of(schema: &str) -> Vec<&'static str> {
    COMMANDS.iter().filter(|c| c.outputs.iter().any(|o| o.schema == schema)).map(|c| c.name).collect()
}

/// `klif-cli help <command>`: the details of one or more commands.
pub fn command_text(cmds: &[&Cmd]) -> String {
    let mut s = String::new();
    for c in cmds {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(&format!("klif-cli {}\n  {}\n", c.synopsis, c.summary));
        if !c.args.is_empty() {
            s.push_str("\narguments\n");
            for a in c.args {
                s.push_str(&format!("  {:<14} {}{}\n", a.name, if a.required { "" } else { "(optional) " }, a.desc));
            }
        }
        if !c.flags.is_empty() {
            s.push_str("\nflags\n");
            for f in c.flags {
                let name = match f.value {
                    Some(v) => format!("{} {v}", f.name),
                    None => f.name.to_string(),
                };
                s.push_str(&format!("  {name:<22} {}\n", f.desc));
            }
        }
        match &c.yes {
            Yes::Always => s.push_str("\nchanges something: needs --yes\n"),
            Yes::When(w) => s.push_str(&format!("\nneeds --yes when {w}\n")),
            Yes::No => {}
        }
        match &c.engine {
            Eng::Never => s.push_str("engine: not needed\n"),
            Eng::May => s.push_str("engine: with no KLIF running, klif-cli runs one itself for the command\n"),
            Eng::Only(w) => s.push_str(&format!("engine: only for {w}\n")),
        }
        let outs: Vec<String> = c.outputs.iter().map(|o| format!("{}{}", o.schema, o.when.map(|w| format!(" ({w})")).unwrap_or_default())).collect();
        s.push_str(&format!("--json prints: {} (klif-cli schema {})\n", outs.join(", "), c.outputs.first().map(|o| o.schema).unwrap_or("")));
    }
    s
}
