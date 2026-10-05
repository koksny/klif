//! hardware: the GPUs, CPU and RAM of this machine (or of a node) with their theoretical peak FP32 TFLOPS.

use crate::args::Args;
use crate::conn;
use crate::out::{table, val, CliError, CliResult, Out};
use crate::outputs::HardwareDoc;
use klif_core::klif_common::config::LoadedConfig;
use klif_core::klif_common::vm::{ComputeDevice, DeviceKind, HardwareInfo, NodeState, TflopsSource};

/// `hardware [--node N]`. This machine is read directly (the adapters, the CPU and the RAM: no engine is started and
/// no GPU is touched); a node's inventory comes from the engine, which asks that node.
pub fn run(mut args: Args, loaded: &LoadedConfig, out: Out) -> CliResult {
    let node = args.opt("--node")?;
    args.done()?;
    let (hw, from) = match node {
        None => (klif_core::klif_telemetry::hardware::detect(&loaded.cfg.hardware), None),
        Some(n) => {
            let (hw, id, name) = node_hardware(&n, loaded)?;
            (hw, Some((id, name)))
        }
    };
    let doc = HardwareDoc { hardware: hw.clone(), node: from.as_ref().map(|(id, _)| id.clone()) };
    out.doc(val(&doc), || human(&hw, from.as_ref().map(|(_, name)| name.as_str())));
    Ok(())
}

/// A node's inventory by its id or name: (hardware, id, name).
fn node_hardware(which: &str, loaded: &LoadedConfig) -> CliResult<(HardwareInfo, String, String)> {
    let conn = conn::connect(loaded)?;
    let vm = conn.snapshot(None)?;
    let w = which.trim();
    let node = vm.nodes.iter().find(|n| n.id.eq_ignore_ascii_case(w) || n.name.eq_ignore_ascii_case(w)).ok_or_else(|| {
        let ids: Vec<&str> = vm.nodes.iter().map(|n| n.id.as_str()).collect();
        let known = if ids.is_empty() { "There are no nodes in klif.toml.".to_string() } else { format!("Nodes: {}.", ids.join(", ")) };
        CliError::new("not_found", format!("There is no node \"{which}\". {known}"))
    })?;
    if node.state != NodeState::Online {
        let why = node.error.clone().unwrap_or_else(|| "it is not connected".into());
        return Err(CliError::new("error", format!("Node {} is {}: {why}", node.id, val(&node.state).as_str().unwrap_or("not online"))));
    }
    match &node.hardware {
        Some(h) => Ok((h.clone(), node.id.clone(), node.name.clone())),
        None => Err(CliError::new("unsupported", format!("Node {} does not report its hardware; use KLIF 0.3.1 or newer on it.", node.id))),
    }
}

fn human(hw: &HardwareInfo, node: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(n) = node {
        out.push_str(&format!("Node {n}\n\n"));
    }
    let mut rows = vec![["ID", "DEVICE", "MEMORY", "FP32 TFLOPS", "COUNTED", "DETAIL"].map(String::from).to_vec()];
    for d in hw.gpus.iter().chain(hw.cpu.iter()) {
        rows.push(vec![d.id.clone(), d.name.clone(), memory(d), tflops(d), counted(d), d.detail.clone().unwrap_or_default()]);
    }
    out.push_str(&table(&rows));
    let unknown = if hw.tflops_unknown > 0 { format!(" (+{} unknown)", hw.tflops_unknown) } else { String::new() };
    out.push_str(&format!("\nTotal   {} TFLOPS FP32 peak{unknown}\n", hw.tflops_fp32));
    let unified = if hw.unified { ", unified with system memory" } else { "" };
    out.push_str(&format!(
        "Memory  {} GiB VRAM pool (largest GPU {} GiB{unified}), {} GiB RAM\n",
        hw.vram_pool_gib, hw.largest_gpu_gib, hw.ram_total_gib
    ));
    out.push_str("\nFP32 is the theoretical peak: the vendor's published figure for a GPU, cores x FLOP per cycle x base clock for the CPU.\n");
    if hw.gpus.iter().any(|g| g.tflops_source == TflopsSource::Computed) {
        // Apple GPUs without a published figure, and Apple CPUs, whose clocks Apple does not publish.
        out.push_str("An Apple GPU without a published figure: cores x 128 lanes x 2 x its highest clock; Apple CPU cores: their highest clock.\n");
    }
    out
}

fn memory(d: &ComputeDevice) -> String {
    match (d.vram_gib, &d.memory_type) {
        (Some(g), Some(t)) => format!("{g} GiB {t}"),
        (Some(g), None) => format!("{g} GiB"),
        (None, _) => "-".into(),
    }
}

fn tflops(d: &ComputeDevice) -> String {
    match (d.tflops_fp32, d.tflops_source) {
        (Some(v), TflopsSource::Config) => format!("{v} (klif.toml)"),
        (Some(v), TflopsSource::Computed) => format!("{v} (estimate)"),
        (Some(v), _) => v.to_string(),
        (None, _) => "?".into(),
    }
}

fn counted(d: &ComputeDevice) -> String {
    match (d.counted, d.integrated, d.kind) {
        (true, _, _) => "yes".into(),
        (false, true, DeviceKind::Gpu) => "no (integrated)".into(),
        (false, _, _) => "no".into(),
    }
}
