//! Cheap native file-existence checks for scripts, model files and server binaries.
//!
//! The exporter's availability states are a snapshot; KLIF re-checks natively. Results are cached for a
//! few seconds so the 2 Hz view-model tick does not hit the disk for every profile every time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub(crate) struct FileProbe {
    // path -> (when checked, size when it is a file)
    files: Mutex<HashMap<PathBuf, (Instant, Option<u64>)>>,
    // model path -> (when checked, total bytes over all shards)
    models: Mutex<HashMap<PathBuf, (Instant, Option<u64>)>>,
}

impl FileProbe {
    fn lookup(&self, p: &Path) -> Option<u64> {
        let now = Instant::now();
        let mut map = self.files.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, size)) = map.get(p) {
            if now.duration_since(*at) < TTL {
                return *size;
            }
        }
        let size = match std::fs::metadata(p) {
            Ok(m) if m.is_file() => Some(m.len()),
            _ => None,
        };
        map.insert(p.to_path_buf(), (now, size));
        size
    }

    /// True when `p` is an existing regular file (`Test-Path -PathType Leaf`).
    pub(crate) fn is_file(&self, p: &Path) -> bool {
        self.lookup(p).is_some()
    }

    /// Total size of a model on disk: the file itself, or all shards of a `...-00001-of-0000N.gguf` set.
    /// `None` when the first file does not exist.
    pub(crate) fn model_bytes(&self, p: &Path) -> Option<u64> {
        let now = Instant::now();
        if let Some((at, size)) = self.models.lock().unwrap_or_else(|e| e.into_inner()).get(p) {
            if now.duration_since(*at) < TTL {
                return *size;
            }
        }
        let size = self.lookup(p).map(|first| match shard_set(p) {
            Some(rest) => first + rest.iter().filter_map(|s| self.lookup(s)).sum::<u64>(),
            None => first,
        });
        self.models.lock().unwrap_or_else(|e| e.into_inner()).insert(p.to_path_buf(), (now, size));
        size
    }

    /// Forget everything (e.g. after the user installed a model).
    pub(crate) fn clear(&self) {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.models.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// For `name-00001-of-00003.gguf` returns the paths of shards 2..=3. `None` for a normal file name.
fn shard_set(first: &Path) -> Option<Vec<PathBuf>> {
    let name = first.file_name()?.to_str()?;
    let stem = name.strip_suffix(".gguf")?;
    let (head, total) = stem.rsplit_once("-of-")?;
    let (base, index) = head.rsplit_once('-')?;
    if index != "00001" || total.len() != 5 || !total.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = total.parse().ok()?;
    let dir = first.parent()?;
    Some((2..=n).map(|i| dir.join(format!("{base}-{i:05}-of-{total}.gguf"))).collect())
}
