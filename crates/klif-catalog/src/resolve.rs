//! One preset, resolved for a System: param selection, placeholders (SPEC 2.3 / 2.4), program and working
//! folder, environment (managed rows, API key, user env, removals), effective port and host, facts and the
//! validation issues. `CommandView`, `LaunchPlan`, `ModelRef`, availability and `preset_hash` all come from this
//! one code path, so what KLIF shows is what it runs.

use crate::facts::{self, ArgFacts};
use crate::probe::FileProbe;
use klif_common::config::{ApiKeySource, Config, ParamChoice, PresetCfg, SystemCfg};
use klif_common::launch::EnvVal;
use klif_common::secret::{args_have_secret, is_secret_env, mask_args};
use klif_common::vm::{AdapterId, EnvView, HealthCheck, Issue, SystemKind};
use klif_common::Secret;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// llama.cpp managed env (SPEC 6): verbosity 4 prints the buffer lines the VRAM composition needs; prefix and
/// timestamps are what the log parser reads (llama.cpp turns them on by default; KLIF pins them).
pub(crate) const LLAMA_VERBOSITY: &str = "LLAMA_ARG_LOG_VERBOSITY";
pub(crate) const LLAMA_PREFIX: &str = "LLAMA_ARG_LOG_PREFIX";
pub(crate) const LLAMA_TIMESTAMPS: &str = "LLAMA_ARG_LOG_TIMESTAMPS";
/// Always removed from the inherited environment (a key in KLIF's own env must never leak into a child).
pub(crate) const ALWAYS_REMOVED: [&str; 2] = ["LLAMA_API_KEY", "VLLM_API_KEY"];
/// llama-server's own default port (when the args never pass one).
const LLAMA_SERVER_OWN_PORT: u16 = 8080;

/// What the resolution knows about the API key.
#[derive(Clone, Copy)]
pub(crate) enum KeyInfo<'a> {
    /// Hash / port-only use: no key rows, no key rules.
    Unknown,
    /// Display: whether a key is configured.
    Set(bool),
    /// Plan: the key itself.
    Value(Option<&'a Secret>),
}

impl KeyInfo<'_> {
    fn known(&self) -> Option<bool> {
        match self {
            KeyInfo::Unknown => None,
            KeyInfo::Set(b) => Some(*b),
            KeyInfo::Value(v) => Some(v.is_some()),
        }
    }
}

/// Which availability an error leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cat {
    Other,
    Exe,
    Model,
}

#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub issue: Issue,
    pub cat: Cat,
}

/// The System a preset is resolved for.
#[derive(Clone, Copy)]
pub(crate) struct ForSystem<'a> {
    pub label: &'a str,
    pub sys: &'a SystemCfg,
}

pub(crate) struct Req<'a> {
    pub cfg: &'a Config,
    pub spec: &'a PresetCfg,
    /// The System's param selection (empty = every default).
    pub params: &'a BTreeMap<String, String>,
    pub system: Option<ForSystem<'a>>,
    pub key: KeyInfo<'a>,
    pub probe: &'a FileProbe,
    /// File existence checks (exe, model, mmproj, cwd, path args).
    pub checks: bool,
    /// The session stamp `{stamp}` expands to (a plan). None: `{stamp}` stays as written (previews, hashes).
    pub stamp: Option<&'a str>,
}

/// An external server's endpoint.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Endpoint {
    /// Normalised URL without a trailing '/'.
    pub url: String,
    pub host: String,
    pub port: u16,
    /// The path prefix as written, without a trailing '/' ("" or e.g. "/v1").
    pub path: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Resolved {
    pub adapter: AdapterId,
    pub kind: Option<SystemKind>,
    pub external: Option<Endpoint>,
    pub program: Option<PathBuf>,
    pub program_display: String,
    pub args: Vec<String>,
    pub args_display: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub cwd_display: String,
    pub env_set: Vec<(String, EnvVal)>,
    pub env_remove: Vec<String>,
    pub env_view: Vec<EnvView>,
    /// For the hash: (name, shown value or "<secret>").
    pub hash_env: Vec<(String, String)>,
    pub hash_remove: Vec<String>,
    pub port: Option<u16>,
    /// The host the server binds (as configured; may be a wildcard).
    pub host: String,
    pub health: HealthCheck,
    pub facts: ArgFacts,
    /// `model` expanded (as written in the preset).
    pub model_path: Option<PathBuf>,
    pub mmproj_path: Option<PathBuf>,
    pub issues: Vec<Found>,
}

impl Resolved {
    pub fn errors(&self) -> impl Iterator<Item = &Found> {
        self.issues.iter().filter(|f| f.issue.is_error())
    }

    pub fn issue_list(&self) -> Vec<Issue> {
        self.issues.iter().map(|f| f.issue.clone()).collect()
    }

    /// The model file the server loads (args first, else the preset's `model`, relative to the server's working
    /// folder), for sizes and the quant.
    pub fn loaded_model(&self) -> Option<PathBuf> {
        self.facts
            .model
            .as_deref()
            .filter(|m| looks_absolute(m))
            .map(PathBuf::from)
            .or_else(|| self.model_path.as_ref().map(|p| self.abs(&p.to_string_lossy())))
    }

    /// A path the server reads, as the server sees it: relative paths are relative to its working folder.
    pub fn abs(&self, p: &str) -> PathBuf {
        abs_for(p, self.cwd.as_deref())
    }
}

fn err(out: &mut Vec<Found>, cat: Cat, field: &str, text: impl Into<String>) {
    out.push(Found { issue: Issue::error(Some(field), text), cat });
}

fn warn(out: &mut Vec<Found>, field: &str, text: impl Into<String>) {
    out.push(Found { issue: Issue::warn(Some(field), text), cat: Cat::Other });
}

/// "an LLM", "an Image", "a Speech (TTS)"...
pub(crate) fn with_article(label: &str) -> String {
    let vowel = label.chars().next().is_some_and(|c| "AEIOUaeiou".contains(c)) || label.starts_with("LLM");
    format!("{} {label}", if vowel { "an" } else { "a" })
}

// ----------------------------------------------------------------------------------------- placeholders

/// What a placeholder may refer to where it appears.
#[derive(Clone, Copy)]
struct Ctx {
    /// `{p.NAME.VAR}` allowed (not inside a var value or a choice's args/env).
    params: bool,
    /// `{model}`, `{mmproj}`, `{port}`, `{host}`, `{ctx}` allowed (not inside model / mmproj).
    refs: bool,
}

const FULL: Ctx = Ctx { params: true, refs: true };
const PATH: Ctx = Ctx { params: true, refs: false };

#[derive(Clone)]
enum NetVal {
    /// Pass 1: leave `{port}` / `{host}` as they are (the literal rules read the args first).
    Keep,
    Val(String),
    Missing,
}

type Sels<'a> = BTreeMap<String, Option<(String, &'a ParamChoice)>>;

struct Exp<'a> {
    cfg: &'a Config,
    stamp: Option<&'a str>,
    ctx_tokens: Option<u32>,
    sels: &'a Sels<'a>,
    model: Option<(String, String)>,
    mmproj: Option<(String, String)>,
    port: NetVal,
    host: NetVal,
}

fn is_placeholder_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-' | '(' | ')'))
}

fn valid_env_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '(' | ')'))
}

impl Exp<'_> {
    /// Expand every placeholder of `s`: (real value, display value with `{env:X}` as `%X%`).
    fn expand(&self, s: &str, field: &str, ctx: Ctx, out: &mut Vec<Found>) -> (String, String) {
        let mut real = String::with_capacity(s.len());
        let mut disp = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            real.push_str(&rest[..i]);
            disp.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            match after.find('}') {
                Some(j) if is_placeholder_name(&after[..j]) => {
                    let (r, d) = self.one(&after[..j], field, ctx, out);
                    real.push_str(&r);
                    disp.push_str(&d);
                    rest = &after[j + 1..];
                }
                _ => {
                    real.push('{');
                    disp.push('{');
                    rest = after;
                }
            }
        }
        real.push_str(rest);
        disp.push_str(rest);
        (real, disp)
    }

    fn one(&self, name: &str, field: &str, ctx: Ctx, out: &mut Vec<Found>) -> (String, String) {
        let lit = || (format!("{{{name}}}"), format!("{{{name}}}"));
        let empty = || (String::new(), String::new());
        let both = |s: String| (s.clone(), s);
        match name {
            "model" | "mmproj" | "port" | "host" | "ctx" if !ctx.refs => {
                err(out, Cat::Other, field, format!("{{{name}}} cannot be used in {field}."));
                lit()
            }
            "model" | "mmproj" => {
                let v = if name == "model" { &self.model } else { &self.mmproj };
                match v {
                    Some((r, d)) => (r.clone(), d.clone()),
                    None => {
                        err(out, Cat::Other, field, format!("{{{name}}} is used, but the preset has no {name}."));
                        empty()
                    }
                }
            }
            "port" | "host" => match if name == "port" { &self.port } else { &self.host } {
                NetVal::Keep => lit(),
                NetVal::Val(v) => both(v.clone()),
                NetVal::Missing => {
                    err(out, Cat::Other, field, format!("{{{name}}} is used, but no {name} is known for this preset."));
                    empty()
                }
            },
            "ctx" => match self.ctx_tokens {
                Some(c) => both(c.to_string()),
                None => {
                    err(out, Cat::Other, field, "{ctx} is used, but the preset has no ctx.");
                    empty()
                }
            },
            "models_dir" => match self.cfg.models_dir() {
                Some(p) => both(p.to_string_lossy().into_owned()),
                None => {
                    err(out, Cat::Other, field, "{models_dir} is used, but [paths] models_dir is not set.");
                    empty()
                }
            },
            // The session stamp (as in the session name and its log names). Shown and hashed as written, so a
            // session's command keeps the hash of the System's command.
            "stamp" => (self.stamp.unwrap_or("{stamp}").to_string(), "{stamp}".to_string()),
            "state_dir" => both(self.cfg.state_dir.to_string_lossy().into_owned()),
            "data_dir" => both(self.cfg.data_dir.to_string_lossy().into_owned()),
            _ if name.starts_with("env:") => {
                let var = &name[4..];
                if !valid_env_name(var) {
                    err(out, Cat::Other, field, format!("{{{name}}}: \"{var}\" is not an environment variable name."));
                    return lit();
                }
                match std::env::var_os(var) {
                    Some(v) => (v.to_string_lossy().into_owned(), format!("%{var}%")),
                    None => {
                        warn(out, field, format!("{var} is not set in KLIF's environment, so {{env:{var}}} expands to nothing."));
                        (String::new(), format!("%{var}%"))
                    }
                }
            }
            _ if name.starts_with("p.") => {
                let rest = &name[2..];
                let Some((pname, var)) = rest.split_once('.') else {
                    err(
                        out,
                        Cat::Other,
                        field,
                        format!("{{{name}}} must be a whole argument on its own (it expands to 0 or more arguments); use {{p.{rest}.VAR}} inside text."),
                    );
                    return lit();
                };
                if !ctx.params {
                    err(out, Cat::Other, field, format!("{{{name}}}: a param value cannot refer to a param."));
                    return lit();
                }
                match self.sels.get(pname) {
                    None => {
                        err(out, Cat::Other, field, format!("{{{name}}}: the preset has no param \"{pname}\"."));
                        empty()
                    }
                    Some(None) => empty(),
                    Some(Some((key, choice))) => match choice.vars.get(var) {
                        Some(v) => self.expand(v, field, Ctx { params: false, refs: ctx.refs }, out),
                        None => {
                            err(out, Cat::Other, field, format!("{{{name}}}: choice \"{key}\" of param \"{pname}\" has no var \"{var}\"."));
                            empty()
                        }
                    },
                }
            }
            _ => {
                err(out, Cat::Other, field, format!("Unknown placeholder {{{name}}}."));
                lit()
            }
        }
    }

    /// The args: `{p.NAME}` tokens become the selected choice's args. (real, display) per token.
    fn args(&self, spec: &PresetCfg, out: &mut Vec<Found>) -> (Vec<String>, Vec<String>) {
        let mut real = Vec::with_capacity(spec.args.len());
        let mut disp = Vec::with_capacity(spec.args.len());
        for tok in &spec.args {
            let t = tok.trim();
            if let Some(pname) = t.strip_prefix("{p.").and_then(|r| r.strip_suffix('}')).filter(|n| !n.contains('.') && !n.is_empty()) {
                match self.sels.get(pname) {
                    None => err(out, Cat::Other, "args", format!("{t}: the preset has no param \"{pname}\".")),
                    Some(None) => {}
                    Some(Some((_, choice))) => {
                        for a in &choice.args {
                            let (r, d) = self.expand(a, &format!("params.{pname}"), Ctx { params: false, refs: true }, out);
                            real.push(r);
                            disp.push(d);
                        }
                    }
                }
                continue;
            }
            let (r, d) = self.expand(tok, "args", FULL, out);
            real.push(r);
            disp.push(d);
        }
        (real, disp)
    }

    /// The user env: the preset's env, then each selected choice's env (choice wins per name).
    /// (name, real, display) in that order.
    fn env(&self, spec: &PresetCfg, out: &mut Vec<Found>) -> Vec<(String, String, String)> {
        let mut rows: Vec<(String, String, String)> = Vec::new();
        let put = |rows: &mut Vec<(String, String, String)>, name: &str, r: String, d: String| match rows
            .iter_mut()
            .find(|(n, _, _)| env_eq(n, name))
        {
            Some(row) => *row = (name.to_string(), r, d),
            None => rows.push((name.to_string(), r, d)),
        };
        for (k, v) in &spec.env {
            let field = format!("env.{k}");
            if !env_name_ok(k) {
                err(out, Cat::Other, &field, format!("\"{k}\" is not a valid environment variable name."));
                continue;
            }
            let (r, d) = self.expand(v, &field, FULL, out);
            put(&mut rows, k, r, d);
        }
        for (pname, sel) in self.sels {
            let Some((_, choice)) = sel else { continue };
            for (k, v) in &choice.env {
                let field = format!("params.{pname}");
                if !env_name_ok(k) {
                    err(out, Cat::Other, &field, format!("\"{k}\" is not a valid environment variable name."));
                    continue;
                }
                let (r, d) = self.expand(v, &field, Ctx { params: false, refs: true }, out);
                put(&mut rows, k, r, d);
            }
        }
        rows
    }
}

fn env_name_ok(k: &str) -> bool {
    !k.trim().is_empty() && !k.contains('=') && !k.contains('\0')
}

/// Environment names compare case-insensitively on Windows.
pub(crate) fn env_eq(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

// ------------------------------------------------------------------------------------------- hosts

/// 127.x, ::1, localhost.
pub(crate) fn is_loopback(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    h == "localhost" || h == "::1" || h.starts_with("127.")
}

/// 0.0.0.0, ::, empty or "*".
pub(crate) fn is_wildcard(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    h.is_empty() || h == "0.0.0.0" || h == "::" || h == "*"
}

/// The address to connect to for a bind host: wildcards become loopback.
pub(crate) fn connect_host(host: &str) -> String {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    if h == "::" {
        "::1".into()
    } else if is_wildcard(h) {
        "127.0.0.1".into()
    } else {
        h.to_string()
    }
}

/// `http://host:port` with IPv6 brackets.
pub(crate) fn base_url(host: &str, port: u16) -> String {
    let h = connect_host(host);
    if h.contains(':') {
        format!("http://[{h}]:{port}")
    } else {
        format!("http://{h}:{port}")
    }
}

/// Two listeners clash: same port, and the same host (loopback names count as one) or either is a wildcard.
pub fn ports_overlap(a: (&str, u16), b: (&str, u16)) -> bool {
    if a.1 != b.1 || a.1 == 0 {
        return false;
    }
    let (ha, hb) = (a.0.trim().trim_start_matches('[').trim_end_matches(']'), b.0.trim().trim_start_matches('[').trim_end_matches(']'));
    is_wildcard(ha) || is_wildcard(hb) || ha.eq_ignore_ascii_case(hb) || (is_loopback(ha) && is_loopback(hb))
}

/// A local listener clashes with an external server's endpoint: the same port, and the endpoint is on this
/// machine (loopback) or names the very host the listener binds (a wildcard bind does not reach a server on
/// another machine). Shared by the catalog's port warning and the engine's SPEC 16.7 check.
pub fn external_port_clash(local: (&str, u16), ext: (&str, u16)) -> bool {
    local.1 == ext.1
        && ports_overlap(local, ext)
        && (is_loopback(ext.0) || ext.0.trim().trim_start_matches('[').trim_end_matches(']').eq_ignore_ascii_case(local.0.trim().trim_start_matches('[').trim_end_matches(']')))
}

/// Parse an external preset's endpoint (`http://192.0.2.20:11434`). Err = one sentence.
pub(crate) fn parse_endpoint(s: &str) -> Result<Endpoint, String> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    let (scheme, rest) = if lower.starts_with("http://") {
        ("http", &t[7..])
    } else if lower.starts_with("https://") {
        return Err(format!(
            "endpoint \"{t}\": https:// endpoints are not supported yet; KLIF probes external servers over plain http://. Use http://host:port."
        ));
    } else {
        return Err(format!("endpoint \"{t}\" must be a URL such as http://192.0.2.20:11434."));
    };
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return Err("endpoint must not contain a user name or password.".into());
    }
    let (host, port) = if let Some(r) = authority.strip_prefix('[') {
        let (h, tail) = r.split_once(']').ok_or_else(|| format!("endpoint \"{t}\" has an unclosed IPv6 address."))?;
        (h.to_string(), tail.strip_prefix(':'))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        }
    };
    if host.trim().is_empty() {
        return Err(format!("endpoint \"{t}\" has no host."));
    }
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| format!("endpoint \"{t}\" has an invalid port."))?,
        None => 80,
    };
    let path = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    let shown_host = if host.contains(':') { format!("[{host}]") } else { host.clone() };
    Ok(Endpoint { url: format!("{scheme}://{shown_host}:{port}{path}"), host, port, path: path.to_string() })
}

// ----------------------------------------------------------------------------------------- programs

enum ProgErr {
    Empty,
    Batch,
    Script(String),
    Relative,
    NotFound,
}

/// Looks like an absolute path on this platform.
pub(crate) fn looks_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    if cfg!(windows) {
        (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || s.starts_with("\\\\")
    } else {
        s.starts_with('/')
    }
}

fn path_dirs(value: &str) -> Vec<PathBuf> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    value.split(sep).map(|d| d.trim().trim_matches('"')).filter(|d| !d.is_empty() && looks_absolute(d)).map(PathBuf::from).collect()
}

/// Absolute as written (`.exe`/`.com` on Windows), else a bare name searched on the child's PATH, then KLIF's PATH.
fn resolve_program(raw: &str, child_path: Option<&str>, probe: &FileProbe) -> Result<PathBuf, ProgErr> {
    let mut cmd = raw.trim();
    if cmd.len() >= 2 && cmd.starts_with('"') && cmd.ends_with('"') {
        cmd = cmd[1..cmd.len() - 1].trim();
    }
    if cmd.is_empty() {
        return Err(ProgErr::Empty);
    }
    let p = Path::new(cmd);
    let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    if matches!(ext.as_deref(), Some("bat" | "cmd")) {
        return Err(ProgErr::Batch);
    }
    const SCRIPTS: &[&str] = &["ps1", "psm1", "py", "pyw", "sh", "js", "vbs", "wsf", "lnk"];
    if cfg!(windows) {
        if let Some(e) = ext.as_deref().filter(|e| SCRIPTS.contains(e)) {
            return Err(ProgErr::Script(e.to_string()));
        }
    }
    let bare = !cmd.contains(['/', '\\']) && !cmd.contains(':');
    if !bare {
        if !p.is_absolute() {
            return Err(ProgErr::Relative);
        }
        if cfg!(windows) && !matches!(ext.as_deref(), Some("exe" | "com")) {
            // CreateProcess needs the real file name: try the program extensions.
            for e in ["exe", "com"] {
                let c = PathBuf::from(format!("{cmd}.{e}"));
                if probe.is_file(&c) {
                    return Ok(c);
                }
            }
            return Err(ProgErr::NotFound);
        }
        return if probe.is_file(p) { Ok(p.to_path_buf()) } else { Err(ProgErr::NotFound) };
    }
    let names: Vec<String> = if cfg!(windows) && !matches!(ext.as_deref(), Some("exe" | "com")) {
        vec![format!("{cmd}.exe"), format!("{cmd}.com")]
    } else {
        vec![cmd.to_string()]
    };
    let mut dirs: Vec<PathBuf> = child_path.map(path_dirs).unwrap_or_default();
    if let Some(own) = std::env::var_os("PATH") {
        for d in path_dirs(&own.to_string_lossy()) {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    }
    for d in &dirs {
        for n in &names {
            let c = d.join(n);
            if probe.is_file(&c) {
                return Ok(c);
            }
        }
    }
    if cfg!(windows) && ext.is_none() {
        for d in &dirs {
            for e in ["bat", "cmd"] {
                if probe.is_file(&d.join(format!("{cmd}.{e}"))) {
                    return Err(ProgErr::Batch);
                }
            }
        }
    }
    Err(ProgErr::NotFound)
}

// -------------------------------------------------------------------------------------------- resolve

/// Resolve `req.spec` (see the module docs). Never fails; problems are in `issues`.
pub(crate) fn resolve(req: &Req) -> Resolved {
    let cfg = req.cfg;
    let spec = req.spec;
    let mut out: Vec<Found> = Vec::new();
    let mut r = Resolved { adapter: spec.adapter, kind: spec.effective_kind(), ..Resolved::default() };

    // ---- kind -----------------------------------------------------------------------------------
    match r.kind {
        None => err(&mut out, Cat::Other, "kind", "A generic preset needs kind = \"llm\", \"image\", \"tts\", \"stt\" or \"video\"."),
        Some(k) => {
            if let Some(fs) = req.system {
                if fs.sys.kind != k {
                    err(
                        &mut out,
                        Cat::Other,
                        "kind",
                        format!(
                            "This is {} preset, but {} is {} System.",
                            with_article(k.label()),
                            fs.label,
                            with_article(fs.sys.kind.label())
                        ),
                    );
                }
            }
        }
    }

    // ---- health ---------------------------------------------------------------------------------
    r.health = match HealthCheck::from_preset(spec.health.as_deref()) {
        Ok(HealthCheck::Auto) if spec.adapter == AdapterId::Generic => HealthCheck::Tcp,
        Ok(h) => h,
        Err(e) => {
            err(&mut out, Cat::Other, "health", format!("{}.", e.trim_end_matches('.')));
            if spec.adapter == AdapterId::Generic {
                HealthCheck::Tcp
            } else {
                HealthCheck::Auto
            }
        }
    };

    // ---- external -------------------------------------------------------------------------------
    if spec.is_external() {
        let ep = spec.endpoint.as_deref().unwrap_or_default();
        match parse_endpoint(ep) {
            Ok(e) => {
                if let Some(p) = spec.port.filter(|p| *p != e.port) {
                    warn(&mut out, "port", format!("port = {p} is ignored: an external server's address comes from endpoint ({}).", e.url));
                }
                if let Some(h) = spec.host.as_deref().map(str::trim).filter(|h| !h.is_empty() && !h.eq_ignore_ascii_case(&e.host)) {
                    warn(
                        &mut out,
                        "host",
                        format!("host = \"{h}\" is ignored: an external server's address comes from endpoint ({}).", e.url),
                    );
                }
                // The API base of an OpenAI-style server may end in /v1; any other prefix is not where KLIF looks.
                if !e.path.is_empty() && !e.path.eq_ignore_ascii_case("/v1") {
                    warn(
                        &mut out,
                        "endpoint",
                        format!(
                            "The endpoint path \"{}\" is not probed: KLIF checks health at the server root (http://{}:{}/...).",
                            e.path,
                            if e.host.contains(':') { format!("[{}]", e.host) } else { e.host.clone() },
                            e.port
                        ),
                    );
                }
                r.port = Some(e.port);
                r.host = e.host.clone();
                r.external = Some(e);
            }
            Err(e) => err(&mut out, Cat::Other, "endpoint", e),
        }
        if !spec.command.trim().is_empty() {
            err(
                &mut out,
                Cat::Other,
                "command",
                "An external preset has no command: KLIF only watches the server at its endpoint. Clear command or endpoint.",
            );
        }
        if !spec.args.is_empty() {
            warn(&mut out, "args", "args are ignored for an external server (KLIF never starts it).");
        }
        r.facts = facts::from_args(spec.adapter, &[], &BTreeMap::new());
        if spec.adapter == AdapterId::Vllm {
            r.facts.metrics = true;
        }
        r.issues = out;
        return r;
    }

    // ---- params ---------------------------------------------------------------------------------
    let who = req.system.map(|s| s.label).unwrap_or("The System");
    let mut sels: Sels = BTreeMap::new();
    for (name, p) in &spec.params {
        let field = format!("params.{name}");
        if p.choices.is_empty() {
            err(&mut out, Cat::Other, &field, format!("Param \"{name}\" has no choices."));
            sels.insert(name.clone(), None);
            continue;
        }
        if let Some(d) = p.default.as_deref().filter(|d| !p.choices.contains_key(*d)) {
            warn(
                &mut out,
                &field,
                format!("Param \"{name}\": default \"{d}\" is not one of its choices; the first choice is the default."),
            );
        }
        let default = p.default_choice().unwrap_or_default().to_string();
        let pick = match req.params.get(name) {
            Some(w) if p.choices.contains_key(w) => w.clone(),
            Some(w) => {
                warn(&mut out, &field, format!("{who} selects {name} = \"{w}\", which is not a choice; KLIF uses \"{default}\"."));
                default
            }
            None => default,
        };
        let choice = &p.choices[&pick];
        sels.insert(name.clone(), Some((pick, choice)));
    }

    // ---- model / mmproj -------------------------------------------------------------------------
    let mut exp =
        Exp { cfg, stamp: req.stamp, ctx_tokens: spec.ctx, sels: &sels, model: None, mmproj: None, port: NetVal::Keep, host: NetVal::Keep };
    let path_field = |v: &Option<String>, field: &str, exp: &Exp, out: &mut Vec<Found>| {
        v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(|s| exp.expand(s, field, PATH, out))
    };
    let model = path_field(&spec.model, "model", &exp, &mut out);
    let mmproj = path_field(&spec.mmproj, "mmproj", &exp, &mut out);
    exp.model = model.clone();
    exp.mmproj = mmproj.clone();

    // ---- pass 1: literals with {port} / {host} unexpanded --------------------------------------
    let mut scratch = Vec::new();
    let (args1, _) = exp.args(spec, &mut scratch);
    let env1: BTreeMap<String, String> = exp.env(spec, &mut scratch).into_iter().map(|(n, r, _)| (n, r)).collect();
    let facts1 = facts::from_args(spec.adapter, &args1, &env1);

    // ---- port / host ----------------------------------------------------------------------------
    let port_flag = match spec.adapter {
        AdapterId::SdCpp => "--listen-port",
        _ => "--port",
    };
    let host_flag = match spec.adapter {
        AdapterId::SdCpp => "--listen-ip",
        _ => "--host",
    };
    let literal_port = match facts1.port_text.as_deref().map(str::trim) {
        Some(t) if t.contains('{') => None,
        Some(t) => match t.parse::<u16>() {
            Ok(p) if p > 0 => Some(p),
            _ => {
                err(&mut out, Cat::Other, "args", format!("{port_flag} \"{t}\" is not a port number (1-65535)."));
                None
            }
        },
        None => None,
    };
    r.port = match (spec.port, literal_port) {
        (Some(0), _) => {
            err(&mut out, Cat::Other, "port", "port must be 1-65535.");
            None
        }
        (Some(p), Some(l)) if p != l => {
            err(
                &mut out,
                Cat::Other,
                "port",
                format!(
                    "port = {p}, but the args say {port_flag} {l}. Keep one: write \"{port_flag}\", \"{{port}}\" in args, or remove port."
                ),
            );
            Some(p)
        }
        (Some(p), _) => Some(p),
        (None, Some(l)) => Some(l),
        (None, None) => spec.adapter.default_port(),
    };
    if r.port.is_none() && spec.port != Some(0) {
        if spec.health.as_deref().is_none_or(|h| h.trim().is_empty()) {
            err(&mut out, Cat::Other, "port", "A generic preset needs port = <number> (or a health check), so KLIF knows when it is up.");
        } else {
            let h = spec.health.as_deref().unwrap_or_default().trim();
            warn(
                &mut out,
                "port",
                format!("No port is known, so KLIF treats the server as online while its process runs; health = \"{h}\" is ignored."),
            );
        }
    }
    let literal_host = facts1.host.clone();
    let spec_host = spec.host.as_deref().map(str::trim).filter(|h| !h.is_empty()).map(str::to_string);
    if let (Some(h), Some(l)) = (&spec_host, &literal_host) {
        if !h.eq_ignore_ascii_case(l) {
            err(
                &mut out,
                Cat::Other,
                "host",
                format!("host = \"{h}\", but the args say {host_flag} {l}. Keep one: write \"{host_flag}\", \"{{host}}\" in args, or remove host."),
            );
        }
    }
    r.host = spec_host.or(literal_host).unwrap_or_else(|| cfg.net.host_for(r.kind.unwrap_or(SystemKind::Llm)).to_string());

    // ---- pass 2: final values -------------------------------------------------------------------
    exp.port = match r.port {
        Some(p) => NetVal::Val(p.to_string()),
        None => NetVal::Missing,
    };
    exp.host = NetVal::Val(r.host.clone());
    let (args, args_disp) = exp.args(spec, &mut out);
    let user_env = exp.env(spec, &mut out);
    let user_env_real: BTreeMap<String, String> = user_env.iter().map(|(n, r, _)| (n.clone(), r.clone())).collect();
    let (cmd_real, cmd_disp) = exp.expand(&spec.command, "command", FULL, &mut out);
    let cwd_tpl = spec.cwd.as_deref().map(str::trim).filter(|c| !c.is_empty());
    let cwd_exp = cwd_tpl.map(|c| exp.expand(c, "cwd", FULL, &mut out));
    r.facts = facts::from_args(spec.adapter, &args, &user_env_real);
    r.model_path = model.as_ref().map(|(m, _)| PathBuf::from(m));
    r.mmproj_path = mmproj.as_ref().map(|(m, _)| PathBuf::from(m));

    // ---- program --------------------------------------------------------------------------------
    let child_path = user_env.iter().find(|(n, _, _)| env_eq(n, "PATH")).map(|(_, r, _)| r.as_str());
    let shown_cmd = cmd_disp.trim().to_string();
    // `{env:X}` (or `{stamp}`) in command: the view and the hash keep the written form (`%X%`, SPEC 2.3), so the
    // hash does not depend on KLIF's environment; the supervisor still gets the resolved program. (Every other
    // placeholder has one form.)
    let env_in_cmd = cmd_real != cmd_disp;
    match resolve_program(&cmd_real, child_path, req.probe) {
        Ok(p) => {
            r.program_display = if env_in_cmd { shown_cmd.trim_matches('"').trim().to_string() } else { p.to_string_lossy().into_owned() };
            r.program = Some(p);
        }
        Err(e) => {
            r.program_display = shown_cmd.clone();
            match e {
                ProgErr::Empty => {
                    err(&mut out, Cat::Other, "command", "No program is set: command is empty (an external server needs endpoint instead).")
                }
                ProgErr::Batch => err(
                    &mut out,
                    Cat::Other,
                    "command",
                    format!("{shown_cmd} is a batch file; KLIF starts only programs. Use command = \"cmd.exe\" with args [\"/c\", \"<the .bat file>\", ...]."),
                ),
                ProgErr::Script(ext) => err(
                    &mut out,
                    Cat::Other,
                    "command",
                    format!("{shown_cmd} is a .{ext} script; Windows starts only .exe/.com programs directly. Set command to its interpreter and put the script in args."),
                ),
                ProgErr::Relative => err(
                    &mut out,
                    Cat::Other,
                    "command",
                    format!("command \"{shown_cmd}\" must be an absolute path or a program name found on PATH."),
                ),
                ProgErr::NotFound => {
                    if req.checks {
                        let how = if looks_absolute(cmd_real.trim().trim_matches('"')) { "does not exist" } else { "was not found on PATH" };
                        err(&mut out, Cat::Exe, "command", format!("The program {shown_cmd} {how}."));
                    }
                }
            }
        }
    }
    if spec.adapter == AdapterId::Vllm {
        let stem = r.program.as_deref().unwrap_or(Path::new(cmd_real.trim())).file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase());
        if stem.as_deref() == Some("wsl") {
            warn(
                &mut out,
                "command",
                "vLLM through wsl.exe: Stop ends wsl.exe only; the Linux server may keep running (and keep its VRAM). Prefer running vLLM on its own machine as an external server.",
            );
        }
    }

    // ---- cwd ------------------------------------------------------------------------------------
    match cwd_exp {
        Some((real, disp)) => {
            r.cwd_display = disp;
            let p = PathBuf::from(real.trim());
            if !p.is_absolute() {
                err(&mut out, Cat::Other, "cwd", format!("cwd \"{}\" must be an absolute folder.", r.cwd_display));
            } else {
                if req.checks && !req.probe.is_dir(&p) {
                    err(&mut out, Cat::Other, "cwd", format!("The working folder {} does not exist.", r.cwd_display));
                }
                r.cwd = Some(p);
            }
        }
        None => {
            let parent =
                r.program.as_deref().and_then(Path::parent).map(Path::to_path_buf).or_else(|| {
                    looks_absolute(cmd_real.trim()).then(|| Path::new(cmd_real.trim()).parent().map(Path::to_path_buf)).flatten()
                });
            // With `{env:X}` in command, the shown folder is the folder of the command as written (`%X%\bin`).
            let shown_dir = env_in_cmd
                .then(|| shown_cmd.trim_matches('"').trim().rsplit_once(['\\', '/']).map(|(d, _)| d.to_string()))
                .flatten()
                .filter(|d| !d.is_empty());
            r.cwd_display = shown_dir.or_else(|| parent.as_deref().map(|p| p.to_string_lossy().into_owned())).unwrap_or_default();
            r.cwd = parent;
        }
    }

    // ---- environment ----------------------------------------------------------------------------
    let user_has = |name: &str| user_env.iter().any(|(n, _, _)| env_eq(n, name));
    let mut managed: Vec<(&str, &str)> = Vec::new();
    if spec.managed && spec.adapter == AdapterId::LlamaCpp {
        if cfg.telemetry.verbose_llama_logs {
            managed.push((LLAMA_VERBOSITY, "4"));
        }
        managed.push((LLAMA_PREFIX, "1"));
        managed.push((LLAMA_TIMESTAMPS, "1"));
    }
    let key_env = spec.adapter.api_key_env().filter(|_| spec.api_key);
    let mut env_set: Vec<(String, EnvVal)> = Vec::new();
    let mut view: Vec<EnvView> = Vec::new();
    let mut hash_env: Vec<(String, String)> = Vec::new();
    for (name, real, disp) in &user_env {
        let secret = is_secret_env(name);
        let val = match Secret::new(real.clone()) {
            Some(s) if secret && s.expose() == real => EnvVal::Secret(s),
            _ => EnvVal::Plain(real.clone()),
        };
        env_set.push((name.clone(), val));
        view.push(EnvView {
            name: name.clone(),
            value: (!secret).then(|| disp.clone()),
            secret,
            managed: false,
            removed: false,
            overridden: false,
        });
        hash_env.push((name.clone(), if secret { "<secret>".into() } else { disp.clone() }));
    }
    for (name, value) in &managed {
        let overridden = user_has(name);
        if !overridden {
            env_set.push((name.to_string(), EnvVal::Plain(value.to_string())));
            hash_env.push((name.to_string(), value.to_string()));
        }
        view.push(EnvView {
            name: name.to_string(),
            value: Some(value.to_string()),
            secret: false,
            managed: true,
            removed: false,
            overridden,
        });
    }
    let mut key_set_name: Option<&str> = None;
    if let Some(name) = key_env {
        let overridden = user_has(name);
        match req.key {
            KeyInfo::Value(Some(secret)) => {
                if !overridden {
                    env_set.push((name.to_string(), EnvVal::Secret(secret.clone())));
                }
                key_set_name = Some(name);
                view.push(EnvView { name: name.into(), value: None, secret: true, managed: true, removed: false, overridden });
            }
            KeyInfo::Set(true) => {
                key_set_name = Some(name);
                view.push(EnvView { name: name.into(), value: None, secret: true, managed: true, removed: false, overridden });
            }
            _ => {}
        }
    }
    let mut env_remove: Vec<String> = Vec::new();
    for name in ALWAYS_REMOVED {
        env_remove.push(name.to_string());
        if key_set_name != Some(name) && !user_has(name) {
            view.push(EnvView { name: name.into(), value: None, secret: false, managed: true, removed: true, overridden: false });
        }
    }
    let mut hash_remove = Vec::new();
    for name in &spec.env_remove {
        let n = name.trim();
        if !env_name_ok(n) {
            err(&mut out, Cat::Other, "env_remove", format!("\"{name}\" is not a valid environment variable name."));
            continue;
        }
        if env_remove.iter().any(|e| env_eq(e, n)) {
            continue;
        }
        env_remove.push(n.to_string());
        hash_remove.push(n.to_string());
        view.push(EnvView { name: n.into(), value: None, secret: false, managed: false, removed: true, overridden: false });
    }
    // api_key = false drops the key row from the child's env, so it changes the hash (Tune then shows "differs
    // from the running session"). The default (key on) adds nothing: existing hashes (bench records, measured VRAM
    // layers, persisted sessions) stay valid. The key itself and whether one is set never count.
    if spec.adapter.api_key_env().is_some() && !spec.api_key {
        hash_env.push(("<klif-api-key>".to_string(), "off".to_string()));
    }
    r.env_set = env_set;
    r.env_remove = env_remove;
    r.env_view = view;
    r.hash_env = hash_env;
    r.hash_remove = hash_remove;

    // ---- telemetry rules --------------------------------------------------------------------------
    if spec.adapter == AdapterId::LlamaCpp {
        for (field, text) in r.facts.log_breakers.clone() {
            warn(&mut out, &field, text);
        }
        if r.facts.port_text.is_none() && r.port.is_some_and(|p| p != LLAMA_SERVER_OWN_PORT) {
            warn(
                &mut out,
                "args",
                format!(
                    "The args never pass the port (\"--port\", \"{{port}}\"), so llama-server listens on its own default {LLAMA_SERVER_OWN_PORT} while KLIF watches {}.",
                    r.port.unwrap_or_default()
                ),
            );
        }
        if r.facts.host_text.is_none() && !is_loopback(&r.host) {
            warn(
                &mut out,
                "args",
                format!(
                    "The args never pass the host (\"--host\", \"{{host}}\"), so llama-server binds 127.0.0.1 while KLIF expects {}.",
                    r.host
                ),
            );
        }
        let mut inherited: Vec<String> = std::env::vars_os()
            .filter_map(|(k, _)| k.into_string().ok())
            .filter(|k| k.to_ascii_uppercase().starts_with("LLAMA_ARG_"))
            .filter(|k| !user_has(k) && !managed.iter().any(|(m, _)| env_eq(m, k)) && !r.env_remove.iter().any(|e| env_eq(e, k)))
            .collect();
        inherited.sort();
        if !inherited.is_empty() {
            warn(
                &mut out,
                "env",
                format!(
                    "KLIF's own environment sets {}; llama-server inherits it. Remove it there or add it to env_remove.",
                    inherited.join(", ")
                ),
            );
        }
    }
    if args_have_secret(&args) {
        warn(
            &mut out,
            "args",
            "A secret is written in args, where every local process can read it. Prefer api_key = true (KLIF's key) or an env variable.",
        );
    }

    // ---- network exposure -----------------------------------------------------------------------
    if let Some(key_known) = req.key.known() {
        if !is_loopback(&r.host) && cfg.security.api_key_source() != ApiKeySource::None {
            let own_key = key_env.is_some() && key_known;
            let user_key = spec.adapter.api_key_env().is_some_and(user_has)
                || args.iter().any(|a| klif_common::secret::is_secret_flag(a) || a.starts_with("--api-key="));
            match spec.adapter.api_key_env() {
                Some(_) if !own_key && !user_key => {
                    let why = if !spec.api_key { "api_key = false" } else { "no KLIF API key is set" };
                    err(
                        &mut out,
                        Cat::Other,
                        "host",
                        format!(
                            "host {} is reachable from the network, but {why}: anyone who can reach it can use the server. Set the KLIF API key (Tune → API key, or klif-cli key set), or bind to 127.0.0.1.",
                            r.host
                        ),
                    );
                }
                Some(_) => {}
                None => warn(
                    &mut out,
                    "host",
                    format!(
                        "host {} is reachable from the network and {} servers have no API key in KLIF: anyone who can reach it can use it.",
                        r.host, spec.adapter
                    ),
                ),
            }
        }
    }

    // ---- files --------------------------------------------------------------------------------------
    if req.checks {
        let vllm_repo = |m: &str| spec.adapter == AdapterId::Vllm && !looks_absolute(m);
        if let Some((m, d)) = &model {
            if !vllm_repo(m) && req.probe.model_bytes(&abs_for(m, r.cwd.as_deref())).is_none() {
                err(&mut out, Cat::Model, "model", format!("The model file {d} does not exist."));
            }
        } else if let Some(m) = r.facts.model.clone().filter(|m| looks_absolute(m)) {
            if req.probe.model_bytes(Path::new(&m)).is_none() {
                err(&mut out, Cat::Model, "args", format!("The model file {m} (in args) does not exist."));
            }
        }
        if let Some((m, d)) = &mmproj {
            if !req.probe.is_file(&abs_for(m, r.cwd.as_deref())) {
                err(&mut out, Cat::Model, "mmproj", format!("The mmproj file {d} does not exist."));
            }
        }
        let known: Vec<&str> =
            [model.as_ref(), mmproj.as_ref()].into_iter().flatten().map(|(m, _)| m.as_str()).chain(r.facts.model.as_deref()).collect();
        let mut prev_flag = String::new();
        for (i, a) in args.iter().enumerate() {
            let (flag, value) = match a.split_once('=') {
                Some((f, v)) if f.starts_with('-') => (f.to_string(), v),
                _ => (prev_flag.clone(), a.as_str()),
            };
            if looks_absolute(value) && !known.contains(&value) {
                let output = ["log", "save", "out", "dump", "cache", "tmp", "temp"].iter().any(|w| flag.to_ascii_lowercase().contains(w));
                if !output && !req.probe.exists(Path::new(value)) {
                    let shown = args_disp.get(i).map(String::as_str).unwrap_or(value);
                    let shown = shown.split_once('=').filter(|_| shown.starts_with('-')).map(|(_, v)| v).unwrap_or(shown);
                    warn(&mut out, "args", format!("{shown} (in args) does not exist."));
                }
            }
            prev_flag = if a.starts_with('-') && !a.contains('=') { a.clone() } else { String::new() };
        }
    }

    r.args = args;
    r.args_display = mask_args(&args_disp);
    r.issues = out;
    r
}

/// A relative model path is relative to the server's working folder.
pub(crate) fn abs_for(p: &str, cwd: Option<&Path>) -> PathBuf {
    let path = Path::new(p.trim());
    match cwd {
        Some(c) if !path.is_absolute() => c.join(path),
        _ => path.to_path_buf(),
    }
}
