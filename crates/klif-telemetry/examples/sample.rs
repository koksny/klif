//! Live telemetry sample: lists DXGI adapters, then prints one GpuMemory + MachineStats snapshot of
//! the inference adapter taken by the real sampler thread.
//!
//!   cargo run -p klif-telemetry --example sample [-- VEN:DEV] [--pid N ...] [--secs S]
//!                                                [--probe HOST:PORT] [--tail LOGCOPY]
//!
//! - The adapter defaults to `gpu.inference` from klif.toml.
//! - `--pid` marks processes as "the session" to show the per-process composition.
//! - `--probe` runs the llama /health probe and the TCP listener check once (no /slots: without a key
//!   it would only add an "unauthorized" line to a running server's log).
//! - `--tail` watches a scratch log next to LOGCOPY (a COPY of a llama-server .err.log) through the
//!   real Telemetry threads (probes on) while this process appends the copy's lines, then prints
//!   the SessionSignals summary and the console tail.
//!
//! Nothing is started or stopped besides this process's own threads.

use klif_telemetry::probe::{tcp_listening, Prober};
use klif_telemetry::{adapters, find_adapter, Telemetry, WatchSpec};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let mut pci: Option<String> = None;
    let mut pids: Vec<u32> = Vec::new();
    let mut secs = 3.5f64;
    let mut probe: Option<String> = None;
    let mut tail: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pid" => pids.extend(args.next().and_then(|v| v.parse::<u32>().ok())),
            "--secs" => secs = args.next().and_then(|v| v.parse().ok()).unwrap_or(secs),
            "--probe" => probe = args.next(),
            "--tail" => tail = args.next().map(PathBuf::from),
            _ => pci = Some(a),
        }
    }
    let cfg = Some(klif_common::config::load().cfg);
    let from_cfg = pci.is_none();
    let pci = pci.or_else(|| cfg.as_ref().and_then(|c| c.gpu.inference.clone()));
    let warn = cfg.as_ref().map(|c| c.telemetry.warn_below_gib).unwrap_or(0.15);

    if let Some(hp) = probe {
        let (h, p) = hp.rsplit_once(':').expect("HOST:PORT");
        let port: u16 = p.parse().expect("port");
        let t0 = std::time::Instant::now();
        let health = Prober::new().health(h, port);
        let dt = t0.elapsed();
        let listening = tcp_listening(h, port, Duration::from_millis(300));
        println!("== probe {hp}: /health -> {health:?} in {} ms; tcp listening: {listening}", dt.as_millis());
    }

    println!("== DXGI adapters (hardware only)");
    for a in adapters() {
        println!(
            "  {}  {:<28} luid {:>3},{:<7} pdh {}  dedicated {:.0} MiB  chromium {}",
            a.pci_id(),
            a.name,
            a.luid_high,
            a.luid_low,
            a.pdh_luid(),
            a.dedicated_bytes as f64 / 1048576.0,
            a.chromium_luid_arg()
        );
    }
    let mut inf = pci.as_deref().and_then(find_adapter);
    if from_cfg {
        if let (Some(a), Some(name)) = (inf.as_mut(), cfg.as_ref().and_then(|c| c.gpu.inference_name.clone())) {
            a.name = name;
        }
    }
    match &inf {
        Some(a) => println!("== inference adapter: {} ({}), display name '{}'", a.pci_id(), a.name, a.display_name()),
        None => println!("== no inference adapter resolved (pass VEN:DEV or set gpu.inference)"),
    }

    let t = Telemetry::start(inf.into_iter().collect(), warn);
    let mut writer: Option<(std::fs::File, Vec<String>)> = None;
    if let Some(src) = &tail {
        let dir = src.parent().unwrap().join("run-live-tail");
        std::fs::create_dir_all(&dir).unwrap();
        let err = dir.join("session.err.log");
        let out = dir.join("session.out.log");
        let f = std::fs::File::create(&err).unwrap();
        std::fs::File::create(&out).unwrap();
        let lines: Vec<String> = String::from_utf8_lossy(&std::fs::read(src).unwrap()).lines().map(|l| l.to_string()).collect();
        t.watch(
            "sample",
            WatchSpec {
                kind: klif_common::vm::SystemKind::Llm,
                adapter: klif_common::vm::AdapterId::LlamaCpp,
                external: false,
                out_log: Some(out),
                err_log: Some(err),
                host: "127.0.0.1".into(),
                port: 7030,
                api_key: None,
                started_at: klif_common::now_s(),
                ctx_tokens: None,
                spec_mode: None,
                health: klif_common::vm::HealthCheck::Auto,
                metrics: false,
                expect_device: None,
                gpu: None,
            },
            false,
        );
        writer = Some((f, lines));
    } else if !pids.is_empty() {
        // A session with no logs: only the per-process measurement feeds the session layer.
        let dir = std::env::temp_dir();
        t.watch(
            "sample",
            WatchSpec {
                kind: klif_common::vm::SystemKind::Llm,
                adapter: klif_common::vm::AdapterId::LlamaCpp,
                external: false,
                out_log: Some(dir.join("klif-sample-none.out.log")),
                err_log: Some(dir.join("klif-sample-none.err.log")),
                host: "127.0.0.1".into(),
                port: 9,
                api_key: None,
                started_at: klif_common::now_s(),
                ctx_tokens: None,
                spec_mode: None,
                health: klif_common::vm::HealthCheck::Auto,
                metrics: false,
                expect_device: None,
                gpu: None,
            },
            false,
        );
    }
    if !pids.is_empty() {
        t.set_session_pids("sample", pids.clone());
    }
    match writer.as_mut() {
        Some((f, lines)) => {
            // Append the copy in ~10 bursts over `secs` seconds.
            let n = lines.len();
            let step = n.div_ceil(10).max(1);
            for chunk in lines.chunks(step) {
                for l in chunk {
                    writeln!(f, "{l}").unwrap();
                }
                std::thread::sleep(Duration::from_secs_f64(secs / 10.0));
            }
            std::thread::sleep(Duration::from_millis(1200));
        }
        None => std::thread::sleep(Duration::from_secs_f64(secs)),
    }
    let snap = t.snapshot();
    println!("== GpuMemory (session pids: {:?})", pids);
    for g in &snap.gpus {
        println!("{}", serde_json::to_string_pretty(&g.memory).unwrap());
    }
    println!("== MachineStats");
    println!("{}", serde_json::to_string_pretty(&snap.machine).unwrap());
    if let Some(s) = snap.sessions.get("sample") {
        println!("spill_mib = {}", s.spill_mib);
        println!("== SessionSignals: health {:?}, load {:.2}, fatal {:?}", s.health, s.load_fraction, s.fatal_hint);
        for st in &s.load_steps {
            println!("   {:?} {:?} {:?}", st.id, st.state, st.detail);
        }
        println!("   layers_from_logs {:?}", s.layers_from_logs.as_ref().map(|l| l.iter().map(|x| (x.label.clone(), x.gib)).collect::<Vec<_>>()));
        if let Some(l) = &s.llm {
            println!(
                "   llm {:?} decodeTps {} generated {} ctx {}/{} requests {} spec {:?}",
                l.activity, l.decode_tps, l.generated_tokens, l.context.used_tokens, l.context.total_tokens, l.totals.requests, l.spec
            );
        }
        println!("== console: {} lines, last 3:", s.console.len());
        for c in s.console.iter().rev().take(3).rev() {
            println!("   {}", c.chars().take(150).collect::<String>());
        }
    }
    t.unwatch("sample");
}
