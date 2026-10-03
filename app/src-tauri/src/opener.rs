//! Hand a file, folder or web address to the system's default handler, and the two native message boxes the tray
//! needs. Windows uses ShellExecuteW / MessageBoxW; other systems fall back to their own opener (untested: the
//! tested build is Windows).

use std::path::Path;

/// Open `target` (a file, a folder or an http(s) address) with its default handler. `Err` carries the
/// ShellExecute result code (<= 32) on Windows.
#[cfg(windows)]
pub fn open(target: &str) -> Result<(), isize> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // ShellExecute returns a value > 32 on success.
    let code = r.0 as isize;
    if code > 32 { Ok(()) } else { Err(code) }
}

#[cfg(not(windows))]
pub fn open(target: &str) -> Result<(), isize> {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(program).arg(target).spawn().map(|_| ()).map_err(|_| -1)
}

/// Open a web address in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    open(url).map_err(|code| format!("Could not open {url} (error {code})."))
}

/// Open a folder in the file manager.
pub fn open_folder(dir: &Path) -> Result<(), String> {
    open(&dir.display().to_string()).map_err(|code| format!("Could not open the folder {} (error {code}).", dir.display()))
}

/// Open a text file: the registered handler for it, else Notepad (".toml" often has no handler). Without the
/// check, ShellExecute would show the "How do you want to open this file?" dialog for such a file.
pub fn open_file(path: &Path) -> Result<(), String> {
    if has_handler(path) && open(&path.display().to_string()).is_ok() {
        return Ok(());
    }
    log::info!("no handler for {}; trying notepad.exe", path.display());
    notepad(path).map_err(|e| format!("Could not open {}: {e}", path.display()))
}

/// Does the file's extension have a program registered (AssocQueryString)? A file without an extension is left
/// to the shell.
#[cfg(windows)]
pub fn has_handler(path: &Path) -> bool {
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_EXECUTABLE};
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return true };
    let dotted = HSTRING::from(format!(".{ext}"));
    let mut buf = [0u16; 520];
    let mut len = buf.len() as u32;
    let hr = unsafe { AssocQueryStringW(ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_EXECUTABLE, &dotted, PCWSTR::null(), Some(PWSTR(buf.as_mut_ptr())), &mut len) };
    hr.is_ok()
}

#[cfg(not(windows))]
pub fn has_handler(_path: &Path) -> bool {
    true
}

#[cfg(windows)]
fn notepad(path: &Path) -> std::io::Result<()> {
    // The system copy, not whatever a PATH lookup finds first.
    let root = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
    std::process::Command::new(root.join("System32").join("notepad.exe")).arg(path).spawn().map(|_| ())
}

#[cfg(not(windows))]
fn notepad(_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("no text editor is registered"))
}

/// A Yes / No question; the default answer is No. Blocks the calling thread until answered. Without a native
/// dialog (non-Windows) the answer is yes: the question only guards a menu click the user made on purpose.
#[cfg(windows)]
pub fn confirm(title: &str, text: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_SETFOREGROUND, MB_YESNO};
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_SETFOREGROUND) == IDYES }
}

#[cfg(not(windows))]
pub fn confirm(_title: &str, _text: &str) -> bool {
    true
}

/// A message with an OK button (an error nobody else would show). Blocks until dismissed.
#[cfg(windows)]
pub fn message(title: &str, text: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND};
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONWARNING | MB_SETFOREGROUND);
    }
}

#[cfg(not(windows))]
pub fn message(title: &str, text: &str) {
    log::warn!("{title}: {text}");
}
