//! Default-off, compiler-only census of retained function source
//! (`PERRY_SOURCE_NEED_REPORT=<path>`): which stored `Function.prototype.
//! toString` text could any execution read? Codegen records, per module, the
//! byte range every function and class occupies in the module's one retained
//! source blob; [`flow::Need`] classifies each owner as never a value (a), a
//! value that reaches no string sink (b), or possibly stringified (c). Nothing
//! here changes emitted code: codegen only reports ranges it already chose.

pub mod flow;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The owner of one stored source range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StoredKey {
    Func(u32),
    Class(u32),
}

#[derive(Debug, Clone)]
pub struct StoredEntry {
    pub key: StoredKey,
    pub symbol: String,
    pub offset: usize,
    pub len: usize,
    pub hash: u64,
}

/// One module's retained source blob.
#[derive(Debug, Clone, Default)]
pub struct StoredModule {
    pub module_name: String,
    /// Logical (decoded) bytes: the raw pool, then the compressed pool's
    /// decoded bytes. Owner offsets index this space.
    pub blob_len: usize,
    /// Bytes the module's pools occupy in `.perry_src` (compressed frames as
    /// stored).
    pub physical_len: usize,
    pub entries: Vec<StoredEntry>,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STORED: Mutex<BTreeMap<String, StoredModule>> = Mutex::new(BTreeMap::new());

/// Enables recording for the lifetime of the returned scope.
pub struct Scope;

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::SeqCst);
    }
}

pub fn install() -> Scope {
    STORED.lock().unwrap_or_else(|e| e.into_inner()).clear();
    ACTIVE.store(true, Ordering::SeqCst);
    Scope
}

#[inline]
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// FNV-1a over the exact source bytes (the runtime read log uses the same).
pub fn source_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Codegen: one module's blob and the ranges its owners occupy.
pub(crate) fn record_module(
    module_prefix: &str,
    blob_len: usize,
    physical_len: usize,
    entries: Vec<StoredEntry>,
) {
    let mut stored = STORED.lock().unwrap_or_else(|e| e.into_inner());
    let m = stored.entry(module_prefix.to_string()).or_default();
    m.blob_len = blob_len;
    m.physical_len = physical_len;
    m.entries = entries;
}

/// Codegen: the HIR module name behind a symbol prefix.
pub(crate) fn record_module_name(module_prefix: &str, module_name: &str) {
    let mut stored = STORED.lock().unwrap_or_else(|e| e.into_inner());
    stored
        .entry(module_prefix.to_string())
        .or_default()
        .module_name = module_name.to_string();
}

pub fn take_stored() -> BTreeMap<String, StoredModule> {
    std::mem::take(&mut *STORED.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Bytes of the stored blobs whose highest-ranked covering owner has each
/// rank. Ranks index the returned array; an uncovered byte (never expected)
/// lands in the last slot.
pub fn union_bytes(blob_len: usize, ranges: &[(usize, usize, u8)], ranks: usize) -> Vec<u64> {
    let mut best = vec![u8::MAX; blob_len];
    for &(offset, len, rank) in ranges {
        for b in &mut best[offset.min(blob_len)..(offset + len).min(blob_len)] {
            if *b == u8::MAX || rank > *b {
                *b = rank;
            }
        }
    }
    let mut out = vec![0u64; ranks + 1];
    for b in best {
        if b == u8::MAX {
            out[ranks] += 1;
        } else {
            out[b as usize] += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests;
