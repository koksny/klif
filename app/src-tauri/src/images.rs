//! Saving the Records export card (a PNG or GIF the UI rendered): into the user's Pictures folder, subfolder
//! `KLIF`. The page chooses only a file name; it is reduced to a safe one, only `.png` and `.gif` are written (and
//! the bytes must really be that format), and an existing file is never overwritten (`-2`, `-3`, ...).

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

/// Subfolder of Pictures the images go to.
const FOLDER: &str = "KLIF";
/// Most decoded bytes accepted (a GIF card is a few MB).
const MAX_BYTES: usize = 64 * 1024 * 1024;
/// How many `-n` suffixes are tried before giving up.
const MAX_COPIES: u32 = 9999;

/// The user's Pictures folder. Windows: the known folder (it follows a Pictures folder moved to another drive or
/// to OneDrive), else `%USERPROFILE%\Pictures`; elsewhere `$HOME/Pictures`.
#[cfg(windows)]
fn pictures_dir() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let known = unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) }.ok().and_then(|p| {
        // The shell allocates the string; it is ours to free.
        let text = unsafe { p.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
        text.filter(|t| !t.is_empty()).map(PathBuf::from)
    });
    known.or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Pictures")))
}

#[cfg(not(windows))]
fn pictures_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Pictures"))
}

/// Standard base64 (whitespace skipped); None when the text is not base64.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' | b' ' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// `(stem, extension)` of a file name reduced to `[A-Za-z0-9._-]`: only the last path segment counts, the
/// extension must be png or gif, a Windows device name is avoided, the stem is at most 80 characters.
fn safe_name(name: &str) -> Result<(String, &'static str), String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let (stem, ext) = last.rsplit_once('.').unwrap_or((last, ""));
    let ext = match ext.to_ascii_lowercase().as_str() {
        "png" => "png",
        "gif" => "gif",
        _ => return Err("Only .png and .gif images can be saved.".into()),
    };
    let cleaned: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' }).collect();
    let mut stem: String = cleaned.trim_matches(|c| c == '.' || c == '-' || c == '_').chars().take(80).collect();
    if stem.is_empty() {
        stem = "klif-record".into();
    }
    let device = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((device.starts_with("COM") || device.starts_with("LPT")) && device.len() == 4 && device.as_bytes()[3].is_ascii_digit());
    if reserved {
        stem = format!("klif-{stem}");
    }
    Ok((stem, ext))
}

/// Does the data start like the format the extension says?
fn matches_format(bytes: &[u8], ext: &str) -> bool {
    match ext {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        _ => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
    }
}

/// Write `data` (base64 of a PNG or GIF) as `name` into Pictures\KLIF and return the full path. Never
/// overwrites: the first free of `name`, `name-2`, `name-3`, ... is created atomically.
pub fn save(name: &str, data: &str) -> Result<String, String> {
    let (stem, ext) = safe_name(name)?;
    if data.len() > MAX_BYTES / 3 * 4 + 8 {
        return Err("The image is too large to save.".into());
    }
    let bytes = base64_decode(data).ok_or_else(|| "The image data is not valid.".to_string())?;
    if bytes.is_empty() || !matches_format(&bytes, ext) {
        return Err(format!("The image data is not a {} file.", ext.to_ascii_uppercase()));
    }
    let dir = pictures_dir().ok_or_else(|| "The Pictures folder could not be found.".to_string())?.join(FOLDER);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    for n in 1..=MAX_COPIES {
        let file = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}-{n}.{ext}") };
        let path = dir.join(&file);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(&bytes).and_then(|_| f.flush()) {
                    drop(f);
                    let _ = std::fs::remove_file(&path);
                    return Err(format!("Could not write {}: {e}", path.display()));
                }
                return Ok(path.display().to_string());
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not create {}: {e}", path.display())),
        }
    }
    Err(format!("{} already holds too many copies of {stem}.{ext}.", dir.display()))
}
