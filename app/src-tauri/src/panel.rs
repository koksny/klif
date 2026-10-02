//! Panel mode: the read-only mini layout on a small status screen (a 3.5" 960x640 monitor beside the main
//! ones). Entering moves the window onto that monitor and fills it; leaving puts the previous window back.
//!
//! * Which monitor: `ui.panel_monitor` in klif.toml (a "WxH" resolution or part of the device name), else
//!   automatic: with two or more monitors the one with the fewest pixels, if it is at most 1280x800.
//! * The previous window is remembered by `GeometryStore` (window.json), which is frozen while panel mode
//!   lasts, so the monitor-sized window never overwrites it. Only the "come back in panel mode" preference
//!   lives here (panel.json).
//! * Filling the monitor: position and size in physical pixels, then borderless fullscreen. A frameless
//!   window keeps invisible resize borders (the outer rectangle is larger than the client area) and a
//!   neighbouring monitor would lose a strip to them; fullscreen drops the borders, so the client area is
//!   exactly the monitor and nothing spills over.
//! * The view model carries the state (`HostInfo.panel`), so the UI switches to the mini layout by itself.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalSize, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

use crate::geometry::Avoid;
use crate::shell::{self, main_window};
use crate::tray;

/// Automatic detection accepts a monitor of at most this many pixels (either orientation).
const AUTO_MAX_LONG: u32 = 1280;
const AUTO_MAX_SHORT: u32 = 800;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    active: bool,
}

/// The monitor panel mode fills (physical pixels).
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f64,
}

impl Target {
    fn of(m: &Monitor) -> Target {
        let (p, s) = (m.position(), m.size());
        Target { name: m.name().cloned().unwrap_or_default(), x: p.x, y: p.y, w: s.width, h: s.height, scale: m.scale_factor() }
    }

    /// "960x640", what the UI shows in the button's tooltip.
    pub fn label(&self) -> String {
        format!("{}x{}", self.w, self.h)
    }

    pub fn avoid(&self) -> Avoid {
        Avoid { x: self.x, y: self.y, w: self.w, h: self.h }
    }
}

pub struct PanelCtl {
    path: PathBuf,
    selector: Option<String>,
    /// panel.json: panel mode was on when KLIF last ran, so it comes back that way.
    wanted: AtomicBool,
    /// Panel mode is on right now.
    active: AtomicBool,
    /// Held while entering or leaving (one switch at a time) and while the monitor list is re-read.
    pub busy: Mutex<()>,
    /// The monitor the window is parked on while panel mode is on.
    parked_on: Mutex<Option<Target>>,
}

impl PanelCtl {
    pub fn load(path: PathBuf, selector: Option<String>) -> PanelCtl {
        let wanted = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Saved>(s.trim_start_matches('\u{feff}')).ok())
            .map(|s| s.active)
            .unwrap_or(false);
        let selector = selector.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        PanelCtl { path, selector, wanted: AtomicBool::new(wanted), active: AtomicBool::new(false), busy: Mutex::new(()), parked_on: Mutex::new(None) }
    }

    pub fn wanted(&self) -> bool {
        self.wanted.load(Ordering::Relaxed)
    }

    pub fn active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn selector(&self) -> Option<&str> {
        self.selector.as_deref()
    }

    /// Record that panel mode is on (and where) for the rest of this run.
    pub fn set_active(&self, target: Option<Target>) {
        self.active.store(target.is_some(), Ordering::Relaxed);
        *self.parked_on.lock().unwrap() = target;
    }

    fn parked_on(&self) -> Option<Target> {
        self.parked_on.lock().unwrap().clone()
    }

    /// Remember the preference across restarts.
    fn persist(&self, active: bool) {
        self.wanted.store(active, Ordering::Relaxed);
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&Saved { active }).unwrap_or_else(|_| "{}".into());
        if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &self.path)) {
            log::warn!("saving {} failed: {e}", self.path.display());
        }
    }
}

/// "960x640" (also "960 x 640" or "960×640") -> (960, 640).
pub fn parse_resolution(s: &str) -> Option<(u32, u32)> {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase().replace('×', "x");
    let (a, b) = t.split_once('x')?;
    let (w, h) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// The monitor panel mode would fill, if there is one.
pub fn find_target(monitors: &[Monitor], selector: Option<&str>) -> Option<Target> {
    let found = match selector {
        Some(sel) => match parse_resolution(sel) {
            Some((w, h)) => monitors.iter().find(|m| {
                let s = m.size();
                (s.width == w && s.height == h) || (s.width == h && s.height == w)
            }),
            None => {
                let needle = sel.to_lowercase();
                monitors.iter().find(|m| m.name().map(|n| n.to_lowercase().contains(&needle)).unwrap_or(false))
            }
        },
        None if monitors.len() >= 2 => monitors
            .iter()
            .min_by_key(|m| m.size().width as u64 * m.size().height as u64)
            .filter(|m| m.size().width.max(m.size().height) <= AUTO_MAX_LONG && m.size().width.min(m.size().height) <= AUTO_MAX_SHORT),
        None => None,
    };
    found.map(Target::of)
}

fn no_target_message(selector: Option<&str>) -> String {
    match selector {
        Some(s) => format!("No monitor matches ui.panel_monitor = \"{s}\" (a resolution like 960x640, or part of the monitor name)."),
        None => "No small screen found: panel mode needs a second monitor of at most 1280x800 (or set ui.panel_monitor in klif.toml).".into(),
    }
}

/// The monitors as one log line each.
pub fn describe_monitors<R: Runtime>(win: &WebviewWindow<R>) -> Vec<String> {
    win.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (p, s, w) = (m.position(), m.size(), m.work_area());
            format!(
                "{} at {},{} {}x{} scale {} (work area {}x{} at {},{})",
                m.name().map(String::as_str).unwrap_or("?"),
                p.x,
                p.y,
                s.width,
                s.height,
                m.scale_factor(),
                w.size.width,
                w.size.height,
                w.position.x,
                w.position.y
            )
        })
        .collect()
}

/// The window's geometry as one log line (physical pixels).
pub fn describe<R: Runtime>(win: &WebviewWindow<R>) -> String {
    let pos = |p: tauri::Result<PhysicalPosition<i32>>| p.map(|p| format!("{},{}", p.x, p.y)).unwrap_or_else(|_| "?".into());
    let size = |s: tauri::Result<PhysicalSize<u32>>| s.map(|s| format!("{}x{}", s.width, s.height)).unwrap_or_else(|_| "?".into());
    let monitor = win.current_monitor().ok().flatten();
    format!(
        "outer {} {}, inner {} {}, on {} (scale {}), maximized {}, fullscreen {}",
        pos(win.outer_position()),
        size(win.outer_size()),
        pos(win.inner_position()),
        size(win.inner_size()),
        monitor.as_ref().and_then(|m| m.name().cloned()).unwrap_or_else(|| "?".into()),
        monitor.as_ref().map(|m| m.scale_factor().to_string()).unwrap_or_else(|| "?".into()),
        win.is_maximized().unwrap_or(false),
        win.is_fullscreen().unwrap_or(false)
    )
}

fn wait_for(timeout: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if ok() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// True when the client area is exactly the monitor.
fn fills<R: Runtime>(win: &WebviewWindow<R>, t: &Target) -> bool {
    win.inner_size().map(|s| s.width == t.w && s.height == t.h).unwrap_or(false)
        && win.inner_position().map(|p| p.x == t.x && p.y == t.y).unwrap_or(false)
}

/// Step 1 of entering (also safe on a hidden window): lift the full layout's minimum size (it would stop
/// the window from shrinking onto a small screen), un-maximize and put the window on the monitor.
pub fn place<R: Runtime>(win: &WebviewWindow<R>, t: &Target) -> Result<(), String> {
    let fail = |what: &'static str| move |e: tauri::Error| format!("Could not {what}: {e}");
    win.set_min_size(None::<tauri::Size>).map_err(fail("lift the window's minimum size"))?;
    if win.is_fullscreen().unwrap_or(false) {
        win.set_fullscreen(false).map_err(fail("leave fullscreen"))?;
    }
    if win.is_maximized().unwrap_or(false) {
        win.unmaximize().map_err(fail("un-maximize the window"))?;
    }
    // Size first (keeps the window from straddling several monitors), then the move, which is where a
    // different DPI scale kicks in and may resize the window, then the size again.
    win.set_size(PhysicalSize::new(t.w, t.h)).map_err(fail("size the window"))?;
    win.set_position(PhysicalPosition::new(t.x, t.y)).map_err(fail("move the window"))?;
    win.set_size(PhysicalSize::new(t.w, t.h)).map_err(fail("size the window"))?;
    Ok(())
}

/// Step 2 of entering (the window must be shown): borderless fullscreen on the monitor, then check that
/// the client area is exactly the monitor, correcting once if it is not.
pub fn fill<R: Runtime>(win: &WebviewWindow<R>, t: &Target) -> Result<(), String> {
    win.set_fullscreen(true).map_err(|e| format!("Could not fill the screen: {e}"))?;
    if wait_for(Duration::from_millis(1500), || win.is_fullscreen().unwrap_or(false) && fills(win, t)) {
        return Ok(());
    }
    log::warn!("window does not fill {} yet ({}); placing it again", t.name, describe(win));
    place(win, t)?;
    let _ = win.set_fullscreen(true);
    if wait_for(Duration::from_millis(1500), || fills(win, t)) {
        return Ok(());
    }
    log::warn!("window still does not fill {} ({})", t.name, describe(win));
    Ok(())
}

/// Publish the panel flags in the host info (and to the engine, so the view model carries them).
fn publish<R: Runtime>(app: &AppHandle<R>, target: Option<&Target>, active: bool) {
    shell::update_host(app, |h| {
        h.panel.available = target.is_some();
        h.panel.active = active;
        h.panel.target = target.map(Target::label);
    });
    tray::sync_panel(app, target.is_some(), active);
}

/// At startup, before the window is shown: report availability, and the target when panel mode was on at
/// the last exit and the screen is still there.
pub fn startup<R: Runtime>(app: &AppHandle<R>, win: &WebviewWindow<R>) -> Option<Target> {
    let s = shell::shell(app);
    for line in describe_monitors(win) {
        log::info!("monitor {line}");
    }
    let found = find_target(&win.available_monitors().unwrap_or_default(), s.panel.selector());
    match &found {
        Some(t) => log::info!("screen found: {} {} at {},{} (scale {})", t.name, t.label(), t.x, t.y, t.scale),
        None => log::info!("no screen found ({})", no_target_message(s.panel.selector())),
    }
    let start_active = s.panel.wanted() && found.is_some();
    if s.panel.wanted() && found.is_none() {
        log::info!("it was on at the last exit but the screen is not connected; starting normally");
    }
    {
        let mut h = s.host.lock().unwrap();
        h.panel.available = found.is_some();
        h.panel.active = start_active;
        h.panel.target = found.as_ref().map(Target::label);
        if start_active {
            h.maximized = false;
        }
    }
    if start_active {
        s.geometry.set_frozen(true);
        s.panel.set_active(found.clone());
        found
    } else {
        None
    }
}

/// Turn panel mode on or off. Runs on a worker thread (it waits on the window).
pub fn toggle<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
    let s = shell::shell(app);
    let _busy = s.panel.busy.try_lock().map_err(|_| "Panel mode is still switching.".to_string())?;
    if s.panel.active() {
        leave_locked(app).map(|_| false)
    } else {
        enter_locked(app).map(|_| true)
    }
}

fn enter_locked<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let s = shell::shell(app);
    let win = main_window(app).ok_or("The window is gone.")?;
    let target = find_target(&win.available_monitors().unwrap_or_default(), s.panel.selector()).ok_or_else(|| no_target_message(s.panel.selector()))?;
    let t0 = Instant::now();
    // From the tray the window may be hidden or minimized.
    shell::show_main(app);
    log::info!("entering on {} {} at {},{}; window before: {}", target.name, target.label(), target.x, target.y, describe(&win));
    // The normal window is remembered by the store, which then stops following the window.
    s.geometry.record(&win);
    s.geometry.set_frozen(true);
    s.geometry.flush();
    // The window moves first (on the small screen its viewport is already a small landscape one, which the
    // UI draws as the mini layout); the view model follows right after.
    let moved = place(&win, &target).and_then(|_| fill(&win, &target));
    if let Err(e) = moved {
        log::warn!("entering failed: {e}");
        s.geometry.set_frozen(false);
        return Err(e);
    }
    s.panel.set_active(Some(target.clone()));
    publish(app, Some(&target), true);
    s.panel.persist(true);
    log::info!("entered in {} ms; window now: {}", t0.elapsed().as_millis(), describe(&win));
    Ok(())
}

fn leave_locked<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let s = shell::shell(app);
    let win = main_window(app).ok_or("The window is gone.")?;
    let target = s.panel.parked_on().or_else(|| find_target(&win.available_monitors().unwrap_or_default(), s.panel.selector()));
    let t0 = Instant::now();
    log::info!("leaving; window before: {}", describe(&win));
    // The view model goes first: the small window still draws the mini layout (small landscape viewport),
    // so nothing flashes while the UI catches up; then the window grows back.
    publish(app, target.as_ref(), false);
    if let Some(e) = s.engine_if_ready() {
        wait_for(Duration::from_millis(600), || !e.snapshot().host.panel.active);
        shell::emit_snapshot(app);
        std::thread::sleep(Duration::from_millis(90));
    }
    let _ = win.set_fullscreen(false);
    wait_for(Duration::from_millis(1000), || !win.is_fullscreen().unwrap_or(true));
    let _ = win.set_min_size(Some(LogicalSize::new(crate::MIN_W, crate::MIN_H)));
    let wanted = s.geometry.restore(&win, target.as_ref().map(Target::avoid));
    // Moving between monitors of different DPI scale lets the window system resize it: re-apply the size.
    if let Some(want) = wanted {
        for _ in 0..5 {
            if wait_for(Duration::from_millis(250), || win.inner_size().map(|g| g == want).unwrap_or(false)) {
                break;
            }
            let _ = win.set_size(want);
        }
    } else {
        // Maximized again: let it settle.
        wait_for(Duration::from_millis(500), || win.is_maximized().unwrap_or(false));
    }
    std::thread::sleep(Duration::from_millis(120));
    s.geometry.set_frozen(false);
    s.geometry.record(&win);
    s.panel.set_active(None);
    s.panel.persist(false);
    shell::update_host(app, |h| h.maximized = win.is_maximized().unwrap_or(false));
    log::info!("left in {} ms; window now: {}", t0.elapsed().as_millis(), describe(&win));
    Ok(())
}

/// Re-read the monitors (a status screen may be plugged in or out while KLIF runs) and update what the
/// UI knows. Skipped while panel mode is on or switching.
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let s = shell::shell(app);
    let Ok(_busy) = s.panel.busy.try_lock() else { return };
    if s.panel.active() {
        return;
    }
    let Some(win) = main_window(app) else { return };
    let found = find_target(&win.available_monitors().unwrap_or_default(), s.panel.selector());
    let changed = shell::update_host(app, |h| {
        h.panel.available = found.is_some();
        h.panel.target = found.as_ref().map(Target::label);
    });
    if changed {
        match &found {
            Some(t) => log::info!("screen connected: {} {}", t.name, t.label()),
            None => log::info!("screen gone"),
        }
        tray::sync_panel(app, found.is_some(), false);
    }
}
