//! `klif-cli settings [record-moment on|off]`: this machine's display settings (`[ui]` in klif.toml), shown or set
//! through the engine (Action UpdateSettings, local only).

use crate::args::Args;
use crate::cmds::act;
use crate::conn;
use crate::out::{val, CliError, CliResult, Out};
use crate::outputs::SettingsDoc;
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::Action;

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let name = args.next_pos();
    let value = args.next_pos();
    args.done()?;
    let conn = conn::connect(loaded)?;
    if let Some(name) = name {
        if name.trim().to_ascii_lowercase().replace('_', "-") != "record-moment" {
            return Err(CliError::usage(format!("Unknown setting \"{name}\" (record-moment).")));
        }
        let on = match value.as_deref().map(|v| v.trim().to_ascii_lowercase()) {
            Some(v) if ["on", "true", "yes", "1"].contains(&v.as_str()) => true,
            Some(v) if ["off", "false", "no", "0"].contains(&v.as_str()) => false,
            _ => return Err(CliError::usage("settings record-moment needs on or off.")),
        };
        act(&conn, Action::UpdateSettings { record_moment: Some(on) })?;
    }
    let vm = conn.snapshot(None)?;
    let doc = SettingsDoc { record_moment: vm.config.record_moment };
    out.doc(val(&doc), || format!("record-moment  {}   (the \"new record\" moment over the skin; [ui] record_moment)", if doc.record_moment { "on" } else { "off" }));
    Ok(())
}
