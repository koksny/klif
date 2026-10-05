//! macOS readers (Apple silicon): the GPU from Metal and the IORegistry, the CPU and RAM from sysctl and mach, a
//! process's memory from `proc_pid_rusage`. Nothing here needs root (no `powermetrics`).
//!
//! - GPU: Metal's default device (`MTLCreateSystemDefaultDevice`, created once and kept) gives the name and
//!   `recommendedMaxWorkingSetSize`, the memory the GPU may use (about two thirds of RAM on a 16 GB Mac; macOS sets
//!   it, `iogpu.wired_limit_mb` changes it). The `AGXAccelerator` service gives the core count (`gpu-core-count`,
//!   it differs within one chip name) and the live `PerformanceStatistics` ("In use system memory"). The device
//!   tree gives the SoC id (`platform-name`, "t8132" for an M4) and the GPU's DVFS table (`arm-io/pmgr`
//!   `voltage-states9`: pairs of u32 frequency in Hz and voltage in mV), whose top state is its maximum clock.
//! - CPU: `machdep.cpu.brand_string` ("Apple M4"), `hw.perflevel<n>.physicalcpu` per core class (0 = the
//!   performance cores). macOS reports no clocks (see `cpu.rs` for the estimate).
//! - RAM: `hw.memsize`; available = total minus what Activity Monitor calls "Memory Used" (app memory, wired,
//!   compressed): file cache and purgeable memory count as available.
//! - A process: its resident memory (mapped model files included) and its footprint (`process_memory`).

use core_foundation_sys::base::{kCFAllocatorDefault, CFGetTypeID, CFRelease, CFTypeRef};
use core_foundation_sys::data::{CFDataGetBytePtr, CFDataGetLength, CFDataGetTypeID};
use core_foundation_sys::dictionary::{CFDictionaryGetTypeID, CFDictionaryGetValue, CFDictionaryRef, CFMutableDictionaryRef};
use core_foundation_sys::number::{kCFNumberSInt64Type, CFNumberGetTypeID, CFNumberGetValue, CFNumberRef};
use core_foundation_sys::string::{kCFStringEncodingUTF8, CFStringCreateWithBytes, CFStringGetCString, CFStringGetTypeID, CFStringRef};
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::OnceLock;

// ------------------------------------------------------------------------------------------- sysctl

fn sysctl_raw(name: &str, buf: &mut [u8]) -> Option<usize> {
    let name = CString::new(name).ok()?;
    let mut len = buf.len();
    let r = unsafe { libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
    (r == 0).then_some(len)
}

pub fn sysctl_u64(name: &str) -> Option<u64> {
    let mut b = [0u8; 8];
    match sysctl_raw(name, &mut b)? {
        8 => Some(u64::from_ne_bytes(b)),
        4 => Some(u64::from(u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))),
        _ => None,
    }
}

pub fn sysctl_string(name: &str) -> Option<String> {
    let mut b = [0u8; 256];
    let n = sysctl_raw(name, &mut b)?;
    let s = CStr::from_bytes_until_nul(&b[..n]).ok()?.to_string_lossy().trim().to_string();
    (!s.is_empty()).then_some(s)
}

// ------------------------------------------------------------------------------------------- IOKit

#[allow(non_camel_case_types)]
type io_object_t = u32;
/// `kIOMainPortDefault`.
const MAIN_PORT: u32 = 0;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingService(main_port: u32, matching: CFDictionaryRef) -> io_object_t;
    fn IORegistryEntryFromPath(main_port: u32, path: *const c_char) -> io_object_t;
    fn IORegistryEntryCreateCFProperty(entry: io_object_t, key: CFStringRef, allocator: *const c_void, options: u32) -> CFTypeRef;
    fn IOObjectRelease(object: io_object_t) -> i32;
}

/// A CF object this code owns (released on drop).
struct Cf(CFTypeRef);

impl Drop for Cf {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

fn cf_str(s: &str) -> Option<Cf> {
    let r = unsafe { CFStringCreateWithBytes(kCFAllocatorDefault, s.as_ptr(), s.len() as _, kCFStringEncodingUTF8, 0) };
    (!r.is_null()).then(|| Cf(r.cast()))
}

fn cf_i64(v: CFTypeRef) -> Option<i64> {
    if v.is_null() || unsafe { CFGetTypeID(v) } != unsafe { CFNumberGetTypeID() } {
        return None;
    }
    let mut out: i64 = 0;
    let ok = unsafe { CFNumberGetValue(v as CFNumberRef, kCFNumberSInt64Type, (&raw mut out).cast()) };
    ok.then_some(out)
}

fn cf_string(v: CFTypeRef) -> Option<String> {
    if v.is_null() || unsafe { CFGetTypeID(v) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    let mut buf = [0 as c_char; 512];
    let ok = unsafe { CFStringGetCString(v as CFStringRef, buf.as_mut_ptr(), buf.len() as _, kCFStringEncodingUTF8) };
    (ok != 0).then(|| unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

fn cf_bytes(v: CFTypeRef) -> Option<Vec<u8>> {
    if v.is_null() || unsafe { CFGetTypeID(v) } != unsafe { CFDataGetTypeID() } {
        return None;
    }
    let (p, n) = unsafe { (CFDataGetBytePtr(v.cast()), CFDataGetLength(v.cast())) };
    (!p.is_null() && n >= 0).then(|| unsafe { std::slice::from_raw_parts(p, n as usize) }.to_vec())
}

/// An IORegistry entry (released on drop).
struct Entry(io_object_t);

impl Drop for Entry {
    fn drop(&mut self) {
        unsafe {
            IOObjectRelease(self.0);
        }
    }
}

impl Entry {
    /// The first service of an IOKit class.
    fn service(class: &CStr) -> Option<Entry> {
        let matching = unsafe { IOServiceMatching(class.as_ptr()) };
        if matching.is_null() {
            return None;
        }
        // IOServiceGetMatchingService consumes the matching dictionary.
        let e = unsafe { IOServiceGetMatchingService(MAIN_PORT, matching as CFDictionaryRef) };
        (e != 0).then_some(Entry(e))
    }

    /// An entry by path ("IODeviceTree:/arm-io/pmgr").
    fn path(path: &CStr) -> Option<Entry> {
        let e = unsafe { IORegistryEntryFromPath(MAIN_PORT, path.as_ptr()) };
        (e != 0).then_some(Entry(e))
    }

    fn property(&self, key: &str) -> Option<Cf> {
        let k = cf_str(key)?;
        let v = unsafe { IORegistryEntryCreateCFProperty(self.0, k.0 as CFStringRef, kCFAllocatorDefault, 0) };
        (!v.is_null()).then_some(Cf(v))
    }

    fn number(&self, key: &str) -> Option<i64> {
        cf_i64(self.property(key)?.0)
    }

    fn string(&self, key: &str) -> Option<String> {
        cf_string(self.property(key)?.0)
    }

    fn data(&self, key: &str) -> Option<Vec<u8>> {
        cf_bytes(self.property(key)?.0)
    }

    /// Numbers of a dictionary property (`None` for a key that is missing or not a number).
    fn dict_numbers(&self, key: &str, names: &[&str]) -> Option<Vec<Option<i64>>> {
        let d = self.property(key)?;
        if unsafe { CFGetTypeID(d.0) } != unsafe { CFDictionaryGetTypeID() } {
            return None;
        }
        Some(
            names
                .iter()
                .map(|n| {
                    let k = cf_str(n)?;
                    cf_i64(unsafe { CFDictionaryGetValue(d.0 as CFDictionaryRef, k.0) })
                })
                .collect(),
        )
    }
}

// ------------------------------------------------------------------------------------------- Metal

#[link(name = "Metal", kind = "framework")]
extern "C" {
    fn MTLCreateSystemDefaultDevice() -> *mut c_void;
}

#[link(name = "objc")]
extern "C" {
    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

/// The default Metal device, created once (a few milliseconds; it does not keep the GPU busy) and never released.
fn metal_device() -> Option<usize> {
    static DEVICE: OnceLock<usize> = OnceLock::new();
    let d = *DEVICE.get_or_init(|| unsafe { MTLCreateSystemDefaultDevice() } as usize);
    (d != 0).then_some(d)
}

/// `[obj sel]` for a method without arguments that returns `R` (an integer, BOOL or object pointer: no struct, so
/// the plain `objc_msgSend` is right on arm64 and x86_64).
unsafe fn send<R>(obj: usize, sel: &CStr) -> R {
    let f: unsafe extern "C" fn(*mut c_void, *const c_void) -> R = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    f(obj as *mut c_void, sel_registerName(sel.as_ptr()))
}

/// What Metal says about the default device.
pub struct MetalFacts {
    pub name: String,
    /// `recommendedMaxWorkingSetSize`.
    pub working_set: u64,
    pub unified: bool,
    pub registry_id: u64,
}

pub fn metal() -> Option<MetalFacts> {
    let d = metal_device()?;
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let name_obj: usize = send(d, c"name");
        let name = if name_obj == 0 {
            String::new()
        } else {
            let p: *const c_char = send(name_obj, c"UTF8String");
            if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
        };
        let facts = MetalFacts {
            name,
            working_set: send(d, c"recommendedMaxWorkingSetSize"),
            unified: send::<bool>(d, c"hasUnifiedMemory"),
            registry_id: send(d, c"registryID"),
        };
        objc_autoreleasePoolPop(pool);
        Some(facts)
    }
}

// ------------------------------------------------------------------------------------------- GPU

/// The Apple GPU: Metal plus the IORegistry.
pub struct AppleGpu {
    pub metal: MetalFacts,
    /// The SoC id as a number: "t8132" -> 0x8132 (the GPU's device id in KLIF's "106B:8132").
    pub soc: Option<u32>,
    pub cores: Option<u32>,
    /// Top state of the GPU's DVFS table.
    pub max_mhz: Option<u32>,
}

/// "t8132" (NUL-padded) -> 0x8132.
fn soc_id() -> Option<u32> {
    let raw = Entry::path(c"IODeviceTree:/")?.data("platform-name")?;
    let s = CStr::from_bytes_until_nul(&raw).ok()?.to_str().ok()?;
    let hex = s.strip_prefix('t').or_else(|| s.strip_prefix('T'))?;
    u32::from_str_radix(hex, 16).ok().filter(|v| *v <= 0xFFFF)
}

/// The GPU's highest DVFS frequency in MHz.
fn gpu_max_mhz() -> Option<u32> {
    let raw = Entry::path(c"IODeviceTree:/arm-io/pmgr")?.data("voltage-states9")?;
    raw.as_chunks::<8>()
        .0
        .iter()
        .map(|p| u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
        .filter(|hz| *hz != u32::MAX)
        .max()
        .map(|hz| hz / 1_000_000)
        .filter(|mhz| *mhz > 0)
}

/// The Apple silicon GPU; None on a Mac without one (an Intel Mac's AMD or Intel GPU is not read here).
pub fn gpu() -> Option<AppleGpu> {
    let metal = metal().filter(|m| m.unified)?;
    let agx = Entry::service(c"AGXAccelerator")?;
    let cores = agx.number("gpu-core-count").and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0);
    Some(AppleGpu { metal, soc: soc_id(), cores, max_mhz: gpu_max_mhz() })
}

/// GPU memory in use now ("In use system memory" of the accelerator's `PerformanceStatistics`), bytes.
pub fn gpu_in_use() -> Option<u64> {
    let v = Entry::service(c"AGXAccelerator")?.dict_numbers("PerformanceStatistics", &["In use system memory"])?;
    v.first().copied().flatten().and_then(|n| u64::try_from(n).ok())
}

/// The model the accelerator names ("Apple M4"), when Metal gives no name.
pub fn gpu_model() -> Option<String> {
    Entry::service(c"AGXAccelerator")?.string("model")
}

// ------------------------------------------------------------------------------------------- host

extern "C" {
    // libSystem; declared here because libc marks its binding deprecated in favour of another crate.
    fn mach_host_self() -> libc::mach_port_t;
}

/// The host port, asked for once (every call adds a reference to the port right).
fn host_port() -> libc::mach_port_t {
    static PORT: OnceLock<libc::mach_port_t> = OnceLock::new();
    *PORT.get_or_init(|| unsafe { mach_host_self() })
}

/// (total bytes, available bytes).
pub fn memory() -> Option<(u64, u64)> {
    let total = sysctl_u64("hw.memsize")?;
    let mut vm: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let r = unsafe { libc::host_statistics64(host_port(), libc::HOST_VM_INFO64, (&raw mut vm).cast(), &mut count) };
    if r != libc::KERN_SUCCESS {
        return Some((total, 0));
    }
    let page = sysctl_u64("hw.pagesize").unwrap_or(16384);
    let app = u64::from(vm.internal_page_count).saturating_sub(u64::from(vm.purgeable_count));
    let used = (app + u64::from(vm.wire_count) + u64::from(vm.compressor_page_count)) * page;
    Some((total, total.saturating_sub(used)))
}

/// Accumulated CPU ticks of every core: (busy, total).
pub fn cpu_ticks() -> Option<(u64, u64)> {
    let mut info: libc::host_cpu_load_info = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    let r = unsafe { libc::host_statistics(host_port(), libc::HOST_CPU_LOAD_INFO, (&raw mut info).cast(), &mut count) };
    if r != libc::KERN_SUCCESS {
        return None;
    }
    let t = info.cpu_ticks.map(u64::from);
    let total: u64 = t.iter().sum();
    Some((total - t[libc::CPU_STATE_IDLE as usize], total))
}

/// "Apple M4".
pub fn cpu_brand() -> Option<String> {
    sysctl_string("machdep.cpu.brand_string")
}

/// Physical cores per performance level, the performance cores first: (level, cores, logical cpus).
pub fn core_levels() -> Vec<(u32, u32, u32)> {
    let levels = sysctl_u64("hw.nperflevels").unwrap_or(0).min(8) as u32;
    (0..levels)
        .filter_map(|l| {
            let cores = sysctl_u64(&format!("hw.perflevel{l}.physicalcpu"))? as u32;
            let logical = sysctl_u64(&format!("hw.perflevel{l}.logicalcpu")).map_or(cores, |v| v as u32);
            (cores > 0).then_some((l, cores, logical))
        })
        .collect()
}

/// A process's memory, bytes: (resident, physical footprint). Resident (`ri_resident_size`) counts the pages of
/// mapped files too: a llama.cpp server maps its weights (`MTL0_Mapped`), so its footprint (`ri_phys_footprint`:
/// dirty, compressed and wired memory, Activity Monitor's "Memory") holds only the KV cache and the compute buffers,
/// while resident matches the server's own memory breakdown. None when the process is gone or not inspectable.
pub fn process_memory(pid: u32) -> Option<(u64, u64)> {
    let mut ri: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V2, (&raw mut ri).cast()) };
    (r == 0).then_some((ri.ri_resident_size, ri.ri_phys_footprint))
}
