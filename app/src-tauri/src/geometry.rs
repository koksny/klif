//! Window geometry persisted in the state directory (`window.json`), so nothing is written to %APPDATA%
//! (tauri-plugin-window-state would). Physical pixels: outer position and inner size of the normal
//! (not maximized, not minimized) window, plus the maximized flag.
//!
//! Panel mode freezes the store (`freeze`): the window then sits on the small monitor, filling it, and
//! that must never overwrite the remembered normal window. Leaving panel mode restores from here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

pub struct GeometryStore {
    path: PathBuf,
    cur: Mutex<Option<Geometry>>,
    dirty: AtomicBool,
    /// While set, `record` does nothing (panel mode: the window is parked on the small monitor).
    frozen: AtomicBool,
}

/// A monitor the window must not be restored onto (the panel monitor): physical position and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Avoid {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Avoid {
    fn is(&self, m: &Monitor) -> bool {
        let (p, s) = (m.position(), m.size());
        p.x == self.x && p.y == self.y && s.width == self.w && s.height == self.h
    }
}

/// Default inner size in logical pixels (the "full" layout, portrait).
const DEFAULT_W: f64 = 1024.0;
const DEFAULT_H: f64 = 1152.0;

impl GeometryStore {
    pub fn load(path: PathBuf) -> GeometryStore {
        let cur = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Geometry>(s.trim_start_matches('\u{feff}')).ok())
            .filter(|g| g.width >= 100 && g.height >= 100);
        GeometryStore { path, cur: Mutex::new(cur), dirty: AtomicBool::new(false), frozen: AtomicBool::new(false) }
    }

    /// Stop (or resume) following the window. Freezing keeps the remembered geometry exactly as it is.
    pub fn set_frozen(&self, frozen: bool) {
        self.frozen.store(frozen, Ordering::Relaxed);
    }

    pub fn saved(&self) -> Option<Geometry> {
        *self.cur.lock().unwrap()
    }

    /// Apply the saved geometry to a freshly built (hidden) window, or size and center it on its monitor.
    /// Returns the inner size it asked for: call `settle` with it once the window is shown.
    ///
    /// `avoid` (leaving panel mode): the window is not put back on that monitor; a saved position there
    /// counts as off-screen and the default placement uses another monitor.
    pub fn restore<R: Runtime>(&self, win: &WebviewWindow<R>, avoid: Option<Avoid>) -> Option<PhysicalSize<u32>> {
        let saved = self.saved();
        let all = win.available_monitors().unwrap_or_default();
        let usable: Vec<Monitor> = match avoid {
            Some(a) => {
                let others: Vec<Monitor> = all.iter().filter(|m| !a.is(m)).cloned().collect();
                if others.is_empty() { all.clone() } else { others }
            }
            None => all.clone(),
        };
        let on_screen = saved.filter(|g| {
            // The top strip of the window (where the skin's drag region is) must be on some monitor.
            let monitors = &usable;
            monitors.iter().any(|m| {
                let r = m.work_area();
                let (mx, my) = (r.position.x, r.position.y);
                let (mw, mh) = (r.size.width as i32, r.size.height as i32);
                let (gx, gy, gw) = (g.x, g.y, g.width as i32);
                gx + gw > mx + 40 && gx < mx + mw - 40 && gy + 24 > my && gy < my + mh - 24
            })
        });
        match on_screen {
            Some(g) => {
                let _ = win.set_size(PhysicalSize::new(g.width, g.height));
                let _ = win.set_position(PhysicalPosition::new(g.x, g.y));
                log::info!("window geometry restored: {}x{} at {},{}{}", g.width, g.height, g.x, g.y, if g.maximized { " (maximized)" } else { "" });
                if g.maximized {
                    let _ = win.maximize();
                    return None;
                }
                Some(PhysicalSize::new(g.width, g.height))
            }
            None => {
                if saved.is_some() {
                    log::info!("saved window position is off-screen; centering");
                }
                let target = if avoid.is_some() {
                    win.primary_monitor().ok().flatten().filter(|p| usable.iter().any(|u| u.position() == p.position())).or_else(|| usable.first().cloned())
                } else {
                    win.current_monitor().ok().flatten()
                };
                if let Some(m) = target {
                    let s = m.scale_factor();
                    let wa = m.work_area();
                    let w = (DEFAULT_W * s).min(wa.size.width as f64 * 0.92).round() as u32;
                    let h = (DEFAULT_H * s).min(wa.size.height as f64 * 0.92).round() as u32;
                    let _ = win.set_size(PhysicalSize::new(w, h));
                    let x = wa.position.x + (wa.size.width as i32 - w as i32) / 2;
                    let y = wa.position.y + (wa.size.height as i32 - h as i32) / 2;
                    let _ = win.set_position(PhysicalPosition::new(x, y));
                    log::info!("window placed by default: {w}x{h} at {x},{y} (scale {s})");
                    if saved.map(|g| g.maximized).unwrap_or(false) {
                        let _ = win.maximize();
                        return None;
                    }
                    return Some(PhysicalSize::new(w, h));
                }
                if saved.map(|g| g.maximized).unwrap_or(false) {
                    let _ = win.maximize();
                }
                None
            }
        }
    }

    /// A frameless window sized while hidden comes up taller by the caption height once shown (tao sizes a
    /// hidden undecorated window as if it had a caption). Re-apply the wanted inner size after `show()`.
    pub fn settle<R: Runtime>(&self, win: &WebviewWindow<R>, wanted: Option<PhysicalSize<u32>>) {
        let Some(wanted) = wanted else { return };
        let Ok(got) = win.inner_size() else { return };
        if got != wanted {
            let _ = win.set_size(wanted);
            log::info!("window size settled: {}x{} -> {}x{} (now {:?})", got.width, got.height, wanted.width, wanted.height, win.inner_size().ok());
        }
    }

    /// Record the window's current geometry (call on Moved / Resized). Minimized states are ignored; a
    /// maximized window only updates the flag so the normal rectangle survives.
    pub fn record<R: Runtime>(&self, win: &WebviewWindow<R>) {
        if self.frozen.load(Ordering::Relaxed) || win.is_minimized().unwrap_or(false) {
            return;
        }
        let maximized = win.is_maximized().unwrap_or(false);
        let mut cur = self.cur.lock().unwrap();
        let next = if maximized {
            match *cur {
                Some(g) => Geometry { maximized: true, ..g },
                None => return,
            }
        } else {
            let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else { return };
            // A minimizing window reports -32000,-32000 before is_minimized flips.
            if pos.x <= -16000 || pos.y <= -16000 || size.width < 100 || size.height < 100 {
                return;
            }
            Geometry { x: pos.x, y: pos.y, width: size.width, height: size.height, maximized: false }
        };
        if *cur != Some(next) {
            *cur = Some(next);
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// Write if anything changed since the last write.
    pub fn flush(&self) {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        let Some(g) = self.saved() else { return };
        let Ok(text) = serde_json::to_string_pretty(&g) else { return };
        let tmp = self.path.with_extension("json.tmp");
        let r = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &self.path));
        match r {
            Ok(()) => log::debug!("window geometry saved: {g:?}"),
            Err(e) => log::warn!("saving window geometry to {} failed: {e}", self.path.display()),
        }
    }
}
