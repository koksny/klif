//! The model's shape from a GGUF file's header (the metadata before the tensor data): layers, experts, heads,
//! width, vocabulary. Read once per file and cached by the probe (`FileProbe::model_arch`), so a System shows its
//! shape before its server ever loaded it, and for servers that do not print it (Strata, a remote engine whose log
//! KLIF does not read). A server that reports its own shape (llama.cpp's `print_info` block) takes precedence.
//!
//! Only metadata is read: at most `MAX_READ` bytes from the start of the file (the vocabulary and merges are
//! skipped). A file that is not GGUF, or a header this reader does not understand, gives None.

use klif_common::vm::ModelArch;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Stop reading after this much of the header.
const MAX_READ: u64 = 128 * 1024 * 1024;
/// More key/value pairs than any real header has: the file is not what it claims.
const MAX_KV: u64 = 1_000_000;

/// The values the shape needs (the rest is skipped).
#[derive(Debug, Clone)]
enum Val {
    Int(u64),
    Str(String),
    /// The largest integer of an integer array, and the array's length.
    Ints(u64, u64),
    Other,
}

struct Reader<R: Read> {
    r: R,
    read: u64,
}

impl<R: Read> Reader<R> {
    fn bytes(&mut self, n: u64) -> Option<Vec<u8>> {
        self.read = self.read.checked_add(n)?;
        if self.read > MAX_READ {
            return None;
        }
        let mut b = vec![0u8; usize::try_from(n).ok()?];
        self.r.read_exact(&mut b).ok()?;
        Some(b)
    }
    fn skip(&mut self, n: u64) -> Option<()> {
        self.read = self.read.checked_add(n)?;
        if self.read > MAX_READ {
            return None;
        }
        let got = std::io::copy(&mut (&mut self.r).take(n), &mut std::io::sink()).ok()?;
        (got == n).then_some(())
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.bytes(8)?.try_into().ok()?))
    }
    fn string(&mut self) -> Option<String> {
        let n = self.u64()?;
        if n > 1 << 20 {
            return None;
        }
        Some(String::from_utf8_lossy(&self.bytes(n)?).into_owned())
    }
    /// One integer of GGUF type `t` (u8..i64), as u64 (negative values as 0); None for a non-integer type.
    fn int(&mut self, t: u32) -> Option<Option<u64>> {
        Some(match t {
            0 => Some(u64::from(self.bytes(1)?[0])),
            1 => Some(u64::try_from(i8::from_le_bytes([self.bytes(1)?[0]])).unwrap_or(0)),
            2 => Some(u64::from(u16::from_le_bytes(self.bytes(2)?.try_into().ok()?))),
            3 => Some(u64::try_from(i16::from_le_bytes(self.bytes(2)?.try_into().ok()?)).unwrap_or(0)),
            4 => Some(u64::from(self.u32()?)),
            5 => Some(u64::try_from(i32::from_le_bytes(self.bytes(4)?.try_into().ok()?)).unwrap_or(0)),
            10 => Some(self.u64()?),
            11 => Some(u64::try_from(i64::from_le_bytes(self.bytes(8)?.try_into().ok()?)).unwrap_or(0)),
            _ => None,
        })
    }
    /// Skip one value of a fixed-size or string type.
    fn skip_scalar(&mut self, t: u32) -> Option<()> {
        match t {
            0 | 1 | 7 => self.skip(1),
            2 | 3 => self.skip(2),
            4 | 5 | 6 => self.skip(4),
            10..=12 => self.skip(8),
            8 => {
                let n = self.u64()?;
                self.skip(n)
            }
            _ => None,
        }
    }
    fn value(&mut self, t: u32) -> Option<Val> {
        match t {
            8 => Some(Val::Str(self.string()?)),
            9 => {
                let et = self.u32()?;
                let n = self.u64()?;
                if matches!(et, 0..=5 | 10 | 11) {
                    let mut max = 0u64;
                    for _ in 0..n {
                        max = max.max(self.int(et)?.unwrap_or(0));
                    }
                    Some(Val::Ints(max, n))
                } else {
                    // Strings (the vocabulary, the merges) or floats: only the length matters.
                    match et {
                        8 => {
                            for _ in 0..n {
                                self.skip_scalar(8)?;
                            }
                        }
                        6 | 7 | 12 => self.skip(n.checked_mul(if et == 12 { 8 } else if et == 7 { 1 } else { 4 })?)?,
                        _ => return None,
                    }
                    Some(Val::Ints(0, n))
                }
            }
            _ => match self.int(t)? {
                Some(v) => Some(Val::Int(v)),
                None => {
                    self.skip_scalar(t)?;
                    Some(Val::Other)
                }
            },
        }
    }
}

/// The header's key/value pairs (`general.*`, `<arch>.*`, `tokenizer.*`), values reduced to what the shape needs.
fn read_kv(path: &Path) -> Option<HashMap<String, Val>> {
    let f = File::open(path).ok()?;
    let mut r = Reader { r: BufReader::with_capacity(1 << 20, f), read: 0 };
    if r.bytes(4)? != b"GGUF" {
        return None;
    }
    let version = r.u32()?;
    if !(2..=3).contains(&version) {
        return None;
    }
    let _tensors = r.u64()?;
    let n = r.u64()?;
    if n > MAX_KV {
        return None;
    }
    let mut kv = HashMap::new();
    for _ in 0..n {
        let key = r.string()?;
        let t = r.u32()?;
        let v = r.value(t)?;
        kv.insert(key, v);
    }
    Some(kv)
}

fn int(kv: &HashMap<String, Val>, key: &str) -> Option<u64> {
    match kv.get(key)? {
        Val::Int(v) => Some(*v),
        Val::Ints(max, _) => Some(*max),
        _ => None,
    }
}

fn small(v: Option<u64>) -> Option<u32> {
    v.filter(|x| *x > 0).and_then(|x| u32::try_from(x).ok())
}

/// The shape in a GGUF file's header, or None (not GGUF, no layer count, unreadable).
pub fn read_arch(path: &Path) -> Option<ModelArch> {
    let kv = read_kv(path)?;
    let arch = match kv.get("general.architecture")? {
        Val::Str(s) => s.clone(),
        _ => return None,
    };
    let k = |name: &str| format!("{arch}.{name}");
    let layers = small(int(&kv, &k("block_count")))?;
    let experts = small(int(&kv, &k("expert_count"))).unwrap_or(0);
    let (experts_used, shared_experts) = if experts == 0 {
        (0, 0)
    } else {
        // As llama.cpp's parser reads it: Qwen-style MoE names a shared expert by its FFN width, DeepSeek-style by a
        // count.
        let shared = small(int(&kv, &k("expert_shared_count")))
            .unwrap_or(u32::from(int(&kv, &k("expert_shared_feed_forward_length")).unwrap_or(0) > 0));
        (small(int(&kv, &k("expert_used_count"))).unwrap_or(0), shared)
    };
    let vocab = small(int(&kv, &k("vocab_size"))).or_else(|| match kv.get("tokenizer.ggml.tokens") {
        Some(Val::Ints(_, n)) => small(Some(*n)),
        _ => None,
    });
    Some(ModelArch {
        layers,
        experts,
        experts_used,
        shared_experts,
        // Hybrid models list them per layer (0 on the linear-attention layers): the largest is the attention's.
        heads: small(int(&kv, &k("attention.head_count"))),
        kv_heads: small(int(&kv, &k("attention.head_count_kv"))),
        embd: small(int(&kv, &k("embedding_length"))),
        vocab,
        // `general.size_label` is no parameter count ("512x56B" for a 512-expert MoE): left to the server's own.
        params: None,
    })
}
