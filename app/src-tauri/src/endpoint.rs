//! Where a System's clients connect, worked out from the address the view model publishes. Pure string work (no
//! URL crate): host fixes for wildcard bind addresses and a remote node's loopback address, and the `/v1` rule
//! (an LLM's client address ends in `/v1`, the address a browser opens does not).

/// A System's address in the two forms the shell needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// What "Open endpoint" opens: scheme, host, port, and the path without a trailing `/v1`.
    pub open: String,
    /// What "Copy endpoint" copies: for an LLM the OpenAI-compatible base URL (`.../v1`), otherwise the
    /// published address.
    pub copy: String,
}

/// `published`: the address as the engine reports it (`System.endpoint`, or `EngineHandle::endpoint_url`).
/// `llm`: the System serves an LLM. `node_host`: the host of the remote node the System lives on, if any;
/// a loopback or wildcard host in `published` is replaced by it, because the address is for clients on THIS
/// machine. A local System bound to a wildcard address (`0.0.0.0`, `[::]`) gets `127.0.0.1`.
pub fn normalize(published: &str, llm: bool, node_host: Option<&str>) -> Result<Endpoint, String> {
    let url = published.trim();
    let (scheme, rest) = if let Some(r) = url.strip_prefix("http://") {
        ("http://", r)
    } else if let Some(r) = url.strip_prefix("https://") {
        ("https://", r)
    } else {
        return Err("The endpoint is not a web address.".into());
    };
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err("The endpoint is not a usable web address.".into());
    }
    let (host, port) = split_host_port(authority);
    let host = fix_host(host, node_host);
    let authority = match port {
        Some(p) => format!("{host}:{p}"),
        None => host,
    };
    // Only the path matters from here: a query or fragment is not part of a base URL.
    let path = path.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    let root_path = path.strip_suffix("/v1").unwrap_or(path);
    let open = format!("{scheme}{authority}{root_path}");
    let copy = if llm { format!("{open}/v1") } else { format!("{scheme}{authority}{path}") };
    Ok(Endpoint { open, copy })
}

/// "host:port" / "[v6]:port" / "host" -> (host as written, port).
fn split_host_port(authority: &str) -> (&str, Option<&str>) {
    if let Some(end) = authority.strip_prefix('[').and_then(|r| r.find(']')) {
        // [v6] or [v6]:port
        let host = &authority[..end + 2];
        let after = &authority[end + 2..];
        return (host, after.strip_prefix(':').filter(|p| !p.is_empty()));
    }
    match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && !h.contains(':') => (h, Some(p)),
        _ => (authority, None),
    }
}

fn is_wildcard(host: &str) -> bool {
    matches!(host, "0.0.0.0" | "[::]" | "::" | "")
}

fn is_loopback(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "localhost" || h == "[::1]" || h == "::1" || h.starts_with("127.")
}

fn fix_host(host: &str, node_host: Option<&str>) -> String {
    if is_wildcard(host) || is_loopback(host) {
        match node_host.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => return bracket(n),
            None if is_wildcard(host) => return "127.0.0.1".into(),
            None => {}
        }
    }
    host.to_string()
}

/// A bare IPv6 literal needs brackets inside a URL.
fn bracket(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

/// The host of a node address ("192.0.2.10:7340", "[fd00::1]:7340", "box.lan:7340", "192.0.2.10").
pub fn host_of_address(address: &str) -> String {
    let a = address.trim();
    let a = a.strip_prefix("http://").or_else(|| a.strip_prefix("https://")).unwrap_or(a);
    let a = a.split('/').next().unwrap_or(a);
    let (host, _) = split_host_port(a);
    host.to_string()
}
