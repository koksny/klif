//! KLIF desktop shell (Tauri 2): one frameless window rendering the Svelte UI, a tray icon, single
//! instance, the WebView2 GPU pinned to the UI adapter, and the native transport to the KLIF core
//! (`klif://vm` events at 2 Hz, `klif_*` commands).
//!
//! Dev: the UI dev server registered in .studio/devserver.json (tauri.conf.json devUrl) must be running;
//! `cargo run` here.
//! Nothing is written outside KLIF's two folders: the state directory (the folder of klif.toml: window
//! geometry, panel mode) and the data directory (the WebView2 profile and the shell log, which lives in the
//! logs folder). The two are the same folder unless klif.toml is the default `%APPDATA%\KLIF` one.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod endpoint;
mod engine;
mod geometry;
mod images;
mod klog;
mod opener;
mod panel;
#[cfg(feature = "selftest")]
mod selftest;
mod shell;
mod tray;

// Windows-only modules; macOS has its own clipboard, other systems get stand-ins from `other_os` under the same
// names.
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod gpu;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod webview;
#[cfg(not(windows))]
mod other_os;
#[cfg(target_os = "macos")]
use macos::clipboard;
#[cfg(not(any(windows, target_os = "macos")))]
use other_os::clipboard;
#[cfg(not(windows))]
use other_os::{gpu, webview};

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use klif_common::config::{Config, LoadedConfig};
use klif_common::vm::{HostInfo, HostKind, PanelInfo};
use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Emitter, RunEvent, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::geometry::GeometryStore;
use crate::panel::PanelCtl;
use crate::shell::{main_window, Shell, MAIN};

/// Smallest window the full layout works in (logical px).
const MIN_W: f64 = 760.0;
const MIN_H: f64 = 640.0;

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}

fn main() {
    // Never fails: a missing or broken klif.toml gives an empty config at its path plus issues (the engine shows
    // them; the state dir is still the right one).
    let loaded = klif_common::config::load();
    let cfg = loaded.cfg.clone();
    // The data folder holds the WebView2 profile and (by default) the logs.
    let _ = std::fs::create_dir_all(&cfg.data_dir);
    // The shell log sits with the session logs (`[paths] logs_dir`, default <data_dir>\logs), so "Open logs"
    // shows both.
    klog::init(Some(&cfg.logs_dir().join("klif-shell.log")));
    log::info!(
        "config {} (state dir {}, data dir {})",
        cfg.source.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(none yet)".into()),
        cfg.state_dir.display(),
        cfg.data_dir.display()
    );
    for i in &loaded.issues {
        log::warn!("config: {}", i.text);
    }
    log::info!("engine: {}", engine::KIND);
    log::info!(
        "build: {} profile, custom-protocol {}, exe {}",
        if cfg!(debug_assertions) { "debug" } else { "release" },
        if cfg!(feature = "custom-protocol") { "on (UI embedded)" } else { "off (UI from devUrl)" },
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
    );
    if !cfg!(debug_assertions) && !cfg!(feature = "custom-protocol") {
        log::warn!("release build without the custom-protocol feature: the window will load devUrl, not the embedded UI (build with scripts/Build-Release.ps1)");
    }

    // DXGI before any WebView2 environment exists; LUIDs change every boot.
    gpu::log_adapters();
    let ui_gpu = gpu::resolve_ui_adapter(&cfg);
    let browser_args = gpu::browser_args(ui_gpu.as_ref());
    match &ui_gpu {
        Some(a) => log::info!("UI adapter pinned: {}", gpu::describe(a)),
        None => log::info!("UI adapter: not pinned"),
    }
    log::info!("additional_browser_args = {browser_args}");

    let geometry = GeometryStore::load(cfg.state_path("window.json"));
    let host = HostInfo {
        kind: HostKind::Tauri,
        frameless: cfg.ui.frameless,
        maximized: geometry.saved().map(|g| g.maximized).unwrap_or(false),
        app_version: env!("CARGO_PKG_VERSION").into(),
        // Filled in once the window exists and the monitors can be listed (panel::startup).
        panel: PanelInfo::default(),
    };
    let panel = PanelCtl::load(cfg.state_path("panel.json"), cfg.ui.panel_monitor.clone());
    let shell = Arc::new(Shell::new(host, geometry, panel, ui_gpu));
    let webview_dir = cfg.data_path("webview-data");

    let app = tauri::Builder::default()
        // Registered first: a second launch hands its argv to us and exits before anything else runs.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            log::info!("second instance turned away (args {:?}); focusing the window", argv.iter().skip(1).collect::<Vec<_>>());
            shell::show_main(app);
        }))
        .manage(shell)
        .invoke_handler(tauri::generate_handler![
            commands::klif_snapshot,
            commands::klif_engine_status,
            commands::klif_act,
            commands::klif_preset_get,
            commands::klif_records_history,
            commands::klif_save_image,
            commands::klif_command_preview,
            commands::klif_set_api_key,
            commands::klif_open_config,
            commands::klif_open_logs,
            commands::klif_open_endpoint,
            commands::klif_copy_endpoint,
            commands::klif_copy_api_key,
            commands::klif_ui_log,
            commands::klif_toggle_panel,
            commands::klif_skins,
        ])
        .setup(move |app| {
            setup(app.handle(), cfg, loaded, browser_args, webview_dir)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("KLIF: building the Tauri app failed");

    app.run(|app, ev| match ev {
        RunEvent::ExitRequested { .. } => {
            let s = shell::shell(app);
            if let Some(w) = main_window(app) {
                s.geometry.record(&w);
            }
            s.geometry.flush();
        }
        RunEvent::Exit => {
            let s = shell::shell(app);
            s.geometry.flush();
            if let Some(e) = s.engine_if_ready() {
                e.shutdown();
            }
            log::info!("exit ({} view models pushed)", s.vm_events.load(Ordering::Relaxed));
            log::logger().flush();
        }
        _ => {}
    });
}

fn setup(app: &AppHandle, cfg: Config, loaded: LoadedConfig, browser_args: String, webview_dir: PathBuf) -> tauri::Result<()> {
    let s = shell::shell(app);
    let t = Instant::now();
    let page_app = app.clone();
    let builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("KLIF")
        .decorations(!cfg.ui.frameless)
        .transparent(false)
        .min_inner_size(MIN_W, MIN_H)
        .inner_size(1024.0, 1152.0)
        .visible(false)
        .on_page_load(move |w, p| on_page_load(&page_app, &w, &p));
    // WebView2's profile folder. WKWebView keeps its data per app itself (~/Library/WebKit/<bundle id>).
    #[cfg(not(target_os = "macos"))]
    let builder = builder.data_directory(webview_dir.clone());
    // WebView2 only: the GPU pin (and wry's own default arguments, re-appended there).
    #[cfg(windows)]
    let builder = builder.additional_browser_args(&browser_args);
    #[cfg(not(windows))]
    let _ = &browser_args;
    let win = builder.build()?;
    log::info!(
        "window built in {} ms (frameless {}, webview data {})",
        t.elapsed().as_millis(),
        cfg.ui.frameless,
        if cfg!(target_os = "macos") { "kept by WebKit".to_string() } else { webview_dir.display().to_string() }
    );
    // Panel mode comes back the way it was left: straight onto the small screen, before the first frame.
    let panel_target = panel::startup(app, &win);
    let wanted = match &panel_target {
        Some(t) => {
            if let Err(e) = panel::place(&win, t) {
                log::warn!("panel: could not place the window at startup: {e}");
            }
            None
        }
        None => s.geometry.restore(&win, None),
    };
    win.show()?;
    match &panel_target {
        Some(t) => {
            if let Err(e) = panel::fill(&win, t) {
                log::warn!("panel: could not fill the screen at startup: {e}");
            }
        }
        None => s.geometry.settle(&win, wanted),
    }
    // Built hidden (so geometry is applied before the first frame): the WebView2 controller was created
    // invisible and show() does not change that.
    webview::set_visible(&win, true);
    let _ = win.set_focus();
    log::info!("window shown: {}", panel::describe(&win));
    shell::set_showing(app, true);
    {
        let h = app.clone();
        win.on_window_event(move |e| on_window_event(&h, e));
    }
    tray::build(app)?;
    start_engine(app.clone(), loaded);
    {
        // Geometry is written at most every 2 s while it changes, and on exit. Every 10 s the monitors are
        // re-read, so a status screen plugged in later makes panel mode available.
        let s = s.clone();
        let h = app.clone();
        std::thread::Builder::new()
            .name("klif-geometry".into())
            .spawn(move || {
                let mut n = 0u32;
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    s.geometry.flush();
                    n += 1;
                    if n % 5 == 0 {
                        panel::refresh(&h);
                    }
                }
            })?;
    }
    #[cfg(feature = "selftest")]
    selftest::exit_after(app);
    Ok(())
}

/// How long to wait before asking again while another process holds the engine.
const ENGINE_RETRY: Duration = Duration::from_secs(2);

/// Start the engine off the main thread so the window appears at once; the UI shows "Connecting" until
/// `klif_snapshot` returns. While another process (klif-cli) holds `engine.lock` the start is retried every
/// 2 s for as long as it takes: there is never a second engine. Any other failure (or a panic in an unfinished
/// engine) becomes the UI's error line.
fn start_engine(app: AppHandle, loaded: LoadedConfig) {
    let spawned = std::thread::Builder::new().name("klif-engine-start".into()).spawn(move || {
        let s = shell::shell(&app);
        let t = Instant::now();
        let mut first = Some(loaded);
        let mut attempt = 0u32;
        let mut shown: Option<String> = None;
        let mut logged: Option<Instant> = None;
        let r = loop {
            attempt += 1;
            // A retry re-reads klif.toml, so fixes made while waiting are picked up.
            let cfg = first.take().unwrap_or_else(klif_common::config::load);
            let host = s.host.lock().unwrap().clone();
            match std::panic::catch_unwind(AssertUnwindSafe(|| begin_engine(cfg, host, attempt))) {
                Ok(Ok(h)) => break Ok(h),
                Ok(Err(engine::StartError::Busy { pid })) => {
                    let msg = format!("klif-cli (pid {pid}) holds the engine \u{2014} retrying");
                    if shown.as_deref() != Some(msg.as_str()) {
                        s.set_waiting(msg.clone());
                        tray::set_tooltip(&app, Some(&format!("KLIF: {msg}")));
                        shown = Some(msg.clone());
                    }
                    // Once at first and then every 30 s, not every retry.
                    if logged.map(|l| l.elapsed() >= Duration::from_secs(30)).unwrap_or(true) {
                        log::warn!("{msg} (attempt {attempt}, {} s so far)", t.elapsed().as_secs());
                        logged = Some(Instant::now());
                    }
                    std::thread::sleep(ENGINE_RETRY);
                }
                Ok(Err(e)) => break Err(e.to_string()),
                Err(p) => break Err(format!("the engine crashed while starting ({})", panic_text(&*p))),
            }
        };
        if shown.is_some() {
            tray::set_tooltip(&app, None);
        }
        match &r {
            Ok(h) => {
                log::info!("engine started in {} ms ({attempt} attempt{})", t.elapsed().as_millis(), if attempt == 1 { "" } else { "s" });
                let push = s.clone();
                let ah = app.clone();
                h.subscribe(Box::new(move |vm| {
                    // Nothing is pushed while the webview is off screen; coming back pushes a fresh snapshot.
                    if push.showing.load(Ordering::Relaxed) && ah.emit("klif://vm", vm).is_ok() {
                        let n = push.vm_events.fetch_add(1, Ordering::Relaxed) + 1;
                        if n == 1 || n % 600 == 0 {
                            // First push and every ~5 min: enough real values to tell from the log alone that
                            // live data (not an empty or sample view model) reaches the UI.
                            log::info!(
                                "klif://vm pushed #{n}: {} systems, session {}, vram {} {:.2}/{:.1} GiB{}, ram {:.1}/{:.1} GiB, cpu {:.0}%",
                                vm.systems.len(),
                                vm.session.as_ref().map(|x| format!("{:?}", x.phase)).unwrap_or_else(|| "none".into()),
                                vm.vram.device,
                                vm.vram.used_gib,
                                vm.vram.total_gib,
                                if vm.vram.dormant.is_some() { " (dormant)" } else { "" },
                                vm.machine.ram_used_gib,
                                vm.machine.ram_total_gib,
                                vm.machine.cpu_pct
                            );
                        }
                    }
                }));
                // Host facts may have changed while the engine started.
                h.set_host(s.host.lock().unwrap().clone());
            }
            Err(e) => log::error!("engine failed to start: {e}"),
        }
        s.set_engine(r);
    });
    if let Err(e) = spawned {
        log::error!("could not start the engine thread: {e}");
    }
}

/// One start attempt. The `selftest` build can pretend that klif-cli holds the engine (KLIF_SELFTEST_BUSY).
fn begin_engine(loaded: LoadedConfig, host: HostInfo, _attempt: u32) -> Result<engine::EngineHandle, engine::StartError> {
    #[cfg(feature = "selftest")]
    if let Some(pid) = selftest::fake_busy(_attempt) {
        return Err(engine::StartError::Busy { pid });
    }
    engine::Engine::start(loaded, host)
}

fn on_page_load<R: Runtime>(app: &AppHandle<R>, _w: &WebviewWindow<R>, p: &PageLoadPayload<'_>) {
    let url = p.url().to_string();
    match p.event() {
        PageLoadEvent::Started => log::info!("page load started: {url}"),
        PageLoadEvent::Finished => {
            log::info!("page loaded: {url} (UI source: {})", if tauri::is_dev() { "DEV SERVER (devUrl)" } else { "embedded assets" });
            let s = shell::shell(app);
            if !s.page_loaded.swap(true, Ordering::Relaxed) {
                check_ui_gpu(app);
                #[cfg(feature = "selftest")]
                {
                    selftest::run(app);
                    selftest::run_panel(app);
                }
            }
        }
    }
}

/// Once the first page is up: check which adapter the WebView2 GPU process renders on, and keep checking
/// when WebView2 restarts it. (WebView2 only.)
#[cfg(windows)]
fn check_ui_gpu<R: Runtime>(app: &AppHandle<R>) {
    let h = app.clone();
    std::thread::spawn(move || {
        // The GPU process exists shortly after the first paint.
        std::thread::sleep(Duration::from_millis(1500));
        verify_ui_gpu(&h);
        watch_gpu_process(&h);
    });
}

#[cfg(not(windows))]
fn check_ui_gpu<R: Runtime>(_app: &AppHandle<R>) {}

/// Locate the WebView2 GPU process and check which adapter it renders on; tell the UI.
#[cfg(windows)]
fn verify_ui_gpu<R: Runtime>(app: &AppHandle<R>) {
    let s = shell::shell(app);
    let Some(win) = main_window(app) else { return };
    let mut pid = None;
    for _ in 0..20 {
        pid = webview::gpu_process_pid(&win);
        if pid.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if pid.is_none() {
        log::warn!("no WebView2 GPU process found (GetProcessInfos)");
    }
    *s.gpu_pid.lock().unwrap() = pid;
    let report = gpu::check_gpu_process(pid, s.ui_gpu.as_ref());
    if !report.ok {
        log::warn!("UI GPU check failed: {report:?}");
    }
    *s.gpu_report.lock().unwrap() = Some(report.clone());
    let _ = app.emit("klif://gpu", &report);
}

/// Re-check when WebView2 restarts its GPU process (e.g. after a driver reset).
#[cfg(windows)]
fn watch_gpu_process<R: Runtime>(app: &AppHandle<R>) {
    let Some(win) = main_window(app) else { return };
    let h = app.clone();
    webview::on_process_infos_changed(&win, move |pid| {
        let s = shell::shell(&h);
        let known = *s.gpu_pid.lock().unwrap();
        if pid.is_some() && pid != known {
            log::info!("WebView2 GPU process changed: {known:?} -> {pid:?}; re-checking");
            *s.gpu_pid.lock().unwrap() = pid;
            let h2 = h.clone();
            std::thread::spawn(move || verify_ui_gpu(&h2));
        }
    });
}

fn on_window_event<R: Runtime>(app: &AppHandle<R>, e: &WindowEvent) {
    let s = shell::shell(app);
    let Some(w) = main_window(app) else { return };
    match e {
        WindowEvent::Resized(size) => {
            // wry never hides the webview on minimize (size is 215x26 then, not 0): do it here, so
            // Chromium stops painting and the page sees visibilityState 'hidden'.
            if w.is_visible().unwrap_or(true) {
                let showing = !w.is_minimized().unwrap_or(false);
                if s.showing.load(Ordering::Relaxed) != showing {
                    log::info!("{} ({}x{}): webview IsVisible({showing})", if showing { "restored" } else { "minimized" }, size.width, size.height);
                    webview::set_visible(&w, showing);
                    shell::set_showing(app, showing);
                }
            }
            let maximized = w.is_maximized().unwrap_or(false);
            let changed = {
                let mut h = s.host.lock().unwrap();
                let changed = h.maximized != maximized;
                h.maximized = maximized;
                changed.then(|| h.clone())
            };
            if let Some(host) = changed {
                log::info!("maximized: {maximized}");
                if let Some(e) = s.engine_if_ready() {
                    e.set_host(host);
                }
            }
            s.geometry.record(&w);
        }
        WindowEvent::Moved(_) => s.geometry.record(&w),
        WindowEvent::Focused(focused) => {
            // Restoring fires Focused four times within ~3 ms: act on the settled value only.
            let seq = s.focus_seq.fetch_add(1, Ordering::Relaxed) + 1;
            let focused = *focused;
            let h = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                let s = shell::shell(&h);
                if s.focus_seq.load(Ordering::Relaxed) != seq {
                    return;
                }
                log::debug!("focus settled: {focused}");
                if focused && !s.showing.load(Ordering::Relaxed) {
                    if let Some(w) = main_window(&h) {
                        if !w.is_minimized().unwrap_or(false) && w.is_visible().unwrap_or(false) {
                            log::info!("focused while marked off screen: webview IsVisible(true)");
                            webview::set_visible(&w, true);
                            shell::set_showing(&h, true);
                        }
                    }
                }
            });
        }
        WindowEvent::CloseRequested { .. } => {
            s.geometry.record(&w);
            s.geometry.flush();
            log::info!("window close requested");
        }
        _ => {}
    }
}
