//! sd-server (stable-diffusion.cpp) log parser: load steps, image jobs, sampling bars, faults and
//! the starter's exit line. sd-server lines carry no timestamps, so times are arrival times.
//! Level tags are unreliable (the Krea build prints the previous message's tag), so lines are
//! classified by message text only.
//!
//! Two log dialects: the older Krea build (`stable-diffusion.cpp:NNN - ...`) and master-929 with the
//! KLIF patches (`diffusion_engine.cpp` / `image.cpp`, `[INFO   ]` tags, eager load, `preprocess ref[N]`
//! before an edit job, `default LoRA: name:1.00 (...)` at startup and `default LoRA applied: name:1.00`
//! before every job the server's `--default-lora` applies to).
//!
//! Video (sd-server with `vid_gen`, e.g. MiniMax H3): `generate_video WxHxT` starts a job (T = frames), the
//! carriage-return-rewritten sampling bar moves it (the tailer splits CR / LF and drops `ESC[K`), `generate_video
//! completed in X.XXs` ends it. Video jobs are `ImageJob`s with `frames` set.

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
// master-929: reference image preprocessing, logged just before `generate_image WxH` of an edit job.
re!(D17, r"^image_preprocess\.cpp:\d+\s+- preprocess ref\[\d+\]");
// KLIF's sd-server patch: the startup default, then one line before every job it is applied to.
re!(D18, r"^\w+\.cpp:\d+\s+- default LoRA: (.+?):(-?\d+(?:\.\d+)?) \(");
re!(D19, r"^\w+\.cpp:\d+\s+- default LoRA applied: (.+?)\s*$");
// Startup validation (`common.cpp: - error: unknown argument: --x`, `--default-lora '..' not found ...`):
// every one of them ends the process before it listens.
re!(D21, r"^\w+\.cpp:\d+\s+- error: (.+?)\s*$");
re!(D22, r"^main\.cpp:\d+\s+- new_sd_ctx_t failed");
// Raw lines (no tag).
re!(D20, r"^sd-server (\S+) zakonczyl dzialanie z kodem (-?\d+)\.\s*$");
// The same starter exit line in English ("sd-server exited with code 1.", "sd-server HIP929 exited with code -1").
re!(D20B, r"(?i)^sd-server(?: (\S+))? (?:exited|ended|stopped) with (?:exit )?code (-?\d+)\.?\s*$");
re!(D26, r"^\s*\|([=>]+)\s*\|\s+(\d+)/(\d+) - ([\d.]+)(s/it|it/s)\s*$");
// Video jobs (src/pipeline/video.cpp; any source-file prefix): start, sampling done, end.
re!(V01, r"^(?:\w+\.cpp:\d+\s+-\s+)?generate_video (\d+)x(\d+)x(\d+)\s*$");
re!(V02, r"^(?:\w+\.cpp:\d+\s+-\s+)?generate_video completed in ([\d.]+)s\s*$");
re!(V03, r"^(?:\w+\.cpp:\d+\s+-\s+)?sampling(?:\([^)]*\))? completed, taking ([\d.]+)s\s*$");
re!(E01, r"^ggml_cuda_init: found (\d+) ROCm devices");
re!(E02, r"^\s+Device (\d+): (.+?), (gfx\w+)(?::\S+)? \((0x[0-9a-f]+)\), VMM: (\w+), Wave Size: (\d+), VRAM: (\d+) MiB");
re!(E04, r"ROCm error: (.+)$");
re!(E05, r"^(.+ggml-cuda\.cu):(\d+): ROCm error\s*$");
re!(E06, r"ggml_cuda_compute_forward: (\w+) failed$");
re!(E07, r"^ggml_cuda_init: failed to initialize ROCm: (.+?)\s*$");
// The compute backend (records): a CUDA build's device table, Vulkan's, and sd.cpp's own "Using X backend" (a HIP
// build says CUDA there, so the ROCm device table wins).
re!(E08, r"^ggml_cuda_init: found \d+ CUDA devices");
re!(E09, r"^ggml_vulkan: Found \d+ Vulkan devices");
re!(E10, r"- Using (CUDA|ROCm|HIP|Vulkan|Metal|SYCL|OpenCL|CPU) backend\b");
// The starters' own refusals before the server runs ("ERROR: Brak C:\...\x.dll", "ERROR: port 1234 jest zajety ...").
re!(S01, r"^ERROR: Brak (.+?)\s*$");
re!(S02, r"^ERROR: port (\d+) jest zaj[eę]ty");
re!(S03, r"^ERROR: (.+?)\s*$");
re!(NATIVE_ERR, r"^[\w.-]+\.exe : ");
// An absolute Windows path (up to a quote or the end of the line); its last component is the file name.
re!(WIN_PATH, r#"[A-Za-z]:\\(?:[^\\'"\r\n]+\\)*([^\\'"\r\n]*)"#);
// Starter banners: "GPU: <name> (ROCm<N> / gfx<arch>)" and the fast Krea starter's
// "GPU: <name> (HIP_VISIBLE_DEVICES=<N> -> ROCm0 / gfx<arch>), Flash Attention: ON".
re!(GPU_BANNER, r"^GPU: (.+?) \((?:[^()]*?)(\S+) / (gfx\w+)\)");

#[derive(Debug, Clone)]
struct Job {
    start_t: f64,
    width: u32,
    height: u32,
    edit: bool,
    /// The server's default LoRA was applied to this job (`default LoRA applied: ...` before it).
    default_lora: bool,
    /// Frames of a video job (`generate_video WxHxT`).
    frames: Option<u32>,
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
    /// The GPU the server runs on as it names it (starter banner, or the only ROCm device), for the device check.
    pub device_name: Option<String>,
    job: Option<Job>,
    last_size: (u32, u32, bool),
    last_frames: Option<u32>,
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
    /// The server's `--default-lora` (name and weight) from its startup line.
    pub default_lora: Option<(String, f64)>,
    /// Finished jobs the default LoRA was applied to.
    pub default_lora_jobs: u64,
    /// ROCm devices the server sees (`ggml_cuda_init: found N ROCm devices`).
    rocm_devices: Option<u32>,
    /// The compute backend another line named (CUDA / Vulkan device tables, "Using X backend").
    backend: Option<String>,
    /// A `preprocess ref[N]` / `default LoRA applied` line arrived for the job that starts next.
    pending_ref: bool,
    pending_lora: bool,
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
            device_name: None,
            job: None,
            last_size: (0, 0, false),
            last_frames: None,
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
            default_lora: None,
            default_lora_jobs: 0,
            rocm_devices: None,
            backend: None,
            pending_ref: false,
            pending_lora: false,
        }
    }

    /// The compute backend: "HIP" when the server listed ROCm devices, else what a CUDA / Vulkan device table or
    /// sd.cpp's "Using X backend" line said.
    pub fn compute_backend(&self) -> Option<String> {
        if self.rocm_devices.is_some() {
            return Some("HIP".into());
        }
        self.backend.clone()
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
        // A PowerShell starter that runs the server directly may get its first stderr line wrapped as
        // `sd-server.exe : <line>` (Windows PowerShell 5.1 NativeCommandError); classify the line itself.
        let msg = match NATIVE_ERR.find(msg) {
            Some(m) => strip_sd_tag(&msg[m.end()..]),
            None => msg,
        };

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
        let start = D07
            .captures(msg)
            .map(|c| (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0), None))
            .or_else(|| V01.captures(msg).map(|c| (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0), c[3].parse::<u32>().ok())));
        if let Some((w, h, frames)) = start {
            if self.job.is_some() {
                self.aborted_jobs += 1;
            }
            // master-929 logs the reference preprocessing (and KLIF's default-LoRA line) before the job line.
            let edit = std::mem::take(&mut self.pending_ref);
            let default_lora = std::mem::take(&mut self.pending_lora);
            self.job = Some(Job {
                start_t: at,
                width: w,
                height: h,
                edit,
                default_lora,
                frames,
                step: 0,
                steps: self.last_steps,
                s_per_it: 0.0,
                sampling: false,
                decoding: false,
            });
            self.last_size = (w, h, edit);
            self.last_frames = frames;
            return;
        }
        if D17.is_match(msg) {
            match self.job.as_mut() {
                Some(j) => j.edit = true,
                None => self.pending_ref = true,
            }
            return;
        }
        if D19.is_match(msg) {
            match self.job.as_mut() {
                Some(j) => j.default_lora = true,
                None => self.pending_lora = true,
            }
            return;
        }
        if let Some(c) = D18.captures(msg) {
            self.default_lora = Some((c[1].trim().to_string(), c[2].parse().unwrap_or(1.0)));
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
        if V03.is_match(msg) {
            if let Some(j) = self.job.as_mut().filter(|j| j.frames.is_some()) {
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
        if let Some(c) = D16.captures(msg).or_else(|| V02.captures(msg)) {
            let secs: f64 = c[1].parse().unwrap_or(0.0);
            if let Some(j) = self.job.take() {
                self.recent.push_back(ImageJob {
                    at: at.round(),
                    seconds: round_to(secs, 2),
                    width: j.width,
                    height: j.height,
                    edit: j.edit,
                    steps: (j.steps > 0).then_some(j.steps),
                    frames: j.frames,
                });
                while self.recent.len() > 24 {
                    self.recent.pop_front();
                }
                self.last_size = (j.width, j.height, j.edit);
                self.last_frames = j.frames;
                if j.default_lora {
                    self.default_lora_jobs += 1;
                }
            }
            self.pending_ref = false;
            self.pending_lora = false;
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
            self.device_name = Some(c[1].trim().to_string());
            self.steps.detail[1] = self.device_detail.clone();
            self.steps.reach(1);
            return;
        }
        if E08.is_match(msg) {
            self.backend = Some("CUDA".into());
            return;
        }
        if E09.is_match(msg) {
            self.backend = Some("Vulkan".into());
            return;
        }
        if let Some(c) = E10.captures(msg) {
            if self.backend.is_none() {
                self.backend = Some(crate::llama::backend_of_device(&c[1]));
            }
            return;
        }
        if let Some(c) = E01.captures(msg) {
            // Without the starter banner, only the device count is known (the selected device is in
            // the launch arguments, not in this table).
            let n: u32 = c[1].parse().unwrap_or(0);
            self.rocm_devices = Some(n);
            if self.device_detail.is_none() {
                self.steps.detail[1] = Some(if n == 1 { "1 ROCm device".to_string() } else { format!("{n} ROCm devices") });
            }
            self.steps.reach(1);
            return;
        }
        if let Some(c) = E02.captures(msg) {
            // Exactly one visible device (HIP_VISIBLE_DEVICES with the gfx1201-only build): that one is it.
            if self.device_name.is_none() && self.rocm_devices == Some(1) {
                self.device_name = Some(c[2].trim().to_string());
            }
            if self.device_detail.is_none() && self.rocm_devices == Some(1) {
                let name = c[2].trim().trim_start_matches("AMD Radeon ").trim();
                self.steps.detail[1] = Some(format!("{name} · ROCm{} · {}", &c[1], &c[3]));
            }
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
        if let Some(c) = D20B.captures(msg) {
            let code: i64 = c[2].parse().unwrap_or(0);
            self.starter_exit = Some(code);
            self.push_error(line);
            if code != 0 {
                let hex = format!("0x{:08X}", code as i32 as u32);
                self.set_fatal(format!("sd-server exited with code {code} ({hex})"));
            }
            return;
        }
        if let Some(c) = E07.captures(msg) {
            self.push_error(line);
            if self.listening.is_none() {
                self.set_fatal(format!("ROCm could not start: {}", c[1].trim()));
            }
            return;
        }
        if D22.is_match(msg) {
            self.push_error(line);
            if self.listening.is_none() {
                self.set_fatal("sd-server could not load the models".into());
            }
            return;
        }
        if let Some(c) = D21.captures(msg) {
            self.push_error(line);
            if self.listening.is_none() {
                self.set_fatal(format!("sd-server refused to start: {}", short_paths(&c[1])));
            }
            return;
        }
        if self.listening.is_none() {
            let starter = if let Some(c) = S01.captures(msg) {
                Some(format!("the starter is missing {}", short_paths(&c[1])))
            } else if let Some(c) = S02.captures(msg) {
                Some(format!("the starter found port {} already in use", &c[1]))
            } else {
                S03.captures(msg).map(|c| format!("the starter stopped: {}", short_paths(&c[1])))
            };
            if let Some(hint) = starter {
                self.push_error(line);
                self.set_fatal(hint);
                return;
            }
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
                frames: j.frames,
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
                frames: self.last_frames,
            },
        }
    }

    /// Median seconds per finished image (0 when none).
    pub fn median_seconds(&self) -> f64 {
        median(&self.job_seconds)
    }
}

/// A startup error with every absolute Windows path cut to its last component (`'D:\dir\loras'` becomes
/// `'loras'`, `D:\dir\x.dll` becomes `x.dll`): no machine path in a UI sentence.
fn short_paths(msg: &str) -> String {
    WIN_PATH.replace_all(msg, |c: &regex::Captures| c[1].to_string()).into_owned()
}

pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut a = v.to_vec();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    a[a.len() / 2]
}
