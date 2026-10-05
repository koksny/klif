//! Linux readers for the platform layer (`platform::linux_impl`): GPUs from the DRM class in sysfs, process GPU
//! memory from `/proc/<pid>/fdinfo`, CPU and RAM from `/proc` and the CPU topology in sysfs. Nothing needs root and
//! nothing touches a GPU beyond reading what its driver publishes.
//!
//! - **GPUs:** every `/sys/class/drm/card<N>` whose PCI device is AMD, NVIDIA or Intel, by PCI ids (the name comes
//!   from the GPU table). Memory totals and use are published by amdgpu (`mem_info_vram_total`, `mem_info_vram_used`,
//!   `mem_info_gtt_total`); other drivers leave them out, and KLIF then shows the GPU without memory figures. The
//!   PCI address stands in for the Windows LUID as the adapter's key.
//! - **Per process:** the DRM fdinfo keys of each open GPU file (`drm-pdev`, `drm-client-id`, `drm-memory-vram` /
//!   `drm-resident-vram`, `drm-total-vram`, the `gtt` twins), each client counted once.
//! - **Power:** the PCI device's `power_state` (D0 / D3hot / D3cold).
//!
//! Tested in WSL2 for the CPU and RAM; the GPU readers follow the kernel's documentation and are untested on a real
//! Linux GPU so far (WSL has no amdgpu: its GPU is paravirtualized through DirectX).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::cpu::CoreClass;
use crate::PowerState;

const AMD: u32 = 0x1002;
const NVIDIA: u32 = 0x10DE;
const INTEL: u32 = 0x8086;

fn read_trim(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn read_hex(p: &Path) -> Option<u32> {
    let s = read_trim(p)?;
    u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

fn read_u64(p: &Path) -> Option<u64> {
    read_trim(p)?.parse().ok()
}

// ------------------------------------------------------------------------------------------------ GPUs

/// One GPU of the DRM class.
#[derive(Debug, Clone)]
pub(crate) struct Card {
    /// The PCI device folder (`/sys/class/drm/cardN/device`).
    pub dev: PathBuf,
    /// "0000:03:00.0".
    pub slot: String,
    pub vendor: u32,
    pub device: u32,
    pub revision: u32,
    pub subsys: u32,
    pub vram_total: u64,
    pub gtt_total: u64,
}

impl Card {
    /// (domain, bus << 8 | device << 3 | function) from the PCI address: the adapter key in place of a LUID.
    pub fn luid(&self) -> (i32, u32) {
        let mut parts = self.slot.split([':', '.']);
        let mut next = |radix| parts.next().and_then(|p| u32::from_str_radix(p, radix).ok()).unwrap_or(0);
        let (domain, bus, dev, func) = (next(16), next(16), next(16), next(10));
        (domain as i32, (bus << 8) | (dev << 3) | func)
    }
}

/// Every AMD, NVIDIA or Intel GPU of the DRM class, in card order.
pub(crate) fn cards() -> Vec<Card> {
    let Ok(dir) = fs::read_dir("/sys/class/drm") else { return Vec::new() };
    let mut names: Vec<(u32, PathBuf)> = dir
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n: u32 = name.strip_prefix("card")?.parse().ok()?;
            Some((n, e.path()))
        })
        .collect();
    names.sort_by_key(|(n, _)| *n);
    let mut seen = BTreeSet::new();
    names
        .into_iter()
        .filter_map(|(_, card)| {
            let dev = card.join("device");
            let vendor = read_hex(&dev.join("vendor"))?;
            if ![AMD, NVIDIA, INTEL].contains(&vendor) {
                return None;
            }
            let slot = fs::canonicalize(&dev).ok()?.file_name()?.to_string_lossy().into_owned();
            if !seen.insert(slot.clone()) {
                return None;
            }
            let subsys = (read_hex(&dev.join("subsystem_device")).unwrap_or(0) << 16) | read_hex(&dev.join("subsystem_vendor")).unwrap_or(0);
            Some(Card {
                slot,
                vendor,
                device: read_hex(&dev.join("device")).unwrap_or(0),
                revision: read_hex(&dev.join("revision")).unwrap_or(0),
                subsys,
                vram_total: read_u64(&dev.join("mem_info_vram_total")).unwrap_or(0),
                gtt_total: read_u64(&dev.join("mem_info_gtt_total")).unwrap_or(0),
                dev,
            })
        })
        .collect()
}

/// VRAM in use on the card (amdgpu), bytes.
pub(crate) fn vram_used(card: &Card) -> Option<u64> {
    read_u64(&card.dev.join("mem_info_vram_used"))
}

/// The PCI device's power state.
pub(crate) fn power_state(dev: &Path) -> Option<PowerState> {
    match read_trim(&dev.join("power_state"))?.as_str() {
        "D0" => Some(PowerState::D0),
        "D1" => Some(PowerState::D1),
        "D2" => Some(PowerState::D2),
        s if s.starts_with("D3") => Some(PowerState::D3),
        _ => None,
    }
}

/// A process's GPU memory per PCI slot: (resident VRAM, GTT, total VRAM + GTT), bytes. Each DRM client is counted
/// once although several file descriptors may point at it.
pub(crate) fn process_gpu_memory(pid: u32) -> BTreeMap<String, (u64, u64, u64)> {
    let mut out: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    let Ok(dir) = fs::read_dir(format!("/proc/{pid}/fdinfo")) else { return out };
    let mut clients = BTreeSet::new();
    for entry in dir.flatten() {
        let Ok(text) = fs::read_to_string(entry.path()) else { continue };
        if !text.contains("drm-pdev") {
            continue;
        }
        let mut kv: BTreeMap<&str, &str> = BTreeMap::new();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once(':') {
                kv.insert(k.trim(), v.trim());
            }
        }
        let Some(slot) = kv.get("drm-pdev").map(|s| s.to_string()) else { continue };
        let client = kv.get("drm-client-id").map(|s| s.to_string()).unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        if !clients.insert((slot.clone(), client)) {
            continue;
        }
        let bytes = |key: &str| kv.get(key).and_then(|v| parse_size(v));
        let vram_res = bytes("drm-resident-vram").or_else(|| bytes("drm-memory-vram")).unwrap_or(0);
        let gtt_res = bytes("drm-resident-gtt").or_else(|| bytes("drm-memory-gtt")).unwrap_or(0);
        let total = match (bytes("drm-total-vram"), bytes("drm-total-gtt")) {
            (None, None) => vram_res + gtt_res,
            (v, g) => v.unwrap_or(vram_res) + g.unwrap_or(gtt_res),
        };
        let e = out.entry(slot).or_insert((0, 0, 0));
        e.0 += vram_res;
        e.1 += gtt_res;
        e.2 += total;
    }
    out
}

/// "1234 KiB", "12 MiB", "512" (bytes) -> bytes.
fn parse_size(v: &str) -> Option<u64> {
    let mut it = v.split_whitespace();
    let n: u64 = it.next()?.parse().ok()?;
    let mult = match it.next() {
        None => 1,
        Some("KiB") => 1 << 10,
        Some("MiB") => 1 << 20,
        Some("GiB") => 1 << 30,
        Some(_) => return None,
    };
    Some(n * mult)
}

// ------------------------------------------------------------------------------------------------ CPU and RAM

/// (total, available) bytes from `/proc/meminfo`.
pub(crate) fn memory() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let kib = |key: &str| {
        text.lines().find_map(|l| l.strip_prefix(key)?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok()).map(|v| v * 1024)
    };
    Some((kib("MemTotal:")?, kib("MemAvailable:")?))
}

/// (busy, total) jiffies over all CPUs from the first line of `/proc/stat`.
pub(crate) fn cpu_ticks() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/stat").ok()?;
    let line = text.lines().next()?.strip_prefix("cpu ")?;
    let v: Vec<u64> = line.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if v.len() < 4 {
        return None;
    }
    let total: u64 = v.iter().take(8).sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some((total.saturating_sub(idle), total))
}

/// The CPU's model name from `/proc/cpuinfo` (non-x86 machines, where CPUID is not there).
pub(crate) fn cpu_model() -> Option<String> {
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    text.lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            matches!(k.trim(), "model name" | "Model" | "Hardware").then(|| v.trim().to_string())
        })
        .filter(|s| !s.is_empty())
}

/// CPU numbers of a list such as "0-7,16-23".
fn cpu_list(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                    out.extend(a..=b);
                }
            }
            None => out.extend(part.parse::<u32>().ok()),
        }
    }
    out
}

/// Physical cores and threads of these CPUs (a core = a distinct package and core id).
fn class_of(cpus: &[u32]) -> (u32, u32) {
    let mut cores = BTreeSet::new();
    for c in cpus {
        let topo = PathBuf::from(format!("/sys/devices/system/cpu/cpu{c}/topology"));
        let pkg = read_trim(&topo.join("physical_package_id")).unwrap_or_default();
        let core = read_trim(&topo.join("core_id")).unwrap_or_else(|| c.to_string());
        cores.insert((pkg, core));
    }
    (cores.len() as u32, cpus.len() as u32)
}

/// Physical cores per efficiency class, the fastest first: Intel hybrid CPUs list their P and E cores in
/// `/sys/devices/cpu_core` and `/sys/devices/cpu_atom`; every other CPU is one class.
pub(crate) fn core_classes() -> Vec<CoreClass> {
    let online = read_trim(Path::new("/sys/devices/system/cpu/online")).map(|s| cpu_list(&s)).unwrap_or_default();
    if online.is_empty() {
        return Vec::new();
    }
    let p = read_trim(Path::new("/sys/devices/cpu_core/cpus")).map(|s| cpu_list(&s));
    let e = read_trim(Path::new("/sys/devices/cpu_atom/cpus")).map(|s| cpu_list(&s));
    match (p, e) {
        (Some(p), Some(e)) if !p.is_empty() && !e.is_empty() => {
            let (pc, pt) = class_of(&p);
            let (ec, et) = class_of(&e);
            vec![CoreClass { efficiency_class: 1, cores: pc, threads: pt }, CoreClass { efficiency_class: 0, cores: ec, threads: et }]
        }
        _ => {
            let (c, t) = class_of(&online);
            vec![CoreClass { efficiency_class: 0, cores: c, threads: t }]
        }
    }
}

/// The CPU's base (nominal) clock in MHz: cpufreq's `base_frequency` (intel_pstate) or `amd_pstate_nominal_freq`,
/// both in kHz. Without cpufreq at all (WSL and other VMs) the `cpu MHz` of `/proc/cpuinfo`, which a hypervisor
/// reports as the nominal clock; with cpufreq present that line is the current clock, so it is not used.
pub(crate) fn base_mhz() -> Option<u32> {
    let dir = Path::new("/sys/devices/system/cpu/cpu0/cpufreq");
    if dir.exists() {
        return ["base_frequency", "amd_pstate_nominal_freq"].iter().find_map(|f| read_u64(&dir.join(f))).map(|khz| (khz / 1000) as u32).filter(|m| *m > 0);
    }
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    let mhz: f64 = text.lines().find_map(|l| l.strip_prefix("cpu MHz")?.split_once(':')?.1.trim().parse().ok())?;
    (mhz >= 100.0).then(|| mhz.round() as u32)
}
