//! Native clipboard (CF_UNICODETEXT). Secrets are marked so Windows keeps them out of the clipboard
//! history and cloud clipboard and clipboard monitors skip them.

use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Closes the clipboard on every exit path.
struct Open;

impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn open(owner: HWND) -> Result<Open, String> {
    // Another application may hold the clipboard for a moment.
    for _ in 0..20 {
        if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
            return Ok(Open);
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    Err("The clipboard is busy. Try again.".into())
}

/// Put `bytes` into a movable global block and hand it to the clipboard (which then owns it).
unsafe fn set_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|e| format!("GlobalAlloc: {e}"))?;
        let p = GlobalLock(mem) as *mut u8;
        if p.is_null() {
            let _ = GlobalFree(Some(mem));
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(mem);
        if let Err(e) = SetClipboardData(format, Some(HANDLE(mem.0))) {
            let _ = GlobalFree(Some(mem));
            return Err(format!("SetClipboardData: {e}"));
        }
        Ok(())
    }
}

/// Copy `text`. `secret` adds the "exclude from history / cloud / monitors" formats.
/// `owner` must be a window of this process (EmptyClipboard with no owner makes SetClipboardData fail).
pub fn copy_text(owner: HWND, text: &str, secret: bool) -> Result<(), String> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let mut bytes: Vec<u8> = wide.iter().flat_map(|c| c.to_le_bytes()).collect();
    let r = copy_bytes(owner, &bytes, secret);
    // Do not leave a copy of a secret in freed heap memory.
    wipe(&mut bytes);
    wipe_u16(&mut wide);
    r
}

fn wipe(b: &mut [u8]) {
    for x in b.iter_mut() {
        unsafe { std::ptr::write_volatile(x, 0) };
    }
}

fn wipe_u16(b: &mut [u16]) {
    for x in b.iter_mut() {
        unsafe { std::ptr::write_volatile(x, 0) };
    }
}

fn copy_bytes(owner: HWND, bytes: &[u8], secret: bool) -> Result<(), String> {
    let _open = open(owner)?;
    unsafe {
        EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
        set_bytes(CF_UNICODETEXT.0 as u32, bytes)?;
        if secret {
            let zero = 0u32.to_le_bytes();
            let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
            let cloud = RegisterClipboardFormatW(w!("CanUploadToCloudClipboard"));
            let monitors = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
            for f in [history, cloud] {
                if f != 0 {
                    set_bytes(f, &zero)?;
                }
            }
            if monitors != 0 {
                set_bytes(monitors, &[0])?;
            }
        }
    }
    Ok(())
}
