//! The platform layer for measurements: GPU adapters / memory / power state (`GpuPlatform`) and CPU / RAM
//! (`HostPlatform`). Windows: DXGI, PDH, SetupDi (win.rs). macOS (Apple silicon): Metal, the IORegistry, sysctl
//! and mach (mac.rs). Elsewhere: a stub that returns None / empty until someone adds sysfs / NVML readers (Linux
//! amdgpu: `/sys/class/drm/card*/device/mem_info_vram_used`, per-process `/proc/<pid>/fdinfo`
//! `drm-memory-vram`; NVIDIA: NVML). Owner: package D.
//!
//! The rest of the crate calls `gpu()` / `host()`, never `win::` directly, so non-Windows targets compile.
//! The CPU name comes from CPUID on x86 / x86_64 on every OS (no registry read); the FP32 estimate (`cpu_facts`)
//! adds the core topology and, on Windows, the base clock from the registry's `~MHz`.

use crate::cpu::{self, CpuFacts};
use crate::{Adapter, PowerState, UlpsSetting};

/// GPU facts of the platform.
pub trait GpuPlatform: Send + Sync {
    /// Hardware adapters in the platform's enumeration order (DXGI order on Windows; software adapters skipped).
    fn adapters(&self) -> Vec<Adapter>;
    /// The current device power state of the first device with this "VEN:DEV" (or the n-th for "VEN:DEV#n").
    fn power_state(&self, pci: &str) -> Option<PowerState>;
    /// A memory / CPU sampler that keeps its query open (one per sampler thread).
    fn sampler(&self) -> Option<Box<dyn Sampler>>;
    /// A power-state handle for one adapter (`nth`: its index among identical adapters, the fallback when the
    /// device node cannot be matched by LUID).
    fn power_handle(&self, adapter: &Adapter, nth: usize) -> Option<Box<dyn PowerHandle>>;
    /// The adapter's AMD ULPS setting (its driver key only, read-only).
    fn ulps(&self, adapter: &Adapter, nth: usize) -> Option<UlpsSetting>;
    /// The LUID (high, low) the adapter's device node publishes (`DEVPKEY_Gpu_Luid`), for diagnostics: equal to
    /// the adapter's LUID when power / ULPS reads are tied to the right card even among identical ones.
    fn device_luid(&self, _adapter: &Adapter, _nth: usize) -> Option<(i32, u32)> {
        None
    }
    /// What the adapter's FP32 peak can be computed from when the GPU table has no number for it (Apple GPUs:
    /// their core count and clock come from the machine). The default knows nothing.
    fn units(&self, _adapter: &Adapter) -> Option<GpuUnits> {
        None
    }
}

/// A GPU's compute units, for the FP32 estimate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuUnits {
    /// GPU cores (Apple: `gpu-core-count`, 128 FP32 ALUs each).
    pub cores: u32,
    /// The highest clock the GPU runs at.
    pub max_mhz: Option<u32>,
}

/// CPU / RAM facts of the platform.
pub trait HostPlatform: Send + Sync {
    /// Short CPU name, e.g. "9950X3D".
    fn cpu_name(&self) -> Option<String>;
    /// (total bytes, available bytes).
    fn memory(&self) -> Option<(u64, u64)>;
    /// The CPU's static facts for the FP32 estimate: CPUID, physical cores per efficiency class, base clock. The
    /// default knows nothing.
    fn cpu_facts(&self) -> Option<CpuFacts> {
        None
    }
}

/// Keeps a measurement query open; `read` is called at 1 Hz by the sampler thread.
pub trait Sampler: Send {
    /// Re-expand per-process instances (new PIDs appeared).
    fn refresh_processes(&mut self);
    /// One collect: adapter memory per GPU, process memory of `pids` (sorted) per GPU, CPU utility.
    fn read(&mut self, pids: &[u32]) -> Frame;
}

/// Reads one device's power state.
pub trait PowerHandle: Send {
    fn read(&mut self) -> Option<PowerState>;
}

/// One sampler reading.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// CPU utility 0..100 (None when unavailable).
    pub cpu_pct: Option<f64>,
    /// Dedicated memory in use per adapter (bytes), keyed by `Adapter::pdh_luid()`.
    pub adapters: Vec<AdapterMem>,
    /// Per-process memory of the requested PIDs per adapter.
    pub procs: Vec<ProcMem>,
}

#[derive(Debug, Clone, Default)]
pub struct AdapterMem {
    /// `Adapter::pdh_luid()`, e.g. "luid_0x00000000_0x00017798".
    pub key: String,
    pub dedicated: f64,
}

/// One process on one adapter (bytes).
#[derive(Debug, Clone, Default)]
pub struct ProcMem {
    pub key: String,
    pub pid: u32,
    /// Resident in VRAM ("Dedicated Usage").
    pub dedicated: f64,
    /// In shared system memory ("Shared Usage").
    pub shared: f64,
    /// Every allocation, resident or not ("Total Committed").
    pub committed: f64,
}

/// The platform without readers: everything None / empty.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoPlatform;

impl GpuPlatform for NoPlatform {
    fn adapters(&self) -> Vec<Adapter> {
        Vec::new()
    }
    fn power_state(&self, _pci: &str) -> Option<PowerState> {
        None
    }
    fn sampler(&self) -> Option<Box<dyn Sampler>> {
        None
    }
    fn power_handle(&self, _adapter: &Adapter, _nth: usize) -> Option<Box<dyn PowerHandle>> {
        None
    }
    fn ulps(&self, _adapter: &Adapter, _nth: usize) -> Option<UlpsSetting> {
        None
    }
}

impl HostPlatform for NoPlatform {
    fn cpu_name(&self) -> Option<String> {
        cpu_brand().map(|b| short_cpu_name(&b))
    }
    fn memory(&self) -> Option<(u64, u64)> {
        None
    }
    /// What CPUID alone tells (the brand and the SIMD flags): no topology and no clock, so no TFLOPS number.
    fn cpu_facts(&self) -> Option<CpuFacts> {
        let (brand, id) = (cpu_brand(), cpu::read_cpuid());
        (brand.is_some() || id.is_some()).then(|| CpuFacts { brand: brand.unwrap_or_default(), id, ..CpuFacts::default() })
    }
}

/// The GPU platform of this build.
pub fn gpu() -> &'static dyn GpuPlatform {
    #[cfg(windows)]
    {
        &windows_impl::WinPlatform
    }
    #[cfg(target_os = "macos")]
    {
        &mac_impl::MacPlatform
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        &NoPlatform
    }
}

/// The host platform of this build.
pub fn host() -> &'static dyn HostPlatform {
    #[cfg(windows)]
    {
        &windows_impl::WinPlatform
    }
    #[cfg(target_os = "macos")]
    {
        &mac_impl::MacPlatform
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        &NoPlatform
    }
}

// ------------------------------------------------------------------------------------------- CPU name

/// The CPU brand string from CPUID leaves 0x80000002..4 (x86 / x86_64), e.g. "AMD Ryzen 9 9950X3D 16-Core
/// Processor". None on other architectures or when the leaves are not supported.
pub fn cpu_brand() -> Option<String> {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        #[cfg(target_arch = "x86")]
        use std::arch::x86::__cpuid;
        #[cfg(target_arch = "x86_64")]
        use std::arch::x86_64::__cpuid;
        // CPUID is available on every x86_64 CPU and every x86 CPU Rust targets.
        #[allow(unused_unsafe)]
        let max = unsafe { __cpuid(0x8000_0000) }.eax;
        if max < 0x8000_0004 {
            return None;
        }
        let mut bytes: Vec<u8> = Vec::with_capacity(48);
        for leaf in 0x8000_0002u32..=0x8000_0004 {
            #[allow(unused_unsafe)]
            let r = unsafe { __cpuid(leaf) };
            for v in [r.eax, r.ebx, r.ecx, r.edx] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        let n = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
        let s = String::from_utf8_lossy(&bytes[..n]).split_whitespace().collect::<Vec<_>>().join(" ");
        if s.is_empty() { None } else { Some(s) }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    {
        None
    }
}

/// "AMD Ryzen 9 9950X3D 16-Core Processor" -> "9950X3D"; "13th Gen Intel(R) Core(TM) i9-13900K" ->
/// "i9-13900K". Falls back to the full brand string.
pub fn short_cpu_name(brand: &str) -> String {
    let skip = |t: &str| {
        let l = t.to_ascii_lowercase();
        l.ends_with("-core") || l.ends_with("-thread") || l.ends_with("ghz") || l.contains("gen") || l.starts_with('@')
    };
    for tok in brand.split_whitespace() {
        let digits = tok.chars().filter(|c| c.is_ascii_digit()).count();
        if digits >= 3 && !skip(tok) {
            return tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-').to_string();
        }
    }
    brand.trim().to_string()
}

/// The canonical adapter key ("luid_0x%08X_0x%08X", as `Adapter::pdh_luid`) inside a PDH instance name such as
/// "pid_1234_luid_0x00000000_0x0000D1F4_phys_0" or "luid_0x00000000_0x0000D1F4_phys_0".
pub fn luid_key(instance: &str) -> Option<String> {
    let i = instance.find("luid_0x").or_else(|| instance.find("luid_0X"))?;
    let rest = &instance[i + 7..];
    let (hi, rest) = rest.split_once('_')?;
    let rest = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X"))?;
    let lo: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    let hi = u32::from_str_radix(hi, 16).ok()?;
    let lo = u32::from_str_radix(&lo, 16).ok()?;
    Some(format!("luid_0x{hi:08X}_0x{lo:08X}"))
}

// -------------------------------------------------------------------------------------------- Windows

#[cfg(windows)]
mod windows_impl {
    use super::{luid_key, AdapterMem, Frame, GpuPlatform, HostPlatform, PowerHandle, ProcMem, Sampler};
    use crate::{parse_gpu_id, win, Adapter, PowerState, UlpsSetting};
    use std::collections::BTreeMap;
    use windows::Win32::System::Performance::PDH_HCOUNTER;

    pub struct WinPlatform;

    impl GpuPlatform for WinPlatform {
        fn adapters(&self) -> Vec<Adapter> {
            win::dxgi_adapters()
        }

        fn power_state(&self, pci: &str) -> Option<PowerState> {
            let (ven, dev, nth) = parse_gpu_id(pci)?;
            let d = match crate::find_adapter(pci) {
                Some(a) => win::DisplayDevice::open_adapter(&a, nth)?,
                None => win::DisplayDevice::open(ven, dev)?,
            };
            d.power_state_raw().and_then(PowerState::from_raw)
        }

        fn sampler(&self) -> Option<Box<dyn Sampler>> {
            PdhSampler::open().map(|s| Box::new(s) as Box<dyn Sampler>)
        }

        fn power_handle(&self, adapter: &Adapter, nth: usize) -> Option<Box<dyn PowerHandle>> {
            win::DisplayDevice::open_adapter(adapter, nth).map(|d| Box::new(DevPower(d)) as Box<dyn PowerHandle>)
        }

        fn device_luid(&self, adapter: &Adapter, nth: usize) -> Option<(i32, u32)> {
            win::DisplayDevice::open_adapter(adapter, nth)?.luid()
        }

        fn ulps(&self, adapter: &Adapter, nth: usize) -> Option<UlpsSetting> {
            let d = win::DisplayDevice::open_adapter(adapter, nth)?;
            let drv = d.driver_key();
            let (enable_ulps, driver_desc) = d.driver_values("EnableUlps", "DriverDesc").unwrap_or((None, None));
            Some(UlpsSetting {
                instance_id: d.instance_id.clone(),
                key: drv.map(|k| format!(r"HKLM\{}\{k}", win::CLASS_KEY)).unwrap_or_default(),
                driver_desc: driver_desc.or_else(|| d.device_desc()),
                enable_ulps,
            })
        }
    }

    impl HostPlatform for WinPlatform {
        fn cpu_name(&self) -> Option<String> {
            super::cpu_brand().map(|b| super::short_cpu_name(&b))
        }
        fn memory(&self) -> Option<(u64, u64)> {
            win::memory_status()
        }
        fn cpu_facts(&self) -> Option<crate::cpu::CpuFacts> {
            Some(crate::cpu::CpuFacts {
                brand: super::cpu_brand().unwrap_or_default(),
                id: crate::cpu::read_cpuid(),
                base_mhz: win::cpu_base_mhz(),
                classes: win::cpu_core_classes(),
            })
        }
    }

    struct DevPower(win::DisplayDevice);

    impl PowerHandle for DevPower {
        fn read(&mut self) -> Option<PowerState> {
            self.0.power_state_raw().and_then(PowerState::from_raw)
        }
    }

    /// PDH: adapter Dedicated Usage, per-process Dedicated / Shared / Total Committed (wildcards), CPU utility.
    struct PdhSampler {
        pdh: win::Pdh,
        adapter_ded: Option<PDH_HCOUNTER>,
        proc_ded: Option<PDH_HCOUNTER>,
        proc_shared: Option<PDH_HCOUNTER>,
        proc_committed: Option<PDH_HCOUNTER>,
        cpu: Option<PDH_HCOUNTER>,
    }

    // The query and counter handles belong to this value; it lives on one thread at a time.
    unsafe impl Send for PdhSampler {}

    impl PdhSampler {
        fn open() -> Option<PdhSampler> {
            let pdh = win::Pdh::open()?;
            let cpu = pdh.add(r"\Processor Information(_Total)\% Processor Utility");
            let adapter_ded = pdh.add(r"\GPU Adapter Memory(*)\Dedicated Usage");
            let proc_ded = pdh.add(r"\GPU Process Memory(*)\Dedicated Usage");
            let proc_shared = pdh.add(r"\GPU Process Memory(*)\Shared Usage");
            let proc_committed = pdh.add(r"\GPU Process Memory(*)\Total Committed");
            pdh.collect();
            Some(PdhSampler { pdh, adapter_ded, proc_ded, proc_shared, proc_committed, cpu })
        }
    }

    impl Sampler for PdhSampler {
        fn refresh_processes(&mut self) {
            for c in [self.proc_ded.take(), self.proc_shared.take(), self.proc_committed.take()].into_iter().flatten() {
                self.pdh.remove(c);
            }
            self.proc_ded = self.pdh.add(r"\GPU Process Memory(*)\Dedicated Usage");
            self.proc_shared = self.pdh.add(r"\GPU Process Memory(*)\Shared Usage");
            self.proc_committed = self.pdh.add(r"\GPU Process Memory(*)\Total Committed");
        }

        fn read(&mut self, pids: &[u32]) -> Frame {
            self.pdh.collect();
            let cpu_pct = self.cpu.and_then(|c| self.pdh.single(c)).map(|v| v.clamp(0.0, 100.0));
            let mut adapters: BTreeMap<String, f64> = BTreeMap::new();
            if let Some(c) = self.adapter_ded {
                for (inst, v) in self.pdh.array(c) {
                    if let Some(k) = luid_key(&inst) {
                        *adapters.entry(k).or_default() += v;
                    }
                }
            }
            let mut procs: BTreeMap<(String, u32), ProcMem> = BTreeMap::new();
            if !pids.is_empty() {
                let mut add = |c: Option<PDH_HCOUNTER>, which: u8| {
                    let Some(c) = c else { return };
                    for (inst, v) in self.pdh.array(c) {
                        let Some(pid) = win::pid_of(&inst) else { continue };
                        if pids.binary_search(&pid).is_err() {
                            continue;
                        }
                        let Some(k) = luid_key(&inst) else { continue };
                        let e = procs.entry((k.clone(), pid)).or_insert_with(|| ProcMem { key: k, pid, ..ProcMem::default() });
                        match which {
                            0 => e.dedicated += v,
                            1 => e.shared += v,
                            _ => e.committed += v,
                        }
                    }
                };
                add(self.proc_ded, 0);
                add(self.proc_shared, 1);
                add(self.proc_committed, 2);
            }
            Frame {
                cpu_pct,
                adapters: adapters.into_iter().map(|(key, dedicated)| AdapterMem { key, dedicated }).collect(),
                procs: procs.into_values().collect(),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod mac_impl {
    use super::{AdapterMem, Frame, GpuPlatform, GpuUnits, HostPlatform, PowerHandle, ProcMem, Sampler};
    use crate::cpu::{CoreClass, CpuFacts};
    use crate::{mac, Adapter, PowerState, UlpsSetting, APPLE_VENDOR};

    pub struct MacPlatform;

    /// The Apple GPU as an adapter: no dedicated memory, the working set as its shared memory (the unified pool
    /// the inventory and the suggester expect of an integrated GPU), Metal's registry id in place of a LUID.
    fn adapter(g: &mac::AppleGpu) -> Adapter {
        let name = if g.metal.name.trim().is_empty() { mac::gpu_model().unwrap_or_else(|| "Apple GPU".into()) } else { g.metal.name.clone() };
        Adapter {
            name,
            vendor_id: APPLE_VENDOR,
            device_id: g.soc.unwrap_or(0),
            luid_high: (g.metal.registry_id >> 32) as i32,
            luid_low: g.metal.registry_id as u32,
            dedicated_bytes: 0,
            shared_bytes: g.metal.working_set,
            ..Adapter::default()
        }
    }

    impl GpuPlatform for MacPlatform {
        fn adapters(&self) -> Vec<Adapter> {
            mac::gpu().map(|g| vec![adapter(&g)]).unwrap_or_default()
        }

        /// Power states and ULPS are AMD-on-Windows features.
        fn power_state(&self, _pci: &str) -> Option<PowerState> {
            None
        }

        fn sampler(&self) -> Option<Box<dyn Sampler>> {
            Some(Box::new(MacSampler::open()))
        }

        fn power_handle(&self, _adapter: &Adapter, _nth: usize) -> Option<Box<dyn PowerHandle>> {
            None
        }

        fn ulps(&self, _adapter: &Adapter, _nth: usize) -> Option<UlpsSetting> {
            None
        }

        fn units(&self, a: &Adapter) -> Option<GpuUnits> {
            if !a.is_apple() {
                return None;
            }
            let g = mac::gpu()?;
            Some(GpuUnits { cores: g.cores?, max_mhz: g.max_mhz })
        }
    }

    impl HostPlatform for MacPlatform {
        /// "M4", "M4 Pro".
        fn cpu_name(&self) -> Option<String> {
            mac::cpu_brand().map(|b| b.strip_prefix("Apple ").unwrap_or(&b).to_string())
        }

        fn memory(&self) -> Option<(u64, u64)> {
            mac::memory()
        }

        /// The brand and the cores per performance level (level 0, the performance cores, is the highest
        /// efficiency class). No CPUID and no clock: `cpu.rs` knows Apple chips by their brand.
        fn cpu_facts(&self) -> Option<CpuFacts> {
            let brand = mac::cpu_brand()?;
            let levels = mac::core_levels();
            let top = levels.len().saturating_sub(1) as u32;
            let classes = levels
                .iter()
                .map(|&(level, cores, logical)| CoreClass { efficiency_class: (top - level.min(top)) as u8, cores, threads: logical })
                .collect();
            Some(CpuFacts { brand, id: None, base_mhz: None, classes })
        }
    }

    /// Mach CPU ticks, the accelerator's memory in use, and each watched process's resident memory as its share of
    /// the unified pool (mapped weights included; its footprint as the allocation when that is larger).
    struct MacSampler {
        key: String,
        ticks: Option<(u64, u64)>,
    }

    impl MacSampler {
        fn open() -> MacSampler {
            let key = mac::gpu().map(|g| adapter(&g).pdh_luid()).unwrap_or_default();
            MacSampler { key, ticks: mac::cpu_ticks() }
        }
    }

    impl Sampler for MacSampler {
        fn refresh_processes(&mut self) {}

        fn read(&mut self, pids: &[u32]) -> Frame {
            let ticks = mac::cpu_ticks();
            let cpu_pct = match (self.ticks, ticks) {
                (Some((b0, t0)), Some((b1, t1))) if t1 > t0 => Some((b1.saturating_sub(b0)) as f64 / (t1 - t0) as f64 * 100.0),
                _ => None,
            };
            if ticks.is_some() {
                self.ticks = ticks;
            }
            if self.key.is_empty() {
                return Frame { cpu_pct, ..Frame::default() };
            }
            let adapters = mac::gpu_in_use().map(|u| vec![AdapterMem { key: self.key.clone(), dedicated: u as f64 }]).unwrap_or_default();
            let procs = pids
                .iter()
                .filter_map(|&pid| {
                    let (resident, footprint) = mac::process_memory(pid)?;
                    let (resident, footprint) = (resident as f64, footprint as f64);
                    Some(ProcMem { key: self.key.clone(), pid, dedicated: resident, shared: 0.0, committed: resident.max(footprint) })
                })
                .collect();
            Frame { cpu_pct, adapters, procs }
        }
    }
}
