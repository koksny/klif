//! Model recommendations per System kind (and LLM class): `data/recommendations.toml` (schema 1), embedded.
//! Starting points measured on one machine, with a disclaimer; files come only from the official Hugging Face hub
//! and only when the user asks (`klif_core::download`). Entries with network/model-fetch flags (`-hf --hf-repo -hff --hf-file -hft
//! --hf-token -mu --model-url -dr --docker-repo`, plus their draft / vocoder / mmproj variants), secret flags or
//! env templates are rejected at load, as are entries whose repo, revision or file names are not plain. Owner:
//! package B (entries: package I).
//!
//! A file may come from another repo than the entry's `hf_repo` (a model whose encoder or VAE lives elsewhere): its
//! own `repo` + `revision` override the entry's for that file (download, install folder). Recommendation args may
//! name a listed file as `{file:<name>}`; a preset made from the recommendation gets that file's install path there
//! (`{model}` / `{mmproj}` come from the files with role model / mmproj).

use klif_common::config::Config;
use klif_common::secret::is_secret_flag;
use klif_common::vm::{AdapterId, LlmClass, Measured, RecFileRole, SystemKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The embedded data file.
pub const RECOMMENDATIONS_TOML: &str = include_str!("../data/recommendations.toml");

/// The schema version this KLIF reads.
pub const SCHEMA: i64 = 1;

/// Flags that make a server fetch models or talk to the network on its own (refused in recommendation args).
pub const NETWORK_FLAGS: &[&str] = &[
    "-hf",
    "-hfr",
    "--hf-repo",
    "-hff",
    "--hf-file",
    "-hft",
    "--hf-token",
    "-mu",
    "--model-url",
    "-dr",
    "--docker-repo",
    "-hfd",
    "-hfrd",
    "--hf-repo-draft",
    "-hffd",
    "--hf-file-draft",
    "-hfv",
    "-hfrv",
    "--hf-repo-v",
    "-hffv",
    "--hf-file-v",
    "-mmu",
    "--mmproj-url",
];

/// One file of a recommendation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecFileSpec {
    /// Path of the file in the repo.
    pub name: String,
    pub role: RecFileRole,
    /// The repo ("owner/name") this file comes from, when not the entry's `hf_repo`. Needs `revision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// The commit sha (40 hex) of the file's repo; default: the entry's `revision`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// LFS sha256 (hex), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// One recommendation (snake_case keys in the TOML).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recommendation {
    pub id: String,
    pub kind: SystemKind,
    /// LLM only: fast | deep | max.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<LlmClass>,
    pub name: String,
    pub adapter: AdapterId,
    pub hf_repo: String,
    /// Commit sha the files were verified at.
    pub revision: String,
    pub files: Vec<RecFileSpec>,
    pub quant: String,
    pub license: String,
    pub hardware_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_vram_gib: Option<f64>,
    /// Arg template for the preset (placeholders allowed; no env templates).
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctx: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<RecMeasured>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Recommendation {
    /// The file with role `model` (else the first file).
    pub fn model_file(&self) -> Option<&RecFileSpec> {
        self.files.iter().find(|f| f.role == RecFileRole::Model).or_else(|| self.files.first())
    }

    /// The file with role `mmproj`, if any.
    pub fn mmproj_file(&self) -> Option<&RecFileSpec> {
        self.files.iter().find(|f| f.role == RecFileRole::Mmproj)
    }

    /// Total size of all files, when every size is known.
    pub fn total_bytes(&self) -> Option<u64> {
        self.files.iter().map(|f| f.size).sum()
    }

    /// A listed file by its path in the repo.
    pub fn file(&self, name: &str) -> Option<&RecFileSpec> {
        self.files.iter().find(|f| f.name == name)
    }

    /// The repo a file comes from: its own `repo`, else the entry's `hf_repo`.
    pub fn repo_of<'a>(&'a self, f: &'a RecFileSpec) -> &'a str {
        f.repo.as_deref().map(str::trim).filter(|r| !r.is_empty()).unwrap_or(&self.hf_repo)
    }

    /// The commit a file is fetched at: its own `revision`, else the entry's `revision`.
    pub fn revision_of<'a>(&'a self, f: &'a RecFileSpec) -> &'a str {
        f.revision.as_deref().map(str::trim).filter(|r| !r.is_empty()).unwrap_or(&self.revision)
    }
}

/// The `{file:<name>}` references in a recommendation arg, in order.
pub fn file_refs(arg: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = arg;
    while let Some(i) = rest.find("{file:") {
        let after = &rest[i + "{file:".len()..];
        let Some(j) = after.find('}') else { break };
        out.push(&after[..j]);
        rest = &after[j + 1..];
    }
    out
}

/// `arg` with every `{file:<name>}` replaced by `path(name)`.
pub(crate) fn expand_file_refs(arg: &str, path: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(i) = rest.find("{file:") {
        let after = &rest[i + "{file:".len()..];
        let Some(j) = after.find('}') else { break };
        out.push_str(&rest[..i]);
        out.push_str(&path(&after[..j]));
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

/// `measured = { ... }` in the TOML (snake_case); shown as `vm::Measured`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecMeasured {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill_tps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds_per_image: Option<f64>,
    #[serde(default)]
    pub hardware: String,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub date: String,
}

impl From<&RecMeasured> for Measured {
    fn from(m: &RecMeasured) -> Measured {
        Measured {
            decode_tps: m.decode_tps,
            prefill_tps: m.prefill_tps,
            seconds_per_image: m.seconds_per_image,
            hardware: m.hardware.clone(),
            backend: m.backend.clone(),
            date: m.date.clone(),
        }
    }
}

/// The embedded recommendations that passed the load rules (parsed once per process). Rejected entries are
/// logged as warnings.
pub fn embedded() -> &'static [Recommendation] {
    static RECS: OnceLock<Vec<Recommendation>> = OnceLock::new();
    RECS.get_or_init(|| {
        let (recs, warnings) = parse(RECOMMENDATIONS_TOML);
        for w in warnings {
            log::warn!("recommendations.toml: {w}");
        }
        recs
    })
}

/// Parse a recommendations file: the entries that pass the load rules, and one sentence per rejected entry.
pub fn parse(text: &str) -> (Vec<Recommendation>, Vec<String>) {
    let mut warnings = Vec::new();
    let doc: toml::Table = match toml::from_str(text) {
        Ok(d) => d,
        Err(e) => return (Vec::new(), vec![format!("does not parse: {}", e.to_string().trim())]),
    };
    match doc.get("schema").and_then(|v| v.as_integer()) {
        Some(SCHEMA) => {}
        other => {
            return (Vec::new(), vec![format!("schema {other:?} is not {SCHEMA}; every entry is ignored.")]);
        }
    }
    let entries = match doc.get("recommendation") {
        None => return (Vec::new(), warnings),
        Some(toml::Value::Array(a)) => a.clone(),
        Some(_) => return (Vec::new(), vec!["\"recommendation\" must be an array of tables ([[recommendation]]).".into()]),
    };
    let mut out: Vec<Recommendation> = Vec::new();
    for (i, entry) in entries.into_iter().enumerate() {
        let label = entry.get("id").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| format!("#{}", i + 1));
        if entry.get("env").is_some() {
            warnings.push(format!("entry {label} is skipped: env templates are not allowed."));
            continue;
        }
        let rec: Recommendation = match entry.try_into() {
            Ok(r) => r,
            Err(e) => {
                warnings.push(format!("entry {label} is skipped: {}", e.to_string().trim().replace(['\r', '\n'], " ")));
                continue;
            }
        };
        if let Err(e) = check(&rec) {
            warnings.push(format!("entry {label} is skipped: {e}"));
            continue;
        }
        if out.iter().any(|r| r.id == rec.id) {
            warnings.push(format!("entry {label} is skipped: the id is used twice."));
            continue;
        }
        out.push(rec);
    }
    (out, warnings)
}

/// The load rules for one entry. Err = one sentence.
pub fn check(rec: &Recommendation) -> Result<(), String> {
    klif_common::config::validate_preset_id(&rec.id)?;
    if !valid_repo(&rec.hf_repo) {
        return Err(format!("hf_repo \"{}\" is not \"owner/name\".", rec.hf_repo));
    }
    if !valid_revision(&rec.revision) {
        return Err("revision must be a 40-character commit sha.".into());
    }
    if rec.files.is_empty() {
        return Err("it lists no files.".into());
    }
    for (i, f) in rec.files.iter().enumerate() {
        if file_segments(&f.name).is_none() {
            return Err(format!("file \"{}\" is not a plain relative path in the repo.", f.name));
        }
        if rec.files[..i].iter().any(|g| g.name == f.name) {
            return Err(format!("file \"{}\" is listed twice.", f.name));
        }
        if let Some(r) = &f.repo {
            if !valid_repo(r) {
                return Err(format!("the repo \"{r}\" of file \"{}\" is not \"owner/name\".", f.name));
            }
            if f.revision.is_none() {
                return Err(format!("file \"{}\" names its own repo, so it needs its own revision (a 40-character commit sha).", f.name));
            }
        }
        if f.revision.as_deref().is_some_and(|r| !valid_revision(r)) {
            return Err(format!("the revision of file \"{}\" must be a 40-character commit sha.", f.name));
        }
        if let Some(s) = &f.sha256 {
            if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("file \"{}\" has a sha256 that is not 64 hex characters.", f.name));
            }
        }
    }
    if rec.class.is_some() && rec.kind != SystemKind::Llm {
        return Err("class only applies to llm recommendations.".into());
    }
    for a in &rec.args {
        let flag = a.split_once('=').map(|(f, _)| f).unwrap_or(a);
        if NETWORK_FLAGS.contains(&flag) {
            return Err(format!("args contain the network flag {flag} (models come only from an explicit download)."));
        }
        if is_secret_flag(flag) {
            return Err(format!("args contain the secret flag {flag}."));
        }
        if let Some(name) = file_refs(a).into_iter().find(|n| rec.file(n).is_none()) {
            return Err(format!("args refer to {{file:{name}}}, which is not one of its files."));
        }
    }
    Ok(())
}

fn valid_revision(rev: &str) -> bool {
    rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `owner/name` with plain characters.
fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok =
        |s: &str| !s.is_empty() && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    matches!((parts.next(), parts.next(), parts.next()), (Some(a), Some(b), None) if ok(a) && ok(b))
}

/// The `/`-separated segments of a repo file path, when it is a plain relative path (no `..`, no drive, no
/// backslash, no empty segment).
fn file_segments(file: &str) -> Option<Vec<&str>> {
    if file.is_empty() || file.starts_with('/') || file.contains('\\') || file.contains(':') || file.contains('\0') {
        return None;
    }
    let segs: Vec<&str> = file.split('/').collect();
    let bad = |s: &str| s.is_empty() || s == "." || s == ".." || s.ends_with(' ') || s.ends_with('.');
    if segs.iter().any(|s| bad(s)) {
        return None;
    }
    Some(segs)
}

/// `[paths] models_dir` (None: downloads are refused until it is set). Relative values are under state_dir.
pub fn models_dir(cfg: &Config) -> Option<PathBuf> {
    cfg.models_dir()
}

/// Where a recommendation's file is installed: `<models_dir>\<repo with '/' -> "--">\<file>`, where repo is the
/// file's own `repo` (a listed file with one), else the entry's `hf_repo`. Shared by the catalog (`installed`,
/// presets made from it) and the downloader, so all agree. None when models_dir is unset or the repo / file name
/// is not a plain relative path.
pub fn install_path(cfg: &Config, rec: &Recommendation, file: &str) -> Option<PathBuf> {
    let dir = models_dir(cfg)?;
    Some(dir.join(install_rel(rec, file)?))
}

/// The install path below models_dir (see [`install_path`]).
pub(crate) fn install_rel(rec: &Recommendation, file: &str) -> Option<PathBuf> {
    let repo = rec.file(file).map(|f| rec.repo_of(f)).unwrap_or(&rec.hf_repo);
    if !valid_repo(repo) {
        return None;
    }
    let segs = file_segments(file)?;
    let mut p = PathBuf::from(repo.replace('/', "--"));
    for s in segs {
        p.push(s);
    }
    Some(p)
}
