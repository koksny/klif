//! UI GPU pinning: pick the adapter the UI renders on, build the WebView2 browser arguments, and verify
//! after startup that the WebView2 GPU process really sits on that adapter (a malformed or stale pin
//! silently falls back to WARP, so it must be checked).
//!
//! Adapters are matched by PCI "VEN:DEV" (LUIDs change every boot). Resolution goes through
//! `klif_telemetry::find_adapter`. The local DXGI enumeration below names whatever adapter the GPU process was
//! found on (including the software renderer).

use klif_common::config::Config;
use klif_telemetry::Adapter;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    PDH_FMT, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

use crate::shell::GpuReport;

/// wry's own default browser arguments, which it applies ONLY when no arguments are set. Re-appended here.
pub const WRY_DEFAULT_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// One DXGI adapter as the shell sees it (software adapters included, for naming).
#[derive(Debug, Clone)]
pub struct DxgiAdapter {
    pub adapter: Adapter,
    pub software: bool,
}

fn wstr(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n]).trim().to_string()
}

/// Every DXGI adapter (EnumAdapters1), software ones flagged.
pub fn dxgi_adapters() -> Vec<DxgiAdapter> {
    let mut out = Vec::new();
    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(e) => {
            log::warn!("CreateDXGIFactory1 failed: {e}");
            return out;
        }
    };
    for i in 0.. {
        let a = match unsafe { factory.EnumAdapters1(i) } {
            Ok(a) => a,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => {
                log::warn!("EnumAdapters1({i}) failed: {e}");
                break;
            }
        };
        let Ok(d) = (unsafe { a.GetDesc1() }) else { continue };
        out.push(DxgiAdapter {
            adapter: Adapter {
                name: wstr(&d.Description),
                vendor_id: d.VendorId,
                device_id: d.DeviceId,
                luid_high: d.AdapterLuid.HighPart,
                luid_low: d.AdapterLuid.LowPart,
                dedicated_bytes: d.DedicatedVideoMemory as u64,
                revision: d.Revision,
                subsys_id: d.SubSysId,
                shared_bytes: d.SharedSystemMemory as u64,
            },
            software: (d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0,
        });
    }
    out
}

/// One log line per DXGI adapter (startup diagnostics).
pub fn log_adapters() {
    for a in dxgi_adapters() {
        log::info!("adapter {}{}", describe(&a.adapter), if a.software { " (software)" } else { "" });
    }
}

/// "VEN:DEV" (hex, optional 0x prefixes) -> (vendor, device).
pub fn parse_pci(s: &str) -> Option<(u32, u32)> {
    let (v, d) = s.trim().split_once(':')?;
    let hex = |x: &str| {
        let x = x.trim();
        let x = x.strip_prefix("0x").or_else(|| x.strip_prefix("0X")).unwrap_or(x);
        u32::from_str_radix(x, 16).ok()
    };
    Some((hex(v)?, hex(d)?))
}

fn find_adapter(pci: &str) -> Option<Adapter> {
    klif_telemetry::find_adapter(pci)
}

/// The adapter the UI must render on.
/// `KLIF_UI_ADAPTER` (VEN:DEV, or "none" to disable the pin) overrides `[gpu] ui`; without either, the
/// first hardware adapter that is not the inference card. None = no pin (WebView2 picks).
pub fn resolve_ui_adapter(cfg: &Config) -> Option<Adapter> {
    let env = std::env::var("KLIF_UI_ADAPTER").ok().filter(|s| !s.trim().is_empty());
    if env.as_deref().map(|s| s.trim().eq_ignore_ascii_case("none")).unwrap_or(false) {
        log::info!("UI adapter pin disabled by KLIF_UI_ADAPTER=none");
        return None;
    }
    let wanted = env.or_else(|| cfg.gpu.ui.clone());
    if let Some(pci) = wanted {
        let found = find_adapter(&pci);
        if found.is_none() {
            log::warn!("UI adapter {pci} not found; no pin (WebView2 picks the adapter)");
        }
        return found;
    }
    let inference = cfg.gpu.inference.as_deref().and_then(parse_pci);
    let auto = dxgi_adapters()
        .into_iter()
        .filter(|a| !a.software)
        .map(|a| a.adapter)
        .find(|a| Some((a.vendor_id, a.device_id)) != inference);
    if auto.is_none() {
        log::info!("no UI adapter configured and no second adapter found; no pin");
    }
    auto
}

/// WebView2 additional browser arguments: the LUID pin (if any) plus wry's defaults.
pub fn browser_args(ui: Option<&Adapter>) -> String {
    match ui {
        Some(a) => format!("{} {}", a.chromium_luid_arg(), WRY_DEFAULT_ARGS),
        None => WRY_DEFAULT_ARGS.to_string(),
    }
}

pub fn describe(a: &Adapter) -> String {
    format!("{} [{:04X}:{:04X}, {}]", a.name, a.vendor_id, a.device_id, a.pdh_luid())
}

// ---------------------------------------------------------------------------------------------- PDH

struct Pdh(PDH_HQUERY);

impl Pdh {
    fn open() -> Option<Pdh> {
        let mut q = PDH_HQUERY::default();
        let s = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut q) };
        (s == 0).then_some(Pdh(q))
    }

    fn add(&self, english_path: &str) -> Result<PDH_HCOUNTER, u32> {
        let mut c = PDH_HCOUNTER::default();
        let s = unsafe { PdhAddEnglishCounterW(self.0, &HSTRING::from(english_path), 0, &mut c) };
        if s == 0 { Ok(c) } else { Err(s) }
    }

    fn collect(&self) -> u32 {
        unsafe { PdhCollectQueryData(self.0) }
    }

    fn array(&self, c: PDH_HCOUNTER) -> Result<Vec<(String, f64)>, u32> {
        // PDH_FMT_NOCAP100 (0x8000) is not exported by windows 0.62.
        let fmt = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);
        let (mut size, mut n) = (0u32, 0u32);
        let s = unsafe { PdhGetFormattedCounterArrayW(c, fmt, &mut size, &mut n, None) };
        #[allow(clippy::unnecessary_cast)]
        if s != PDH_MORE_DATA as u32 {
            return if s == 0 { Ok(Vec::new()) } else { Err(s) };
        }
        // Items hold pointers and f64: the buffer must be 8-byte aligned.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let s = unsafe {
            PdhGetFormattedCounterArrayW(c, fmt, &mut size, &mut n, Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W))
        };
        if s != 0 {
            return Err(s);
        }
        let items = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, n as usize) };
        Ok(items
            .iter()
            .map(|it| {
                let name = if it.szName.is_null() { String::new() } else { unsafe { it.szName.to_string().unwrap_or_default() } };
                (name, unsafe { it.FmtValue.Anonymous.doubleValue })
            })
            .collect())
    }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.0);
        }
    }
}

/// The "luid_0x..._0x..." fragment of a PDH GPU instance name.
fn luid_fragment(instance: &str) -> Option<&str> {
    let i = instance.find("luid_0x")?;
    let rest = &instance[i..];
    // luid_0xHHHHHHHH_0xLLLLLLLL = 5 + 10 + 1 + 10
    rest.get(..26)
}

/// Which adapter a process has its GPU memory on: the PDH "GPU Process Memory" instance of that PID with
/// the most dedicated memory. Returns the luid fragment and the dedicated bytes.
pub fn process_adapter_luid(pid: u32) -> Option<(String, f64)> {
    let pdh = Pdh::open()?;
    let c = pdh.add(r"\GPU Process Memory(*)\Dedicated Usage").ok()?;
    let prefix = format!("pid_{pid}_");
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        pdh.collect();
        let rows = pdh.array(c).unwrap_or_default();
        let best = rows
            .iter()
            .filter(|(n, _)| n.starts_with(&prefix))
            .filter_map(|(n, v)| luid_fragment(n).map(|l| (l.to_string(), *v)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if best.is_some() {
            return best;
        }
    }
    None
}

/// Compare where the GPU process `pid` sits with the wanted adapter.
pub fn check_gpu_process(pid: Option<u32>, want: Option<&Adapter>) -> GpuReport {
    let expected = want.map(|a| a.name.clone());
    let Some(pid) = pid else {
        return GpuReport { ok: false, adapter: None, expected, pid: None };
    };
    let Some((luid, bytes)) = process_adapter_luid(pid) else {
        log::warn!("GPU process {pid}: no PDH GPU Process Memory instance");
        return GpuReport { ok: false, adapter: None, expected, pid: Some(pid) };
    };
    let all = dxgi_adapters();
    let on = all.iter().find(|a| a.adapter.pdh_luid() == luid);
    let name = on.map(|a| a.adapter.name.clone()).unwrap_or_else(|| format!("unknown adapter {luid}"));
    let ok = match want {
        Some(w) => w.pdh_luid() == luid,
        None => on.map(|a| !a.software).unwrap_or(false),
    };
    log::info!(
        "GPU process pid {pid} renders on {name} ({luid}, {:.1} MiB dedicated); wanted {}: {}",
        bytes / 1048576.0,
        want.map(describe).unwrap_or_else(|| "no pin".into()),
        if ok { "VERIFIED" } else { "MISMATCH" }
    );
    GpuReport { ok, adapter: Some(name), expected, pid: Some(pid) }
}
