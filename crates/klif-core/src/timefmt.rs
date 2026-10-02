//! Local wall-clock stamps for session names (the old GUI used local time: yyyyMMdd-HHmmss-fff).

use klif_catalog::chrono_like::LocalStamp;

/// A fixed stamp for plans that are only inspected (ports, display), never launched.
pub const FIXED_STAMP: LocalStamp = LocalStamp { year: 2000, month: 1, day: 1, hour: 0, minute: 0, second: 0, millis: 0 };

pub fn local_stamp() -> LocalStamp {
    // SAFETY: GetLocalTime only writes the returned SYSTEMTIME.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    LocalStamp {
        year: t.wYear,
        month: t.wMonth as u8,
        day: t.wDay as u8,
        hour: t.wHour as u8,
        minute: t.wMinute as u8,
        second: t.wSecond as u8,
        millis: t.wMilliseconds,
    }
}
