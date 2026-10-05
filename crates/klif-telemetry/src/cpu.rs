//! The CPU's static facts and its theoretical peak FP32 throughput (an estimate, labelled so).
//!
//! Facts: vendor / family / model and the SIMD flags from CPUID (x86 and x86_64 on every OS; other architectures
//! have no CPUID reader, so their number stays unknown), physical cores per efficiency class and the base clock from
//! the platform (`platform::HostPlatform::cpu_facts`; Windows: GetLogicalProcessorInformationEx and the registry's
//! `~MHz`).
//!
//! Peak = sum over core classes of cores x FP32 FLOP per cycle per core x base clock. FLOP per cycle per core
//! (FMA = 2 FLOP; vector FMA units x lanes x 2):
//!
//! | core | FLOP/cycle |
//! | --- | --- |
//! | AMD Zen 1 / Zen+ (2 x 128-bit) | 16 |
//! | AMD Zen 2 / 3 (2 x 256-bit), Zen 4 (AVX-512 double-pumped) | 32 |
//! | AMD Zen 5 with the full 512-bit datapath (desktop, server, Strix Halo) | 64 |
//! | AMD Zen 5 mobile (Strix Point, Krackan: double-pumped) | 32 |
//! | Intel client Haswell .. Raptor Lake P-cores (AVX2 + FMA, 2 x 256-bit) | 32 |
//! | Intel Gracemont / Crestmont E-cores (2 x 128-bit) | 16 |
//! | Intel Skymont E-cores (4 x 128-bit) | 32 |
//! | Intel Xeon with two 512-bit FMA units (Skylake-SP .. Granite Rapids) | 64 |
//! | AVX only (Sandy / Ivy Bridge) | 16 |
//! | anything else | 8 |
//!
//! The base clock is one number for the whole CPU (E-cores of a hybrid Intel chip clock lower than the registry's
//! value, so their share is a little optimistic). It is the clock the vendor guarantees, not the boost.
//!
//! Apple silicon has no CPUID and macOS reports no clocks, so Apple chips are known by their brand string
//! (`APPLE_CPUS`): the performance and efficiency cores' highest clocks as measured by third parties (Apple
//! publishes none), times FLOP per cycle per core: performance cores 4 x 128-bit NEON FMA pipes = 32,
//! efficiency cores 2 x 128-bit = 16. A chip missing from the table has no number ("?").

/// The CPU maker, as far as the FLOP table cares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CpuVendor {
    Amd,
    Intel,
    #[default]
    Other,
}

/// What CPUID says about the CPU.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuId {
    pub vendor: CpuVendor,
    /// Display family (base + extended family where the vendor defines it).
    pub family: u32,
    /// Display model (base | extended model << 4 for family 6 / 15 and AMD's extended families).
    pub model: u32,
    pub stepping: u32,
    pub avx: bool,
    pub avx2: bool,
    pub fma: bool,
    pub avx512f: bool,
}

/// The physical cores of one efficiency class (Windows `EfficiencyClass`: a higher class is a faster core; a
/// CPU without hybrid cores has the single class 0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CoreClass {
    pub efficiency_class: u8,
    pub cores: u32,
    pub threads: u32,
}

/// Everything the FP32 estimate and the device line need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuFacts {
    /// The CPUID brand string, e.g. "AMD Ryzen 9 9950X3D 16-Core Processor" (empty when unknown).
    pub brand: String,
    pub id: Option<CpuId>,
    /// Base clock in MHz.
    pub base_mhz: Option<u32>,
    /// Physical cores per efficiency class, the fastest class first. Empty when the platform cannot tell.
    pub classes: Vec<CoreClass>,
}

impl CpuFacts {
    /// Physical cores over all classes.
    pub fn cores(&self) -> u32 {
        self.classes.iter().map(|c| c.cores).sum()
    }

    /// More than one efficiency class: performance and efficiency cores.
    pub fn hybrid(&self) -> bool {
        self.classes.len() > 1
    }

    /// Theoretical peak FP32 TFLOPS: cores x FLOP/cycle x base clock, summed over the core classes. None without
    /// CPUID, the topology or the clock. Apple chips: cores x FLOP/cycle x the class's highest clock.
    pub fn tflops_fp32(&self) -> Option<f64> {
        if let Some(chip) = apple_cpu(&self.brand) {
            if self.classes.is_empty() {
                return None;
            }
            let mflops: u64 = self
                .classes
                .iter()
                .map(|c| match self.kind_of(c) {
                    CoreKind::Performance => u64::from(c.cores) * u64::from(APPLE_P_FLOPS) * u64::from(chip.p_mhz),
                    CoreKind::Efficiency => u64::from(c.cores) * u64::from(APPLE_E_FLOPS) * u64::from(chip.e_mhz),
                })
                .sum();
            return Some(mflops as f64 / 1e6);
        }
        let id = self.id.as_ref()?;
        let mhz = self.base_mhz.filter(|m| *m > 0)?;
        if self.classes.is_empty() {
            return None;
        }
        let flops: u64 = self
            .classes
            .iter()
            .map(|c| u64::from(c.cores) * u64::from(fp32_flops_per_cycle(id, &self.brand, self.kind_of(c))))
            .sum();
        // FLOP per cycle x MHz = MFLOPS; / 1e6 = TFLOPS.
        Some(flops as f64 * f64::from(mhz) / 1e6)
    }

    /// "16 cores, AVX-512, 4.3 GHz" / "8P + 16E cores, AVX2, 3.0 GHz".
    pub fn detail(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let cores = self.cores();
        if cores > 0 {
            if self.hybrid() {
                let p = self.classes.first().map_or(0, |c| c.cores);
                parts.push(format!("{p}P + {}E cores", cores - p));
            } else {
                parts.push(format!("{cores} core{}", if cores == 1 { "" } else { "s" }));
            }
        }
        if let Some(id) = &self.id {
            parts.push(simd_label(id).to_string());
        }
        if let Some(mhz) = self.base_mhz.filter(|m| *m > 0) {
            parts.push(format!("{:.1} GHz", f64::from(mhz) / 1000.0));
        }
        if let Some(chip) = apple_cpu(&self.brand) {
            parts.push("NEON".to_string());
            parts.push(format!("up to {:.1} / {:.1} GHz", f64::from(chip.p_mhz) / 1000.0, f64::from(chip.e_mhz) / 1000.0));
        }
        parts.join(", ")
    }

    /// Performance or efficiency cores: the fastest class of a hybrid CPU is the performance class, the rest are
    /// efficiency cores; a single class is a performance class unless the model is an Atom-only part.
    fn kind_of(&self, c: &CoreClass) -> CoreKind {
        if self.hybrid() {
            let top = self.classes.iter().map(|x| x.efficiency_class).max().unwrap_or(0);
            return if c.efficiency_class == top { CoreKind::Performance } else { CoreKind::Efficiency };
        }
        match &self.id {
            Some(id) if id.vendor == CpuVendor::Intel && id.family == 6 && intel_atom_only(id.model) => CoreKind::Efficiency,
            _ => CoreKind::Performance,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreKind {
    Performance,
    Efficiency,
}

fn simd_label(id: &CpuId) -> &'static str {
    if id.avx512f {
        "AVX-512"
    } else if id.avx2 && id.fma {
        "AVX2"
    } else if id.avx {
        "AVX"
    } else {
        "SSE"
    }
}

// ------------------------------------------------------------------------------------------- Apple silicon

/// FP32 FLOP per cycle of an Apple performance core (4 x 128-bit FMA pipes) and efficiency core (2 x 128-bit).
const APPLE_P_FLOPS: u32 = 32;
const APPLE_E_FLOPS: u32 = 16;

/// One Apple chip: its brand string and the highest clocks of its performance and efficiency cores (MHz).
struct AppleCpu {
    brand: &'static str,
    p_mhz: u32,
    e_mhz: u32,
}

/// Clocks as third parties measured them (Apple publishes none); the source next to each row. FLOP per cycle:
/// 4 x 128-bit pipes in the performance cores (Firestorm, Avalanche, Everest), 2 in the efficiency cores
/// (Icestorm, Blizzard, Sawtooth): https://www.anandtech.com/show/16226/apple-silicon-m1-a14-deep-dive/2,
/// https://dougallj.github.io/applecpu/icestorm.html, https://en.wikipedia.org/wiki/Comparison_of_ARM_processors.
/// The M4's cores are listed as Everest and Sawtooth in its device tree. Not listed: chips without a sourced
/// clock for both core types (M3 Ultra) or without a sourced pipe count (M5).
const APPLE_CPUS: &[AppleCpu] = &[
    // https://www.notebookcheck.net/Apple-M1-Processor-Benchmarks-and-Specs.503613.0.html
    AppleCpu { brand: "Apple M1", p_mhz: 3228, e_mhz: 2064 },
    // https://www.notebookcheck.net/Apple-M1-Pro-Processor-Benchmarks-and-Specs.579915.0.html
    AppleCpu { brand: "Apple M1 Pro", p_mhz: 3220, e_mhz: 2060 },
    // https://www.notebookcheck.net/Apple-M1-Max-Processor-Benchmarks-and-Specs.579971.0.html
    AppleCpu { brand: "Apple M1 Max", p_mhz: 3220, e_mhz: 2060 },
    // https://en.wikipedia.org/wiki/Apple_M1
    AppleCpu { brand: "Apple M1 Ultra", p_mhz: 3220, e_mhz: 2060 },
    // https://www.notebookcheck.net/Apple-M2-Processor-Benchmarks-and-Specs.632312.0.html
    AppleCpu { brand: "Apple M2", p_mhz: 3500, e_mhz: 2400 },
    // https://www.notebookcheck.net/Apple-M2-Pro-Processor-Benchmarks-and-Specs.682450.0.html
    AppleCpu { brand: "Apple M2 Pro", p_mhz: 3700, e_mhz: 2420 },
    // https://www.notebookcheck.net/Apple-M2-Max-Processor-Benchmarks-and-Specs.682771.0.html
    AppleCpu { brand: "Apple M2 Max", p_mhz: 3700, e_mhz: 2420 },
    // https://en.wikipedia.org/wiki/Apple_M2
    AppleCpu { brand: "Apple M2 Ultra", p_mhz: 3700, e_mhz: 2420 },
    // https://www.notebookcheck.net/Apple-MacBook-Air-13-M3-review-A-lot-faster-and-with-Wi-Fi-6E.811129.0.html
    AppleCpu { brand: "Apple M3", p_mhz: 4056, e_mhz: 2748 },
    // https://www.notebookcheck.net/Apple-MacBook-Pro-14-2023-M3-Pro-review-Improved-runtimes-and-better-performance.779538.0.html
    AppleCpu { brand: "Apple M3 Pro", p_mhz: 4056, e_mhz: 2748 },
    // https://www.notebookcheck.net/Apple-M3-Max-16-Core-Processor-Benchmarks-and-Specs.781712.0.html (P),
    // https://en.wikipedia.org/wiki/MacBook_Pro_(Apple_silicon) (E)
    AppleCpu { brand: "Apple M3 Max", p_mhz: 4056, e_mhz: 2570 },
    // https://notebookcheck.net/Apple-M4-10-cores-Processor-Benchmarks-and-Specs.835975.0.html
    AppleCpu { brand: "Apple M4", p_mhz: 4400, e_mhz: 2900 },
    // https://www.notebookcheck.net/Apple-M4-Pro-12-cores-Processor-Benchmarks-and-Specs.922301.0.html
    AppleCpu { brand: "Apple M4 Pro", p_mhz: 4510, e_mhz: 2590 },
    // https://www.notebookcheck.net/Apple-M4-Max-16-cores-Processor-Benchmarks-and-Specs.920458.0.html
    AppleCpu { brand: "Apple M4 Max", p_mhz: 4510, e_mhz: 2590 },
];

/// The table row of an Apple chip ("Apple M4 Pro"), by its exact brand string.
fn apple_cpu(brand: &str) -> Option<&'static AppleCpu> {
    let b = brand.trim();
    APPLE_CPUS.iter().find(|c| c.brand.eq_ignore_ascii_case(b))
}

// ------------------------------------------------------------------------------------------- FLOP table

/// FP32 FLOP per cycle per core for a CPU of this family / model and kind of core.
fn fp32_flops_per_cycle(id: &CpuId, brand: &str, kind: CoreKind) -> u32 {
    match id.vendor {
        CpuVendor::Amd => amd_flops(id),
        CpuVendor::Intel => match kind {
            CoreKind::Efficiency => intel_e_core_flops(id),
            CoreKind::Performance => intel_p_core_flops(id, brand),
        },
        CpuVendor::Other => by_features(id),
    }
}

/// The estimate from the SIMD flags alone (an unknown maker or microarchitecture): two 256-bit FMA units with
/// AVX2 + FMA, AVX without FMA 16, otherwise one 128-bit multiply-add pair.
fn by_features(id: &CpuId) -> u32 {
    if id.avx2 && id.fma {
        32
    } else if id.avx {
        16
    } else {
        8
    }
}

fn amd_flops(id: &CpuId) -> u32 {
    match id.family {
        // Zen 1 / Zen+ (Summit / Pinnacle Ridge, Raven / Picasso, Dali: models below 0x30) have 128-bit FMA units;
        // Zen 2 (Rome 0x31, Renoir 0x60, Matisse 0x71, Van Gogh 0x90, Mendocino 0xA0) doubled them.
        0x17 => {
            if id.model < 0x30 {
                16
            } else {
                32
            }
        }
        // Zen 3 (Milan, Vermeer, Rembrandt, Cezanne) and Zen 4 (Genoa, Raphael, Phoenix, Bergamo): 2 x 256-bit.
        0x19 => 32,
        // Zen 5 and later. Strix Point (0x20..) and Krackan (0x60..) keep the double-pumped 256-bit datapath of
        // Zen 4; Granite Ridge, Turin and Strix Halo have the full 512-bit one.
        f if f >= 0x1A => match id.model {
            0x20..=0x2F | 0x60..=0x6F if f == 0x1A => 32,
            _ if id.avx512f => 64,
            _ => 32,
        },
        // Bulldozer-class modules and Jaguar share or halve the vector unit; Phenom and older have 128-bit SSE.
        _ => 8,
    }
}

/// Intel P-cores (every core of a CPU without efficiency cores).
fn intel_p_core_flops(id: &CpuId, brand: &str) -> u32 {
    if id.avx512f && id.family == 6 {
        let server = matches!(id.model, 0x55 | 0x6A | 0x6C | 0x8F | 0xCF | 0xAD | 0xAE);
        let phi = matches!(id.model, 0x57 | 0x85);
        if phi {
            return 64;
        }
        if server && two_avx512_units(id.model, brand) {
            return 64;
        }
        // AVX-512 with one 512-bit unit (Ice Lake / Tiger Lake / Rocket Lake client, Silver / Bronze Xeons) is as
        // wide as two 256-bit units.
        return 32;
    }
    by_features(id)
}

/// Whether a Xeon (or Skylake-X Core i9) has two 512-bit FMA units. Silver, Bronze, Core i7 and the small Gold
/// 5000 / 4000 / 3000 series have one; a brand that names no tier counts as the big server part.
fn two_avx512_units(model: u32, brand: &str) -> bool {
    let b = brand.to_ascii_lowercase();
    if b.contains("silver") || b.contains("bronze") {
        return false;
    }
    if let Some(i) = b.find("gold") {
        let digit = b[i + 4..].trim_start().chars().next();
        return matches!(digit, Some('6'..='9'));
    }
    if model == 0x55 && b.contains("core") {
        // Skylake-X / Cascade Lake-X HEDT: the Core i9 has both units, the i7 one.
        return b.contains("i9");
    }
    true
}

/// Intel efficiency cores: Gracemont (Alder / Raptor Lake, Alder Lake-N) and Crestmont (Meteor Lake, Sierra
/// Forest) have two 128-bit FMA units; Skymont (Arrow Lake, Lunar Lake) has four.
fn intel_e_core_flops(id: &CpuId) -> u32 {
    if id.family != 6 {
        return by_features(id).min(16);
    }
    match id.model {
        0xB5 | 0xC5 | 0xC6 | 0xBD => 32,
        _ if id.avx2 && id.fma => 16,
        _ => 8,
    }
}

/// Intel parts made only of efficiency cores (Atom family, Alder Lake-N, Sierra Forest).
fn intel_atom_only(model: u32) -> bool {
    matches!(model, 0x37 | 0x4C | 0x4D | 0x5A | 0x5C | 0x5F | 0x7A | 0x86 | 0x96 | 0x9C | 0xAF | 0xBE)
}

// ------------------------------------------------------------------------------------------- CPUID

/// Vendor, family, model and SIMD flags from CPUID. None off x86 / x86_64.
pub fn read_cpuid() -> Option<CpuId> {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        #[cfg(target_arch = "x86")]
        use std::arch::x86::__cpuid;
        #[cfg(target_arch = "x86_64")]
        use std::arch::x86_64::__cpuid;
        // CPUID is available on every x86_64 CPU and every x86 CPU Rust targets.
        #[allow(unused_unsafe)]
        let leaf0 = unsafe { __cpuid(0) };
        let mut vendor = [0u8; 12];
        vendor[0..4].copy_from_slice(&leaf0.ebx.to_le_bytes());
        vendor[4..8].copy_from_slice(&leaf0.edx.to_le_bytes());
        vendor[8..12].copy_from_slice(&leaf0.ecx.to_le_bytes());
        let vendor = match &vendor {
            b"AuthenticAMD" => CpuVendor::Amd,
            b"GenuineIntel" => CpuVendor::Intel,
            _ => CpuVendor::Other,
        };
        if leaf0.eax < 1 {
            return None;
        }
        #[allow(unused_unsafe)]
        let eax = unsafe { __cpuid(1) }.eax;
        let stepping = eax & 0xF;
        let base_model = (eax >> 4) & 0xF;
        let base_family = (eax >> 8) & 0xF;
        let ext_model = (eax >> 16) & 0xF;
        let ext_family = (eax >> 20) & 0xFF;
        let family = if base_family == 0xF { base_family + ext_family } else { base_family };
        let model = if base_family == 0x6 || base_family == 0xF { base_model | (ext_model << 4) } else { base_model };
        // std's detection also checks that the OS saves the YMM / ZMM state (XGETBV).
        Some(CpuId {
            vendor,
            family,
            model,
            stepping,
            avx: std::is_x86_feature_detected!("avx"),
            avx2: std::is_x86_feature_detected!("avx2"),
            fma: std::is_x86_feature_detected!("fma"),
            avx512f: std::is_x86_feature_detected!("avx512f"),
        })
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    {
        None
    }
}

// ------------------------------------------------------------------------------------------- names

/// "AMD Ryzen 9 9950X3D 16-Core Processor" -> "Ryzen 9 9950X3D"; "13th Gen Intel(R) Core(TM) i9-13900K" ->
/// "Core i9-13900K"; "Intel(R) Xeon(R) Gold 6148 CPU @ 2.40GHz" -> "Xeon Gold 6148". An empty brand gives "CPU".
pub fn display_name(brand: &str) -> String {
    let cleaned = brand.replace("(R)", "").replace("(TM)", "").replace("(r)", "").replace("(tm)", "");
    let mut words: Vec<&str> = Vec::new();
    for w in cleaned.split_whitespace() {
        let l = w.to_ascii_lowercase();
        // Everything after "@ 2.40GHz" is the clock, after "w/ Radeon 780M Graphics" the integrated GPU.
        if matches!(l.as_str(), "@" | "w/" | "with") {
            break;
        }
        // The vendor, the "CPU" / "Processor" suffixes, "16-Core" and "12th Gen" say nothing about the model.
        let ordinal = l.len() > 2
            && ["th", "nd", "rd", "st"].iter().any(|s| l.ends_with(s))
            && l[..l.len() - 2].chars().all(|c| c.is_ascii_digit());
        if matches!(l.as_str(), "cpu" | "processor" | "intel" | "amd" | "gen") || l.ends_with("-core") || l.ends_with("-thread") || ordinal {
            continue;
        }
        words.push(w);
    }
    if words.is_empty() {
        "CPU".to_string()
    } else {
        words.join(" ")
    }
}
