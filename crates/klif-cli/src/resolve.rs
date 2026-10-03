//! System arguments (SPEC 8 + 16.21): exact id (`s1`, `render-box/s1`) -> local label ignoring case and spaces
//! (`system1`) -> legacy `low|medium|high|krea` (local s1/s2/s3/cgi when they exist) -> a label unique across the
//! remote nodes (also `<node>/<label>`) -> otherwise an error listing the candidates with their ids.

use crate::out::{CliError, CliResult};
use klif_core::klif_common::vm::{System, SystemId, ViewModel};

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect()
}

fn listing(systems: &[&System]) -> String {
    systems.iter().map(|s| format!("{} ({})", s.id, s.label)).collect::<Vec<_>>().join(", ")
}

/// The System an argument names.
pub fn resolve(vm: &ViewModel, arg: &str) -> CliResult<SystemId> {
    resolve_system(vm, arg).map(|s| s.id.clone())
}

pub fn resolve_system<'a>(vm: &'a ViewModel, arg: &str) -> CliResult<&'a System> {
    let a = arg.trim();
    if a.is_empty() {
        return Err(CliError::usage("Missing System."));
    }
    // 1. exact id
    if let Some(s) = vm.systems.iter().find(|s| s.id.as_str().eq_ignore_ascii_case(a)) {
        return Ok(s);
    }
    let n = norm(a);
    let local = |s: &&System| s.node.is_none() && !s.id.is_remote();
    // 2. local label
    let hits: Vec<&System> = vm.systems.iter().filter(local).filter(|s| norm(&s.label) == n).collect();
    match hits.len() {
        1 => return Ok(hits[0]),
        0 => {}
        _ => return Err(CliError::new("ambiguous", format!("\"{a}\" names several Systems: {}. Use an id.", listing(&hits)))),
    }
    // 3. legacy 0.2 tier names, local only
    let legacy = match n.as_str() {
        "low" => Some("s1"),
        "medium" => Some("s2"),
        "high" => Some("s3"),
        "krea" => Some("cgi"),
        _ => None,
    };
    if let Some(id) = legacy {
        if let Some(s) = vm.systems.iter().filter(local).find(|s| s.id.as_str() == id) {
            return Ok(s);
        }
    }
    // 4. a label unique across the nodes, or "<node>/<label>"
    let remote = |s: &&System| s.node.is_some() || s.id.is_remote();
    let mut hits: Vec<&System> = vm.systems.iter().filter(remote).filter(|s| norm(&s.label) == n).collect();
    if hits.is_empty() {
        if let Some((node, label)) = a.split_once('/') {
            let (node, label) = (node.trim(), norm(label));
            hits = vm
                .systems
                .iter()
                .filter(remote)
                .filter(|s| s.id.node().is_some_and(|x| x.eq_ignore_ascii_case(node)) && norm(&s.label) == label)
                .collect();
        }
    }
    match hits.len() {
        1 => Ok(hits[0]),
        0 => {
            let all: Vec<&System> = vm.systems.iter().collect();
            if all.is_empty() {
                Err(CliError::new("not_found", format!("There is no System \"{a}\": no Systems are configured (klif-cli systems add --kind llm).")))
            } else {
                Err(CliError::new("not_found", format!("There is no System \"{a}\". Systems: {}.", listing(&all))))
            }
        }
        _ => Err(CliError::new("ambiguous", format!("\"{a}\" names Systems on several nodes: {}. Use an id.", listing(&hits)))),
    }
}
