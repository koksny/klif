//! The machine inventory (SPEC 0.3.1 section 1): GPUs, CPU and RAM with their theoretical peak FP32 throughput.
//! Static facts only, read once at engine start and again when `[hardware]` changes; no live load.
//!
//! - GPUs come from the DXGI adapters (`adapters()`: description, PCI ids, revision, dedicated and shared memory).
//!   Nothing here creates a D3D12 device or opens the card, so a sleeping GPU stays asleep. The peak FP32 number comes
//!   from the embedded table (`data/gpus.toml`: vendor-published, found by name, then PCI ids), else `[hardware]
//!   tflops`, else it is unknown ("?").
//! - Integrated GPUs: the table's flag, else a heuristic on the name and the memory size.
//! - Counted: every discrete GPU; integrated GPUs only on a machine without a discrete one (their memory is then the
//!   unified pool, `unified = true`); the CPU always. `[hardware] exclude / include` override.
//! - The CPU's number is computed (`cpu::CpuFacts::tflops_fp32`): cores x FLOP per cycle x base clock.

use crate::cpu::{self, CpuFacts};
use crate::gputable::{self, GpuEntry};
use crate::{gpu_id, gpu_id_eq, platform, text, Adapter};
use klif_common::config::HardwareCfg;
use klif_common::vm::{ComputeDevice, DeviceKind, HardwareInfo, TflopsSource};

/// PCI vendor ids.
const AMD: u32 = 0x1002;
const INTEL: u32 = 0x8086;
/// Microsoft's software and virtual display adapters (Basic Render Driver, Hyper-V video): not compute devices.
const MICROSOFT: u32 = 0x1414;

/// An integrated GPU with at most this much dedicated memory (a BIOS carve-out) is told by its name alone.
const SMALL_CARVE_OUT_GIB: f64 = 4.0;

/// What the machine reports, before any rule is applied.
#[derive(Debug, Clone, Default)]
pub struct Probe {
    /// DXGI order, software adapters skipped.
    pub adapters: Vec<Adapter>,
    pub cpu: Option<CpuFacts>,
    /// Physical memory the OS can use.
    pub ram_bytes: u64,
}

impl Probe {
    /// Read the adapters, the CPU facts and the RAM size (DXGI enumeration, CPUID, the registry and the processor
    /// topology: nothing that touches a GPU).
    pub fn read() -> Probe {
        let host = platform::host();
        Probe { adapters: crate::adapters(), cpu: host.cpu_facts(), ram_bytes: host.memory().map_or(0, |(total, _)| total) }
    }
}

/// The inventory of this machine under `cfg`.
pub fn detect(cfg: &HardwareCfg) -> HardwareInfo {
    inventory(&Probe::read(), cfg)
}

/// Apply the rules to what the machine reports.
pub fn inventory(probe: &Probe, cfg: &HardwareCfg) -> HardwareInfo {
    let listed = |list: &[String], id: &str| list.iter().any(|x| gpu_id_eq(x, id));
    let tflops_of = |id: &str| cfg.tflops.iter().find(|(k, _)| gpu_id_eq(k, id)).map(|(_, v)| *v);

    let adapters: Vec<&Adapter> = probe.adapters.iter().filter(|a| a.vendor_id != MICROSOFT).collect();
    let mut gpus: Vec<ComputeDevice> = adapters.iter().map(|a| gpu_device(a, &gpu_id(a, &probe.adapters))).collect();
    let any_discrete = gpus.iter().any(|g| !g.integrated);
    for g in &mut gpus {
        // Excluding wins over including; otherwise the rule: discrete always, integrated only without a discrete one.
        g.counted = if listed(&cfg.exclude, &g.id) {
            false
        } else if listed(&cfg.include, &g.id) {
            true
        } else {
            !g.integrated || !any_discrete
        };
        if let Some(v) = tflops_of(&g.id) {
            g.tflops_fp32 = Some(text::round_to(v, 2));
            g.tflops_source = TflopsSource::Config;
        }
    }
    // A counted integrated GPU addresses its carve-out and system memory besides: that is its pool. One that is not
    // counted shows only its carve-out (the shared part is the RAM already listed).
    for (g, a) in gpus.iter_mut().zip(&adapters) {
        if g.integrated && g.counted {
            let vram = (a.dedicated_bytes + a.shared_bytes) as f64 / text::GIB;
            g.vram_gib = (vram > 0.0).then(|| text::round_to(vram, 2));
            let shared = a.shared_bytes as f64 / text::GIB;
            g.shared_gib = (shared > 0.0).then(|| text::round_to(shared, 2));
        }
    }

    let cpu = probe.cpu.as_ref().map(|c| {
        let mut d = cpu_device(c);
        d.counted = !listed(&cfg.exclude, "cpu");
        if let Some(v) = tflops_of("cpu") {
            d.tflops_fp32 = Some(text::round_to(v, 2));
            d.tflops_source = TflopsSource::Config;
        }
        d
    });

    let counted_gpus = || gpus.iter().filter(|g| g.counted);
    let vram_pool = counted_gpus().filter_map(|g| g.vram_gib).sum::<f64>();
    let largest = counted_gpus().filter_map(|g| g.vram_gib).fold(0.0, f64::max);
    let unified = counted_gpus().next().is_some() && counted_gpus().all(|g| g.integrated);
    let counted_all = || counted_gpus().chain(cpu.iter().filter(|c| c.counted));
    let tflops = counted_all().filter_map(|d| d.tflops_fp32).sum::<f64>();
    let unknown = counted_all().filter(|d| d.tflops_fp32.is_none()).count() as u32;

    HardwareInfo {
        ram_total_gib: text::round_to(probe.ram_bytes as f64 / text::GIB, 1),
        vram_pool_gib: text::round_to(vram_pool, 2),
        largest_gpu_gib: text::round_to(largest, 2),
        unified,
        tflops_fp32: text::round_to(tflops, 1),
        tflops_unknown: unknown,
        cpu,
        gpus,
    }
}

// ------------------------------------------------------------------------------------------------ GPUs

fn gpu_device(a: &Adapter, id: &str) -> ComputeDevice {
    let entry = gputable::lookup(&a.name, a.vendor_id, a.device_id, a.revision);
    let integrated = entry.and_then(|e| e.integrated).unwrap_or_else(|| looks_integrated(a));
    let vram = a.dedicated_bytes as f64 / text::GIB;
    let memory_type = match entry.and_then(|e| e.memory_type.clone()) {
        Some(t) => Some(t),
        None if integrated => Some("shared system memory".to_string()),
        None => None,
    };
    ComputeDevice {
        id: id.to_string(),
        name: a.display_name(),
        kind: DeviceKind::Gpu,
        integrated,
        counted: false,
        vram_gib: (vram > 0.0).then(|| text::round_to(vram, 2)),
        shared_gib: None,
        memory_type,
        tflops_fp32: entry.and_then(|e| e.tflops_fp32),
        tflops_source: if entry.is_some_and(|e| e.tflops_fp32.is_some()) { TflopsSource::Table } else { TflopsSource::Unknown },
        detail: entry.and_then(|e| gpu_detail(a.vendor_id, e)),
    }
}

/// "64 CU, 2970 MHz" (AMD), "16384 CUDA cores, 2520 MHz" (NVIDIA), "2560 shaders, 1905 MHz".
fn gpu_detail(vendor_id: u32, e: &GpuEntry) -> Option<String> {
    let units = e.shaders.map(|s| match vendor_id {
        AMD => format!("{} CU", s / 64),
        0x10DE => format!("{s} CUDA cores"),
        _ => format!("{s} shaders"),
    });
    let clock = e.boost_mhz.map(|m| format!("{m} MHz"));
    let parts: Vec<String> = [units, clock].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// An integrated GPU by its name (the table's flag has priority): AMD and Intel only, a generic iGPU name, and for
/// Intel and small AMD carve-outs a dedicated memory of at most 4 GiB. A Strix Halo style APU can carve out far more,
/// so its "8060S" model token is enough.
fn looks_integrated(a: &Adapter) -> bool {
    let n = gputable::normalize_name(&a.name);
    let tokens: Vec<&str> = n.split(' ').collect();
    let small = a.dedicated_bytes as f64 / text::GIB <= SMALL_CARVE_OUT_GIB;
    match a.vendor_id {
        AMD => {
            let apu_model = tokens.iter().any(|t| is_amd_apu_model(t));
            let generic = (n.starts_with("amd radeon") && !tokens.contains(&"rx")) || (n.contains("rx vega") && n.ends_with(" graphics"));
            let named = generic && (n.ends_with(" graphics") || apu_model);
            named && (small || apu_model)
        }
        INTEL => {
            // Discrete Arc cards carry an A- or B-series model (A770, B580) or "Pro"; the integrated ones are "Graphics",
            // "Arc(TM) Graphics" and "Arc(TM) 140V GPU".
            let discrete = tokens.iter().any(|t| is_intel_discrete_model(t)) || tokens.contains(&"pro") || n.contains("data center");
            small && !discrete && n.starts_with("intel") && (n.contains("graphics") || tokens.iter().any(|t| is_intel_arc_igpu_model(t)))
        }
        _ => false,
    }
}

/// `digits` ASCII digits followed by one of `suffixes` ("780m": 3..=4 digits then m or s).
fn digits_then(t: &str, digits: std::ops::RangeInclusive<usize>, suffixes: &[u8]) -> bool {
    match t.as_bytes().split_last() {
        Some((last, head)) => digits.contains(&head.len()) && head.iter().all(u8::is_ascii_digit) && suffixes.contains(last),
        None => false,
    }
}

/// "780m", "890m", "8060s": the model token of an AMD APU's graphics.
fn is_amd_apu_model(t: &str) -> bool {
    digits_then(t, 3..=4, b"ms")
}

/// "a770", "a380m", "b580": Intel Arc discrete model tokens.
fn is_intel_discrete_model(t: &str) -> bool {
    match t.as_bytes().split_first() {
        Some((b'a' | b'b', rest)) => {
            let digits = rest.strip_suffix(b"m").unwrap_or(rest);
            digits.len() == 3 && digits.iter().all(u8::is_ascii_digit)
        }
        _ => false,
    }
}

/// "140v", "130t": the model token of a Lunar Lake / Arrow Lake integrated Arc GPU.
fn is_intel_arc_igpu_model(t: &str) -> bool {
    digits_then(t, 3..=3, b"vt")
}

// ------------------------------------------------------------------------------------------------ CPU

fn cpu_device(c: &CpuFacts) -> ComputeDevice {
    let tflops = c.tflops_fp32().map(|v| text::round_to(v, 2));
    let detail = c.detail();
    ComputeDevice {
        id: "cpu".to_string(),
        name: cpu::display_name(&c.brand),
        kind: DeviceKind::Cpu,
        integrated: false,
        counted: true,
        vram_gib: None,
        shared_gib: None,
        memory_type: None,
        tflops_fp32: tflops,
        tflops_source: if tflops.is_some() { TflopsSource::Computed } else { TflopsSource::Unknown },
        detail: (!detail.is_empty()).then_some(detail),
    }
}
