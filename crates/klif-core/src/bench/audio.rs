//! TTS and STT bench runs (`klif-cli bench <system>` on a tts / stt System). Both record audio seconds per wall
//! second (`ttsRtf`, `sttRtf`: higher is better).
//!
//! TTS: fixed English texts to `POST /v1/audio/speech`. audio.cpp gets `response_format: "json"` and answers
//! `timing {wall_ms, audio_duration_ms, rtf}`; its `rtf` is wall / audio, KLIF records audio / wall. Any other
//! OpenAI-compatible server gets `response_format: "wav"` (and the OpenAI voice "alloy"): the wall time is measured
//! here and the audio length read from the returned WAV header (or from audio.cpp's `X-AudioCPP-*` headers).
//!
//! STT: real speech from `--audio <file.wav>` (required), posted as multipart `file` to the server's transcription
//! route: audio.cpp `/v1/audio/transcriptions`; whisper-server its `--inference-path` (below `--request-path`) when
//! the command names one, else `/v1/audio/transcriptions`, then `/inference`. The audio length comes from the WAV
//! header; the wall time is measured here unless the server reports `timing.wall_ms`.

use anyhow::{anyhow, bail, Context, Result};
use klif_common::vm::{AdapterId, System, SystemId};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

use super::{auth_body, get_json, one_line, round3, with_sampling, BenchOpts, BenchRun, Peak, Target};
use crate::link::EngineLink;

/// The texts the TTS runs speak (one per run, in turn): about ten seconds of speech each.
const TEXTS: [&str; 3] = [
    "The lighthouse keeper climbed the narrow stairs every evening, lit the great lamp, and watched the ships pass \
     safely along the rocky coast until the morning came.",
    "Fresh bread, warm coffee and the quiet hum of the market: the small town woke up slowly, while the river \
     carried the first light of the day down toward the sea.",
    "Please remember that the meeting starts at nine thirty tomorrow, in the conference room on the second floor, \
     and bring the printed reports with you.",
];

/// Largest speech file the STT bench sends.
const MAX_AUDIO_BYTES: u64 = 200 * 1024 * 1024;
/// Largest TTS answer read.
const MAX_SPEECH_BYTES: u64 = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------------------------- WAV

/// The duration (s) of a RIFF / WAVE file: the `fmt ` chunk gives the byte rate, the `data` chunk the length (a
/// streaming header's 0 or 0xFFFFFFFF length, or one past the end, means "to the end of the bytes").
pub(crate) fn wav_seconds(bytes: &[u8]) -> Option<f64> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let le16 = |at: usize| bytes.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let le32 = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let mut pos = 12usize;
    let mut byte_rate: Option<f64> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = le32(pos + 4)? as usize;
        let body = pos + 8;
        if id == b"fmt " {
            let channels = le16(body + 2)? as f64;
            let rate = le32(body + 4)? as f64;
            let stated = le32(body + 8)? as f64;
            let bits = le16(body + 14)? as f64;
            let computed = rate * channels * (bits / 8.0).ceil();
            byte_rate = Some(if stated > 0.0 { stated } else { computed });
        } else if id == b"data" {
            let br = byte_rate.filter(|b| *b > 0.0)?;
            let avail = bytes.len() - body;
            let len = if size == 0 || size >= 0xFFFF_FFF0 || size > avail { avail } else { size };
            return Some(len as f64 / br);
        }
        // Chunks are padded to an even length.
        pos = body.checked_add(size)?.checked_add(size & 1)?;
    }
    None
}

/// Read a speech file for the STT bench: its bytes and its length in seconds.
pub(crate) fn read_wav(path: &Path) -> Result<(Vec<u8>, f64)> {
    let shown = path.display();
    let meta = std::fs::metadata(path).with_context(|| format!("The audio file {shown} could not be read"))?;
    if !meta.is_file() {
        bail!("The audio file {shown} is not a file.");
    }
    if meta.len() > MAX_AUDIO_BYTES {
        bail!("The audio file {shown} is larger than {} MiB; use a shorter recording.", MAX_AUDIO_BYTES / (1024 * 1024));
    }
    let bytes = std::fs::read(path).with_context(|| format!("The audio file {shown} could not be read"))?;
    let secs = wav_seconds(&bytes).ok_or_else(|| anyhow!("{shown} is not a WAV file KLIF can read (RIFF/WAVE with fmt and data chunks)."))?;
    if !(secs.is_finite() && secs >= 0.5) {
        bail!("{shown} holds less than half a second of audio; use a real recording of speech.");
    }
    Ok((bytes, secs))
}

/// Standard base64 (also with a `data:...;base64,` prefix and whitespace); None when it is not base64.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let t = text.trim();
    let t = t.split_once(";base64,").map(|(_, b)| b).unwrap_or(t);
    let mut out = Vec::with_capacity(t.len() / 4 * 3);
    let (mut acc, mut nbits) = (0u32, 0u32);
    for c in t.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\r' | b'\n' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
            acc &= (1 << nbits) - 1;
        }
    }
    Some(out)
}

// ------------------------------------------------------------------------------------------- models

/// The model id to ask for: the `/v1/models` entry whose `task` fits (`want`), preferring a loaded one; else the
/// first entry; else what the session reports; else `fallback`.
fn pick_model(agent: &ureq::Agent, t: &Target, sys: &System, want: &[&str], fallback: &str) -> String {
    let listed: Vec<Value> =
        get_json(agent, t, "/v1/models").and_then(|v| v.get("data").and_then(Value::as_array).cloned()).unwrap_or_default();
    let id = |m: &Value| m.get("id").and_then(Value::as_str).map(str::to_string);
    let fits = |m: &&Value| m.get("task").and_then(Value::as_str).is_none_or(|task| want.iter().any(|w| task.eq_ignore_ascii_case(w)));
    let loaded = |m: &&Value| m.get("loaded").and_then(Value::as_bool).unwrap_or(false);
    listed
        .iter()
        .filter(fits)
        .find(loaded)
        .or_else(|| listed.iter().find(fits))
        .or_else(|| listed.first())
        .and_then(id)
        .or_else(|| sys.session.as_ref().and_then(|s| s.generic.as_ref()).and_then(|g| g.model_id.clone()))
        .unwrap_or_else(|| fallback.to_string())
}

fn num(v: Option<&Value>, key: &str) -> Option<f64> {
    v.and_then(|v| v.get(key)).and_then(Value::as_f64).filter(|x| x.is_finite() && *x > 0.0)
}

fn header_f64(resp: &ureq::http::Response<ureq::Body>, name: &str) -> Option<f64> {
    resp.headers().get(name).and_then(|h| h.to_str().ok()).and_then(|s| s.trim().parse::<f64>().ok()).filter(|x| x.is_finite() && *x > 0.0)
}

fn log_run(i: u32, runs: u32, audio_s: f64, wall_s: f64) {
    log::info!("run {}/{runs}: {audio_s:.2} s of audio in {wall_s:.2} s ({:.2}x real time)", i + 1, audio_s / wall_s);
}

// ---------------------------------------------------------------------------------------------- TTS

/// One speech request: (audio seconds, wall seconds).
fn speak(agent: ureq::Agent, url: String, key: Option<klif_common::Secret>, body: Value) -> Result<(f64, f64)> {
    let t0 = Instant::now();
    let resp = auth_body(agent.post(&url), &key).send_json(&body).map_err(|e| anyhow!("{url}: {e}"))?;
    let status = resp.status().as_u16();
    let head_wall = header_f64(&resp, "x-audiocpp-wall-ms").map(|ms| ms / 1000.0);
    let head_audio = header_f64(&resp, "x-audiocpp-audio-duration-ms").map(|ms| ms / 1000.0);
    let json_body = resp
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|c| c.to_ascii_lowercase().contains("json"));
    let bytes = resp.into_body().with_config().limit(MAX_SPEECH_BYTES).read_to_vec().map_err(|e| anyhow!("{url}: {e}"))?;
    let client_wall = t0.elapsed().as_secs_f64();
    match status {
        200 => {}
        404 | 405 => bail!("This server has no POST /v1/audio/speech, so its TTS bench is not supported."),
        401 | 403 => bail!("The server refused the bench request ({status}): check the API key."),
        _ => bail!("The server answered {status}: {}", one_line(&String::from_utf8_lossy(&bytes), 300)),
    }
    let looks_json = json_body || bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{');
    if looks_json {
        let v: Value = serde_json::from_slice(&bytes).map_err(|_| anyhow!("The TTS server answered JSON KLIF cannot read."))?;
        if let Some(err) = v.get("error") {
            let msg = err.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| err.to_string());
            bail!("The TTS server reported an error: {}", one_line(&msg, 300));
        }
        // audio.cpp: `timing {wall_ms, audio_duration_ms, rtf}` (rtf = wall / audio).
        let timing = v.get("timing");
        let wall = num(timing, "wall_ms").map(|ms| ms / 1000.0).or(head_wall);
        let audio = num(timing, "audio_duration_ms").map(|ms| ms / 1000.0).or(head_audio).or_else(|| {
            let wall = wall?;
            num(timing, "rtf").map(|rtf| wall / rtf)
        });
        let audio = audio.or_else(|| {
            let b64 = ["audio", "b64_json", "data"]
                .iter()
                .find_map(|k| v.get(*k).and_then(Value::as_str))
                .or_else(|| v.get("data").and_then(|d| d.get(0)).and_then(|d| d.get("b64_json")).and_then(Value::as_str))?;
            wav_seconds(&base64_decode(b64)?)
        });
        let audio = audio.ok_or_else(|| anyhow!("The TTS server answered without timing or audio KLIF can measure."))?;
        return Ok((audio, wall.unwrap_or(client_wall)));
    }
    let audio = head_audio.or_else(|| wav_seconds(&bytes)).ok_or_else(|| {
        anyhow!("The TTS server returned audio that is not WAV ({} bytes); KLIF asks for response_format \"wav\".", bytes.len())
    })?;
    Ok((audio, head_wall.unwrap_or(client_wall)))
}

pub(super) fn bench_tts(
    link: &dyn EngineLink,
    id: &SystemId,
    sys: &System,
    agent: &ureq::Agent,
    t: &Target,
    opts: &BenchOpts,
    peak: &mut Peak,
) -> Result<Vec<BenchRun>> {
    let label = sys.label.as_str();
    let audiocpp = t.adapter == AdapterId::AudioCpp;
    let model = pick_model(agent, t, sys, &["tts", "speech"], "tts-1");
    let url = format!("{}/v1/audio/speech", t.base);
    let mut runs = Vec::new();
    for i in 0..opts.runs {
        let text = TEXTS[i as usize % TEXTS.len()];
        let body = if audiocpp {
            json!({ "model": model, "input": text, "response_format": "json" })
        } else {
            json!({ "model": model, "input": text, "voice": "alloy", "response_format": "wav" })
        };
        let (a, u, k) = (agent.clone(), url.clone(), t.key.clone());
        let (audio_s, wall_s) =
            with_sampling(link, id, peak, move || speak(a, u, k, body)).with_context(|| format!("Bench run {} of {label} failed", i + 1))?;
        if !(audio_s > 0.0 && wall_s > 0.0) {
            bail!("Bench run {} of {label}: the server returned no audio.", i + 1);
        }
        log_run(i, opts.runs, audio_s, wall_s);
        runs.push(BenchRun {
            audio_s: Some(round3(audio_s)),
            wall_s: Some(round3(wall_s)),
            tts_rtf: Some(round3(audio_s / wall_s)),
            ..BenchRun::default()
        });
    }
    Ok(runs)
}

// ---------------------------------------------------------------------------------------------- STT

/// The value after `flag` in the command the System runs.
fn arg_after(sys: &System, flag: &str) -> Option<String> {
    let cmd = sys.session.as_ref().and_then(|s| s.command.as_ref()).or(sys.command.as_ref())?;
    let args = &cmd.args;
    args.iter().enumerate().find_map(|(i, a)| {
        if a == flag {
            args.get(i + 1).cloned()
        } else {
            a.strip_prefix(flag).and_then(|r| r.strip_prefix('=')).map(str::to_string)
        }
    })
}

/// The transcription routes to try, in order.
fn stt_paths(sys: &System, adapter: AdapterId) -> Vec<String> {
    if adapter == AdapterId::AudioCpp {
        return vec!["/v1/audio/transcriptions".into()];
    }
    let slash = |p: String| if p.starts_with('/') { p } else { format!("/{p}") };
    let prefix = arg_after(sys, "--request-path").map(slash).map(|p| p.trim_end_matches('/').to_string()).unwrap_or_default();
    match arg_after(sys, "--inference-path").map(slash) {
        Some(p) => vec![format!("{prefix}{p}")],
        None => vec![format!("{prefix}/v1/audio/transcriptions"), format!("{prefix}/inference")],
    }
}

fn multipart(boundary: &str, wav: &[u8], fields: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(wav.len() + 1024);
    out.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"klif-bench.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(wav);
    out.extend_from_slice(b"\r\n");
    for (name, value) in fields {
        out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

/// What one transcription request reported.
enum Heard {
    /// (wall seconds, the server's own wall time if it reported one, characters of text)
    Done(f64, Option<f64>, usize),
    /// The route does not exist (404 / 405): try the next one.
    NoRoute,
}

fn transcribe(agent: ureq::Agent, url: String, key: Option<klif_common::Secret>, body: Vec<u8>, boundary: String) -> Result<Heard> {
    let t0 = Instant::now();
    let mut req = agent.post(&url).header("Content-Type", format!("multipart/form-data; boundary={boundary}"));
    if let Some(k) = &key {
        req = req.header("Authorization", format!("Bearer {}", k.expose()));
    }
    let resp = req.send(&body[..]).map_err(|e| anyhow!("{url}: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.into_body().with_config().limit(64 * 1024 * 1024).read_to_string().map_err(|e| anyhow!("{url}: {e}"))?;
    let wall = t0.elapsed().as_secs_f64();
    match status {
        200 => {}
        404 | 405 => return Ok(Heard::NoRoute),
        401 | 403 => bail!("The server refused the bench request ({status}): check the API key."),
        _ => bail!("The server answered {status}: {}", one_line(&text, 300)),
    }
    let (server_wall, chars) = match serde_json::from_str::<Value>(&text) {
        Ok(v) => {
            if let Some(err) = v.get("error") {
                let msg = err.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| err.to_string());
                bail!("The STT server reported an error: {}", one_line(&msg, 300));
            }
            let chars = v.get("text").and_then(Value::as_str).map(|t| t.trim().chars().count()).unwrap_or(0);
            (num(v.get("timing"), "wall_ms").map(|ms| ms / 1000.0), chars)
        }
        Err(_) => (None, text.trim().chars().count()),
    };
    Ok(Heard::Done(wall, server_wall, chars))
}

pub(super) fn bench_stt(
    link: &dyn EngineLink,
    id: &SystemId,
    sys: &System,
    agent: &ureq::Agent,
    t: &Target,
    opts: &BenchOpts,
    peak: &mut Peak,
) -> Result<Vec<BenchRun>> {
    let label = sys.label.as_str();
    let path = opts.audio.as_deref().ok_or_else(|| anyhow!("Bench of {label} needs real speech: pass --audio <file.wav>."))?;
    let (wav, audio_s) = read_wav(path)?;
    log::info!("{label}: transcribing {:.1} s of audio from {} per run", audio_s, path.file_name().map(|f| f.to_string_lossy()).unwrap_or_default());
    let model = pick_model(agent, t, sys, &["asr", "stt", "transcribe", "transcription"], "whisper-1");
    let mut routes = stt_paths(sys, t.adapter);
    let mut runs = Vec::new();
    for i in 0..opts.runs {
        let boundary = format!("klif-bench-{}", super::nonce());
        let body = multipart(&boundary, &wav, &[("model", &model), ("response_format", "json"), ("temperature", "0")]);
        let (wall_s, chars) = loop {
            let Some(route) = routes.first().cloned() else {
                bail!("{label} answered 404 on every transcription route KLIF knows (/v1/audio/transcriptions, /inference); pass --inference-path to the server.");
            };
            let (a, u, k, b, bd) = (agent.clone(), format!("{}{route}", t.base), t.key.clone(), body.clone(), boundary.clone());
            let heard =
                with_sampling(link, id, peak, move || transcribe(a, u, k, b, bd)).with_context(|| format!("Bench run {} of {label} failed", i + 1))?;
            match heard {
                Heard::Done(wall, server_wall, chars) => break (server_wall.unwrap_or(wall), chars),
                Heard::NoRoute => {
                    log::info!("{label} has no POST {route}; trying the next transcription route");
                    routes.remove(0);
                }
            }
        };
        if chars == 0 {
            log::warn!("run {}/{}: the transcript is empty (is the file speech?)", i + 1, opts.runs);
        }
        log_run(i, opts.runs, audio_s, wall_s);
        runs.push(BenchRun {
            audio_s: Some(round3(audio_s)),
            wall_s: Some(round3(wall_s)),
            stt_rtf: Some(round3(audio_s / wall_s.max(1e-6))),
            ..BenchRun::default()
        });
    }
    Ok(runs)
}
