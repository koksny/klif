//! Replay of the dormant-GPU detector on synthetic 1 Hz samples (no GPU, no server, no PDH).
//!
//!   cargo run -p klif-core --example dormant_replay [-- --ulps 0|1] [--verbose]
//!
//! Each scenario feeds `DormantTracker` the way the telemetry sampler does, composes the cliff the
//! way it does (resident composition, or the allocations while dormant), and narrates the episodes
//! the way the engine does (`klif_core::narrate`). It prints the transitions, the console lines, the
//! serialized `vram.dormant`, and a verdict per scenario.

use klif_core::klif_common::vm::{Dormant, VramLayer, VramLayerId};
use klif_core::klif_telemetry::dormant::{DormancyEvent, DormantSample, DormantTracker, PowerState};
use klif_core::klif_telemetry::vram;
use klif_core::narrate::{dormant_line, wake_line};

const DEV: &str = "RX 9070 XT";
/// VRAM held by others (driver, desktop): the 'other' layer.
const OTHER: f64 = 0.45;

#[derive(Clone, Copy)]
struct Row {
    t: f64,
    live: bool,
    committed: f64,
    resident: f64,
    shared: f64,
    power: Option<PowerState>,
    busy_since: Option<f64>,
}

fn row(t: f64, committed: f64, resident: f64, shared: f64, power: Option<PowerState>) -> Row {
    Row { t, live: true, committed, resident, shared, power, busy_since: None }
}

struct Outcome {
    lines: Vec<String>,
    events: Vec<(f64, DormancyEvent)>,
    max_dormant_s: f64,
    dormant_at_end: bool,
}

fn llama_layers() -> Vec<VramLayer> {
    vec![
        VramLayer { id: VramLayerId::Weights, label: "weights".into(), gib: 11.29 },
        VramLayer { id: VramLayerId::Kv, label: "KV cache".into(), gib: 3.1 },
        VramLayer { id: VramLayerId::Buffers, label: "buffers".into(), gib: 1.05 },
        VramLayer { id: VramLayerId::Draft, label: "draft".into(), gib: 0.5 },
    ]
}

fn run(name: &str, rows: &[Row], log_layers: Option<&[VramLayer]>, ulps_on: bool, verbose: bool) -> Outcome {
    println!("\n=== {name}");
    let t0 = 1_790_000_000.0;
    let mut tr = DormantTracker::default();
    let (mut dormant_logged, mut wake_logged) = (0u64, 0u64);
    let mut out = Outcome { lines: Vec::new(), events: Vec::new(), max_dormant_s: 0.0, dormant_at_end: false };
    let mut shown_json = false;
    for r in rows {
        let at = t0 + r.t;
        let s = DormantSample {
            at,
            live: r.live,
            committed_gib: Some(r.committed),
            resident_gib: Some(r.resident),
            shared_gib: r.shared,
            power: r.power,
            busy_since: r.busy_since.map(|b| t0 + b),
        };
        for e in tr.sample(&s) {
            let shown = match &e {
                DormancyEvent::Entered(x) => format!(
                    "ENTER  episode {} since t={:.0} paged out {:.2} GiB power {:?}",
                    x.episode,
                    x.at - t0,
                    x.paged_out_gib,
                    x.power
                ),
                DormancyEvent::Cleared { episode, .. } => format!("CLEAR  episode {episode}"),
                DormancyEvent::Woke(w) => format!(
                    "WOKE   episode {} {:.1} s (t={:.1}..{:.1}) resident {:.2} GiB reached={}",
                    w.episode,
                    w.seconds,
                    w.start - t0,
                    w.end - t0,
                    w.resident_gib,
                    w.reached_target
                ),
            };
            println!("  t={:>5.0}  {shown}", r.t);
            out.events.push((r.t, e));
        }
        // The engine's narration (Engine::narrate_dormancy, live phase).
        let f = tr.facts();
        if let Some(e) = f.last_entry.as_ref().filter(|e| e.episode > dormant_logged) {
            dormant_logged = e.episode;
            out.lines.push(dormant_line(DEV, e, ulps_on));
        }
        if let Some(w) = f.last_wake.as_ref().filter(|w| w.episode > wake_logged) {
            wake_logged = w.episode;
            out.lines.push(wake_line(DEV, w));
        }
        // The sampler's composition.
        let used = OTHER + r.resident;
        let (layers, dormant) = match tr.current() {
            Some(d) => {
                let c = vram::compose_dormant(used, d.resident_gib, d.vram_gib, log_layers);
                let dm = Dormant {
                    paged_out_gib: (d.paged_out_gib * 1000.0).round() / 1000.0,
                    since_s: ((at - d.since) * 10.0).round() / 10.0,
                    power_state: d.power.map(|p| p.as_str().to_string()),
                };
                out.max_dormant_s = out.max_dormant_s.max(dm.since_s);
                (c.layers, Some(dm))
            }
            None => (vram::compose(used, r.live, Some(r.resident), log_layers, Some(OTHER)).layers, None),
        };
        if let (Some(d), false) = (&dormant, shown_json) {
            if d.since_s >= 3.0 {
                shown_json = true;
                let sum: f64 = layers.iter().map(|l| l.gib).sum();
                println!("  t={:>5.0}  vram.dormant = {}", r.t, serde_json::to_string(d).unwrap());
                println!(
                    "            usedGiB {:.2} (resident) | layers {} = {:.2} GiB (allocations)",
                    used,
                    layers.iter().map(|l| format!("{}:{:.2}", l.label, l.gib)).collect::<Vec<_>>().join(", "),
                    sum
                );
            }
        }
        if verbose {
            println!(
                "  t={:>5.0}  C {:>5.2} R {:>5.2} S {:>5.2} {:<4} busy {:<5} -> {}",
                r.t,
                r.committed,
                r.resident,
                r.shared,
                r.power.map(|p| p.as_str()).unwrap_or("-"),
                r.busy_since.is_some(),
                dormant.as_ref().map(|d| format!("DORMANT {:.2} GiB {:.0}s", d.paged_out_gib, d.since_s)).unwrap_or_else(|| "awake".into())
            );
        }
        out.dormant_at_end = dormant.is_some();
    }
    for l in &out.lines {
        println!("  console: {l}");
    }
    out
}

/// Fill `rows` with a constant state from t0 to t1 (inclusive start, exclusive end).
fn hold(rows: &mut Vec<Row>, t0: u32, t1: u32, mut f: impl FnMut(f64) -> Row) {
    for t in t0..t1 {
        rows.push(f(t as f64));
    }
}

fn verdict(name: &str, ok: bool, what: &str, fails: &mut u32) {
    println!("  verdict [{name}]: {} - {what}", if ok { "OK" } else { "FAIL" });
    if !ok {
        *fails += 1;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ulps_on = args.windows(2).any(|w| w[0] == "--ulps" && w[1] == "1");
    let verbose = args.iter().any(|a| a == "--verbose");
    let mut fails = 0u32;
    let d0 = Some(PowerState::D0);
    let d3 = Some(PowerState::D3);
    // The measured case: committed ~16.4 GiB, ~0.35 GiB of it in shared memory (host buffers).
    let (c, sh, awake) = (16.40, 0.35, 15.98);
    let layers = llama_layers();

    // 1) ULPS: idle -> D3 + VRAM paged out -> a request wakes it (~4 s restore) -> idle -> sleeps again
    //    -> something else wakes it (resident rises, no request).
    let mut r: Vec<Row> = Vec::new();
    hold(&mut r, 0, 5, |t| Row { live: false, ..row(t, c * t / 5.0, awake * t / 5.0, sh, d0) });
    hold(&mut r, 5, 32, |t| Row { busy_since: (8.0..14.0).contains(&t).then_some(8.2), ..row(t, c, awake, sh, d0) });
    r.push(row(32.0, c, 0.06, sh, d0)); // evicted, power not updated yet
    hold(&mut r, 33, 91, |t| row(t, c, 0.06, sh, d3));
    // A request at t=91.3; the device is D0 again, VRAM is restored over ~4 s.
    for (t, res) in [(92.0, 0.06), (93.0, 3.9), (94.0, 8.1), (95.0, 12.4), (96.0, 15.97)] {
        r.push(Row { busy_since: Some(91.3), ..row(t, c, res, sh, d0) });
    }
    hold(&mut r, 97, 120, |t| Row { busy_since: (t < 110.0).then_some(91.3), ..row(t, c, awake, sh, d0) });
    r.push(row(120.0, c, 0.05, sh, d0));
    hold(&mut r, 121, 140, |t| row(t, c, 0.05, sh, d3));
    for (t, res) in [(140.0, 0.05), (141.0, 6.0), (142.0, 11.0), (143.0, 15.98)] {
        r.push(row(t, c, res, sh, d0));
    }
    hold(&mut r, 144, 150, |t| row(t, c, awake, sh, d0));
    let o = run("1. ULPS sleep with a model loaded, woken by a request, then by other activity", &r, Some(&layers), ulps_on, verbose);
    let wakes: Vec<f64> = o
        .events
        .iter()
        .filter_map(|(_, e)| match e {
            DormancyEvent::Woke(w) => Some(w.seconds),
            _ => None,
        })
        .collect();
    let entries = o.events.iter().filter(|(_, e)| matches!(e, DormancyEvent::Entered(_))).count();
    verdict(
        "1",
        entries == 2 && wakes.len() == 2 && (4.0..=5.0).contains(&wakes[0]) && wakes[1] <= 4.0 && o.lines.len() == 4 && !o.dormant_at_end,
        &format!("2 episodes, wake times {wakes:?} s (request at 91.3 s, back at 96 s), 4 console lines"),
        &mut fails,
    );

    // 2) A model far over the cliff: most of it lives in shared memory (spill), nothing is paged out.
    let mut r: Vec<Row> = Vec::new();
    hold(&mut r, 0, 30, |t| row(t, 35.0, 15.6, 19.0, d0));
    let o = run("2. Heavy spill (35 GiB committed, 19 GiB in shared memory, 15.6 resident)", &r, None, ulps_on, verbose);
    let literal = 35.0 - 15.6 >= f64::max(1.0, 0.5 * 35.0);
    verdict(
        "2",
        o.events.is_empty(),
        &format!("never dormant (the literal committed-resident rule would have said dormant: {literal})"),
        &mut fails,
    );

    // 3) D3 while the VRAM stays resident: dormant on D3, cleared at D0.
    let mut r: Vec<Row> = Vec::new();
    hold(&mut r, 0, 10, |t| row(t, c, awake, sh, d0));
    hold(&mut r, 10, 20, |t| row(t, c, awake, sh, d3));
    hold(&mut r, 20, 30, |t| row(t, c, awake, sh, d0));
    let o = run("3. D3 reported, memory still resident", &r, Some(&layers), ulps_on, verbose);
    let woke = o.events.iter().any(|(_, e)| matches!(e, DormancyEvent::Woke(_)));
    verdict("3", o.lines.len() == 2 && woke && !o.dormant_at_end, "enter on D3 (no GiB claimed), wake at D0", &mut fails);

    // 4) One-sample dip (a glitch): not dormant (2 consecutive samples needed).
    let mut r: Vec<Row> = Vec::new();
    hold(&mut r, 0, 10, |t| row(t, c, awake, sh, d0));
    r.push(row(10.0, c, 0.1, sh, d0));
    hold(&mut r, 11, 20, |t| row(t, c, awake, sh, d0));
    let o = run("4. Single-sample dip", &r, None, ulps_on, verbose);
    verdict("4", o.events.is_empty(), "no episode", &mut fails);

    // 5) The session stops while dormant: state dropped, no wake line.
    let mut r: Vec<Row> = Vec::new();
    hold(&mut r, 0, 10, |t| row(t, c, awake, sh, d0));
    hold(&mut r, 10, 20, |t| row(t, c, 0.05, sh, d3));
    hold(&mut r, 20, 25, |t| Row { live: false, ..row(t, 0.0, 0.0, 0.0, d3) });
    let o = run("5. Session stopped while dormant", &r, None, ulps_on, verbose);
    verdict("5", o.lines.len() == 1 && !o.dormant_at_end, "one entry line, then reset without a wake line", &mut fails);

    // 6) Restore plateaus below the pre-sleep level (85%): cleared at 80%, the wake ends at the last rise.
    let mut r: Vec<Row> = Vec::new();
    hold(&mut r, 0, 10, |t| row(t, c, awake, sh, d0));
    hold(&mut r, 10, 30, |t| row(t, c, 0.05, sh, d3));
    r.push(Row { busy_since: Some(30.4), ..row(30.0, c, 0.05, sh, d0) });
    r.push(Row { busy_since: Some(30.4), ..row(31.0, c, 7.0, sh, d0) });
    hold(&mut r, 32, 45, |t| Row { busy_since: Some(30.4), ..row(t, c, 13.6, sh, d0) });
    let o = run("6. Restore plateaus at 85%", &r, None, ulps_on, verbose);
    let w = o.events.iter().find_map(|(_, e)| match e {
        DormancyEvent::Woke(w) => Some(w.clone()),
        _ => None,
    });
    verdict(
        "6",
        w.as_ref().map(|w| !w.reached_target && (1.0..=2.0).contains(&w.seconds)).unwrap_or(false) && !o.dormant_at_end,
        &format!("cleared, wake ended at the plateau: {:?}", w.map(|w| (w.seconds, w.resident_gib))),
        &mut fails,
    );

    // 7) Memory paged out without D3 (power D0): dormant by the ratio rule alone.
    let mut r: Vec<Row> = Vec::new();
    hold(&mut r, 0, 10, |t| row(t, c, awake, sh, d0));
    hold(&mut r, 10, 20, |t| row(t, c, 2.0, sh, d0));
    hold(&mut r, 20, 25, |t| row(t, c, awake, sh, d0));
    let o = run("7. Paged out, device still D0", &r, None, ulps_on, verbose);
    verdict("7", o.lines.len() == 2 && !o.dormant_at_end, "ratio rule enters, recovery clears", &mut fails);

    println!("\n{} scenario(s) failed", fails);
    if fails > 0 {
        std::process::exit(1);
    }
}
