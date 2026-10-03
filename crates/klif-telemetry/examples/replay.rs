//! Replay real session logs through the tailer + parsers, on a virtual clock.
//!
//!   cargo run -p klif-telemetry --example replay -- <dir>/<stem> [--kind llm|image] [--err-only <file>]
//!
//! `<stem>` names a pair `<stem>.out.log` / `<stem>.err.log` (COPIES of real logs; never point this at
//! a live runtime-logs folder). The harness creates empty logs under `<dir>/run-<name>/` and appends
//! the original bytes chunk by chunk as virtual time passes (llama lines at their own timestamps;
//! sd-server pieces split before every `\r` and after every `\n`, timed by the durations they print),
//! polling a `SessionTracker` at 2 Hz like the live sampler. It prints what KLIF derives: load steps,
//! VRAM layers from the log, LlmLive / ImageLive at key moments, records, totals, faults.
//! No server is started; no HTTP probe runs (health comes from log markers).

use klif_common::vm::{LlmActivity, LoadStep, StepState, SystemKind};
use klif_telemetry::session::SessionTracker;
use klif_telemetry::text::{strip_sd_tag, REDACTED};
use klif_telemetry::{llama, WatchSpec};
use regex::Regex;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Llm,
    Image,
}

struct Chunk {
    t: f64,
    err: bool,
    bytes: Vec<u8>,
}

fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").unwrap();
    re.replace_all(s, "").to_string()
}

/// Split after `\n` and before `\r` (how the process actually writes bars and lines).
fn pieces(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\r' && !cur.is_empty() && bytes.get(i + 1) != Some(&b'\n') {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(b);
        if b == b'\n' {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn llm_timeline(out: &[u8], err: &[u8]) -> Vec<Chunk> {
    let mut v = Vec::new();
    // Starter banner first; the server process starts about a second later.
    for p in pieces(out) {
        v.push(Chunk { t: 0.0, err: false, bytes: p });
    }
    let mut t = 1.0;
    for p in pieces(err) {
        let text = strip_ansi(&String::from_utf8_lossy(&p));
        if let Some((ts, _, _)) = llama::split_ts(text.trim_start()) {
            t = 1.0 + ts;
        }
        v.push(Chunk { t, err: true, bytes: p });
    }
    v.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    v
}

fn sd_timeline(out: &[u8], err: &[u8]) -> Vec<Chunk> {
    let bar = Regex::new(r"\|[=>]+\s*\|\s+(\d+)/(\d+) - ([\d.]+)(s/it|it/s)").unwrap();
    let taking = Regex::new(r"- (apply_loras|encode_first_stage|get_learned_condition|decode_first_stage) completed, taking ([\d.]+)s|loading tensors completed, taking ([\d.]+)s").unwrap();
    let gen_start = Regex::new(r"- generate_image \d+x\d+$").unwrap();
    let gen_done = Regex::new(r"- generate_image completed in ([\d.]+)s").unwrap();
    let exit = Regex::new(r"^sd-server \S+ zakonczyl").unwrap();
    let mut v = Vec::new();
    let mut t = 0.0f64;
    let mut job_start = 0.0f64;
    let mut jobs = 0;
    let mut exit_t = None;
    for p in pieces(out) {
        let text = strip_ansi(&String::from_utf8_lossy(&p));
        let msg = strip_sd_tag(text.trim_matches(|c| c == '\r' || c == '\n')).trim().to_string();
        if gen_start.is_match(&msg) {
            t += if jobs == 0 { 3.0 } else { 20.0 };
            jobs += 1;
            job_start = t;
        } else if let Some(c) = bar.captures(&msg) {
            let x: f64 = c[3].parse().unwrap_or(0.0);
            t += if &c[4] == "it/s" && x > 0.0 { 1.0 / x } else { x };
        } else if let Some(c) = taking.captures(&msg) {
            let x: f64 = c.get(2).or(c.get(3)).and_then(|m| m.as_str().parse().ok()).unwrap_or(0.0);
            t += x;
        } else if let Some(c) = gen_done.captures(&msg) {
            let x: f64 = c[1].parse().unwrap_or(0.0);
            t = t.max(job_start + x);
        } else if exit.is_match(&msg) {
            t += 0.5;
            exit_t = Some(t);
        } else {
            t += 0.01;
        }
        v.push(Chunk { t, err: false, bytes: p });
    }
    let end = t;
    for p in pieces(err) {
        let text = String::from_utf8_lossy(&p).to_string();
        let early = text.starts_with("ggml_cuda_init") || text.trim_start().starts_with("Device ");
        let at = if early { 0.3 } else { exit_t.unwrap_or(end) - 0.05 };
        v.push(Chunk { t: at, err: true, bytes: p });
    }
    v.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    v
}

/// "klif-20260923-041019-313-..." / "flashnext-20260830-160824" -> epoch seconds (as if UTC).
fn epoch_from_stem(stem: &str) -> f64 {
    let re = Regex::new(r"(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})").unwrap();
    let Some(c) = re.captures(stem) else { return 1_790_000_000.0 };
    let n = |i: usize| c[i].parse::<i64>().unwrap();
    let (y, m, d) = (n(1), n(2), n(3));
    // days from civil (Howard Hinnant)
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days * 86400 + n(4) * 3600 + n(5) * 60 + n(6)) as f64
}

fn steps_str(steps: &[LoadStep]) -> String {
    steps
        .iter()
        .map(|s| {
            let st = match s.state {
                StepState::Done => "done",
                StepState::Active => "ACTIVE",
                StepState::Pending => "pending",
                StepState::Failed => "FAILED",
            };
            match &s.detail {
                Some(d) => format!("{:?}={st} [{d}]", s.id),
                None => format!("{:?}={st}", s.id),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
        .to_lowercase()
        .replace("active", "ACTIVE")
        .replace("failed", "FAILED")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut base: Option<PathBuf> = None;
    let mut kind: Option<Kind> = None;
    let mut err_only: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--kind" => {
                kind = match args.next().as_deref() {
                    Some("image") => Some(Kind::Image),
                    _ => Some(Kind::Llm),
                }
            }
            "--err-only" => err_only = args.next().map(PathBuf::from),
            _ => base = Some(PathBuf::from(a)),
        }
    }
    let (out_src, err_src, name) = match (&err_only, &base) {
        (Some(e), _) => (None, e.clone(), e.file_stem().unwrap().to_string_lossy().to_string()),
        (None, Some(b)) => {
            let stem = b.file_name().unwrap().to_string_lossy().to_string();
            let dir = b.parent().unwrap_or(Path::new("."));
            (Some(dir.join(format!("{stem}.out.log"))), dir.join(format!("{stem}.err.log")), stem)
        }
        _ => {
            eprintln!("usage: replay <dir>/<stem> [--kind llm|image] | --err-only <file>");
            std::process::exit(2);
        }
    };
    let out_bytes = out_src.as_ref().map(|p| std::fs::read(p).expect("read out log")).unwrap_or_default();
    let err_bytes = std::fs::read(&err_src).expect("read err log");
    let kind = kind.unwrap_or_else(|| {
        let o = String::from_utf8_lossy(&out_bytes);
        if o.contains("stable-diffusion.cpp") || String::from_utf8_lossy(&err_bytes).starts_with("ggml_cuda_init") {
            Kind::Image
        } else {
            Kind::Llm
        }
    });
    let timeline = match kind {
        Kind::Llm => llm_timeline(&out_bytes, &err_bytes),
        Kind::Image => sd_timeline(&out_bytes, &err_bytes),
    };

    // Fresh, empty run logs the tracker tails while we append.
    let run_dir = err_src.parent().unwrap().join(format!("run-{name}"));
    std::fs::create_dir_all(&run_dir).unwrap();
    let out_path = run_dir.join("session.out.log");
    let err_path = run_dir.join("session.err.log");
    let mut out_f = File::create(&out_path).unwrap();
    let mut err_f = File::create(&err_path).unwrap();
    let b = epoch_from_stem(&name);
    let spec = WatchSpec {
        kind: if kind == Kind::Llm { SystemKind::Llm } else { SystemKind::Image },
        adapter: if kind == Kind::Llm { klif_common::vm::AdapterId::LlamaCpp } else { klif_common::vm::AdapterId::SdCpp },
        external: false,
        out_log: Some(out_path.clone()),
        err_log: Some(err_path.clone()),
        host: "127.0.0.1".into(),
        port: if kind == Kind::Llm { 7030 } else { 1234 },
        api_key: None,
        started_at: b,
        ctx_tokens: None,
        spec_mode: None,
        health: klif_common::vm::HealthCheck::Auto,
        metrics: false,
        expect_device: None,
        gpu: None,
    };
    let mut tr = SessionTracker::new(spec, false);
    tr.set_probes_enabled(false);

    println!("== replay {name} ({kind:?}), {} chunks, {:.0} s of virtual time", timeline.len(), timeline.last().map(|c| c.t).unwrap_or(0.0));
    let mut i = 0usize;
    let mut now_v = 0.0f64;
    let mut last_steps = String::new();
    let mut last_layers = String::new();
    let mut key_leaks = 0u32;
    let mut redacted_seen = false;
    let mut req_n = 0u32;
    let mut cur_task: Option<u64> = None;
    let mut cur_detail = false;
    let mut long_shown = 0u32;
    let mut last_print_v = f64::NEG_INFINITY;
    let mut last_act = LlmActivity::Idle;
    let mut printed_records = 0usize;
    let mut jobs_seen = 0u64;
    let mut job_running = false;
    // Step-by-step detail for the first two jobs and the first edit job.
    let mut job_detail = false;
    let mut edit_detailed = false;
    let mut last_bar = (0u32, 0u32);
    let mut prev_fatal: Option<String> = None;
    loop {
        // Append everything due by now.
        while i < timeline.len() && timeline[i].t <= now_v + 1e-9 {
            let c = &timeline[i];
            if c.err { err_f.write_all(&c.bytes).unwrap() } else { out_f.write_all(&c.bytes).unwrap() }
            i += 1;
        }
        let now = b + now_v;
        tr.poll(now);
        let sig = tr.signals(now);

        // Redaction check on every poll.
        let console = tr.console();
        key_leaks += console.iter().filter(|l| l.contains("api_keys:")).count() as u32;
        redacted_seen |= console.iter().any(|l| l == REDACTED);

        let st = steps_str(&sig.load_steps);
        if st != last_steps {
            println!("[t={now_v:8.1}] steps ({:.0}%, health {:?}): {st}", sig.load_fraction * 100.0, sig.health);
            last_steps = st;
        }
        let layers = sig
            .layers_from_logs
            .as_ref()
            .map(|l| l.iter().map(|x| format!("{}={:.3}", x.label, x.gib)).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        if layers != last_layers {
            let sum: f64 = sig.layers_from_logs.as_ref().map(|l| l.iter().map(|x| x.gib).sum()).unwrap_or(0.0);
            println!("[t={now_v:8.1}] layers from log (GiB): {layers}  | sum {sum:.3} GiB = {:.2} MiB", sum * 1024.0);
            last_layers = layers;
        }
        if sig.fatal_hint != prev_fatal {
            println!("[t={now_v:8.1}] FATAL: {:?} (starter exit {:?})", sig.fatal_hint, sig.starter_exit);
            prev_fatal = sig.fatal_hint.clone();
        }

        if let Some(l) = &sig.llm {
            let p = tr.llama().unwrap();
            let task = p.cur.as_ref().map(|r| r.task);
            if let Some(task_id) = task.filter(|_| task != cur_task) {
                req_n += 1;
                cur_task = task;
                cur_detail = req_n <= 3;
                last_print_v = f64::NEG_INFINITY;
                if cur_detail {
                    println!("[t={now_v:8.1}] request #{req_n} task {task_id} starts");
                }
            }
            // Long prefills (the Flash-Next hero state): show a few in detail even later on.
            if !cur_detail && l.activity == LlmActivity::Prefill {
                if let Some(pf) = &l.prefill {
                    if pf.tokens >= 8000 && long_shown < 4 {
                        cur_detail = true;
                        long_shown += 1;
                        println!("[t={now_v:8.1}] request #{req_n} task {:?}: long prefill", task);
                    }
                }
            }
            if cur_detail && l.activity != LlmActivity::Idle && (now_v - last_print_v >= 10.0 || l.activity != last_act) {
                last_print_v = now_v;
                match l.activity {
                    LlmActivity::Prefill => {
                        let pf = l.prefill.as_ref().unwrap();
                        println!(
                            "[t={now_v:8.1}]   prefill {}/{} tok (cached {:?}), {:.1} tok/s, elapsed {:.1}s, ETA {:.1}s | ctx {}/{}",
                            pf.done_tokens, pf.tokens, pf.cached_tokens, pf.tps, pf.elapsed_s, pf.eta_s, l.context.used_tokens, l.context.total_tokens
                        );
                    }
                    LlmActivity::Decode => {
                        println!(
                            "[t={now_v:8.1}]   decode {:.1} tok/s, generated {}, ctx {}/{}, spec {:?}",
                            l.decode_tps,
                            l.generated_tokens,
                            l.context.used_tokens,
                            l.context.total_tokens,
                            l.spec.as_ref().map(|s| (s.mode.clone(), s.acceptance_pct, s.active))
                        );
                    }
                    LlmActivity::Idle => {}
                }
            }
            if l.totals.requests as usize != printed_records {
                if let Some(rec) = l.requests.last() {
                    let n = l.totals.requests as usize;
                    if cur_detail || n.is_multiple_of(25) {
                        let pf = l.prefill.as_ref();
                        println!(
                            "[t={now_v:8.1}]   done #{n}: record {{id {}, prompt {}, cached {}, prefill {:.2}s, gen {}, decode {:.2}s}} | prefill tps {:.1}, decodeTps {:.1}, ctx {}/{}, spec {:?}",
                            rec.id,
                            rec.prompt_tokens,
                            rec.cached_tokens,
                            rec.prefill_s,
                            rec.generated_tokens,
                            rec.decode_s,
                            pf.map(|p| p.tps).unwrap_or(0.0),
                            l.decode_tps,
                            l.context.used_tokens,
                            l.context.total_tokens,
                            l.spec.as_ref().map(|s| (s.mode.clone(), s.acceptance_pct))
                        );
                    }
                    printed_records = n;
                }
            }
            last_act = l.activity;
        }

        if let Some(im) = &sig.image {
            let gen = im.activity == klif_common::vm::ImageActivity::Generating;
            if gen && !job_running {
                jobs_seen += 1;
                job_detail = jobs_seen <= 2 || (im.edit && !edit_detailed);
                edit_detailed |= job_detail && im.edit;
                if job_detail {
                    println!("[t={now_v:8.1}] job #{jobs_seen} starts: {}x{} edit={}", im.width, im.height, im.edit);
                }
            }
            if gen && job_detail && (im.step, im.steps) != last_bar && im.step > 0 {
                println!(
                    "[t={now_v:8.1}]   step {}/{} {:.2} s/it, elapsed {:.1}s, {}x{} edit={}",
                    im.step, im.steps, im.s_per_it, im.elapsed_s, im.width, im.height, im.edit
                );
                last_bar = (im.step, im.steps);
            }
            if !gen && job_running {
                if let Some(j) = im.recent.last() {
                    println!(
                        "[t={now_v:8.1}]   job done: {:.2}s {}x{} edit={} (images this session {})",
                        j.seconds, j.width, j.height, j.edit, im.images_this_session
                    );
                } else {
                    println!("[t={now_v:8.1}]   job ended without a result (aborted)");
                }
                last_bar = (0, 0);
            }
            job_running = gen;
        }

        if i >= timeline.len() {
            // Let derived state settle for a few seconds after the last line.
            if now_v > timeline.last().map(|c| c.t).unwrap_or(0.0) + 3.0 {
                break;
            }
        }
        // Skip idle gaps quickly (never while a request or job is running).
        let busy = sig.llm.as_ref().map(|l| l.activity != LlmActivity::Idle).unwrap_or(false)
            || sig.image.as_ref().map(|im| im.activity == klif_common::vm::ImageActivity::Generating).unwrap_or(false);
        if !busy && i < timeline.len() && timeline[i].t > now_v + 5.0 {
            now_v = timeline[i].t - 0.5;
        }
        now_v += 0.5;
    }

    let now = b + now_v;
    let sig = tr.signals(now);
    println!("== final (t={now_v:.1})");
    println!("health {:?}, load fraction {:.2}", sig.health, sig.load_fraction);
    println!("steps: {}", steps_str(&sig.load_steps));
    if let Some(l) = &sig.llm {
        let nz = l.decode_history.iter().filter(|v| **v > 0.0).count();
        let mx = l.decode_history.iter().cloned().fold(0.0, f64::max);
        println!(
            "llm: activity {:?}, decodeTps {:.1}, generated {}, ctx {}/{}, spec {:?}",
            l.activity,
            l.decode_tps,
            l.generated_tokens,
            l.context.used_tokens,
            l.context.total_tokens,
            l.spec.as_ref().map(|s| (s.mode.clone(), s.acceptance_pct, s.active))
        );
        println!("llm: prefill {:?}", l.prefill);
        println!("llm: decodeHistory {} samples, {} non-zero, max {:.1}", l.decode_history.len(), nz, mx);
        println!(
            "llm: totals requests {}, promptTokens {}, generatedTokens {}",
            l.totals.requests, l.totals.prompt_tokens, l.totals.generated_tokens
        );
        println!("llm: median decode tps over requests {:?}", tr.median_decode_tps().map(|v| (v * 100.0).round() / 100.0));
        println!("llm: last {} records (oldest first):", l.requests.len());
        for r in &l.requests {
            println!(
                "   id {:>6} prompt {:>6} cached {:>6} prefill {:>7.2}s gen {:>5} decode {:>7.2}s",
                r.id, r.prompt_tokens, r.cached_tokens, r.prefill_s, r.generated_tokens, r.decode_s
            );
        }
    }
    if let Some(im) = &sig.image {
        let sd = tr.sd().unwrap();
        println!(
            "image: activity {:?}, images {}, median {:.2}s, last size {}x{} edit {}, steps {}, aborted jobs {}",
            im.activity,
            im.images_this_session,
            sd.median_seconds(),
            im.width,
            im.height,
            im.edit,
            im.steps,
            sd.aborted_jobs
        );
        for j in &im.recent {
            println!("   job {:>6.2}s {}x{} edit={}", j.seconds, j.width, j.height, j.edit);
        }
        println!(
            "image: default LoRA {:?}, applied to {} of {} jobs",
            sd.default_lora,
            sd.default_lora_jobs,
            im.images_this_session
        );
    }
    println!("fatal_hint {:?}, starter_exit {:?}", sig.fatal_hint, sig.starter_exit.map(|c| format!("{c} (0x{:08X})", c as i32 as u32)));
    println!("error_tail ({}):", sig.error_tail.len());
    for e in sig.error_tail.iter().rev().take(8).rev() {
        println!("   {}", e.chars().take(160).collect::<String>());
    }
    println!("redaction: console lines containing 'api_keys:' seen {key_leaks} times; marker seen: {redacted_seen}");
    println!("console tail:");
    for c in tr.console().iter().rev().take(6).rev() {
        println!("   {}", c.chars().take(160).collect::<String>());
    }
}
