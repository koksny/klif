//! The embedded GPU table (`data/gpus.toml`): vendor-published peak FP32 throughput per SKU, found by the name
//! Windows reports or by PCI ids. The file is embedded at build time; a table that does not parse is logged and
//! treated as empty (every card is then "unknown", never a wrong number).

use serde::Deserialize;
use std::sync::OnceLock;

const DATA: &str = include_str!("../data/gpus.toml");

/// One SKU of `data/gpus.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GpuEntry {
    /// The name DXGI reports ("AMD Radeon RX 9070 XT").
    pub name: String,
    /// Other names Windows may report for the same SKU.
    pub aliases: Vec<String>,
    pub vendor_id: u32,
    pub device_ids: Vec<u32>,
    /// PCI revisions, only for SKUs that share a device id with another entry.
    pub revisions: Vec<u32>,
    pub shaders: Option<u32>,
    pub boost_mhz: Option<u32>,
    /// The vendor's published peak FP32 TFLOPS.
    pub tflops_fp32: Option<f64>,
    pub vram_gb: Option<f64>,
    pub memory_type: Option<String>,
    pub integrated: Option<bool>,
    pub source: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TableFile {
    gpu: Vec<GpuEntry>,
}

/// Every entry of the table, in file order.
pub fn entries() -> &'static [GpuEntry] {
    static TABLE: OnceLock<Vec<GpuEntry>> = OnceLock::new();
    TABLE.get_or_init(|| match toml::from_str::<TableFile>(DATA) {
        Ok(t) => t.gpu,
        Err(e) => {
            log::warn!("data/gpus.toml could not be read, so no GPU has a TFLOPS number: {e}");
            Vec::new()
        }
    })
}

/// "AMD Radeon(TM) RX 9070 XT " -> "amd radeon rx 9070 xt": case, spacing and (R) / (TM) marks do not matter.
pub fn normalize_name(name: &str) -> String {
    let lower = name.to_lowercase().replace("(r)", " ").replace("(tm)", " ").replace(['®', '™'], " ");
    lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The table entry of an adapter: the normalized name first, then vendor + device + revision, then vendor + device
/// when exactly one entry carries it. None for a card the table does not know.
pub fn lookup(name: &str, vendor_id: u32, device_id: u32, revision: u32) -> Option<&'static GpuEntry> {
    lookup_in(entries(), name, vendor_id, device_id, revision)
}

fn lookup_in(entries: &'static [GpuEntry], name: &str, vendor_id: u32, device_id: u32, revision: u32) -> Option<&'static GpuEntry> {
    let wanted = normalize_name(name);
    if !wanted.is_empty() {
        let by_name = |e: &&GpuEntry| {
            e.vendor_id == vendor_id
                && (normalize_name(&e.name) == wanted || e.aliases.iter().any(|a| normalize_name(a) == wanted))
        };
        // Two entries with one name (a SKU listed per device id): the one that also has the device id wins.
        let hits: Vec<&GpuEntry> = entries.iter().filter(by_name).collect();
        if let Some(e) = hits.iter().find(|e| e.device_ids.contains(&device_id)).or(hits.first()) {
            return Some(e);
        }
    }
    let same_device: Vec<&GpuEntry> = entries.iter().filter(|e| e.vendor_id == vendor_id && e.device_ids.contains(&device_id)).collect();
    if let Some(e) = same_device.iter().find(|e| e.revisions.contains(&revision)) {
        return Some(e);
    }
    // An entry limited to other revisions is another SKU.
    let open: Vec<&&GpuEntry> = same_device.iter().filter(|e| e.revisions.is_empty()).collect();
    match open.as_slice() {
        [only] => Some(**only),
        _ => None,
    }
}
