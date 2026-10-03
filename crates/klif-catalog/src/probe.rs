//! Cheap native file-existence checks for programs, model files, folders and paths named in presets.
//!
//! The view-model tick asks about every preset's files twice a second, and a path on a dead network share can
//! block a metadata call for half a minute. So the shared probe (`FileProbe::default()`) is
//! stale-while-revalidate: a fresh answer (younger than `TTL`) is returned as is; an expired one is returned at
//! once while one background thread per path refreshes it; a path never seen before waits at most `COLD_WAIT` for
//! the disk and then counts as missing until its answer lands. A launch checks the disk itself instead
//! (`FileProbe::fresh()`), so it never acts on a stale or unknown answer.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(5);
/// How long a lookup with nothing cached waits for the disk (a local disk answers in microseconds; a sleeping
/// one may need longer and is then reported on a later tick).
const COLD_WAIT: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Fact {
    /// Size when it is a regular file.
    File,
    /// Is a folder.
    Dir,
}

#[derive(Debug, Clone, Copy)]
enum Answer {
    File(Option<u64>),
    Dir(bool),
}

impl Answer {
    fn size(self) -> Option<u64> {
        match self {
            Answer::File(s) => s,
            Answer::Dir(_) => None,
        }
    }
    fn dir(self) -> bool {
        matches!(self, Answer::Dir(true))
    }
}

type Key = (Fact, PathBuf);

#[derive(Debug, Default)]
struct Cache {
    /// key -> (when the answer was measured, the answer)
    entries: HashMap<Key, (Instant, Answer)>,
    /// Keys a background thread is measuring right now (at most one thread per path).
    in_flight: HashSet<Key>,
    /// Bumped by `clear`: an answer measured before it is dropped instead of cached.
    generation: u64,
}

#[derive(Debug, Default)]
pub struct FileProbe {
    /// Measure on the calling thread every time (no cache, no threads): for a launch and one-off hashing.
    sync: bool,
    cache: Arc<Mutex<Cache>>,
}

fn lock(c: &Mutex<Cache>) -> MutexGuard<'_, Cache> {
    c.lock().unwrap_or_else(|e| e.into_inner())
}

fn measure(fact: Fact, p: &Path) -> Answer {
    match fact {
        Fact::File => Answer::File(match std::fs::metadata(p) {
            Ok(m) if m.is_file() => Some(m.len()),
            _ => None,
        }),
        Fact::Dir => Answer::Dir(std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)),
    }
}

impl FileProbe {
    /// A probe that always asks the disk on the calling thread (blocking, never stale).
    pub fn fresh() -> Self {
        FileProbe { sync: true, cache: Arc::default() }
    }

    fn ask(&self, fact: Fact, p: &Path) -> Option<Answer> {
        if self.sync {
            return Some(measure(fact, p));
        }
        let key: Key = (fact, p.to_path_buf());
        let (cached, generation) = {
            let mut c = lock(&self.cache);
            let cached = c.entries.get(&key).copied();
            if let Some((at, a)) = cached {
                if at.elapsed() < TTL {
                    return Some(a);
                }
            }
            if c.in_flight.contains(&key) {
                // Being measured: the last answer, or unknown (missing) until the thread is done.
                return cached.map(|(_, a)| a);
            }
            c.in_flight.insert(key.clone());
            (cached, c.generation)
        };
        let (tx, rx) = mpsc::channel();
        let cache = Arc::clone(&self.cache);
        let job = key.clone();
        let spawned = std::thread::Builder::new().name("klif-file-probe".into()).spawn(move || {
            let a = measure(job.0, &job.1);
            let at = Instant::now();
            let mut c = lock(&cache);
            // After a `clear` this answer is too old, and the in-flight mark (if any) is a newer thread's.
            if c.generation == generation {
                c.in_flight.remove(&job);
                c.entries.insert(job, (at, a));
            }
            drop(c);
            let _ = tx.send(a);
        });
        if spawned.is_err() {
            // No thread to spare: measure here, as before.
            let a = measure(fact, p);
            let mut c = lock(&self.cache);
            c.in_flight.remove(&key);
            if c.generation == generation {
                c.entries.insert(key, (Instant::now(), a));
            }
            return Some(a);
        }
        match cached {
            // Expired: the last answer now, the fresh one on a later call.
            Some((_, a)) => Some(a),
            None => rx.recv_timeout(COLD_WAIT).ok(),
        }
    }

    fn lookup(&self, p: &Path) -> Option<u64> {
        self.ask(Fact::File, p).and_then(Answer::size)
    }

    /// True when `p` is an existing regular file (`Test-Path -PathType Leaf`).
    pub fn is_file(&self, p: &Path) -> bool {
        self.lookup(p).is_some()
    }

    /// Size of `p` when it is an existing regular file.
    pub fn file_size(&self, p: &Path) -> Option<u64> {
        self.lookup(p)
    }

    /// True when `p` is an existing folder.
    pub fn is_dir(&self, p: &Path) -> bool {
        self.ask(Fact::Dir, p).is_some_and(Answer::dir)
    }

    /// True when `p` exists (file or folder).
    pub fn exists(&self, p: &Path) -> bool {
        self.is_file(p) || self.is_dir(p)
    }

    /// Total size of a model on disk: the file itself, or all shards of a `...-00001-of-0000N.gguf` set.
    /// `None` when the first file does not exist.
    pub fn model_bytes(&self, p: &Path) -> Option<u64> {
        self.lookup(p).map(|first| match shard_set(p) {
            Some(rest) => first + rest.iter().filter_map(|s| self.lookup(s)).sum::<u64>(),
            None => first,
        })
    }

    /// Forget everything (e.g. after the user installed a model). Answers being measured right now are dropped.
    pub fn clear(&self) {
        let mut c = lock(&self.cache);
        c.entries.clear();
        c.in_flight.clear();
        c.generation = c.generation.wrapping_add(1);
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
