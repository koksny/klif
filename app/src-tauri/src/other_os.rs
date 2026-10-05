//! Stand-ins for the Windows-only modules (`clipboard`, `gpu`, `webview`) so the shell's own code does not need
//! `cfg` at every call site. Compiled only off Windows; main.rs re-exports them at the crate root. They do the
//! least that is honest: no GPU pin (WKWebView and WebKitGTK pick their own GPU), no WebView2 visibility calls. The
//! clipboard is macOS's own module there, GTK's on Linux, and missing elsewhere.

/// GTK's CLIPBOARD selection, on the main thread (commands.rs calls through `on_main`). A secret is copied as plain
/// text: Linux desktops share no marker that keeps it out of a clipboard manager's history.
#[cfg(target_os = "linux")]
pub mod clipboard {
    use tauri::{AppHandle, Runtime};

    pub fn copy<R: Runtime>(_app: &AppHandle<R>, text: &str, _secret: bool) -> Result<(), String> {
        let cb = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        cb.set_text(text);
        cb.store();
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub mod clipboard {
    use tauri::{AppHandle, Runtime};

    pub fn copy<R: Runtime>(_app: &AppHandle<R>, _text: &str, _secret: bool) -> Result<(), String> {
        Err("Copying to the clipboard is not implemented on this platform yet.".into())
    }
}

pub mod gpu {
    use klif_common::config::Config;
    use klif_telemetry::Adapter;

    pub fn log_adapters() {}

    pub fn resolve_ui_adapter(_cfg: &Config) -> Option<Adapter> {
        None
    }

    pub fn browser_args(_ui: Option<&Adapter>) -> String {
        String::new()
    }

    pub fn describe(a: &Adapter) -> String {
        a.name.clone()
    }
}

pub mod webview {
    use tauri::{Runtime, WebviewWindow};

    pub fn set_visible<R: Runtime>(_win: &WebviewWindow<R>, _visible: bool) {}

    #[cfg(feature = "selftest")]
    pub fn is_visible<R: Runtime>(_win: &WebviewWindow<R>) -> Option<bool> {
        None
    }
}
