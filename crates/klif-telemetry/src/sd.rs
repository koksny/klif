//! sd-server (stable-diffusion.cpp) log parser: load steps, image jobs, sampling bars, faults and
//! the starter's exit line. sd-server lines carry no timestamps, so times are arrival times.
//! Level tags are unreliable (the Krea build prints the previous message's tag), so lines are
//! classified by message text only.

use crate::llama::Steps;
use crate::text::{redact, round_to, strip_sd_tag};
use klif_common::vm::{ImageActivity, ImageJob, ImageLive};
use regex::Regex;
use std::collections::VecDeque;
use std::sync::LazyLock;

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).expect($pat));
    };
}

// Message part (after the optional `[LEVEL]` tag).
re!(D01, r"^(?:stable-diffusion|diffusion_engine)\.cpp:\d+\s+- loading (diffusion model|llm vision|llm|vae|tae|clip_l|clip_g|t5xxl|high noise diffusion model|model) from '(.+)'$");
re!(D03, r"^(?:stable-diffusion|diffusion_engine)\.cpp:\d+\s+- Version: (.+?)\s*$");
re!(D04, r"^(?:stable-diffusion|diffusion_engine)\.cpp:\d+\s+- total params memory size = ([\d.]+)MB \(VRAM ([\d.]+)MB, RAM ([\d.]+)MB\)");
re!(D05, r"^(?:stable-diffusion|diffusion_engine)\.cpp:\d+\s+- running in (.+?) mode\s*$");
re!(D06, r"^main\.cpp:\d+\s+- listening on: (http://\S+)$");
re!(D07, r"^(?:stable-diffusion|image)\.cpp:\d+\s+- generate_image (\d+)x(\d+)$");
re!(D08, r"^(?:stable-diffusion|diffusion_engine|image)\.cpp:\d+\s+- EDIT mode$");
re!(D13, r"^(?:stable-diffusion|image)\.cpp:\d+\s+- generating image: (\d+)/(\d+) - seed (-?\d+)$");
re!(D14, r"^(?:stable-diffusion|image)\.cpp:\d+\s+- sampling completed, taking ([\d.]+)s$");
re!(D15, r"^(?:stable-diffusion|image)\.cpp:\d+\s+- decoding (\d+)(?:/\d+)? latents$");
re!(D16, r"^(?:stable-diffusion|image)\.cpp:\d+\s+- generate_image completed in ([\d.]+)s$");
// Raw lines (no tag).
re!(D20, r"^sd-server (\S+) zakonczyl dzialanie z kodem (-?\d+)\.\s*$");
re!(D26, r"^\s*\|([=>]+)\s*\|\s+(\d+)/(\d+) - ([\d.]+)(s/it|it/s)\s*$");
re!(E01, r"^ggml_cuda_init: found (\d+) ROCm devices");
re!(E02, r"^\s+Device (\d+): (.+?), (gfx\w+)(?::\S+)? \((0x[0-9a-f]+)\), VMM: (\w+), Wave Size: (\d+), VRAM: (\d+) MiB");
re!(E04, r"ROCm error: (.+)$");
re!(E05, r"^(.+ggml-cuda\.cu):(\d+): ROCm error\s*$");
re!(E06, r"ggml_cuda_compute_forward: (\w+) failed$");
re!(GPU_BANNER, r"^GPU: (.+?) \((\S+) / (gfx\w+)\)\s*$");

#[derive(Debug, Clone)]
struct Job {
    start_t: f64,
    width: u32,
    height: u32,
    edit: bool,
    step: u32,
    steps: u32,
    s_per_it: f64,
    sampling: bool,
    decoding: bool,
}

#[derive(Debug)]
pub struct SdParser {
    pub steps: Steps,
    pub any_line: bool,
    pub listening: Option<String>,
    pub device_detail: Option<String>,
    job: Option<Job>,
    last_size: (u32, u32, bool),
    last_steps: u32,
    pub recent: VecDeque<ImageJob>,
    pub images: u64,
    pub job_seconds: Vec<f64>,
    pub fatal_hint: Option<String>,
    pub starter_exit: Option<i64>,
    pub error_tail: VecDeque<String>,
    failed_op: Option<String>,
    pub aborted_jobs: u32,
    /// RAM-resident params (MB) from D04, for information.
    pub params_ram_mb: Option<f64>,
}

impl Default for SdParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SdParser {
    pub fn new() -> Self {
        Self {
            steps: Steps::default(),
            any_line: false,
            listening: None,
            device_detail: None,
            job: None,
            last_size: (0, 0, false),
            last_steps: 0,
            recent: VecDeque::new(),
            images: 0,
            job_seconds: Vec::new(),
            fatal_hint: None,
            starter_exit: None,
            error_tail: VecDeque::new(),
            failed_op: None,
            aborted_jobs: 0,
            params_ram_mb: None,
        }
    }

    fn push_error(&mut self, line: &str) {
        self.error_tail.push_back(redact(line.trim_end().to_string()));
        while self.error_tail.len() > 30 {
            self.error_tail.pop_front();
        }
    }

    fn set_fatal(&mut self, hint: String) {
        if self.fatal_hint.is_none() {
            self.fatal_hint = Some(hint);
        }
        if self.listening.is_none() {
            self.steps.fail_active();
        }
        if self.job.take().is_some() {
            self.aborted_jobs += 1;
        }
    }

    fn first_line(&mut self) {
        if !self.any_line {
            self.any_line = true;
            self.steps.finish(0);
            self.steps.reach(1);
        }
    }

    /// One segment from stdout or stderr (bars arrive as separate segments).
    pub fn feed(&mut self, raw: &str, at: f64, from_err: bool) {
        let line = raw.trim_end();
        if line.trim().is_empty() || crate::text::is_tag_only(line) {
            return;
        }
        self.first_line();
        let msg = strip_sd_tag(line);

        // Sampling bar (`  |=====>   | 3/8 - 2.50s/it`); tensor-upload bars use `#` and are ignored.
        if let Some(c) = D26.captures(msg) {
            if let Some(j) = self.job.as_mut() {
                if !j.decoding {
                    j.sampling = true;
                    j.step = c[2].parse().unwrap_or(0);
                    j.steps = c[3].parse().unwrap_or(0);
                    let v: f64 = c[4].parse().unwrap_or(0.0);
                    j.s_per_it = if &c[5] == "it/s" && v > 0.0 { 1.0 / v } else { v };
                    self.last_steps = j.steps;
                }
            }
            return;
        }
        if let Some(c) = D07.captures(msg) {
            if self.job.is_some() {
                self.aborted_jobs += 1;
            }
            let (w, h) = (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
            self.job = Some(Job {
                start_t: at,
                width: w,
                height: h,
                edit: false,
                step: 0,
                steps: self.last_steps,
                s_per_it: 0.0,
                sampling: false,
                decoding: false,
            });
            self.last_size = (w, h, false);
            return;
        }
        if D08.is_match(msg) {
            if let Some(j) = self.job.as_mut() {
                j.edit = true;
                self.last_size.2 = true;
            }
            return;
        }
        if D13.is_match(msg) {
            if let Some(j) = self.job.as_mut() {
                j.sampling = true;
            }
            return;
        }
        if D14.is_match(msg) {
            if let Some(j) = self.job.as_mut() {
                j.step = j.steps;
            }
            return;
        }
        if D15.is_match(msg) {
            if let Some(j) = self.job.as_mut() {
                j.decoding = true;
            }
            return;
        }
        if let Some(c) = D16.captures(msg) {
            let secs: f64 = c[1].parse().unwrap_or(0.0);
            if let Some(j) = self.job.take() {
                self.recent.push_back(ImageJob {
                    at: at.round(),
                    seconds: round_to(secs, 2),
                    width: j.width,
                    height: j.height,
                    edit: j.edit,
                });
                while self.recent.len() > 24 {
                    self.recent.pop_front();
                }
                self.last_size = (j.width, j.height, j.edit);
            }
            self.images += 1;
            self.job_seconds.push(secs);
            return;
        }
        // ---- load phase ----
        if let Some(c) = D01.captures(msg) {
            match &c[1] {
                "diffusion model" | "model" | "high noise diffusion model" => self.steps.reach(2),
                _ => self.steps.reach(3),
            }
            return;
        }
        if D03.is_match(msg) || D05.is_match(msg) {
            self.steps.reach(4);
            return;
        }
        if let Some(c) = D04.captures(msg) {
            self.params_ram_mb = c[3].parse().ok();
            self.steps.reach(4);
            return;
        }
        if let Some(c) = D06.captures(msg) {
            self.listening = Some(c[1].to_string());
            self.steps.finish(4);
            self.steps.reach(5);
            return;
        }
        if let Some(c) = GPU_BANNER.captures(msg) {
            self.device_detail = Some(format!("{} · {}", &c[2], &c[3]));
            self.steps.detail[1] = self.device_detail.clone();
            self.steps.reach(1);
            return;
        }
        if let Some(c) = E01.captures(msg) {
            // Without the starter banner, only the device count is known (the selected device is in
            // the launch arguments, not in this table).
            if self.device_detail.is_none() {
                self.steps.detail[1] = Some(format!("{} ROCm devices", &c[1]));
            }
            self.steps.reach(1);
            return;
        }
        if E02.is_match(msg) {
            return;
        }
        // ---- faults ----
        if let Some(c) = D20.captures(msg) {
            let code: i64 = c[2].parse().unwrap_or(0);
            self.starter_exit = Some(code);
            self.push_error(line);
            if code != 0 {
                let hex = format!("0x{:08X}", code as i32 as u32);
                self.set_fatal(format!("sd-server exited with code {code} ({hex})"));
            }
            return;
        }
        if let Some(c) = E06.captures(msg) {
            self.failed_op = Some(c[1].to_string());
            self.push_error(line);
            return;
        }
        if let Some(c) = E04.captures(msg) {
            self.push_error(line);
            let op = self.failed_op.as_ref().map(|o| format!(" (in {o})")).unwrap_or_default();
            self.set_fatal(format!("ROCm error: {}{op}", c[1].trim()));
            return;
        }
        if E05.is_match(msg) {
            self.push_error(line);
            self.set_fatal("ROCm error: sd-server aborted".into());
            return;
        }
        if line.trim_start().starts_with("[ERROR") || (from_err && msg.to_ascii_lowercase().contains("error")) {
            self.push_error(line);
        }
    }

    pub fn generating(&self) -> bool {
        self.job.is_some()
    }

    /// Start of the job in flight (epoch s), if one is.
    pub fn job_start(&self) -> Option<f64> {
        self.job.as_ref().map(|j| j.start_t)
    }

    pub fn live(&self, now: f64) -> ImageLive {
        match &self.job {
            Some(j) => ImageLive {
                activity: ImageActivity::Generating,
                step: j.step,
                steps: j.steps,
                s_per_it: round_to(j.s_per_it, 2),
                elapsed_s: round_to((now - j.start_t).max(0.0), 1),
                width: j.width,
                height: j.height,
                edit: j.edit,
                recent: self.recent.iter().cloned().collect(),
                images_this_session: self.images,
            },
            None => ImageLive {
                activity: ImageActivity::Idle,
                step: 0,
                steps: self.last_steps,
                s_per_it: 0.0,
                elapsed_s: 0.0,
                width: self.last_size.0,
                height: self.last_size.1,
                edit: self.last_size.2,
                recent: self.recent.iter().cloned().collect(),
                images_this_session: self.images,
            },
        }
    }

    /// Median seconds per finished image (0 when none).
    pub fn median_seconds(&self) -> f64 {
        median(&self.job_seconds)
    }
}

pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut a = v.to_vec();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    a[a.len() / 2]
}
