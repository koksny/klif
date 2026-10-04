//! `key status | set | clear` (the KLIF API key; never printed) and `node status | token [--create]` (this
//! machine as a node; the token is printed once, when it is created). Neither needs an engine: the engine
//! re-reads api-key.txt / node-token.txt when they change.

use crate::args::Args;
use crate::out::{note, val, CliError, CliResult, Out};
use crate::outputs::*;
use klif_core::keys;
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::Secret;
use klif_core::nodes::node_token;
use std::io::{BufRead, IsTerminal};

pub fn key(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let sub = args.next_pos().unwrap_or_else(|| "status".into());
    let cfg = &loaded.cfg;
    match sub.as_str() {
        "status" => {
            args.done()?;
            let info = keys::info(cfg);
            let file = (info.source == "file").then(|| keys::key_file(cfg).display().to_string());
            out.doc(val(&KeyStatusDoc { api_key: info.clone(), file: file.clone() }), || {
                format!(
                    "API key: {} (source: {}{})",
                    if info.set { "set" } else { "not set" },
                    info.source,
                    file.as_deref().map(|f| format!(", {f}")).unwrap_or_default()
                )
            });
            Ok(())
        }
        "set" => {
            args.done()?;
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                note("Paste the API key and press Enter (it is not shown again):");
            }
            let mut line = String::new();
            stdin.lock().read_line(&mut line).map_err(|e| CliError::new("io", format!("stdin could not be read: {e}")))?;
            let key = Secret::new(line.trim_end_matches(['\r', '\n'])).ok_or_else(|| CliError::usage("No key on stdin (one line expected)."))?;
            drop(line);
            keys::store(cfg, Some(&key)).map_err(|e| CliError::new("refused", format!("{e:#}")))?;
            let info = keys::info(cfg);
            out.doc(val(&KeyUpdateDoc { api_key: info }), || "API key stored (the value is not shown). KLIF-launched servers get it on their next launch.".into());
            Ok(())
        }
        "clear" => {
            let yes = args.flag("--yes");
            args.done()?;
            if !yes {
                return Err(CliError::needs_yes("key clear removes the stored API key: add --yes."));
            }
            keys::store(cfg, None).map_err(|e| CliError::new("refused", format!("{e:#}")))?;
            let info = keys::info(cfg);
            out.doc(val(&KeyUpdateDoc { api_key: info }), || "API key removed.".into());
            Ok(())
        }
        other => Err(CliError::usage(format!("Unknown key subcommand \"{other}\" (status, set, clear)."))),
    }
}

pub fn node(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let sub = args.next_pos().unwrap_or_else(|| "status".into());
    let cfg = &loaded.cfg;
    let token_file = cfg.state_path(node_token::FILE);
    match sub.as_str() {
        "status" => {
            args.done()?;
            let node = cfg.node.as_ref();
            let token_set = node_token::load(cfg).is_some();
            let listen = node.and_then(|n| n.listen_addr());
            let allow: Vec<&str> = node.map(|n| n.allow.iter().map(|r| r.as_str()).collect()).unwrap_or_default();
            let name = node.and_then(|n| n.name.clone());
            let doc = NodeStatusDoc {
                node: NodeStatus {
                    configured: node.is_some(),
                    name: name.clone(),
                    listen: listen.clone(),
                    allow: allow.iter().map(|a| a.to_string()).collect(),
                    token_set,
                    token_file: token_file.display().to_string(),
                },
            };
            out.doc(val(&doc), || {
                let mut s = match &listen {
                    Some(l) => format!("This machine listens for other KLIF nodes on {l} (rights: view{}).", allow.iter().map(|a| format!(", {a}")).collect::<String>()),
                    None => "This machine does not listen for other KLIF nodes ([node] listen is not set).".into(),
                };
                s.push_str(&format!(
                    "\nNode token: {} ({})",
                    if token_set { "set" } else { "not set; klif-cli node token --create makes one" },
                    token_file.display()
                ));
                if listen.is_some() && !token_set {
                    s.push_str("\nThe listener stays off until a token exists.");
                }
                if allow.contains(&"edit") {
                    s.push_str("\n\"edit\" lets other nodes run arbitrary commands on this machine.");
                }
                s
            });
            Ok(())
        }
        "token" => {
            let create = args.flag("--create");
            let yes = args.flag("--yes");
            args.done()?;
            let exists = node_token::load(cfg).is_some();
            if !create {
                out.doc(val(&NodeTokenInfoDoc { token_set: exists, token_file: token_file.display().to_string() }), || {
                    if exists {
                        format!("A node token exists ({}); it is only shown when it is created.", token_file.display())
                    } else {
                        "No node token yet: klif-cli node token --create makes one (printed once).".into()
                    }
                });
                return Ok(());
            }
            if exists && !yes {
                return Err(CliError::needs_yes(
                    "A node token exists: --create --yes replaces it, and every machine that uses the old one must get the new one.",
                ));
            }
            let token = node_token::create(cfg).map_err(|e| CliError::new("io", format!("{e:#}")))?;
            out.doc(val(&NodeTokenCreatedDoc { token: token.expose().to_string(), token_file: token_file.display().to_string() }), || {
                note("Node token (shown once; put it into the other machine's token file, see [nodes.<id>] token):");
                token.expose().to_string()
            });
            Ok(())
        }
        other => Err(CliError::usage(format!("Unknown node subcommand \"{other}\" (status, token)."))),
    }
}
