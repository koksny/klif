//! `klif-cli settings [record-moment on|off | skin <id>]`: this machine's display settings (`[ui]` in klif.toml),
//! shown or set through the engine (Action UpdateSettings, local only). klif-webui has its own command (`webui`).

use crate::args::Args;
use crate::cmds::act;
use crate::conn;
use crate::out::{table, val, CliError, CliResult, Out};
use crate::outputs::SettingsDoc;
use klif_core::klif_catalog::store::valid_skin_id;
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::Action;

/// The skins the window ships with, for the usage sentence (the window ignores an id it does not have).
const SKINS: &str = "cliff, silicon, instrument, phosphor, decode, loom, ether, rings, spirit";

fn update(record_moment: Option<bool>, skin: Option<String>) -> Action {
    Action::UpdateSettings { record_moment, skin, webui_enabled: None, webui_host: None, webui_port: None }
}

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let name = args.next_pos();
    let value = args.next_pos();
    args.done()?;
    // Check the whole command line before an engine is started or anything is written.
    let change = match name.as_deref().map(|n| n.trim().to_ascii_lowercase().replace('_', "-")) {
        None => None,
        Some(n) if n == "record-moment" => {
            let on = match value.as_deref().map(|v| v.trim().to_ascii_lowercase()) {
                Some(v) if ["on", "true", "yes", "1"].contains(&v.as_str()) => true,
                Some(v) if ["off", "false", "no", "0"].contains(&v.as_str()) => false,
                _ => return Err(CliError::usage("settings record-moment needs on or off.")),
            };
            Some(update(Some(on), None))
        }
        Some(n) if n == "skin" => {
            let id = value.as_deref().map(str::trim).unwrap_or_default();
            if id.is_empty() {
                return Err(CliError::usage(format!("settings skin needs a skin id ({SKINS}).")));
            }
            if !valid_skin_id(id) {
                return Err(CliError::usage(format!("\"{id}\" is not a skin id (lowercase letters, digits, - and _; the skins are {SKINS}).")));
            }
            Some(update(None, Some(id.to_string())))
        }
        Some(_) => return Err(CliError::usage(format!("Unknown setting \"{}\" (record-moment, skin).", name.unwrap_or_default()))),
    };
    let conn = conn::connect(loaded)?;
    if let Some(action) = change {
        act(&conn, action)?;
    }
    let vm = conn.snapshot(None)?;
    let doc = SettingsDoc { record_moment: vm.config.record_moment, skin: vm.config.skin.clone() };
    out.doc(val(&doc), || {
        table(&[
            vec![
                "record-moment".into(),
                if doc.record_moment { "on" } else { "off" }.into(),
                "(the \"new record\" moment over the skin; [ui] record_moment)".into(),
            ],
            vec!["skin".into(), doc.skin.clone().unwrap_or_else(|| "-".into()), "([ui] skin; the window writes it, klif-webui follows it)".into()],
        ])
    });
    Ok(())
}
