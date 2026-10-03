//! Rendering a program + argument list as ONE command-line string.
//!
//! [`render`] is the single source of truth for what KLIF shows AND what it runs: on Windows the supervisor
//! passes exactly this string (as UTF-16) to `CreateProcessW`, and the Tune drawer / `klif-cli plan` show
//! the same string (with secret values masked before rendering). So it must follow the parser on the other
//! side exactly:
//!
//! - Windows ([`msvc`]): the MSVC CRT / `CommandLineToArgvW` rules. The program (argv[0]) is always wrapped
//!   in double quotes; the CRT reads argv[0] up to the next `"` with no escape processing, which is why a
//!   path can never contain `"` (Windows forbids it in file names anyway). Each argument is left bare unless
//!   it is empty or contains a space, tab, newline, vertical tab or `"`; a quoted argument escapes `"` as
//!   `\"`, doubles every run of backslashes that precedes a `"` (2n+1 before an embedded quote) and doubles a
//!   trailing run of backslashes (2n before the closing quote). Backslashes anywhere else are literal.
//! - Elsewhere ([`posix`]): POSIX shell quoting (`'...'`, an embedded `'` becomes `'\''`), so the string can
//!   be pasted into sh/bash/zsh.

/// The command line as the current platform runs it: [`msvc`] on Windows, [`posix`] elsewhere.
pub fn render(exe: &str, args: &[impl AsRef<str>]) -> String {
    #[cfg(windows)]
    {
        msvc(exe, args)
    }
    #[cfg(not(windows))]
    {
        posix(exe, args)
    }
}

/// MSVC CRT command line: `"exe" arg1 "arg 2" ...`.
pub fn msvc(exe: &str, args: &[impl AsRef<str>]) -> String {
    let mut out = String::with_capacity(exe.len() + 2 + args.iter().map(|a| a.as_ref().len() + 3).sum::<usize>());
    out.push('"');
    out.push_str(exe);
    out.push('"');
    for a in args {
        out.push(' ');
        quote_msvc_arg(a.as_ref(), &mut out);
    }
    out
}

/// Append one argument quoted by the MSVC CRT rules (see the module docs).
pub fn quote_msvc_arg(arg: &str, out: &mut String) {
    let needs = arg.is_empty() || arg.chars().any(|c| matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '"'));
    if !needs {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                out.push('\\');
            }
            '"' => {
                // The n backslashes already written become 2n, plus one that escapes the quote.
                out.extend(std::iter::repeat_n('\\', backslashes + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                backslashes = 0;
                out.push(c);
            }
        }
    }
    // A trailing run would escape the closing quote: double it.
    out.extend(std::iter::repeat_n('\\', backslashes));
    out.push('"');
}

/// POSIX shell command line: `exe arg1 'arg 2' ...`.
pub fn posix(exe: &str, args: &[impl AsRef<str>]) -> String {
    let mut out = String::new();
    quote_posix_arg(exe, &mut out);
    for a in args {
        out.push(' ');
        quote_posix_arg(a.as_ref(), &mut out);
    }
    out
}

/// Append one word quoted for a POSIX shell: bare when it only has safe characters, else `'...'`.
pub fn quote_posix_arg(arg: &str, out: &mut String) {
    let safe = !arg.is_empty()
        && arg.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-'));
    if safe {
        out.push_str(arg);
        return;
    }
    out.push('\'');
    for c in arg.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
}
