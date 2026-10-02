//! Win32 measurement primitives: DXGI adapters, PDH queries, RAM, CPU name, RAM type.

use crate::Adapter;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW,
    SetupDiGetDeviceRegistryPropertyW, DIGCF_PRESENT, GUID_DEVCLASS_DISPLAY, HDEVINFO, SETUP_DI_REGISTRY_PROPERTY, SPDRP_DEVICEDESC,
    SPDRP_DRIVER, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{DEVPKEY_Device_PowerData, DEVPROPTYPE};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PdhRemoveCounter, PDH_FMT, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER,
    PDH_HQUERY, PDH_MORE_DATA,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, GlobalMemoryStatusEx, MEMORYSTATUSEX, RSMB};

fn wstr(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

// ------------------------------------------------------------------------------------------- DXGI

pub fn dxgi_adapters() -> Vec<Adapter> {
    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for i in 0u32.. {
        let adapter = match unsafe { factory.EnumAdapters1(i) } {
            Ok(a) => a,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(_) => break,
        };
        let Ok(d) = (unsafe { adapter.GetDesc1() }) else { continue };
        if d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        out.push(Adapter {
            name: wstr(&d.Description).trim().to_string(),
            vendor_id: d.VendorId,
            device_id: d.DeviceId,
            luid_high: d.AdapterLuid.HighPart,
            luid_low: d.AdapterLuid.LowPart,
            dedicated_bytes: d.DedicatedVideoMemory as u64,
        });
    }
    out
}

// -------------------------------------------------------------------------------------------- PDH

/// A PDH query. Counter paths are English (`PdhAddEnglishCounterW`), so this works on localized
/// (e.g. pl-PL) systems.
pub struct Pdh {
    q: PDH_HQUERY,
}

impl Pdh {
    pub fn open() -> Option<Self> {
        let mut q = PDH_HQUERY::default();
        let s = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut q) };
        if s == 0 { Some(Pdh { q }) } else { None }
    }

    pub fn add(&self, english_path: &str) -> Option<PDH_HCOUNTER> {
        let mut c = PDH_HCOUNTER::default();
        let p = HSTRING::from(english_path);
        let s = unsafe { PdhAddEnglishCounterW(self.q, &p, 0, &mut c) };
        if s == 0 { Some(c) } else { None }
    }

    pub fn remove(&self, c: PDH_HCOUNTER) {
        unsafe { PdhRemoveCounter(c) };
    }

    pub fn collect(&self) -> bool {
        unsafe { PdhCollectQueryData(self.q) == 0 }
    }

    /// All instances of a wildcard counter as (instance name, value).
    pub fn array(&self, c: PDH_HCOUNTER) -> Vec<(String, f64)> {
        // PDH_FMT_NOCAP100 (0x8000) is not exported by windows 0.62.
        let fmt = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);
        let mut size = 0u32;
        let mut count = 0u32;
        let s = unsafe { PdhGetFormattedCounterArrayW(c, fmt, &mut size, &mut count, None) };
        if s != PDH_MORE_DATA {
            return Vec::new();
        }
        // Items hold pointers and f64s: the buffer must be 8-byte aligned.
        let mut buf = vec![0u64; (size as usize).div_ceil(8) + 1];
        let s = unsafe {
            PdhGetFormattedCounterArrayW(c, fmt, &mut size, &mut count, Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W))
        };
        if s != 0 {
            return Vec::new();
        }
        let items = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize) };
        items
            .iter()
            .filter(|it| it.FmtValue.CStatus == 0)
            .map(|it| (unsafe { pwstr(it.szName) }, unsafe { it.FmtValue.Anonymous.doubleValue }))
            .collect()
    }

    pub fn single(&self, c: PDH_HCOUNTER) -> Option<f64> {
        let mut v = PDH_FMT_COUNTERVALUE::default();
        let s = unsafe { PdhGetFormattedCounterValue(c, PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000), None, &mut v) };
        if s == 0 && v.CStatus == 0 { Some(unsafe { v.Anonymous.doubleValue }) } else { None }
    }
}

unsafe fn pwstr(p: PWSTR) -> String {
    if p.is_null() { String::new() } else { unsafe { p.to_string().unwrap_or_default() } }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.q) };
    }
}

/// PID from a `GPU Process Memory` instance name (`pid_1234_luid_0x..._phys_0`).
pub fn pid_of(inst: &str) -> Option<u32> {
    inst.strip_prefix("pid_")?.split('_').next()?.parse().ok()
}

// --------------------------------------------------------------------------- SetupDi: device power

/// The inference card's device node in the display class (matched by "VEN_xxxx&DEV_yyyy" in its
/// instance id). Kept open by the sampler: reading the power state is then one
/// `SetupDiGetDevicePropertyW` (a PnP manager query, it never touches the device).
pub struct DisplayDevice {
    set: HDEVINFO,
    data: SP_DEVINFO_DATA,
    pub instance_id: String,
}

// HDEVINFO is a handle owned by this value; SP_DEVINFO_DATA is plain data.
unsafe impl Send for DisplayDevice {}

impl DisplayDevice {
    pub fn open(vendor_id: u32, device_id: u32) -> Option<DisplayDevice> {
        let set = unsafe { SetupDiGetClassDevsW(Some(&GUID_DEVCLASS_DISPLAY), PCWSTR::null(), None, DIGCF_PRESENT) }.ok()?;
        let needle = format!("VEN_{vendor_id:04X}&DEV_{device_id:04X}");
        for i in 0u32..64 {
            let mut data = SP_DEVINFO_DATA { cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
            if unsafe { SetupDiEnumDeviceInfo(set, i, &mut data) }.is_err() {
                break;
            }
            let mut buf = vec![0u16; 512];
            let mut need = 0u32;
            if unsafe { SetupDiGetDeviceInstanceIdW(set, &data, Some(&mut buf), Some(&mut need)) }.is_err() {
                continue;
            }
            let id = wstr(&buf);
            if id.to_ascii_uppercase().contains(&needle) {
                return Some(DisplayDevice { set, data, instance_id: id });
            }
        }
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(set);
        }
        None
    }

    /// `DEVPKEY_Device_PowerData` -> `CM_POWER_DATA.PD_MostRecentPowerState` (raw: 1 = D0 .. 4 = D3).
    pub fn power_state_raw(&self) -> Option<u32> {
        let mut ty = DEVPROPTYPE::default();
        let mut buf = [0u8; 64];
        let mut need = 0u32;
        unsafe { SetupDiGetDevicePropertyW(self.set, &self.data, &DEVPKEY_Device_PowerData, &mut ty, Some(&mut buf), Some(&mut need), 0) }
            .ok()?;
        // CM_POWER_DATA: PD_Size (u32), PD_MostRecentPowerState (u32), ...
        if (need as usize) < 8 {
            return None;
        }
        Some(u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]))
    }

    fn registry_string(&self, prop: SETUP_DI_REGISTRY_PROPERTY) -> Option<String> {
        let mut buf = vec![0u8; 1024];
        let mut need = 0u32;
        unsafe { SetupDiGetDeviceRegistryPropertyW(self.set, &self.data, prop, None, Some(&mut buf), Some(&mut need)) }.ok()?;
        let n = (need as usize).min(buf.len()) / 2;
        let w: Vec<u16> = buf[..n * 2].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let s = wstr(&w).trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    /// The device's driver key below the Class key, e.g. "{4d36e968-e325-11ce-bfc1-08002be10318}\0000".
    pub fn driver_key(&self) -> Option<String> {
        self.registry_string(SPDRP_DRIVER)
    }

    /// The device description (the driver's DriverDesc), e.g. "AMD Radeon RX 9070 XT".
    pub fn device_desc(&self) -> Option<String> {
        self.registry_string(SPDRP_DEVICEDESC)
    }
}

impl Drop for DisplayDevice {
    fn drop(&mut self) {
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.set);
        }
    }
}

pub const CLASS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Class";
pub const DISPLAY_CLASS: &str = "{4d36e968-e325-11ce-bfc1-08002be10318}";

/// A REG_SZ value under HKLM (read-only).
pub fn reg_sz(subkey: &str, value: &str) -> Option<String> {
    let mut buf = vec![0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let s = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(subkey),
            &HSTRING::from(value),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if s.0 != 0 {
        return None;
    }
    Some(wstr(&buf).trim().to_string())
}

/// A REG_DWORD value under HKLM (read-only). None when the key or value cannot be read.
pub fn reg_dword(subkey: &str, value: &str) -> Option<u32> {
    let mut v = 0u32;
    let mut size = 4u32;
    let s = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(subkey),
            &HSTRING::from(value),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if s.0 == 0 { Some(v) } else { None }
}

// ------------------------------------------------------------------------------------- RAM / CPU

/// (total bytes, available bytes).
pub fn memory_status() -> Option<(u64, u64)> {
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    Some((m.ullTotalPhys, m.ullAvailPhys))
}

/// `ProcessorNameString` of CPU 0 from the registry.
pub fn cpu_brand() -> Option<String> {
    let mut buf = vec![0u16; 256];
    let mut size = (buf.len() * 2) as u32;
    let s = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"),
            &HSTRING::from("ProcessorNameString"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if s.0 != 0 {
        return None;
    }
    let s = wstr(&buf).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
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

/// Memory type from SMBIOS type 17 records (one firmware-table read at startup).
pub fn ram_type() -> Option<String> {
    let n = unsafe { GetSystemFirmwareTable(RSMB, 0, None) };
    if n == 0 || n > 4 * 1024 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; n as usize];
    let got = unsafe { GetSystemFirmwareTable(RSMB, 0, Some(&mut buf)) };
    if got == 0 || got as usize > buf.len() {
        return None;
    }
    // RawSMBIOSData: 4 bytes of version info, u32 length, then the structure table.
    if buf.len() < 8 {
        return None;
    }
    let len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    let table = &buf[8..(8 + len).min(buf.len())];
    let mut i = 0usize;
    while i + 4 <= table.len() {
        let typ = table[i];
        let flen = table[i + 1] as usize;
        if flen < 4 || i + flen > table.len() {
            break;
        }
        if typ == 17 && flen > 0x12 {
            let size = u16::from_le_bytes([table[i + 0x0C], table[i + 0x0D]]);
            let mtype = table[i + 0x12];
            if size != 0 && size != 0xFFFF {
                if let Some(name) = smbios_mem_type(mtype) {
                    return Some(name.to_string());
                }
            }
        }
        if typ == 127 {
            break;
        }
        // Skip the formatted area, then the string set (ends with a double NUL).
        let mut j = i + flen;
        while j + 1 < table.len() && !(table[j] == 0 && table[j + 1] == 0) {
            j += 1;
        }
        i = j + 2;
    }
    None
}

fn smbios_mem_type(t: u8) -> Option<&'static str> {
    Some(match t {
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => return None,
    })
}
