//! The documents klif-cli prints, as types: one struct per command output (plus the line events of `watch`, `logs
//! --follow` and `models download`). They are the source of `klif-cli schema` (JSON Schema, serialize contract,
//! `schemaVersion` added to every root) and, for the commands that print them, of the documents themselves, so a
//! schema cannot drift from the output. The big view-model types (`System`, `PresetInfo`, `RecordEntry`...) live in
//! `klif_common::vm`; mirrored 1:1 in `app/ui/src/lib/model/types.ts`.
//!
//! Keys are camelCase; an optional field is omitted when it is `None` unless it is documented as `null`.

use klif_core::bench::BenchRecord;
use klif_core::klif_common::vm::{
    ApiKeyInfo, BenchSummary, DownloadInfo, DownloadState, Ended, HardwareInfo, Issue, NodeView, ParamView, PresetDetail, PresetInfo,
    RecommendationInfo, RecordEntry, RecordEvent, RecordMetric, Suggestion, System, SystemId, SystemKind, SystemStatus,
};
use klif_core::wire::{StatusJson, StatusSystem};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

/// The compact JSON of one System after an action (`launch`, `select`, `dismiss`...).
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SysBrief {
    pub id: SystemId,
    pub label: String,
    pub kind: SystemKind,
    pub status: SystemStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl SysBrief {
    pub fn of(s: &System) -> SysBrief {
        SysBrief {
            id: s.id.clone(),
            label: s.label.clone(),
            kind: s.kind,
            status: s.status,
            reason: s.reason.clone(),
            preset: s.preset.clone(),
            endpoint: s.endpoint.clone(),
        }
    }
}

// ------------------------------------------------------------------------------------------ status, diag

/// `status`: see [`StatusJson`] (`klif_core::wire`); `status <system>` prints one [`StatusSystem`].
pub type StatusDoc = StatusJson;
pub type StatusSystemDoc = StatusSystem;

/// `diag`: what klif-cli itself sees, and the engine's own report (free-form, for a human or a bug report; no secrets).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagDoc {
    pub cli: DiagCli,
    /// The engine's report: GPUs, versions, paths, listeners. Its keys may change between versions.
    pub engine: Value,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagCli {
    pub version: String,
    pub exe: Option<String>,
    /// "connected" (to the running KLIF) or "in-process" (klif-cli runs the engine itself).
    pub engine: String,
    pub engine_pid: Option<u32>,
    pub config: DiagConfig,
    pub config_issues: Vec<Issue>,
    pub smbios_ram_type: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagConfig {
    pub path: String,
    /// "env" (KLIF_CONFIG), "exeDir", "cwd" or "default".
    pub origin: String,
    pub exists: bool,
    pub state_dir: String,
    pub data_dir: String,
}

// ------------------------------------------------------------------------------------- hardware, suggest

/// `hardware`: this machine's inventory (or a node's, with `node`).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HardwareDoc {
    #[serde(flatten)]
    pub hardware: HardwareInfo,
    /// The node the inventory belongs to (only with `--node`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

// -------------------------------------------------------------------------------- plan and System actions

/// `plan <system>`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanDoc {
    pub system: SystemId,
    pub label: String,
    pub preset: Option<String>,
    /// False when a launch would be refused (`refused` says why).
    pub launchable: bool,
    /// The exact command (secrets masked), with its issues.
    pub command: klif_core::klif_common::vm::CommandView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

/// `select <system>`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectDoc {
    pub selected: Option<SystemId>,
    pub system: Option<SysBrief>,
}

/// `launch <system>`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LaunchDoc {
    pub launched: bool,
    pub system: SysBrief,
}

/// `restart <system>`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestartDoc {
    pub restarted: bool,
    pub system: SysBrief,
}

/// `stop <system>` / `stop --all`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StopDoc {
    pub stopped: Vec<SysBrief>,
}

/// `dismiss <system>`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DismissDoc {
    pub system: Option<SysBrief>,
}

// --------------------------------------------------------------------------------------- systems, nodes

/// `systems list`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemsListDoc {
    pub selected: Option<SystemId>,
    pub systems: Vec<System>,
}

/// `systems add`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemsAddDoc {
    /// `null` when the new System was not visible yet.
    pub added: Option<SysBrief>,
}

/// `systems remove`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemsRemoveDoc {
    pub removed: SystemId,
}

/// `systems rename | move | exclusive`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SystemsUpdateDoc {
    pub system: Option<SysBrief>,
    /// 0-based tab index among the local Systems.
    pub index: Option<usize>,
    pub exclusive: Option<bool>,
}

/// `nodes list`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodesListDoc {
    pub nodes: Vec<NodeView>,
}

/// `serve` (printed once, when the engine runs).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServeDoc {
    pub serving: bool,
    pub pid: u32,
    pub config: Option<String>,
    pub state_dir: String,
    /// The node listener's address, when `[node] listen` is set.
    pub network: Option<String>,
}

// ----------------------------------------------------------------------------------------------- presets

/// `presets list`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresetsListDoc {
    pub node: Option<String>,
    pub presets: Vec<PresetInfo>,
}

/// `presets show`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresetsShowDoc {
    pub node: Option<String>,
    pub preset: PresetDetail,
}

/// `presets use`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresetsUseDoc {
    pub system: Option<SysBrief>,
}

/// `presets param`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresetsParamDoc {
    pub system: SystemId,
    pub params: Vec<ParamView>,
}

/// `presets save | set`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresetsSaveDoc {
    pub saved: String,
    pub node: Option<String>,
    pub used_for: Option<String>,
    pub warnings: Vec<String>,
    /// The preset as stored (`null` when it could not be read back).
    pub preset: Option<PresetDetail>,
}

/// `presets delete`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresetsDeleteDoc {
    pub deleted: String,
    pub node: Option<String>,
}

// ------------------------------------------------------------------------------------------------ bench

/// `bench <system>`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BenchDoc {
    /// Where the result was stored.
    pub file: String,
    pub summary: BenchSummary,
    pub record: BenchRecord,
}

/// `bench list`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BenchListDoc {
    pub data_dir: String,
    pub presets: Vec<BenchPresetFile>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BenchPresetFile {
    pub preset_id: String,
    pub file: String,
    pub records: Vec<BenchRecord>,
}

// ---------------------------------------------------------------------------------------------- records

/// `records`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordsDoc {
    pub records: Vec<RecordEntry>,
}

/// `records forget`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordsForgetDoc {
    pub forgotten: String,
}

/// `records history <key>`: the climb of one entry, oldest first. Every point is a record that was broken
/// (`old` = the previous best, absent for the first value).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecordsHistoryDoc {
    pub key: String,
    pub model: String,
    pub quant: Option<String>,
    pub backend: String,
    pub machine: String,
    /// The node that keeps the history, when the entry is a node's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// The metric asked for with `--metric`, else every metric of the entry.
    pub metric: Option<RecordMetric>,
    pub points: Vec<RecordEvent>,
}

// ------------------------------------------------------------------------------------------------ models

/// `models list`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelsListDoc {
    pub recommendations: Vec<RecommendationInfo>,
    pub models_dir: Option<String>,
    pub disclaimer: String,
}

/// `models download`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDownloadDoc {
    pub downloaded: String,
    pub files: Vec<DownloadInfo>,
}

/// `models adopt`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelsAdoptDoc {
    pub adopted: String,
    /// The presets the adoption created.
    pub presets: Vec<String>,
    pub system: Option<SysBrief>,
}

/// `suggest`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuggestDoc {
    pub hardware: SuggestHardware,
    pub suggestions: Vec<Suggestion>,
    pub disclaimer: String,
}

/// The machine facts the suggestions were made for.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuggestHardware {
    pub gpus: Vec<SuggestGpu>,
    pub cpu: Option<String>,
    #[serde(rename = "vramPoolGiB")]
    pub vram_pool_gib: f64,
    #[serde(rename = "largestGpuGiB")]
    pub largest_gpu_gib: f64,
    #[serde(rename = "ramTotalGiB")]
    pub ram_total_gib: f64,
    pub unified: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuggestGpu {
    pub id: String,
    pub name: String,
    #[serde(rename = "vramGiB")]
    pub vram_gib: Option<f64>,
    pub integrated: bool,
}

// ------------------------------------------------------------------------------------------- key, node

/// `key status`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatusDoc {
    pub api_key: ApiKeyInfo,
    pub file: Option<String>,
}

/// `key set | clear`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeyUpdateDoc {
    pub api_key: ApiKeyInfo,
}

/// `node status`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeStatusDoc {
    pub node: NodeStatus,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeStatus {
    pub configured: bool,
    pub name: Option<String>,
    pub listen: Option<String>,
    /// Rights besides view: "launch", "edit".
    pub allow: Vec<String>,
    pub token_set: bool,
    pub token_file: String,
}

/// `node token` (without `--create`).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeTokenInfoDoc {
    pub token_set: bool,
    pub token_file: String,
}

/// `node token --create`: the only time the token is shown.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeTokenCreatedDoc {
    pub token: String,
    pub token_file: String,
}

// ----------------------------------------------------------------------------- help, schema, version, logs

/// `--version`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionDoc {
    pub version: String,
}

/// `help --json`: the command catalog.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HelpDoc {
    /// The text `klif-cli --help` prints.
    pub usage: String,
    /// What the `<system>` argument accepts.
    pub system_argument: String,
    /// Flags every command accepts.
    pub global_flags: Vec<CatalogFlag>,
    /// The codes of the `error` document.
    pub error_codes: Vec<CatalogErrorCode>,
    pub commands: Vec<CatalogCommand>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCommand {
    /// The command as typed, subcommand included: "status", "records history".
    pub name: String,
    pub group: String,
    /// `klif-cli <synopsis>`.
    pub synopsis: String,
    pub summary: String,
    pub args: Vec<CatalogArg>,
    pub flags: Vec<CatalogFlag>,
    /// True when the command refuses to run without `--yes`.
    pub needs_yes: bool,
    /// When `--yes` is needed although `needsYes` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yes_when: Option<String>,
    /// True when the command talks to the engine: with no KLIF running klif-cli then runs one itself for the
    /// duration of the command (model servers it launched keep running afterwards).
    pub may_start_engine: bool,
    /// Which part of the command needs the engine when not all of it does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_when: Option<String>,
    /// What the command prints with `--json`.
    pub outputs: Vec<CatalogOutput>,
    /// `klif-cli --json <name>` prints one JSON document (false: JSON lines until it ends, see `outputs`).
    pub streams: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogArg {
    pub name: String,
    pub required: bool,
    /// True when several may follow.
    pub repeats: bool,
    pub description: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFlag {
    /// "--wait".
    pub name: String,
    /// The value's placeholder ("N", "S"); absent for a switch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub description: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogOutput {
    /// A name `klif-cli schema` knows.
    pub schema: String,
    /// When this document is printed instead of the first one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogErrorCode {
    pub code: String,
    pub exit_code: u8,
    pub meaning: String,
}

/// `schema` without a name.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaListDoc {
    pub schemas: Vec<SchemaInfo>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInfo {
    pub name: String,
    pub summary: String,
    /// The commands that print it.
    pub commands: Vec<String>,
}

/// `logs <system>` without `--follow`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogsDoc {
    pub system: SystemId,
    pub label: String,
    pub status: SystemStatus,
    /// The engine's console of the System, oldest first (at most the last 200 lines KLIF keeps; secrets redacted).
    pub lines: Vec<String>,
}

/// One line of `logs --follow --json`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LogEvent {
    /// A new console line.
    Log {
        /// Epoch seconds when klif-cli saw the line (the console lines carry no time).
        at: f64,
        system: SystemId,
        line: String,
    },
    /// The System stopped (last line of the stream).
    End { at: f64, system: SystemId, status: SystemStatus },
}

// ------------------------------------------------------------------------------------------------ watch

/// One line of `watch --json`. `at` is epoch seconds: when klif-cli saw the change (a `record` event: when the
/// record was broken).
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WatchEvent {
    /// The first line: what the stream starts from. Not subject to `--types`.
    Watching {
        at: f64,
        systems: Vec<WatchSystem>,
        /// Event types that are printed.
        types: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        until: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_s: Option<f64>,
    },
    /// A System's status changed.
    Status {
        at: f64,
        system: SystemId,
        label: String,
        /// `null`: the System appeared while watching.
        from: Option<SystemStatus>,
        to: SystemStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A session started (a launch, a restart, or a session KLIF adopted).
    Launch {
        at: f64,
        system: SystemId,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        endpoint: Option<String>,
    },
    /// A session ended (stopped, or ended by a fault that was dismissed).
    Stop {
        at: f64,
        system: SystemId,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        ended: Option<Ended>,
        #[serde(skip_serializing_if = "Option::is_none")]
        uptime_s: Option<f64>,
    },
    /// A session faulted: the server exited or logged a fatal error.
    Fault {
        at: f64,
        system: SystemId,
        label: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i64>,
        /// The last log lines (at most 12).
        log_tail: Vec<String>,
    },
    /// A record was broken (`klif-cli records` shows the entry).
    Record {
        at: f64,
        key: String,
        metric: Option<RecordMetric>,
        /// The previous best; absent for the first value.
        #[serde(skip_serializing_if = "Option::is_none")]
        old: Option<f64>,
        new: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        quant: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        backend: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        machine: Option<String>,
    },
    /// A model file download changed state, or moved on (at most once a second per file).
    Download {
        at: f64,
        /// The recommendation id.
        id: String,
        file: String,
        state: DownloadState,
        done_bytes: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        total_bytes: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// `--until` was reached (the last line; exit code 0).
    Until { at: f64, system: SystemId, status: SystemStatus, elapsed_s: f64 },
}

/// One System in the `watching` line.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WatchSystem {
    pub id: SystemId,
    pub label: String,
    pub status: SystemStatus,
}

// ------------------------------------------------------------------------------------ download progress

/// One line of the stderr stream of `models download --json`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DownloadEvent {
    /// The download begins.
    Start {
        at: f64,
        id: String,
        name: String,
        /// Where the files come from ("huggingface.co/org/name").
        source: String,
        into: String,
        total_bytes: Option<u64>,
    },
    /// A file's progress or state (at most once a second per file, and at every state change).
    Progress {
        at: f64,
        id: String,
        file: String,
        done_bytes: u64,
        total_bytes: Option<u64>,
        state: DownloadState,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A remark (for instance that klif-cli itself runs the engine).
    Note { at: f64, message: String },
}

// -------------------------------------------------------------------------------------------------- error

/// The failure document (`--json`, exit code 1 or 2): `{schemaVersion, error: {code, message}}`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDoc {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    /// One of the codes `help --json` lists under `errorCodes`.
    pub code: String,
    /// One sentence meant to be shown to the user.
    pub message: String,
}
