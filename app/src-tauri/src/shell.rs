//! Shared shell state (managed by Tauri) and the main-window helpers used by the tray, the commands and
//! the window events.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use klif_common::vm::HostInfo;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};

use crate::engine::EngineHandle;
use crate::geometry::GeometryStore;
use crate::gpu::{Adapter, GpuReport};
use crate::panel::PanelCtl;
use crate::webview;

pub const MAIN: &str = "main";

enum EngineState {
    Starting,
    Ready(EngineHandle),
    Failed(String),
}

pub struct Shell {
    engine: Mutex<EngineState>,
    engine_cv: Condvar,
    /// Host facts reported in the view model (maximized changes at runtime).
    pub host: Mutex<HostInfo>,
    /// The webview is on screen (window shown and not minimized): only then are view models pushed.
    pub showing: AtomicBool,
    /// View models pushed to the UI (diagnostics).
    pub vm_events: AtomicU64,
    pub geometry: GeometryStore,
    /// Panel mode: the small-screen target, the persisted preference and the enter / leave lock.
    pub panel: PanelCtl,
    /// The adapter the UI is pinned to, if any.
    pub ui_gpu: Option<Adapter>,
    /// GPU process PID of the last check (re-checked when WebView2 restarts it).
    pub gpu_pid: Mutex<Option<u32>>,
    pub gpu_report: Mutex<Option<GpuReport>>,
    /// Debounce state for focus flaps (restore fires Focused four times within ~3 ms).
    pub focus_seq: AtomicU64,
    pub page_loaded: AtomicBool,
}

impl Shell {
    pub fn new(host: HostInfo, geometry: GeometryStore, panel: PanelCtl, ui_gpu: Option<Adapter>) -> Shell {
        Shell {
            engine: Mutex::new(EngineState::Starting),
            engine_cv: Condvar::new(),
            host: Mutex::new(host),
            showing: AtomicBool::new(true),
            vm_events: AtomicU64::new(0),
            geometry,
            panel,
            ui_gpu,
            gpu_pid: Mutex::new(None),
            gpu_report: Mutex::new(None),
            focus_seq: AtomicU64::new(0),
            page_loaded: AtomicBool::new(false),
        }
    }

    pub fn set_engine(&self, r: Result<EngineHandle, String>) {
        *self.engine.lock().unwrap() = match r {
            Ok(h) => EngineState::Ready(h),
            Err(e) => EngineState::Failed(e),
        };
        self.engine_cv.notify_all();
    }

    /// The engine if it is ready (never blocks).
    pub fn engine(&self) -> Result<EngineHandle, String> {
        match &*self.engine.lock().unwrap() {
            EngineState::Ready(h) => Ok(h.clone()),
            EngineState::Starting => Err("The KLIF core is still starting.".into()),
            EngineState::Failed(e) => Err(e.clone()),
        }
    }

    pub fn engine_if_ready(&self) -> Option<EngineHandle> {
        self.engine().ok()
    }

    /// Wait (blocking) until the engine started or failed.
    pub fn wait_engine(&self, timeout: Duration) -> Result<EngineHandle, String> {
        let deadline = Instant::now() + timeout;
        let mut g = self.engine.lock().unwrap();
        loop {
            match &*g {
                EngineState::Ready(h) => return Ok(h.clone()),
                EngineState::Failed(e) => return Err(e.clone()),
                EngineState::Starting => {}
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("The KLIF core did not start in time.".into());
            }
            g = self.engine_cv.wait_timeout(g, left).unwrap().0;
        }
    }
}

pub fn shell<R: Runtime>(app: &AppHandle<R>) -> Arc<Shell> {
    app.state::<Arc<Shell>>().inner().clone()
}

pub fn main_window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(MAIN)
}

/// Change the host facts the view model reports (maximized, panel mode) and hand them to the engine.
/// Returns whether anything changed. Never call it with the host lock held.
pub fn update_host<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&mut HostInfo)) -> bool {
    let s = shell(app);
    let changed = {
        let mut h = s.host.lock().unwrap();
        let before = h.clone();
        f(&mut h);
        (*h != before).then(|| h.clone())
    };
    match changed {
        Some(host) => {
            if let Some(e) = s.engine_if_ready() {
                e.set_host(host);
            }
            true
        }
        None => false,
    }
}

/// Push the current view model right away (after the window came back on screen).
pub fn emit_snapshot<R: Runtime>(app: &AppHandle<R>) {
    if let Some(e) = shell(app).engine_if_ready() {
        let vm = e.snapshot();
        let _ = app.emit("klif://vm", &vm);
    }
}

/// Mark the webview on / off screen; coming back pushes a fresh view model.
pub fn set_showing<R: Runtime>(app: &AppHandle<R>, showing: bool) {
    let was = shell(app).showing.swap(showing, Ordering::Relaxed);
    if showing && !was {
        emit_snapshot(app);
    }
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = main_window(app) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        webview::set_visible(&w, true);
        set_showing(app, true);
    }
}

pub fn hide_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = main_window(app) {
        shell(app).geometry.record(&w);
        // Hiding the window does not hide the WebView2 controller either.
        webview::set_visible(&w, false);
        let _ = w.hide();
        set_showing(app, false);
    }
}

/// Tray "Show / Hide": hide when the window is on screen, otherwise bring it back.
pub fn toggle_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = main_window(app) {
        let on_screen = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
        if on_screen {
            hide_main(app);
        } else {
            show_main(app);
        }
    }
}
