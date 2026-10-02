//! Console lines KLIF writes about the inference GPU (the engine adds them to the session console,
//! in arrival order with the server's log lines).

use klif_telemetry::dormant::{EntryRecord, PowerState, WakeRecord};

/// "[KLIF] RX 9070 XT went to sleep (D3): 15.4 GiB paged out to RAM; the next request waits while it
/// is restored (ULPS is on for this card)". ULPS is named only when its registry value is 1.
pub fn dormant_line(dev: &str, e: &EntryRecord, ulps_on: bool) -> String {
    let ulps = if ulps_on { " (ULPS is on for this card)" } else { "" };
    let gib = e.paged_out_gib;
    match e.power {
        Some(PowerState::D3) if gib >= 0.5 => {
            format!("[KLIF] {dev} went to sleep (D3): {gib:.1} GiB paged out to RAM; the next request waits while it is restored{ulps}")
        }
        Some(PowerState::D3) => {
            format!("[KLIF] {dev} went to sleep (D3) with the model loaded; the next request waits while it wakes{ulps}")
        }
        other => {
            let state = other.map(|p| format!(", device in {}", p.as_str())).unwrap_or_default();
            format!("[KLIF] {dev} paged the model out to RAM ({gib:.1} GiB{state}); the next request waits while it is restored{ulps}")
        }
    }
}

/// "[KLIF] RX 9070 XT woke up: model back in VRAM after 4.2 s".
pub fn wake_line(dev: &str, w: &WakeRecord) -> String {
    if w.reached_target {
        format!("[KLIF] {dev} woke up: model back in VRAM after {:.1} s", w.seconds)
    } else {
        format!("[KLIF] {dev} woke up: {:.1} GiB back in VRAM after {:.1} s", w.resident_gib, w.seconds)
    }
}
