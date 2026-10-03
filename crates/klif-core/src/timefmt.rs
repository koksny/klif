//! Local wall-clock stamps for session names: `yyyyMMdd-HHmmss-fff` (local time).
//! Windows: GetLocalTime. Unix: the C library's `localtime_r` (part of the libc std already links). Elsewhere: UTC.

/// A fixed stamp for plans that are only inspected (ports, display), never launched.
pub const FIXED_STAMP: &str = "20000101-000000-000";

/// Now, as `yyyyMMdd-HHmmss-fff`.
pub fn local_stamp() -> String {
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime only writes the returned SYSTEMTIME.
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
        )
    }
    #[cfg(unix)]
    {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        match unix_local(d.as_secs() as i64) {
            Some((y, mo, dd, h, mi, s)) => format!("{y:04}{mo:02}{dd:02}-{h:02}{mi:02}{s:02}-{:03}", d.subsec_millis()),
            None => utc_stamp(d),
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        utc_stamp(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default())
    }
}

/// `struct tm` as the C library fills it: the nine standard ints first (glibc, musl and the BSDs agree), then
/// room for the platform's extra fields (tm_gmtoff, tm_zone).
#[cfg(unix)]
#[repr(C)]
struct Tm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
    _extra: [u64; 4],
}

#[cfg(unix)]
extern "C" {
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}

/// Local (year, month, day, hour, minute, second) of epoch seconds.
#[cfg(unix)]
fn unix_local(secs: i64) -> Option<(i32, i32, i32, i32, i32, i32)> {
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, _extra: [0; 4] };
    // SAFETY: localtime_r reads the time value and writes only into `tm`, which is larger than the C struct tm.
    let r = unsafe { localtime_r(&secs, &mut tm) };
    (!r.is_null()).then_some((tm.year + 1900, tm.mon + 1, tm.mday, tm.hour, tm.min, tm.sec))
}

/// UTC stamp (fallback without a local-time source).
#[cfg(not(windows))]
fn utc_stamp(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, dd) = civil_from_days(days as i64);
    format!("{y:04}{m:02}{dd:02}-{:02}{:02}{:02}-{:03}", rem / 3600, rem % 3600 / 60, rem % 60, d.subsec_millis())
}

/// Days since 1970-01-01 -> (year, month, day) (Howard Hinnant's algorithm).
#[cfg(not(windows))]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}
