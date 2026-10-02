//! The "dormant GPU" detector: the inference card powered down (AMD ULPS / device state D3) or paged
//! the session out while a model is loaded. The session's allocations still exist (PDH Total
//! Committed) but are not resident in VRAM (PDH Dedicated Usage), and the next request first waits
//! while the driver restores them.
//!
//! Pure state machine (no Win32): the sampler feeds one `DormantSample` per second, a replay can feed
//! synthetic ones. Quantities are GiB, times epoch seconds.
//!
//! Rules (session = the watched server's processes on the inference adapter):
//! - `vram` = committed - shared: the allocations that belong in VRAM (what sits in shared system
//!   memory is spill / host buffers, never "paged out"). `paged out` = vram - resident.
//! - Enter: the session has been ready (live) AND (power state D3 OR paged out >= max(1 GiB, 50% of
//!   committed)) on 2 consecutive samples. `since` = the first of them.
//! - Clear: resident >= 80% of vram and the device is not in D3 (checked first while dormant).
//! - Wake time: from the first sign of waking (the request that woke it, by its start time in the
//!   log, else the sample before resident rose 0.25 GiB above its floor) until resident >= 95% of the
//!   level before it slept (vram when that is unknown); if it plateaus below that after clearing
//!   (no rise for 5 s), the last rise ends it.

use crate::text::round_to;

pub const ENTER_MIN_GIB: f64 = 1.0;
pub const ENTER_FRAC: f64 = 0.5;
pub const ENTER_SAMPLES: u32 = 2;
pub const EXIT_FRAC: f64 = 0.8;
pub const WAKE_DONE_FRAC: f64 = 0.95;
/// Resident above the dormant floor by this much = restoring has begun.
const RISE_GIB: f64 = 0.25;
/// Growth smaller than this between samples is noise, not restoring.
const STEP_GIB: f64 = 0.05;
/// After clearing, no growth for this long ends the wake measurement below its target.
const PLATEAU_S: f64 = 5.0;
/// A session committing less than this holds no model worth calling dormant.
const MIN_COMMITTED_GIB: f64 = 0.5;
/// Samples without measurements before the state is dropped (the processes are gone).
const MISSING_RESET: u32 = 3;

/// Device power state (CM_POWER_DATA.PD_MostRecentPowerState).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    D0,
    D1,
    D2,
    D3,
}

impl PowerState {
    /// DEVICE_POWER_STATE: 1 = D0 .. 4 = D3; 0 (unspecified) and 5 (maximum) are not states.
    pub fn from_raw(v: u32) -> Option<PowerState> {
        match v {
            1 => Some(PowerState::D0),
            2 => Some(PowerState::D1),
            3 => Some(PowerState::D2),
            4 => Some(PowerState::D3),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            PowerState::D0 => "D0",
            PowerState::D1 => "D1",
            PowerState::D2 => "D2",
            PowerState::D3 => "D3",
        }
    }
}

/// One 1 Hz observation of the session on the inference adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct DormantSample {
    pub at: f64,
    /// The session's server has answered ready since it was watched.
    pub live: bool,
    /// Session PIDs' Total Committed on the adapter (None: no PIDs / no instances yet).
    pub committed_gib: Option<f64>,
    /// Session PIDs' Dedicated Usage on the adapter.
    pub resident_gib: Option<f64>,
    /// Session PIDs' Shared Usage on the adapter.
    pub shared_gib: f64,
    pub power: Option<PowerState>,
    /// Start of the request / job in flight, if the server is busy.
    pub busy_since: Option<f64>,
}

/// The dormant state right now.
#[derive(Debug, Clone, PartialEq)]
pub struct DormantNow {
    pub episode: u64,
    pub since: f64,
    pub committed_gib: f64,
    /// committed - shared: what belongs in VRAM.
    pub vram_gib: f64,
    pub resident_gib: f64,
    pub paged_out_gib: f64,
    pub power: Option<PowerState>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntryRecord {
    pub episode: u64,
    pub at: f64,
    pub paged_out_gib: f64,
    pub power: Option<PowerState>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WakeRecord {
    pub episode: u64,
    pub start: f64,
    pub end: f64,
    pub seconds: f64,
    /// Resident at the end of the wake.
    pub resident_gib: f64,
    /// Resident reached 95% of its level before sleep (else it plateaued below it).
    pub reached_target: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DormancyEvent {
    Entered(EntryRecord),
    Cleared { episode: u64, at: f64 },
    Woke(WakeRecord),
}

/// What the engine needs to narrate dormancy (episode ids let it log each transition once).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DormancyFacts {
    /// Episodes since the session was watched (id of the current or last one; 0 = none yet).
    pub episode: u64,
    pub current: Option<DormantNow>,
    pub last_entry: Option<EntryRecord>,
    pub last_wake: Option<WakeRecord>,
}

#[derive(Debug, Clone)]
struct Waking {
    episode: u64,
    start: Option<f64>,
    floor: f64,
    target: f64,
    last_r: f64,
    last_rise_at: f64,
    cleared_at: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct DormantTracker {
    streak: u32,
    first_hit: Option<f64>,
    missing: u32,
    prev_at: Option<f64>,
    current: Option<DormantNow>,
    waking: Option<Waking>,
    /// Resident at the last healthy (awake) sample: the level a wake restores.
    awake_resident: Option<f64>,
    episode: u64,
    last_entry: Option<EntryRecord>,
    last_wake: Option<WakeRecord>,
}

impl DormantTracker {
    pub fn current(&self) -> Option<&DormantNow> {
        self.current.as_ref()
    }

    pub fn facts(&self) -> DormancyFacts {
        DormancyFacts {
            episode: self.episode,
            current: self.current.clone(),
            last_entry: self.last_entry.clone(),
            last_wake: self.last_wake.clone(),
        }
    }

    /// Forget everything (new session, or the session ended).
    pub fn reset(&mut self) {
        *self = DormantTracker::default();
    }

    /// Feed one sample; returns the transitions it caused, in order.
    pub fn sample(&mut self, s: &DormantSample) -> Vec<DormancyEvent> {
        let mut ev = Vec::new();
        if !s.live {
            self.reset();
            return ev;
        }
        let (Some(c), Some(r)) = (s.committed_gib, s.resident_gib) else {
            self.missing += 1;
            if self.missing >= MISSING_RESET {
                self.reset();
            }
            return ev;
        };
        self.missing = 0;
        let prev_at = self.prev_at.replace(s.at).unwrap_or(s.at);
        let c = c.max(0.0);
        let r = r.max(0.0);
        let vram = (c - s.shared_gib.max(0.0)).max(0.0);
        let paged = (vram - r).max(0.0);
        let d3 = s.power == Some(PowerState::D3);
        let healthy = r >= EXIT_FRAC * vram && !d3;

        if let Some(cur) = self.current.as_mut() {
            cur.committed_gib = c;
            cur.vram_gib = vram;
            cur.resident_gib = r;
            cur.paged_out_gib = paged;
            cur.power = s.power;
            let since = cur.since;
            let episode = cur.episode;
            if let Some(w) = self.waking.as_mut().filter(|w| w.episode == episode) {
                if w.start.is_none() {
                    if let Some(b) = s.busy_since {
                        w.start = Some(if b >= since { b } else { s.at });
                    } else if r > w.floor + RISE_GIB {
                        w.start = Some(prev_at);
                    }
                }
                w.floor = w.floor.min(r);
            }
            if healthy {
                self.current = None;
                if let Some(w) = self.waking.as_mut() {
                    w.cleared_at = Some(s.at);
                }
                ev.push(DormancyEvent::Cleared { episode, at: s.at });
            }
        } else {
            if healthy {
                self.awake_resident = Some(r);
            }
            let hit = c >= MIN_COMMITTED_GIB && (d3 || paged >= ENTER_MIN_GIB.max(ENTER_FRAC * c));
            if hit {
                self.streak += 1;
                self.first_hit.get_or_insert(s.at);
            } else {
                self.streak = 0;
                self.first_hit = None;
            }
            if self.streak >= ENTER_SAMPLES {
                // A wake still being measured ends where it last rose.
                if let Some(w) = self.waking.take() {
                    ev.push(DormancyEvent::Woke(self.finish_wake(&w, w.last_rise_at, w.last_r, false)));
                }
                let since = self.first_hit.take().unwrap_or(s.at);
                self.streak = 0;
                self.episode += 1;
                let entry = EntryRecord { episode: self.episode, at: since, paged_out_gib: round_to(paged, 3), power: s.power };
                self.current = Some(DormantNow {
                    episode: self.episode,
                    since,
                    committed_gib: c,
                    vram_gib: vram,
                    resident_gib: r,
                    paged_out_gib: paged,
                    power: s.power,
                });
                let target = self.awake_resident.filter(|a| *a > 0.0).unwrap_or(vram);
                self.waking = Some(Waking {
                    episode: self.episode,
                    start: None,
                    floor: r,
                    target,
                    last_r: r,
                    last_rise_at: s.at,
                    cleared_at: None,
                });
                self.last_entry = Some(entry.clone());
                ev.push(DormancyEvent::Entered(entry));
                return ev;
            }
        }

        // Wake measurement (while restoring, and after clearing until resident is back).
        if let Some(mut w) = self.waking.take() {
            if r > w.last_r + STEP_GIB {
                w.last_rise_at = s.at;
            }
            w.last_r = r;
            let mut done = None;
            // Only once the dormant state cleared: the "woke up" line never precedes it.
            if w.cleared_at.is_some() {
                if r >= WAKE_DONE_FRAC * w.target {
                    if w.start.is_none() {
                        w.start = Some(prev_at);
                    }
                    done = Some((s.at, true));
                } else if s.at - w.last_rise_at >= PLATEAU_S {
                    done = Some((w.last_rise_at, false));
                }
            }
            match done {
                Some((end, reached)) => ev.push(DormancyEvent::Woke(self.finish_wake(&w, end, r, reached))),
                None => self.waking = Some(w),
            }
        }
        ev
    }

    fn finish_wake(&mut self, w: &Waking, end: f64, resident: f64, reached: bool) -> WakeRecord {
        let start = w.start.or(w.cleared_at).unwrap_or(end).min(end);
        let rec = WakeRecord {
            episode: w.episode,
            start,
            end,
            seconds: round_to((end - start).max(0.0), 1),
            resident_gib: round_to(resident, 3),
            reached_target: reached,
        };
        self.last_wake = Some(rec.clone());
        rec
    }
}
