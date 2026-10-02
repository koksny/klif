//! The dormant detector on REAL counters, without any server: treat existing processes as "the
//! session" and feed their PDH memory on the inference adapter plus the card's real power state into
//! `DormantTracker` at ~1 Hz. Read-only: nothing is started or stopped.
//!
//!   cargo run -p klif-telemetry --example dormant_live -- --pid N [--pid M ...] [--samples K]
//!
//! (Processes that opened the display-less card, e.g. Electron apps, usually hold allocations there
//! that are not resident: the same "allocated but paged out" pattern a sleeping model shows.)

use klif_telemetry::dormant::{DormantSample, DormantTracker};
use klif_telemetry::{find_adapter, read_gpu_once, text, vram, PowerReader};

fn main() {
    let mut pids: Vec<u32> = Vec::new();
    let mut samples = 4u32;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pid" => pids.extend(args.next().and_then(|v| v.parse::<u32>().ok())),
            "--samples" => samples = args.next().and_then(|v| v.parse().ok()).unwrap_or(samples),
            _ => {}
        }
    }
    let cfg = klif_common::config::Config::load().expect("klif.toml");
    let pci = cfg.gpu.inference.clone().expect("gpu.inference");
    let adapter = find_adapter(&pci).expect("inference adapter");
    println!("== {} ({}), pdh {}, pids {:?}", adapter.name, adapter.pci_id(), adapter.pdh_luid(), pids);
    let mut power = PowerReader::new(Some(&adapter));
    let mut tracker = DormantTracker::default();
    for i in 0..samples {
        // Opens the PDH query, waits 1 s, collects (the sampler keeps its query open instead).
        let Some(g) = read_gpu_once(&adapter, &pids) else {
            println!("no PDH reading");
            return;
        };
        let now = klif_common::now_s();
        let p = power.read(now);
        let gib = |b: f64| b / text::GIB;
        let s = DormantSample {
            at: now,
            live: true,
            committed_gib: g.session_committed_bytes.map(gib),
            resident_gib: g.session_bytes.map(gib),
            shared_gib: gib(g.session_shared_bytes),
            power: p,
            busy_since: None,
        };
        let ev = tracker.sample(&s);
        println!(
            "#{i} adapter resident {:.3} GiB | session committed {:?} resident {:?} shared {:.3} GiB | power {:?} | events {:?}",
            gib(g.used_bytes),
            s.committed_gib.map(|v| text::round_to(v, 3)),
            s.resident_gib.map(|v| text::round_to(v, 3)),
            s.shared_gib,
            p,
            ev
        );
        if let Some(d) = tracker.current() {
            let used = gib(g.used_bytes);
            let c = vram::compose_dormant(used, d.resident_gib, d.vram_gib, None);
            let vm = klif_common::vm::Dormant {
                paged_out_gib: text::round_to(d.paged_out_gib, 3),
                since_s: text::round_to(now - d.since, 1),
                power_state: d.power.map(|p| p.as_str().to_string()),
            };
            println!("   vram.dormant = {}", serde_json::to_string(&vm).unwrap());
            println!(
                "   usedGiB {:.3} (resident) | layers {}",
                used,
                c.layers.iter().map(|l| format!("{}:{:.3}", l.label, l.gib)).collect::<Vec<_>>().join(", ")
            );
        }
    }
}
