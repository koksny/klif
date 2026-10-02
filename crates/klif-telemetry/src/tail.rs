//! Log tailing by offset, with terminal-like line assembly.
//!
//! - Files are opened per poll with std's default share mode (read | write | delete), so the writer
//!   (the server, via the supervisor's file handles) is never blocked.
//! - Bytes are assembled like a terminal would show them: `\r\n` and `\n` end a line, a lone `\r`
//!   returns to column 0 (sd-server progress bars overwrite themselves), ANSI CSI sequences are
//!   stripped, and `ESC[K` (erase to end of line, which ends every sd-server bar) closes a segment.
//! - Parsers receive every segment (each bar update separately); the console receives the final
//!   state of every line plus the line currently being drawn.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Which log a segment came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// One unit of text for the parsers: a complete line, or one carriage-return segment of a line.
#[derive(Debug, Clone)]
pub struct Segment {
    pub stream: Stream,
    pub text: String,
    /// Arrival time (seconds since epoch) as seen by the tailer.
    pub at: f64,
}

/// A committed console line (final state of a terminal line).
#[derive(Debug, Clone)]
pub struct ConsoleLine {
    pub stream: Stream,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ansi {
    Text,
    Escape,
    Csi,
}

const MAX_LINE: usize = 64 * 1024;

/// Byte-stream to segments/lines state machine. Keeps partial state across reads.
#[derive(Debug)]
pub struct LineAssembler {
    stream: Stream,
    /// Bytes of the current segment (after the last `\r`), ANSI-stripped.
    cur: Vec<u8>,
    /// The current segment was already handed to the parsers (closed by `ESC[K`).
    cur_emitted: bool,
    /// Last non-empty segment of the current line (what a terminal would still show after `\r`).
    line_last: String,
    esc: Ansi,
    cr_pending: bool,
}

impl LineAssembler {
    pub fn new(stream: Stream) -> Self {
        Self { stream, cur: Vec::new(), cur_emitted: false, line_last: String::new(), esc: Ansi::Text, cr_pending: false }
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.stream);
    }

    fn take_segment(&mut self, at: f64, segs: &mut Vec<Segment>) {
        let text = String::from_utf8_lossy(&self.cur).trim_end().to_string();
        if !self.cur_emitted && !text.trim().is_empty() {
            segs.push(Segment { stream: self.stream, text: text.clone(), at });
        }
        if !text.trim().is_empty() && !crate::text::is_tag_only(&text) {
            self.line_last = text;
        }
        self.cur.clear();
        self.cur_emitted = false;
    }

    fn end_line(&mut self, at: f64, segs: &mut Vec<Segment>, lines: &mut Vec<ConsoleLine>) {
        self.take_segment(at, segs);
        let last = std::mem::take(&mut self.line_last);
        if !last.trim().is_empty() {
            lines.push(ConsoleLine { stream: self.stream, text: last });
        }
    }

    /// Feed raw bytes. Complete segments go to `segs`, committed lines to `lines`.
    pub fn feed(&mut self, bytes: &[u8], at: f64, segs: &mut Vec<Segment>, lines: &mut Vec<ConsoleLine>) {
        for &b in bytes {
            match self.esc {
                Ansi::Escape => {
                    self.esc = if b == b'[' { Ansi::Csi } else { Ansi::Text };
                    continue;
                }
                Ansi::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.esc = Ansi::Text;
                        if b == b'K' {
                            // Erase to end of line: closes an sd-server bar segment.
                            let text = String::from_utf8_lossy(&self.cur).trim_end().to_string();
                            if !self.cur_emitted && !text.trim().is_empty() {
                                segs.push(Segment { stream: self.stream, text, at });
                                self.cur_emitted = true;
                            }
                        }
                    }
                    continue;
                }
                Ansi::Text => {}
            }
            if self.cr_pending {
                self.cr_pending = false;
                if b == b'\n' {
                    self.end_line(at, segs, lines);
                    continue;
                }
                // Lone CR: back to column 0, the next text overwrites this segment.
                self.take_segment(at, segs);
            }
            match b {
                b'\r' => self.cr_pending = true,
                b'\n' => self.end_line(at, segs, lines),
                0x1b => self.esc = Ansi::Escape,
                _ => {
                    if self.cur.len() < MAX_LINE {
                        self.cur.push(b);
                    }
                }
            }
        }
    }

    /// What a terminal shows on the line being drawn right now (e.g. a live progress bar).
    pub fn live_line(&self) -> Option<String> {
        let cur = String::from_utf8_lossy(&self.cur).trim_end().to_string();
        if !cur.trim().is_empty() && !crate::text::is_tag_only(&cur) {
            return Some(cur);
        }
        if !self.line_last.trim().is_empty() {
            return Some(self.line_last.clone());
        }
        None
    }
}

/// Tails one log file by offset.
#[derive(Debug)]
pub struct LogTailer {
    path: PathBuf,
    offset: u64,
    asm: LineAssembler,
}

/// At most this much is read per poll (an adopted multi-hour log is caught up over a few polls).
const MAX_READ: u64 = 8 * 1024 * 1024;

impl LogTailer {
    /// `from_start = false` starts at the current end of the file (a missing file starts at 0).
    pub fn new(path: &Path, stream: Stream, from_start: bool) -> Self {
        let offset = if from_start { 0 } else { std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) };
        Self { path: path.to_path_buf(), offset, asm: LineAssembler::new(stream) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read whatever was appended since the last poll. Returns true if anything was read.
    pub fn poll(&mut self, at: f64, segs: &mut Vec<Segment>, lines: &mut Vec<ConsoleLine>) -> bool {
        let Ok(mut f) = File::open(&self.path) else { return false };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // Truncated or replaced: start over.
            self.offset = 0;
            self.asm.reset();
        }
        if len == self.offset {
            return false;
        }
        let want = (len - self.offset).min(MAX_READ);
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return false;
        }
        let mut buf = vec![0u8; want as usize];
        let mut got = 0usize;
        while got < buf.len() {
            match f.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(_) => break,
            }
        }
        buf.truncate(got);
        self.offset += got as u64;
        self.asm.feed(&buf, at, segs, lines);
        got > 0
    }

    pub fn live_line(&self) -> Option<String> {
        self.asm.live_line()
    }
}
