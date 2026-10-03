//! Direct WebView2 access through `with_webview` (runs on the main thread): controller visibility and the
//! environment's process list.

use std::sync::mpsc;
use std::time::Duration;

use tauri::{Runtime, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment8, COREWEBVIEW2_PROCESS_KIND, COREWEBVIEW2_PROCESS_KIND_GPU,
};
use webview2_com::ProcessInfosChangedEventHandler;
use windows::core::Interface;

/// Tell WebView2 whether it is visible. wry never does this on minimize, so a minimized KLIF would keep
/// rendering; with IsVisible(false) Chromium stops painting and the page sees visibilityState 'hidden'.
pub fn set_visible<R: Runtime>(win: &WebviewWindow<R>, visible: bool) {
    let _ = win.with_webview(move |pw| unsafe {
        if let Err(e) = pw.controller().SetIsVisible(visible) {
            log::warn!("SetIsVisible({visible}) failed: {e}");
        }
    });
}

/// Read the controller's IsVisible (diagnostics; only the selftest feature uses it).
#[cfg(feature = "selftest")]
pub fn is_visible<R: Runtime>(win: &WebviewWindow<R>) -> Option<bool> {
    let (tx, rx) = mpsc::channel();
    let _ = win.with_webview(move |pw| unsafe {
        let mut v = windows::core::BOOL(0);
        let r = pw.controller().IsVisible(&mut v).ok().map(|_| v.as_bool());
        let _ = tx.send(r);
    });
    rx.recv_timeout(Duration::from_secs(3)).ok().flatten()
}

fn gpu_pid_of(env8: &ICoreWebView2Environment8) -> Option<u32> {
    unsafe {
        let infos = env8.GetProcessInfos().ok()?;
        let mut n = 0u32;
        infos.Count(&mut n).ok()?;
        for i in 0..n {
            let Ok(p) = infos.GetValueAtIndex(i) else { continue };
            let mut pid = 0i32;
            let mut kind = COREWEBVIEW2_PROCESS_KIND::default();
            if p.ProcessId(&mut pid).is_ok() && p.Kind(&mut kind).is_ok() && kind == COREWEBVIEW2_PROCESS_KIND_GPU {
                return Some(pid as u32);
            }
        }
        None
    }
}

/// The PID of the WebView2 GPU process, if it exists yet (ICoreWebView2Environment8::GetProcessInfos).
pub fn gpu_process_pid<R: Runtime>(win: &WebviewWindow<R>) -> Option<u32> {
    let (tx, rx) = mpsc::channel();
    let _ = win.with_webview(move |pw| {
        let pid = pw.environment().cast::<ICoreWebView2Environment8>().ok().and_then(|e| gpu_pid_of(&e));
        let _ = tx.send(pid);
    });
    rx.recv_timeout(Duration::from_secs(3)).ok().flatten()
}

/// Call `f(gpu_pid)` (on the main thread) whenever the environment's process list changes, e.g. after the
/// GPU process crashed and was restarted. The registration lives as long as the environment.
pub fn on_process_infos_changed<R: Runtime>(win: &WebviewWindow<R>, f: impl Fn(Option<u32>) + Send + 'static) {
    let _ = win.with_webview(move |pw| unsafe {
        let Ok(env8) = pw.environment().cast::<ICoreWebView2Environment8>() else {
            log::warn!("ICoreWebView2Environment8 unavailable: no process-change notifications");
            return;
        };
        let handler = ProcessInfosChangedEventHandler::create(Box::new(move |env, _| {
            let pid = env.and_then(|e| e.cast::<ICoreWebView2Environment8>().ok()).and_then(|e| gpu_pid_of(&e));
            f(pid);
            Ok(())
        }));
        let mut token = 0i64;
        if let Err(e) = env8.add_ProcessInfosChanged(&handler, &mut token) {
            log::warn!("add_ProcessInfosChanged failed: {e}");
        }
    });
}
