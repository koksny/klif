//! Development hooks, driven by environment variables (no effect unless set):
//!   KLIF_EXIT_AFTER=<seconds>  the app exits by itself after that many seconds.
//!   KLIF_SELFTEST=1            after the page loaded: minimize/restore and hide/show with the WebView2
//!                              IsVisible state read back, then a second instance is launched and must be
//!                              turned away by the single-instance plugin. Results go to the shell log.
//!                              The engine actions (setRecipe, select, stop, launch krea) run only in a
//!                              `stub-engine` build: against the real engine they would start a server.
//!   KLIF_SELFTEST_PANEL=1      after the page loaded: enter panel mode (the window moves onto the small
//!                              screen and fills it), hold ~3 s, leave again; the window geometry before, in
//!                              and after panel mode and the UI's layout class are written to the shell log.
//!                              Use a scratch KLIF_CONFIG (its directory is the state dir): panel mode
//!                              persists its preference and the window geometry there.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Runtime};

use crate::panel;
use crate::shell::{self, main_window};
use crate::webview;

pub fn exit_after<R: Runtime>(app: &AppHandle<R>) {
    let Some(secs) = std::env::var("KLIF_EXIT_AFTER").ok().and_then(|s| s.trim().parse::<f64>().ok()) else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(secs.max(0.5)));
        let s = shell::shell(&app);
        log::info!(
            "KLIF_EXIT_AFTER={secs}: exiting ({} view models pushed to the UI)",
            s.vm_events.load(Ordering::Relaxed)
        );
        app.exit(0);
    });
}

/// Ask the page to describe itself into the shell log (skin, viewport, visibility, visible text).
fn dom_probe<R: Runtime>(win: &tauri::WebviewWindow<R>, tag: &str) {
    let js = format!(
        "(() => {{ const h = document.querySelector('.host'); const t = (document.body.innerText || '').replace(/\\s+/g, ' ').slice(0, 160); \
         window.__TAURI_INTERNALS__.invoke('klif_ui_log', {{ line: `dom[{tag}]: skin=${{h ? h.dataset.skin : 'none'}} layout=${{(document.querySelector('.stage') || {{ dataset: {{}} }}).dataset.size}} viewport=${{innerWidth}}x${{innerHeight}} visibility=${{document.visibilityState}} text=${{t}}` }}); }})()"
    );
    if let Err(e) = win.eval(js) {
        log::warn!("dom probe failed: {e}");
    }
}

fn wait_until(timeout: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    ok()
}

pub fn panel_enabled() -> bool {
    std::env::var("KLIF_SELFTEST_PANEL").map(|v| v == "1").unwrap_or(false)
}

/// Panel mode round trip (KLIF_SELFTEST_PANEL=1), run after the first page load.
pub fn run_panel<R: Runtime>(app: &AppHandle<R>) {
    if !panel_enabled() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(win) = main_window(&app) else { return };
        let s = shell::shell(&app);
        let mut fails = 0;
        let mut check = |what: &str, ok: bool| {
            log::info!("SELFTEST PANEL {what}: {}", if ok { "ok" } else { "FAIL" });
            if !ok {
                fails += 1;
            }
        };
        // Let the UI connect and draw a few frames first.
        std::thread::sleep(Duration::from_millis(2500));
        for m in panel::describe_monitors(&win) {
            log::info!("SELFTEST PANEL monitor {m}");
        }
        let target = panel::find_target(&win.available_monitors().unwrap_or_default(), s.panel.selector());
        log::info!("SELFTEST PANEL target {target:?}");
        let before = (win.outer_position().ok(), win.outer_size().ok(), win.inner_position().ok(), win.inner_size().ok(), win.is_maximized().ok());
        log::info!("SELFTEST PANEL before: {}; host.panel {:?}", panel::describe(&win), s.host.lock().unwrap().panel);
        dom_probe(&win, "panel before");
        let Some(t) = target else {
            check("a small screen was found", false);
            return;
        };
        check(&format!("the small screen is {} (960x640 expected)", t.label()), true);

        // Enter exactly as a user does: the F3 key in the page (keys.ts -> actions.togglePanel -> the
        // klif_toggle_panel command).
        let t0 = Instant::now();
        let _ = win.eval("window.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }))");
        let entered = wait_until(Duration::from_secs(4), || s.panel.active());
        log::info!("SELFTEST PANEL F3 in the page -> panel active {entered} after {} ms", t0.elapsed().as_millis());
        check("F3 entered panel mode", entered);
        std::thread::sleep(Duration::from_millis(1300));
        log::info!("SELFTEST PANEL in panel mode: {}; host.panel {:?}", panel::describe(&win), s.host.lock().unwrap().panel);
        dom_probe(&win, "panel on");
        let inner = (win.inner_position().ok(), win.inner_size().ok());
        check(
            &format!("window fills {} exactly: client {inner:?}", t.name),
            inner == (Some(tauri::PhysicalPosition::new(t.x, t.y)), Some(tauri::PhysicalSize::new(t.w, t.h))),
        );
        let outer = (win.outer_position().ok(), win.outer_size().ok());
        check(
            &format!("no frame spills past the monitor: outer {outer:?}"),
            outer == (Some(tauri::PhysicalPosition::new(t.x, t.y)), Some(tauri::PhysicalSize::new(t.w, t.h))),
        );
        let vm_active = s.engine_if_ready().map(|e| e.snapshot().host.panel.active);
        check(&format!("view model says panel active ({vm_active:?})"), vm_active == Some(true) || s.engine_if_ready().is_none());

        std::thread::sleep(Duration::from_millis(3000));
        // Leave through the command the skins' button uses (actions.togglePanel on the native transport).
        let t1 = Instant::now();
        let _ = win.eval(
            "window.__TAURI_INTERNALS__.invoke('klif_toggle_panel').then(() => window.__TAURI_INTERNALS__.invoke('klif_ui_log', { line: 'klif_toggle_panel: ok' }), (e) => window.__TAURI_INTERNALS__.invoke('klif_ui_log', { line: 'klif_toggle_panel: rejected: ' + e }))",
        );
        let left = wait_until(Duration::from_secs(4), || !s.panel.active());
        log::info!("SELFTEST PANEL klif_toggle_panel from the page -> panel off {left} after {} ms", t1.elapsed().as_millis());
        check("klif_toggle_panel left panel mode", left);
        std::thread::sleep(Duration::from_millis(1500));
        log::info!("SELFTEST PANEL after: {}; host.panel {:?}", panel::describe(&win), s.host.lock().unwrap().panel);
        dom_probe(&win, "panel off");
        let after = (win.outer_position().ok(), win.outer_size().ok(), win.inner_position().ok(), win.inner_size().ok(), win.is_maximized().ok());
        check(&format!("previous window restored: {before:?} -> {after:?}"), before == after);
        let vm_active = s.engine_if_ready().map(|e| e.snapshot().host.panel.active);
        check(&format!("view model says panel off ({vm_active:?})"), vm_active == Some(false) || s.engine_if_ready().is_none());
        std::thread::sleep(Duration::from_millis(500));
        log::info!("SELFTEST PANEL done: {}", if fails == 0 { "all ok".to_string() } else { format!("{fails} failed") });
    });
}

pub fn enabled() -> bool {
    std::env::var("KLIF_SELFTEST").map(|v| v == "1").unwrap_or(false)
}

/// Run after the first page load finished.
pub fn run<R: Runtime>(app: &AppHandle<R>) {
    if !enabled() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut fails = 0;
        let mut check = |what: &str, ok: bool| {
            log::info!("SELFTEST {what}: {}", if ok { "ok" } else { "FAIL" });
            if !ok {
                fails += 1;
            }
        };
        std::thread::sleep(Duration::from_secs(4));
        let Some(win) = main_window(&app) else { return };
        let s = shell::shell(&app);
        let pushed = s.vm_events.load(Ordering::Relaxed);
        check(&format!("view models flowing ({pushed} pushed in the first ~4 s after load)"), pushed >= 4);
        let v = webview::is_visible(&win);
        check(&format!("initial IsVisible={v:?}"), v == Some(true));
        dom_probe(&win, "start");
        // KLIF_DUMP_VM=<file>: write one serialized view model (to diff its keys against types.ts).
        if let (Ok(path), Some(e)) = (std::env::var("KLIF_DUMP_VM"), s.engine_if_ready()) {
            let r = serde_json::to_string_pretty(&e.snapshot()).map_err(|e| e.to_string()).and_then(|j| std::fs::write(&path, j).map_err(|e| e.to_string()));
            log::info!("view model dumped to {path}: {r:?}");
        }

        // Minimize: the Resized handler must turn the WebView2 controller invisible, restore turns it back.
        let size_before = win.inner_size().ok();
        log::info!("SELFTEST minimize() (inner {size_before:?}, outer {:?})", win.outer_size().ok());
        let _ = win.minimize();
        std::thread::sleep(Duration::from_millis(900));
        let v = webview::is_visible(&win);
        let paused = s.vm_events.load(Ordering::Relaxed);
        check(&format!("minimized -> IsVisible={v:?}, pushes paused"), v == Some(false) && !s.showing.load(Ordering::Relaxed));
        dom_probe(&win, "minimized");
        std::thread::sleep(Duration::from_millis(1100));
        let during = s.vm_events.load(Ordering::Relaxed) - paused;
        check(&format!("no view models pushed while minimized ({during})"), during == 0);
        log::info!("SELFTEST unminimize()");
        let _ = win.unminimize();
        std::thread::sleep(Duration::from_millis(900));
        let v = webview::is_visible(&win);
        check(&format!("restored -> IsVisible={v:?}"), v == Some(true) && s.showing.load(Ordering::Relaxed));
        let size_after = win.inner_size().ok();
        check(&format!("size kept across minimize/restore ({size_before:?} -> {size_after:?})"), size_before == size_after);

        // Hide to tray and back (the tray menu's Show / Hide path).
        shell::hide_main(&app);
        std::thread::sleep(Duration::from_millis(700));
        let v = webview::is_visible(&win);
        check(&format!("hidden -> IsVisible={v:?}"), v == Some(false));
        shell::show_main(&app);
        std::thread::sleep(Duration::from_millis(700));
        let v = webview::is_visible(&win);
        check(&format!("shown -> IsVisible={v:?}"), v == Some(true));

        // Single instance: a second launch must hand over to us and exit at once.
        match std::env::current_exe() {
            Ok(exe) => {
                let t = Instant::now();
                let child = std::process::Command::new(exe)
                    .arg("--selftest-second-instance")
                    .env_remove("KLIF_SELFTEST")
                    .env_remove("KLIF_EXIT_AFTER")
                    .spawn();
                match child {
                    Ok(mut c) => {
                        let mut status = None;
                        while t.elapsed() < Duration::from_secs(15) {
                            if let Ok(Some(st)) = c.try_wait() {
                                status = Some(st);
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        if status.is_none() {
                            let _ = c.kill();
                            let _ = c.wait();
                        }
                        check(
                            &format!("second instance exited by itself: {:?} after {} ms", status.map(|s| s.code()), t.elapsed().as_millis()),
                            status.map(|s| s.success()).unwrap_or(false),
                        );
                    }
                    Err(e) => check(&format!("second instance could not be started: {e}"), false),
                }
            }
            Err(e) => check(&format!("current_exe: {e}"), false),
        }
        let report = s.gpu_report.lock().unwrap().clone();
        check(&format!("UI GPU verified: {report:?}"), report.map(|r| r.ok).unwrap_or(false));

        // The transport's wire shapes, sent from the page exactly as transport/tauri.ts sends them.
        let invoke = |cmd: &str, args: &str, tag: &str| {
            let js = format!(
                "window.__TAURI_INTERNALS__.invoke('{cmd}', {args}).then(() => window.__TAURI_INTERNALS__.invoke('klif_ui_log', {{ line: '{tag}: ok' }}), (e) => window.__TAURI_INTERNALS__.invoke('klif_ui_log', {{ line: '{tag}: rejected: ' + e }}))"
            );
            let _ = win.eval(js);
        };
        // Engine actions only against the stub: with the real engine they would persist a recipe, stop an
        // adopted server and start a real one.
        if !cfg!(feature = "stub-engine") {
            log::info!("SELFTEST engine actions skipped (real engine: they would change recipes and start or stop servers)");
        } else if let Some(e) = s.engine_if_ready() {
            invoke("klif_act", "{ action: { type: 'setRecipe', slot: 'medium', patch: { ctxTokens: 32768, promptCacheMiB: 8192, kvType: 'q4_0' } } }", "setRecipe");
            std::thread::sleep(Duration::from_millis(500));
            let r = e.snapshot().slots.iter().find(|x| x.id == klif_common::vm::SlotId::Medium).and_then(|x| x.recipe.clone());
            check(
                &format!("setRecipe from the page applied: {r:?}"),
                r.map(|r| r.ctx_tokens == Some(32768) && r.prompt_cache_mib == Some(8192) && r.kv_type.as_deref() == Some("q4_0")).unwrap_or(false),
            );
            invoke("klif_act", "{ action: { type: 'select', slot: 'low' } }", "select while running (must be refused)");
            invoke("klif_act", "{ action: { type: 'stop' } }", "stop");
            std::thread::sleep(Duration::from_millis(2500));
            let vm = e.snapshot();
            check(&format!("stop -> session {:?}, last session {:?}", vm.session.as_ref().map(|x| x.phase), vm.last_session.as_ref().map(|l| l.ended)), vm.session.is_none() && vm.last_session.is_some());
            invoke("klif_act", "{ action: { type: 'launch', slot: 'krea' } }", "launch krea");
            std::thread::sleep(Duration::from_millis(1600));
            let p1 = e.snapshot().session.map(|x| (x.slot, x.phase));
            std::thread::sleep(Duration::from_millis(5500));
            let p2 = e.snapshot().session.map(|x| (x.slot, x.phase));
            check(&format!("launch krea -> {p1:?} then {p2:?}"), p2.map(|(sl, ph)| sl == klif_common::vm::SlotId::Krea && ph == klif_common::vm::Phase::Live).unwrap_or(false));
            dom_probe(&win, "krea live");
        }
        // Window chrome through the JS window API's commands (capability check), then back.
        invoke("plugin:window|toggle_maximize", "{ label: 'main' }", "toggleMaximize");
        std::thread::sleep(Duration::from_millis(800));
        let maxed = win.is_maximized().unwrap_or(false);
        let host_max = s.host.lock().unwrap().maximized;
        check(&format!("toggleMaximize from the page -> maximized {maxed}, HostInfo.maximized {host_max}"), maxed && host_max);
        invoke("plugin:window|toggle_maximize", "{ label: 'main' }", "toggleMaximize back");
        std::thread::sleep(Duration::from_millis(800));
        check("toggleMaximize back", !win.is_maximized().unwrap_or(true));
        // Tray Skin submenu path: the same event the menu emits.
        let _ = tauri::Emitter::emit(&app, "klif://skin", "phosphor");
        std::thread::sleep(Duration::from_millis(1200));
        dom_probe(&win, "after klif://skin phosphor");
        std::thread::sleep(Duration::from_millis(400));
        let _ = tauri::Emitter::emit(&app, "klif://skin", "cliff");
        log::info!("SELFTEST done: {}", if fails == 0 { "all ok".to_string() } else { format!("{fails} failed") });
    });
}
