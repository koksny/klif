//! Downloads of recommended models, only on explicit request, only from the official Hugging Face hub:
//! sizes + LFS sha256 from `https://huggingface.co/api/models/{repo}/tree/{revision}?recursive=true`, files from
//! `https://huggingface.co/{repo}/resolve/{revision}/{file}`, redirects only to HF-owned hosts, `HF_TOKEN` never
//! forwarded across hosts, `.part` + Range resume (re-resolved), sha256 verify, rename; free-space check;
//! refused while `[paths] models_dir` is unset. Files go to `klif_catalog::recommend::install_path`.
//! A file may name its own `repo` / `revision` (e.g. a projector from another repo); those override the entry's
//! `hf_repo` / `revision` for that file's tree lookup, resolve URL and checks (validated the same way).
//! ureq needs `TlsProvider::NativeTls` in its config (the workspace enables only the native-tls feature).
//! Owner: package E2.
//!
//! Every hop (API, resolve, each redirect) must be `https` on `huggingface.co`, `*.huggingface.co` or `*.hf.co`;
//! redirects are followed by hand so the allowlist is checked per hop, and `Authorization: Bearer $HF_TOKEN` is
//! only ever sent to `huggingface.co` itself (never to a CDN host). A file is fetched in Range windows; each window
//! re-resolves the `resolve/` URL (signed CDN URLs expire), so a broken or stalled connection resumes where it
//! stopped. The sha256 is computed while writing (an existing `.part` is hashed first), checked against the
//! recommendation's / the hub's LFS sha256, and only then is the `.part` renamed to the final name. Cancel keeps the
//! `.part` for the next resume. An existing final file is never overwritten.

use klif_catalog::recommend::{install_path, models_dir};
use klif_catalog::Recommendation;
use klif_common::config::Config;
use klif_common::vm::{DownloadInfo, DownloadState};
use klif_common::Secret;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The hub every download comes from (the only host that ever receives `HF_TOKEN`).
pub const HF_HOST: &str = "huggingface.co";
/// Bytes per ranged request (each re-resolves the file URL).
const WINDOW: u64 = 128 * 1024 * 1024;
/// Failed windows in a row (without progress) before a file fails.
const MAX_FAILS: u32 = 6;
/// Kept free on the target volume besides the download itself.
const FREE_MARGIN: u64 = 256 * 1024 * 1024;
/// Progress callbacks at most this often per file (state changes always go out).
const EMIT_EVERY: Duration = Duration::from_millis(250);
/// Most pages of the repo tree that are read.
const MAX_TREE_PAGES: usize = 100;

/// A running download of one recommendation (all its files).
pub struct DownloadHandle {
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl DownloadHandle {
    /// Ask the download to stop (the `.part` file is kept for a resume).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// True once the download thread ended (every file is done, failed or cancelled).
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

/// Start downloading `rec` on a background thread; `progress` gets one `DownloadInfo` per (id, file) change.
pub fn start(cfg: &Config, rec: &Recommendation, progress: Box<dyn Fn(DownloadInfo) + Send>) -> DownloadHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let prepared = prepare(cfg, rec);
    let rec = rec.clone();
    let (c, f) = (cancel.clone(), finished.clone());
    let spawned = std::thread::Builder::new().name(format!("klif-download-{}", rec.id)).spawn(move || {
        let emit = Emitter { progress, id: rec.id.clone() };
        match prepared {
            Ok((dir, files)) => run(&dir, &files, &emit, &c),
            Err(msg) => {
                for file in &rec.files {
                    emit.state(&file.name, 0, file.size, DownloadState::Failed, Some(msg.clone()));
                }
            }
        }
        f.store(true, Ordering::SeqCst);
    });
    if let Err(e) = spawned {
        log::error!("download thread could not start: {e}");
        finished.store(true, Ordering::SeqCst);
    }
    DownloadHandle { cancel, finished }
}

// ------------------------------------------------------------------------------------------ plan

struct FilePlan {
    /// Path in the repo.
    name: String,
    /// The repo and revision the file comes from (the file's own, else the entry's).
    repo: String,
    revision: String,
    target: PathBuf,
    rec_sha: Option<String>,
    rec_size: Option<u64>,
}

fn valid_repo_part(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A repo-relative file path that cannot leave the install folder.
fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name.contains(':')
        && name.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.chars().any(|c| c.is_control()))
}

/// `org/name` (or a bare `name`) made of plain parts.
fn valid_repo_id(repo: &str) -> bool {
    let parts: Vec<&str> = repo.split('/').collect();
    parts.len() <= 2 && parts.iter().all(|p| valid_repo_part(p))
}

fn prepare(cfg: &Config, rec: &Recommendation) -> Result<(PathBuf, Vec<FilePlan>), String> {
    let dir = models_dir(cfg)
        .ok_or_else(|| "Downloads are off until [paths] models_dir is set in klif.toml (the folder KLIF downloads models to).".to_string())?;
    if rec.files.is_empty() {
        return Err(format!("Recommendation \"{}\" lists no files.", rec.id));
    }
    let mut files = Vec::new();
    for f in &rec.files {
        if !valid_file_name(&f.name) {
            return Err(format!("\"{}\" is not a safe file path inside the repo.", f.name));
        }
        // A file's own repo / revision override the entry's (the same rule `install_path` uses), validated the
        // same way.
        let repo = rec.repo_of(f);
        let revision = rec.revision_of(f);
        if !valid_repo_id(repo) {
            return Err(if repo != rec.hf_repo {
                format!("\"{repo}\" (the repo of {}) is not a Hugging Face repo id (org/name).", f.name)
            } else {
                format!("\"{repo}\" is not a Hugging Face repo id (org/name).")
            });
        }
        if !valid_repo_part(revision) {
            return Err(if revision != rec.revision {
                format!("Recommendation \"{}\" has no usable revision (a commit sha) for {}.", rec.id, f.name)
            } else {
                format!("Recommendation \"{}\" has no usable revision (a commit sha).", rec.id)
            });
        }
        let target =
            install_path(cfg, rec, &f.name).ok_or_else(|| format!("\"{}\" has no install path under {}.", f.name, dir.display()))?;
        let rec_sha = match f.sha256.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
            None => None,
            Some(s) if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) => Some(s),
            Some(_) => return Err(format!("The sha256 of \"{}\" in the recommendation is not 64 hex digits.", f.name)),
        };
        files.push(FilePlan {
            name: f.name.clone(),
            repo: repo.to_string(),
            revision: revision.to_string(),
            target,
            rec_sha,
            rec_size: f.size,
        });
    }
    Ok((dir, files))
}

// ------------------------------------------------------------------------------------ progress

struct Emitter {
    progress: Box<dyn Fn(DownloadInfo) + Send>,
    id: String,
}

impl Emitter {
    fn state(&self, file: &str, done: u64, total: Option<u64>, state: DownloadState, error: Option<String>) {
        (self.progress)(DownloadInfo { id: self.id.clone(), file: file.to_string(), done_bytes: done, total_bytes: total, state, error });
    }
}

/// Throttled running-progress for one file.
struct FileProgress<'a> {
    emit: &'a Emitter,
    file: &'a str,
    total: u64,
    last: Instant,
}

impl FileProgress<'_> {
    fn running(&mut self, done: u64, force: bool) {
        if force || self.last.elapsed() >= EMIT_EVERY {
            self.last = Instant::now();
            self.emit.state(self.file, done, Some(self.total), DownloadState::Running, None);
        }
    }
}

// ----------------------------------------------------------------------------------------- run

/// Why a step did not finish.
#[derive(Debug)]
enum Fail {
    Cancelled,
    /// Give up on this file (a sentence).
    Fatal(String),
    /// Try again (resume) after a pause (a sentence for when the retries run out).
    Retry(String),
    /// The server ignored the range: start the file over.
    Restart,
}

/// What the hub's tree says about one file.
#[derive(Debug, Clone)]
struct TreeEntry {
    size: u64,
    /// LFS sha256 (hex, lower case); None for files stored in git directly.
    lfs_sha: Option<String>,
}

fn run(dir: &Path, files: &[FilePlan], emit: &Emitter, cancel: &AtomicBool) {
    let fail_all = |msg: String| {
        for f in files {
            emit.state(&f.name, 0, f.rec_size, DownloadState::Failed, Some(msg.clone()));
        }
    };
    let token = std::env::var("HF_TOKEN").ok().and_then(Secret::new);
    let agent = agent();
    // One tree per (repo, revision) the files come from.
    let mut trees: HashMap<(&str, &str), HashMap<String, TreeEntry>> = HashMap::new();
    for f in files {
        let key = (f.repo.as_str(), f.revision.as_str());
        if trees.contains_key(&key) {
            continue;
        }
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        match fetch_tree(&agent, key.0, key.1, token.as_ref()) {
            Ok(t) => {
                trees.insert(key, t);
            }
            Err(Fail::Cancelled) => return,
            Err(Fail::Fatal(m) | Fail::Retry(m)) => return fail_all(m),
            Err(Fail::Restart) => return fail_all("The file list could not be read from Hugging Face.".into()),
        }
    }

    // Sizes and hashes: the recommendation must agree with the hub.
    let mut plan: Vec<(&FilePlan, u64, Option<String>)> = Vec::new();
    for f in files {
        let Some(entry) = trees.get(&(f.repo.as_str(), f.revision.as_str())).and_then(|t| t.get(&f.name)) else {
            return fail_all(format!("{} is not in {} at revision {}.", f.name, f.repo, short(&f.revision)));
        };
        if let (Some(a), Some(b)) = (&f.rec_sha, &entry.lfs_sha) {
            if a != b {
                return fail_all(format!("The hub's sha256 of {} differs from the recommendation, so KLIF does not download it.", f.name));
            }
        }
        if f.rec_size.is_some_and(|s| s != entry.size) {
            return fail_all(format!("The hub's size of {} differs from the recommendation, so KLIF does not download it.", f.name));
        }
        plan.push((f, entry.size, f.rec_sha.clone().or_else(|| entry.lfs_sha.clone())));
    }

    // Free space for what is still missing.
    if let Err(e) = std::fs::create_dir_all(dir) {
        return fail_all(format!("The models folder {} could not be created: {e}.", dir.display()));
    }
    let mut needed = 0u64;
    for (f, size, _) in &plan {
        if f.target.exists() {
            continue;
        }
        let have = std::fs::metadata(part_path(&f.target)).map(|m| m.len()).unwrap_or(0);
        needed += size.saturating_sub(have.min(*size));
    }
    if let Some(free) = free_space(dir) {
        if needed + FREE_MARGIN > free {
            return fail_all(format!("Not enough free space in {}: {} needed, {} free.", dir.display(), gib(needed + FREE_MARGIN), gib(free)));
        }
    }

    for (i, (f, size, sha)) in plan.iter().enumerate() {
        let result = if cancel.load(Ordering::SeqCst) {
            Err(Fail::Cancelled)
        } else {
            download_file(&agent, f, *size, sha.as_deref(), token.as_ref(), emit, cancel)
        };
        match result {
            Ok(()) => emit.state(&f.name, *size, Some(*size), DownloadState::Done, None),
            Err(Fail::Cancelled) => {
                let have = std::fs::metadata(part_path(&f.target)).map(|m| m.len()).unwrap_or(0);
                emit.state(&f.name, have, Some(*size), DownloadState::Cancelled, None);
                for (g, s, _) in &plan[i + 1..] {
                    emit.state(&g.name, 0, Some(*s), DownloadState::Cancelled, None);
                }
                return;
            }
            Err(Fail::Fatal(m) | Fail::Retry(m)) => {
                let have = std::fs::metadata(part_path(&f.target)).map(|m| m.len()).unwrap_or(0);
                emit.state(&f.name, have, Some(*size), DownloadState::Failed, Some(m));
                for (g, s, _) in &plan[i + 1..] {
                    emit.state(&g.name, 0, Some(*s), DownloadState::Failed, Some(format!("Not downloaded because {} failed.", f.name)));
                }
                return;
            }
            Err(Fail::Restart) => {
                emit.state(&f.name, 0, Some(*size), DownloadState::Failed, Some("The download could not be completed.".into()));
                return;
            }
        }
    }
}

fn short(rev: &str) -> &str {
    rev.get(..12).unwrap_or(rev)
}

fn gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

fn part_path(target: &Path) -> PathBuf {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    target.with_file_name(format!("{name}.part"))
}

// ---------------------------------------------------------------------------------------- http

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        // Redirects are followed by hand: every hop is checked against the allowlist.
        .max_redirects(0)
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        // One window (WINDOW bytes) must arrive within this; a stalled connection then resumes.
        .timeout_recv_body(Some(Duration::from_secs(10 * 60)))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .user_agent(format!("KLIF/{}", klif_common::KLIF_VERSION))
        .build()
        .into()
}

/// `huggingface.co`, `*.huggingface.co`, `*.hf.co`.
fn host_allowed(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    h == HF_HOST || h.ends_with(".huggingface.co") || h.ends_with(".hf.co")
}

/// Percent-encode one URL path segment (RFC 3986 unreserved characters stay).
fn enc_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn enc_path(p: &str) -> String {
    p.split('/').map(enc_segment).collect::<Vec<_>>().join("/")
}

/// A redirect target resolved against the URL that answered.
fn join_location(base: &ureq::http::Uri, loc: &str) -> Option<String> {
    let loc = loc.trim();
    if loc.starts_with("https://") || loc.starts_with("http://") {
        return Some(loc.to_string());
    }
    let scheme = base.scheme_str()?;
    let authority = base.authority()?.as_str();
    if let Some(rest) = loc.strip_prefix("//") {
        return Some(format!("{scheme}://{rest}"));
    }
    if loc.starts_with('/') {
        return Some(format!("{scheme}://{authority}{loc}"));
    }
    let path = base.path();
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    Some(format!("{scheme}://{authority}{dir}/{loc}"))
}

/// GET `url`, following redirects by hand: https only, HF-owned hosts only, the token only to huggingface.co.
/// Returns the final response and its host.
fn open(
    agent: &ureq::Agent,
    url: &str,
    token: Option<&Secret>,
    range: Option<(u64, u64)>,
) -> Result<(ureq::http::Response<ureq::Body>, String), Fail> {
    let mut url = url.to_string();
    for _ in 0..10 {
        let uri: ureq::http::Uri = url.parse().map_err(|_| Fail::Fatal(format!("\"{url}\" is not a valid URL.")))?;
        if uri.scheme_str() != Some("https") {
            return Fail::fatal("KLIF refused a redirect to a non-https address; downloads come only from huggingface.co.");
        }
        let host = uri.host().unwrap_or("").to_ascii_lowercase();
        if !host_allowed(&host) || uri.authority().is_some_and(|a| a.as_str().contains('@')) {
            return Fail::fatal(&format!("KLIF refused a redirect to {host}; downloads come only from huggingface.co."));
        }
        log::debug!("download: GET https://{host}/...{}", range.map(|(a, b)| format!(" bytes={a}-{b}")).unwrap_or_default());
        let mut req = agent.get(&url);
        if host == HF_HOST {
            if let Some(t) = token {
                req = req.header("Authorization", format!("Bearer {}", t.expose()));
            }
        }
        if let Some((a, b)) = range {
            req = req.header("Range", format!("bytes={a}-{b}"));
        }
        let resp = req.call().map_err(|e| Fail::Retry(format!("Hugging Face could not be reached: {e}.")))?;
        let status = resp.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let loc = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Fail::Retry("Hugging Face sent a redirect without a target.".into()))?;
            url = join_location(&uri, loc).ok_or_else(|| Fail::Fatal("Hugging Face sent an unusable redirect.".into()))?;
            continue;
        }
        return Ok((resp, host));
    }
    Fail::fatal("Too many redirects from Hugging Face.")
}

impl Fail {
    fn fatal<T>(msg: &str) -> Result<T, Fail> {
        Err(Fail::Fatal(msg.to_string()))
    }
}

fn status_fail(status: u16, host: &str, what: &str) -> Fail {
    match status {
        401 | 403 if host == HF_HOST => Fail::Fatal(format!(
            "Hugging Face refused {what} ({status}): the model may be gated; accept its terms on huggingface.co and set HF_TOKEN."
        )),
        404 => Fail::Fatal(format!("Hugging Face does not have {what} (404).")),
        // A signed CDN URL may have expired: re-resolve.
        _ => Fail::Retry(format!("Hugging Face answered {status} for {what}.")),
    }
}

/// `next` URL of a `Link` header (`<url>; rel="next"`).
fn next_link(resp: &ureq::http::Response<ureq::Body>) -> Option<String> {
    let link = resp.headers().get("link")?.to_str().ok()?;
    link.split(',').find_map(|part| {
        let (url, params) = part.split_once(';')?;
        params.contains("rel=\"next\"").then(|| url.trim().trim_start_matches('<').trim_end_matches('>').to_string())
    })
}

/// The tree of `repo` at `revision` (recursive, all pages): path -> size + LFS sha256.
fn fetch_tree(agent: &ureq::Agent, repo: &str, revision: &str, token: Option<&Secret>) -> Result<HashMap<String, TreeEntry>, Fail> {
    let mut url = format!("https://{HF_HOST}/api/models/{}/tree/{}?recursive=true", enc_path(repo), enc_segment(revision));
    let mut out = HashMap::new();
    let mut attempts = 0;
    let mut pages = 0;
    loop {
        let (resp, host) = match open(agent, &url, token, None) {
            Ok(r) => r,
            Err(Fail::Retry(m)) if attempts < 3 => {
                attempts += 1;
                log::warn!("{m} Retrying.");
                std::thread::sleep(Duration::from_secs(2 * attempts));
                continue;
            }
            Err(e) => return Err(e),
        };
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(status_fail(status, &host, &format!("the file list of {repo}")));
        }
        let next = next_link(&resp);
        let items: Vec<serde_json::Value> = resp
            .into_body()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_json()
            .map_err(|e| Fail::Fatal(format!("The file list of {repo} could not be read: {e}.")))?;
        for it in items {
            if it.get("type").and_then(|t| t.as_str()) != Some("file") {
                continue;
            }
            let Some(path) = it.get("path").and_then(|p| p.as_str()) else {
                continue;
            };
            let lfs = it.get("lfs");
            let size = lfs.and_then(|l| l.get("size")).and_then(|s| s.as_u64()).or_else(|| it.get("size").and_then(|s| s.as_u64())).unwrap_or(0);
            let lfs_sha = lfs
                .and_then(|l| l.get("oid"))
                .and_then(|o| o.as_str())
                .map(|o| o.trim_start_matches("sha256:").to_ascii_lowercase())
                .filter(|o| o.len() == 64 && o.chars().all(|c| c.is_ascii_hexdigit()));
            out.insert(path.to_string(), TreeEntry { size, lfs_sha });
        }
        pages += 1;
        match next {
            Some(n) if pages < MAX_TREE_PAGES => url = n,
            _ => return Ok(out),
        }
    }
}

// ---------------------------------------------------------------------------------------- file

#[allow(clippy::too_many_arguments)]
fn download_file(
    agent: &ureq::Agent,
    f: &FilePlan,
    size: u64,
    sha: Option<&str>,
    token: Option<&Secret>,
    emit: &Emitter,
    cancel: &AtomicBool,
) -> Result<(), Fail> {
    let mut prog = FileProgress { emit, file: &f.name, total: size, last: Instant::now() };
    // An existing final file is never overwritten.
    if let Ok(meta) = std::fs::metadata(&f.target) {
        if meta.len() != size {
            return Fail::fatal(&format!(
                "{} already exists with another size; move it away to download it again.",
                f.target.display()
            ));
        }
        if let Some(want) = sha {
            emit.state(&f.name, size, Some(size), DownloadState::Verifying, None);
            let got = hash_file(&f.target, cancel)?;
            if got != want {
                return Fail::fatal(&format!(
                    "{} already exists but its sha256 differs; move it away to download it again.",
                    f.target.display()
                ));
            }
        }
        return Ok(());
    }
    if let Some(parent) = f.target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Fail::Fatal(format!("{} could not be created: {e}.", parent.display())))?;
    }
    let part = part_path(&f.target);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&part)
        .map_err(|e| Fail::Fatal(format!("{} could not be opened: {e}.", part.display())))?;
    let mut pos = file.metadata().map(|m| m.len()).unwrap_or(0);
    if pos > size {
        file.set_len(0).map_err(io_fail(&part))?;
        pos = 0;
    }
    // Resume: hash what is already there.
    let mut hasher = Sha256::new();
    if pos > 0 {
        log::info!("resuming {} at {}", f.name, gib(pos));
        file.seek(SeekFrom::Start(0)).map_err(io_fail(&part))?;
        let mut buf = vec![0u8; 1 << 20];
        let mut left = pos;
        while left > 0 {
            if cancel.load(Ordering::SeqCst) {
                return Err(Fail::Cancelled);
            }
            let n = file.read(&mut buf[..left.min(1 << 20) as usize]).map_err(io_fail(&part))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            left -= n as u64;
        }
    }
    file.seek(SeekFrom::Start(pos)).map_err(io_fail(&part))?;
    prog.running(pos, true);

    let url = format!("https://{HF_HOST}/{}/resolve/{}/{}", enc_path(&f.repo), enc_segment(&f.revision), enc_path(&f.name));
    let mut fails = 0u32;
    let mut pos_at_fail = pos;
    while pos < size {
        if cancel.load(Ordering::SeqCst) {
            let _ = file.flush();
            return Err(Fail::Cancelled);
        }
        let end = (pos + WINDOW).min(size) - 1;
        match fetch_window(agent, &url, token, pos, end, size, &mut file, &part, &mut hasher, &mut pos, &mut prog, cancel) {
            Ok(()) => fails = 0,
            Err(Fail::Restart) => {
                log::warn!("the server ignored the range for {}; starting it over", f.name);
                file.set_len(0).map_err(io_fail(&part))?;
                file.seek(SeekFrom::Start(0)).map_err(io_fail(&part))?;
                hasher = Sha256::new();
                pos = 0;
                fails += 1;
                if fails > MAX_FAILS {
                    return Fail::fatal(&format!("The download of {} kept restarting.", f.name));
                }
            }
            Err(Fail::Retry(m)) => {
                if pos > pos_at_fail {
                    fails = 0;
                }
                pos_at_fail = pos;
                fails += 1;
                if fails > MAX_FAILS {
                    return Err(Fail::Fatal(m));
                }
                log::warn!("{m} Resuming {} at {} (attempt {fails}).", f.name, gib(pos));
                let wait = Duration::from_secs(1 << fails.min(5));
                let t0 = Instant::now();
                while t0.elapsed() < wait {
                    if cancel.load(Ordering::SeqCst) {
                        return Err(Fail::Cancelled);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Err(e) => {
                let _ = file.flush();
                return Err(e);
            }
        }
    }
    file.flush().map_err(io_fail(&part))?;
    file.sync_all().map_err(io_fail(&part))?;
    drop(file);

    emit.state(&f.name, size, Some(size), DownloadState::Verifying, None);
    let len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if len != size {
        return Fail::fatal(&format!("{} has {len} bytes instead of {size}.", f.name));
    }
    if let Some(want) = sha {
        let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != want {
            // A corrupt .part must not be resumed.
            let _ = std::fs::remove_file(&part);
            return Fail::fatal(&format!("The sha256 of {} does not match; the download was discarded.", f.name));
        }
    }
    let mut last = None;
    for attempt in 0..10u64 {
        match std::fs::rename(&part, &f.target) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(50 * (attempt + 1)));
            }
        }
    }
    Fail::fatal(&format!("{} could not be renamed: {}.", part.display(), last.map(|e| e.to_string()).unwrap_or_default()))
}

fn io_fail(path: &Path) -> impl Fn(std::io::Error) -> Fail + '_ {
    move |e| Fail::Fatal(format!("{}: {e}.", path.display()))
}

/// Fetch bytes `pos..=end` (re-resolving the file URL) into `file`, hashing as it writes.
#[allow(clippy::too_many_arguments)]
fn fetch_window(
    agent: &ureq::Agent,
    url: &str,
    token: Option<&Secret>,
    start: u64,
    end: u64,
    size: u64,
    file: &mut File,
    part: &Path,
    hasher: &mut Sha256,
    pos: &mut u64,
    prog: &mut FileProgress<'_>,
    cancel: &AtomicBool,
) -> Result<(), Fail> {
    let (resp, host) = open(agent, url, token, Some((start, end)))?;
    let status = resp.status().as_u16();
    let limit = match status {
        206 => {
            let ok = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().strip_prefix("bytes "))
                .and_then(|v| v.split_once('-'))
                .and_then(|(a, _)| a.trim().parse::<u64>().ok())
                .is_none_or(|a| a == start);
            if !ok {
                return Err(Fail::Retry("Hugging Face sent another range than asked.".into()));
            }
            end + 1
        }
        200 if start == 0 => size,
        200 => return Err(Fail::Restart),
        416 if start >= size => return Ok(()),
        416 => return Err(Fail::Restart),
        s => return Err(status_fail(s, &host, prog.file)),
    };
    let mut reader = resp.into_body().into_reader();
    let mut buf = vec![0u8; 1 << 20];
    while *pos < limit {
        if cancel.load(Ordering::SeqCst) {
            return Err(Fail::Cancelled);
        }
        let want = (limit - *pos).min(buf.len() as u64) as usize;
        let n = match reader.read(&mut buf[..want]) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Fail::Retry(format!("The connection broke off: {e}."))),
        };
        file.write_all(&buf[..n]).map_err(io_fail(part))?;
        hasher.update(&buf[..n]);
        *pos += n as u64;
        prog.running(*pos, false);
    }
    prog.running(*pos, true);
    if *pos < limit {
        return Err(Fail::Retry("The connection closed early.".into()));
    }
    Ok(())
}

fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<String, Fail> {
    let mut file = File::open(path).map_err(io_fail(path))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Fail::Cancelled);
        }
        let n = file.read(&mut buf).map_err(io_fail(path))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

// ---------------------------------------------------------------------------------- free space

/// Bytes free for this user on the volume of `dir` (None when unknown).
#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out pointers are valid u64s.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
    (ok != 0).then_some(avail)
}

#[cfg(not(windows))]
fn free_space(_dir: &Path) -> Option<u64> {
    // Off Windows the check is skipped (statvfs would need libc, which KLIF does not depend on).
    None
}
