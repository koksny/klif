//! `schema [<name>]`: JSON Schema (draft 2020-12, the serialize side: what klif-cli prints) of every document. The
//! schemas are derived (schemars) from the types in `outputs.rs` and `klif_common::vm`, so they follow the serde
//! renames; `schemaVersion` is added to every root because `--json` adds it to every document.

use crate::args::Args;
use crate::catalog;
use crate::outputs::*;
use crate::out::{CliError, CliResult, Out};
use schemars::generate::SchemaSettings;
use schemars::JsonSchema;
use serde_json::{json, Value};

struct Entry {
    name: &'static str,
    summary: &'static str,
    make: fn() -> Value,
}

macro_rules! entry {
    ($name:literal, $summary:literal, $ty:ty) => {
        Entry { name: $name, summary: $summary, make: schema_of::<$ty> }
    };
}

fn schema_of<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::draft2020_12().for_serialize().into_generator();
    serde_json::to_value(generator.into_root_schema_for::<T>()).unwrap_or(Value::Null)
}

/// JSON Schema's own meta-schema, for the `schema <name>` output.
fn json_schema_meta() -> Value {
    json!({ "$schema": "https://json-schema.org/draft/2020-12/schema", "$ref": "https://json-schema.org/draft/2020-12/schema" })
}

// A name per document; commands that print the same document share it (see catalog.rs for who prints what).
static REGISTRY: &[Entry] = &[
    entry!("status", "Systems, GPUs and nodes", StatusDoc),
    entry!("status-system", "One System of the status", StatusSystemDoc),
    entry!("diag", "Diagnostics of klif-cli and the engine", DiagDoc),
    entry!("hardware", "GPUs, CPU and RAM with peak FP32 TFLOPS", HardwareDoc),
    entry!("suggest", "The suggested model per slot for this machine", SuggestDoc),
    entry!("plan", "The command a System would launch", PlanDoc),
    entry!("select", "The selected System", SelectDoc),
    entry!("launch", "A launched System", LaunchDoc),
    entry!("restart", "A restarted System", RestartDoc),
    entry!("stop", "The Systems that were stopped", StopDoc),
    entry!("dismiss", "A System after dismissing its fault", DismissDoc),
    entry!("settings", "This machine's display settings", SettingsDoc),
    entry!("webui", "klif-webui: settings, state, paired devices", WebUiDoc),
    entry!("webui-pair", "An open klif-webui pairing: code and address (a credential)", WebUiPairDoc),
    entry!("logs", "A System's console lines", LogsDoc),
    entry!("log-event", "One line of logs --follow --json", LogEvent),
    entry!("watch-event", "One line of watch --json", WatchEvent),
    entry!("systems-list", "Every System", SystemsListDoc),
    entry!("systems-add", "The System that was added", SystemsAddDoc),
    entry!("systems-remove", "The System that was removed", SystemsRemoveDoc),
    entry!("systems-update", "A System after rename, move or exclusive", SystemsUpdateDoc),
    entry!("presets-list", "Presets with availability and bench", PresetsListDoc),
    entry!("presets-show", "One preset in full", PresetsShowDoc),
    entry!("presets-use", "A System after presets use", PresetsUseDoc),
    entry!("presets-param", "A System's params after presets param", PresetsParamDoc),
    entry!("presets-save", "A preset after presets save or set", PresetsSaveDoc),
    entry!("presets-delete", "The preset that was deleted", PresetsDeleteDoc),
    entry!("bench", "A bench result", BenchDoc),
    entry!("bench-list", "Stored bench results", BenchListDoc),
    entry!("records", "Best values per model file and backend", RecordsDoc),
    entry!("records-forget", "The record entry that was forgotten", RecordsForgetDoc),
    entry!("records-history", "The climb of one record entry", RecordsHistoryDoc),
    entry!("models-list", "The recommended models", ModelsListDoc),
    entry!("models-download", "The files that were downloaded", ModelsDownloadDoc),
    entry!("models-adopt", "The preset made from a recommendation", ModelsAdoptDoc),
    entry!("download-event", "One stderr line of models download --json", DownloadEvent),
    entry!("key-status", "Whether an API key is set", KeyStatusDoc),
    entry!("key-update", "The API key info after key set or clear", KeyUpdateDoc),
    entry!("node-status", "This machine as a node", NodeStatusDoc),
    entry!("node-token", "Whether a node token exists", NodeTokenInfoDoc),
    entry!("node-token-created", "A new node token (shown once)", NodeTokenCreatedDoc),
    entry!("nodes-list", "The remote nodes", NodesListDoc),
    entry!("serve", "The engine started by serve", ServeDoc),
    entry!("help", "The command catalog", HelpDoc),
    entry!("schema-list", "The schema names", SchemaListDoc),
    entry!("version", "The klif-cli version", VersionDoc),
    entry!("error", "Every failure with --json (exit code 1 or 2)", ErrorDoc),
    Entry { name: "json-schema", summary: "JSON Schema itself (what schema <name> prints)", make: json_schema_meta },
];

/// Add `schemaVersion` (always 1) to a document schema; for a union (`oneOf` / `anyOf`) to every branch.
fn add_schema_version(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else { return };
    let union_key = ["oneOf", "anyOf"].into_iter().find(|k| obj.get(*k).is_some_and(Value::is_array));
    if let Some(k) = union_key {
        if let Some(Value::Array(branches)) = obj.get_mut(k) {
            for b in branches {
                add_schema_version(b);
            }
        }
        return;
    }
    obj.entry("type").or_insert_with(|| json!("object"));
    let props = obj.entry("properties").or_insert_with(|| json!({}));
    if let Some(p) = props.as_object_mut() {
        p.insert("schemaVersion".into(), json!({ "const": crate::out::SCHEMA_VERSION, "type": "integer" }));
    }
    let req = obj.entry("required").or_insert_with(|| json!([]));
    if let Some(r) = req.as_array_mut() {
        if !r.iter().any(|x| x == "schemaVersion") {
            r.insert(0, json!("schemaVersion"));
        }
    }
}

/// The error document's `code` is one of the catalog's codes.
fn patch_error(schema: &mut Value) {
    let codes: Vec<Value> = catalog::ERROR_CODES.iter().map(|(c, _, _)| json!(c)).collect();
    if let Some(code) = schema.pointer_mut("/$defs/ErrorBody/properties/code").and_then(Value::as_object_mut) {
        code.insert("enum".into(), Value::Array(codes));
    }
}

fn build(e: &Entry) -> Value {
    let mut v = (e.make)();
    if e.name != "json-schema" {
        add_schema_version(&mut v);
    }
    if e.name == "error" {
        patch_error(&mut v);
    }
    if let Some(o) = v.as_object_mut() {
        o.insert("title".into(), json!(e.name));
        o.entry("description").or_insert_with(|| json!(e.summary));
    }
    v
}

/// `schema [<name>]`.
pub fn run(mut args: Args, out: Out) -> CliResult {
    let name = args.next_pos();
    args.done()?;
    let Some(name) = name else {
        let schemas: Vec<SchemaInfo> = REGISTRY
            .iter()
            .map(|e| SchemaInfo { name: e.name.into(), summary: e.summary.into(), commands: catalog::commands_of(e.name).into_iter().map(String::from).collect() })
            .collect();
        out.doc(crate::out::val(&SchemaListDoc { schemas }), || {
            let mut rows = vec![["SCHEMA", "COMMANDS", "DOCUMENT"].map(String::from).to_vec()];
            for e in REGISTRY {
                rows.push(vec![e.name.into(), catalog::commands_of(e.name).join(", "), e.summary.into()]);
            }
            let mut s = crate::out::table(&rows);
            s.push_str("\nklif-cli schema <name> prints the JSON Schema (draft 2020-12); a command name works too (schema launch).\n");
            s
        });
        return Ok(());
    };
    let wanted = name.trim().to_ascii_lowercase().replace(' ', "-");
    let entry = REGISTRY.iter().find(|e| e.name == wanted).or_else(|| {
        // A command ("launch", "records history"): its first document.
        let words: Vec<String> = name.split_whitespace().map(String::from).collect();
        let cmds = catalog::matching(&words);
        let first = cmds.first().filter(|c| c.name == words.join(" ").to_ascii_lowercase()).or(cmds.first().filter(|_| cmds.len() == 1))?;
        let schema = first.outputs.first()?.schema;
        REGISTRY.iter().find(|e| e.name == schema)
    });
    let Some(entry) = entry else {
        return Err(CliError::new("not_found", format!("There is no schema \"{name}\" (klif-cli schema lists them).")));
    };
    let mut v = build(entry);
    if let Some(o) = v.as_object_mut() {
        // The schema is the document; it carries schemaVersion like every other output.
        o.insert("schemaVersion".into(), json!(crate::out::SCHEMA_VERSION));
    }
    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    Ok(())
}
