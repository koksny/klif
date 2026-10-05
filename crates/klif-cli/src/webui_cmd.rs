//! `klif-cli webui [on|off] [--host IP] [--port N]`, `webui pair --yes`, `webui cancel`, `webui forget <device>|--all
//! --yes`: klif-webui, the control page for a phone or a browser on the LAN (`[webui]` in klif.toml, off by default).
//! Everything goes through the engine (Action UpdateSettings, PairWebDevice, CancelWebPairing, ForgetWebDevice: all
//! local only, never over the network).
//!
//! Only the KLIF window app (klif.exe) gives its engine the page's files, so an engine that klif-cli runs itself
//! (no window, no `serve`) serves no page: `webui` then reports that in `error`, and `webui pair` is refused. For a
//! page check without the window, a debug build of klif-cli started as `serve` reads the files of a UI build from
//! `KLIF_WEBUI_DIR=<app\ui\dist>` (conn.rs, `dev_web_assets`).
//!
//! `webui` never prints the pairing secret, the pairing URL or the code: they are a credential, and only `webui pair`
//! (which needs `--yes`) shows them.

use crate::args::Args;
use crate::cmds::act;
use crate::conn::{self, Conn};
use crate::out::{date, note, table, val, CliError, CliResult, Out};
use crate::outputs::{WebUiDoc, WebUiPairDoc};
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::{Action, WebUiDevice, WebUiInfo};
use klif_core::klif_common::now_s;
use std::net::IpAddr;

/// What `webui pair` says on stderr, in both output modes.
const PAIR_NOTE: &str = "note: this code and address let a device pair and then control KLIF (launch, stop, restart, presets, params). \
They work once, for 5 minutes. Show them only to the person pairing, and nowhere else.";

pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    // Options first: `--host 192.0.2.10 on` must not mistake the address for the subcommand.
    let host = args.opt("--host")?;
    let port = args.opt("--port")?;
    let yes = args.flag("--yes");
    let all = args.flag("--all");
    let sub = args.next_pos().map(|s| s.trim().to_ascii_lowercase());
    match sub.as_deref() {
        None | Some("on") | Some("off") => {
            let word = sub.as_deref().map_or_else(|| "webui".to_string(), |s| format!("webui {s}"));
            unused(&word, &[("--yes", yes), ("--all", all)])?;
            args.done()?;
            let enabled = match sub.as_deref() {
                Some("on") => Some(true),
                Some("off") => Some(false),
                _ => None,
            };
            let host = host.as_deref().map(host_arg).transpose()?;
            let port = port.as_deref().map(port_arg).transpose()?;
            let conn = conn::connect(loaded)?;
            if enabled.is_some() || host.is_some() || port.is_some() {
                act(&conn, Action::UpdateSettings { record_moment: None, skin: None, webui_enabled: enabled, webui_host: host, webui_port: port })?;
            }
            show(&conn, out, None)
        }
        Some("pair") => {
            unused("webui pair", &[("--host", host.is_some()), ("--port", port.is_some()), ("--all", all)])?;
            args.done()?;
            if !yes {
                return Err(CliError::needs_yes(
                    "webui pair opens a pairing that lets one device control KLIF; the code and address it prints are a credential for 5 minutes. Add --yes when the person who pairs is here.",
                ));
            }
            pair(loaded, out)
        }
        Some("cancel") => {
            unused("webui cancel", &[("--host", host.is_some()), ("--port", port.is_some()), ("--yes", yes), ("--all", all)])?;
            args.done()?;
            let conn = conn::connect(loaded)?;
            act(&conn, Action::CancelWebPairing)?;
            show(&conn, out, Some("The open pairing is closed.".into()))
        }
        Some("forget") => {
            unused("webui forget", &[("--host", host.is_some()), ("--port", port.is_some())])?;
            let device = args.next_pos();
            args.done()?;
            match (&device, all) {
                (None, false) => return Err(CliError::usage("webui forget needs a device id (klif-cli webui lists them) or --all.")),
                (Some(_), true) => return Err(CliError::usage("webui forget takes a device id or --all, not both.")),
                _ => {}
            }
            if !yes {
                return Err(CliError::needs_yes(match &device {
                    Some(d) => format!("webui forget removes the paired device {d}, which has to pair again: add --yes."),
                    None => "webui forget --all removes every paired device, and each has to pair again: add --yes.".into(),
                }));
            }
            forget(loaded, out, device)
        }
        Some(other) => Err(CliError::usage(format!("Unknown webui subcommand \"{other}\" (on, off, pair, cancel, forget).")))
    }
}

/// A usage error for an option the subcommand does not take.
fn unused(what: &str, flags: &[(&str, bool)]) -> CliResult {
    match flags.iter().find(|(_, present)| *present) {
        Some((name, _)) => Err(CliError::usage(format!("{what} does not take {name}."))),
        None => Ok(()),
    }
}

/// `--host`: an IP address ("0.0.0.0" for every network of this machine; an IPv6 address may carry brackets).
fn host_arg(v: &str) -> CliResult<String> {
    let h = v.trim().trim_start_matches('[').trim_end_matches(']');
    match h.parse::<IpAddr>() {
        Ok(_) => Ok(h.to_string()),
        Err(_) => Err(CliError::usage(format!(
            "--host needs an IP address, not \"{}\": 0.0.0.0 for every network of this machine, or the address of one of them.",
            v.trim()
        ))),
    }
}

/// `--port`: 1 to 65535.
fn port_arg(v: &str) -> CliResult<u16> {
    match v.trim().parse::<u16>() {
        Ok(p) if p > 0 => Ok(p),
        _ => Err(CliError::usage(format!("--port needs a port number from 1 to 65535, not \"{}\".", v.trim()))),
    }
}

fn doc_of(i: &WebUiInfo) -> WebUiDoc {
    WebUiDoc {
        enabled: i.enabled,
        host: i.host.clone(),
        port: i.port,
        listening: i.listening,
        urls: i.urls.clone(),
        error: i.error.clone().filter(|e| !e.trim().is_empty()),
        devices: i.devices.clone(),
        pairing_open: i.pairing.is_some(),
        pairing_expires_at: i.pairing.as_ref().map(|p| p.expires_at),
    }
}

/// Print klif-webui as the engine reports it now; `lead` is a sentence in front of the text output.
fn show(conn: &Conn, out: Out, lead: Option<String>) -> CliResult {
    let info = conn.snapshot(None)?.config.webui;
    let doc = doc_of(&info);
    out.doc(val(&doc), || {
        let text = text_of(&doc);
        match &lead {
            Some(l) => format!("{l}\n\n{text}"),
            None => text,
        }
    });
    Ok(())
}

fn text_of(d: &WebUiDoc) -> String {
    let listen = format!("{}:{}", d.host, d.port);
    let mut s = String::new();
    if !d.enabled {
        s.push_str(&format!("klif-webui is off ([webui] enabled = false). klif-cli webui on turns it on; it would listen on {listen}.\n"));
    } else if d.listening {
        s.push_str(&format!("klif-webui is on and serving on {listen}.\n"));
        match d.urls.as_slice() {
            [] => s.push_str("No network address to show: this machine has no route to a network.\n"),
            urls => s.push_str(&format!("Open on a paired device: {}\n", urls.join("  "))),
        }
    } else {
        s.push_str(&format!("klif-webui is on ({listen}) but not serving.\n"));
        if let Some(e) = &d.error {
            s.push_str(&format!("{e}\n"));
        }
    }
    s.push('\n');
    if d.devices.is_empty() {
        s.push_str("No device is paired.\n");
    } else {
        let mut rows = vec![["ID", "DEVICE", "PAIRED", "LAST SEEN"].map(String::from).to_vec()];
        for dev in &d.devices {
            rows.push(vec![dev.id.clone(), dev.name.clone(), date(dev.paired_at), dev.last_seen.map_or_else(|| "-".into(), date)]);
        }
        s.push_str(&table(&rows));
    }
    match d.pairing_expires_at {
        Some(at) => s.push_str(&format!("\nA pairing is open until {} (klif-cli webui cancel closes it).\n", date(at))),
        None if d.enabled && d.listening => s.push_str("\nNo pairing is open (klif-cli webui pair --yes opens one).\n"),
        None => {}
    }
    s
}

/// `pair`: open a pairing and print its credential.
fn pair(loaded: &LoadedConfig, out: Out) -> CliResult {
    let conn = conn::connect(loaded)?;
    let info = conn.snapshot(None)?.config.webui;
    if !info.enabled {
        return Err(CliError::new("refused", "klif-webui is off: klif-cli webui on turns it on first."));
    }
    if !info.listening {
        return Err(CliError::new(
            "refused",
            match info.error.as_deref().filter(|e| !e.trim().is_empty()) {
                Some(e) => format!("klif-webui is not serving, so no device can pair: {e}"),
                None => "klif-webui is not serving yet, so no device can pair; try again in a few seconds.".into(),
            },
        ));
    }
    act(&conn, Action::PairWebDevice)?;
    let info = conn.snapshot(None)?.config.webui;
    let Some(p) = info.pairing else {
        return Err(CliError::new("error", "The engine opened no pairing; run webui pair again."));
    };
    note(PAIR_NOTE);
    let doc = WebUiPairDoc { code: p.code.clone(), url: Some(p.url.clone()).filter(|u| !u.is_empty()), expires_at: p.expires_at };
    out.doc(val(&doc), || {
        let code = if p.code.len() == 6 && p.code.is_ascii() { format!("{} {}", &p.code[..3], &p.code[3..]) } else { p.code.clone() };
        let mins = ((p.expires_at - now_s()) / 60.0).round().max(0.0);
        let mut s = format!("Pairing code  {code}\n");
        match &doc.url {
            Some(u) => s.push_str(&format!("Address       {u}\n")),
            None => s.push_str("Address       none: this machine has no route to a network, so only the code exists\n"),
        }
        s.push_str(&format!("Valid until   {} (about {mins} min); one device pairs, then it closes", date(p.expires_at)));
        s
    });
    Ok(())
}

/// `forget <device>` or `forget --all`.
fn forget(loaded: &LoadedConfig, out: Out, device: Option<String>) -> CliResult {
    let conn = conn::connect(loaded)?;
    let devices: Vec<WebUiDevice> = conn.snapshot(None)?.config.webui.devices;
    let (target, lead) = match &device {
        Some(d) => {
            let want = d.trim().to_ascii_lowercase();
            let Some(found) = devices.iter().find(|x| x.id.to_ascii_lowercase() == want) else {
                let known = if devices.is_empty() {
                    "No device is paired.".to_string()
                } else {
                    format!("Paired: {}.", devices.iter().map(|x| format!("{} ({})", x.id, x.name)).collect::<Vec<_>>().join(", "))
                };
                return Err(CliError::new("not_found", format!("There is no paired device \"{}\". {known}", d.trim())));
            };
            (Some(found.id.clone()), format!("Removed {} ({}); it has to pair again.", found.id, found.name))
        }
        None => (None, match devices.len() {
            0 => "No device was paired.".to_string(),
            1 => "Removed the paired device; it has to pair again.".to_string(),
            n => format!("Removed all {n} paired devices; each has to pair again."),
        }),
    };
    act(&conn, Action::ForgetWebDevice { device: target })?;
    show(&conn, out, Some(lead))
}
