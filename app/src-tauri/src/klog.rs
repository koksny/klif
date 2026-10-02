//! The shell log: one file in the state directory (`klif-shell.log`), shared with the engine through the
//! `log` facade, mirrored to stderr (a release build has no console, so the file is the only record). Lines
//! carry the process id (a blocked second instance logs too) and milliseconds since start. Level:
//! `KLIF_LOG` (error|warn|info|debug|trace), default info.
//!
//! Size is bounded: at `MAX_BYTES` the file is renamed to `klif-shell.log.old` (replacing the previous one)
//! and a fresh file is started, both at startup and while the app runs, so a shortcut-launched app that
//! stays up for weeks cannot fill the disk. Panics are written to the same file.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Rotate at this size (the previous file is kept as `.old`).
const MAX_BYTES: u64 = 4 * 1024 * 1024;

struct LogFile {
    path: PathBuf,
    file: File,
    /// Bytes in the file now (its length at open plus what this process wrote).
    written: u64,
}

impl LogFile {
    fn open(path: &Path) -> Option<LogFile> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = OpenOptions::new().create(true).append(true).open(path).ok()?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Some(LogFile { path: path.to_path_buf(), file, written })
    }

    /// Start a fresh file; the full one becomes `<name>.log.old`. Windows lets a file that another process
    /// has open be renamed (Rust opens with FILE_SHARE_DELETE), so a second instance cannot block this.
    fn rotate(&mut self) {
        let _ = self.file.flush();
        let _ = std::fs::rename(&self.path, self.path.with_extension("log.old"));
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            self.file = f;
            self.written = 0;
        }
    }

    fn write_line(&mut self, line: &str) {
        if self.written >= MAX_BYTES {
            self.rotate();
        }
        if writeln!(self.file, "{line}").is_ok() {
            self.written += line.len() as u64 + 1;
        }
    }
}

struct Logger {
    t0: Instant,
    pid: u32,
    file: Mutex<Option<LogFile>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

impl Logger {
    fn line(&self, level: log::Level, tag: &str, text: &std::fmt::Arguments) {
        let line = format!("[{:>7} ms pid {}] {:<5} {}: {}", self.t0.elapsed().as_millis(), self.pid, level, tag, text);
        // No console in a release build: the write fails and is ignored.
        let _ = writeln!(std::io::stderr(), "{line}");
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                f.write_line(&line);
            }
        }
    }
}

impl log::Log for Logger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let target = record.target();
        // Our own lines are tagged by module; dependencies keep their crate name.
        let tag = target.strip_prefix("klif::").unwrap_or(target);
        self.line(record.level(), tag, record.args());
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = f.file.flush();
            }
        }
    }
}

/// Install the logger. `path` = None logs to stderr only.
pub fn init(path: Option<&Path>) {
    let file = path.and_then(|p| {
        // Rotate before opening if the previous run left a full file behind.
        if std::fs::metadata(p).map(|m| m.len() >= MAX_BYTES).unwrap_or(false) {
            let _ = std::fs::rename(p, p.with_extension("log.old"));
        }
        LogFile::open(p)
    });
    let level = match std::env::var("KLIF_LOG").unwrap_or_default().to_ascii_lowercase().as_str() {
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => log::LevelFilter::Info,
    };
    let logger = LOGGER.get_or_init(|| Logger { t0: Instant::now(), pid: std::process::id(), file: Mutex::new(file) });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
    // A release build has no console to print a panic to: put it in the log (then run the default hook).
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(l) = LOGGER.get() {
            let thread = std::thread::current();
            let where_ = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_else(|| "?".into());
            let what = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            l.line(log::Level::Error, "panic", &format_args!("thread '{}' at {where_}: {what}", thread.name().unwrap_or("?")));
        }
        default_hook(info);
    }));
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    log::info!("---- KLIF shell {} start (unix {unix}) ----", env!("CARGO_PKG_VERSION"));
}
