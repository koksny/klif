//! A small Prometheus text-format parser (`name{label="v",...} value [timestamp]`) for llama.cpp `--metrics`,
//! vLLM `/metrics` and any server that answers Prometheus text. No dependencies. Owner: package D.
//!
//! Exposition format 0.0.4: `# HELP` / `# TYPE` comments and blank lines are skipped; label values may escape
//! `\\`, `\"` and `\n`; values may be `NaN`, `+Inf`, `-Inf`; an optional integer timestamp follows the value.
//! Malformed lines are skipped (never an error: a /metrics body is untrusted input).

/// One sample line.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Metric name, e.g. "vllm:generation_tokens_total" or "llamacpp:prompt_tokens_total".
    pub name: String,
    pub labels: Vec<(String, String)>,
    pub value: f64,
}

impl Sample {
    /// A label's value.
    pub fn label(&self, name: &str) -> Option<&str> {
        self.labels.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// At most this many samples are kept from one body (a pathological exporter cannot exhaust memory).
const MAX_SAMPLES: usize = 20_000;

/// Every sample of a /metrics body (comments and malformed lines skipped).
pub fn parse(text: &str) -> Vec<Sample> {
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(s) = parse_line(l) {
            out.push(s);
            if out.len() >= MAX_SAMPLES {
                break;
            }
        }
    }
    out
}

/// True when `text` looks like Prometheus exposition (at least one well-formed sample, and no HTML/JSON).
pub fn looks_like(text: &str) -> bool {
    let t = text.trim_start();
    if t.starts_with('<') || t.starts_with('{') || t.starts_with('[') {
        return false;
    }
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).take(8).any(|l| parse_line(l).is_some())
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == ':'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == ':'
}

fn parse_line(l: &str) -> Option<Sample> {
    let mut chars = l.char_indices().peekable();
    let (_, c0) = *chars.peek()?;
    if !is_name_start(c0) {
        return None;
    }
    let mut name_end = l.len();
    for (i, c) in l.char_indices() {
        if !is_name_char(c) {
            name_end = i;
            break;
        }
    }
    let name = &l[..name_end];
    let mut rest = &l[name_end..];
    let mut labels = Vec::new();
    if let Some(r) = rest.strip_prefix('{') {
        let (ls, after) = parse_labels(r)?;
        labels = ls;
        rest = after;
    }
    let mut it = rest.split_whitespace();
    let value = parse_value(it.next()?)?;
    // Optional timestamp (ignored), nothing after it.
    if let Some(ts) = it.next() {
        ts.parse::<i64>().ok()?;
    }
    if it.next().is_some() {
        return None;
    }
    Some(Sample { name: name.to_string(), labels, value })
}

/// Labels after the opening `{`; returns them and the text after the closing `}`.
fn parse_labels(s: &str) -> Option<(Vec<(String, String)>, &str)> {
    let mut labels = Vec::new();
    let b = s.as_bytes();
    let mut i = 0usize;
    loop {
        while i < b.len() && (b[i] == b' ' || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'}' {
            return Some((labels, &s[i + 1..]));
        }
        let k0 = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        if i == k0 {
            return None;
        }
        let key = &s[k0..i];
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        if b.get(i) != Some(&b'=') {
            return None;
        }
        i += 1;
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        if b.get(i) != Some(&b'"') {
            return None;
        }
        i += 1;
        let mut val = String::new();
        let mut closed = false;
        let mut chars = s[i..].char_indices();
        while let Some((j, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some((_, 'n')) => val.push('\n'),
                    Some((_, other)) => val.push(other),
                    None => return None,
                },
                '"' => {
                    i += j + 1;
                    closed = true;
                    break;
                }
                _ => val.push(c),
            }
        }
        if !closed {
            return None;
        }
        labels.push((key.to_string(), val));
    }
}

fn parse_value(v: &str) -> Option<f64> {
    match v {
        "NaN" => Some(f64::NAN),
        "+Inf" | "Inf" => Some(f64::INFINITY),
        "-Inf" => Some(f64::NEG_INFINITY),
        _ => v.parse::<f64>().ok(),
    }
}

/// Sum of every sample named `name` (all label sets), None when absent. Non-finite values are skipped.
pub fn sum(samples: &[Sample], name: &str) -> Option<f64> {
    let mut found = false;
    let mut t = 0.0;
    for s in samples.iter().filter(|s| s.name == name) {
        found = true;
        if s.value.is_finite() {
            t += s.value;
        }
    }
    if found { Some(t) } else { None }
}

/// The first of `names` that is present, summed over its label sets.
pub fn first_sum(samples: &[Sample], names: &[&str]) -> Option<f64> {
    names.iter().find_map(|n| sum(samples, n))
}
