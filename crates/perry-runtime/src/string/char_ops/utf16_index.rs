//! Lazy WTF-8 indexing for #10055. A cursor makes sequential access linear;
//! sparse byte/UTF-16 checkpoints also bound rescans after backward/random seeks.
//!
//! Four per-thread entries hold weak string identities, never interior pointers.
//! The GC rewrites live identities and prunes dead ones before address reuse.
//! Checkpoints are Rust-owned offsets, so neither building nor reading an index
//! can trigger a Perry collection. Header length changes invalidate an entry
//! (uniquely owned strings may be appended to in place).

use super::*;
use std::cell::RefCell;

const CHECKPOINT_BYTES: usize = 128;
const CACHE_ENTRIES: usize = 4;

#[derive(Clone, Copy, Default)]
struct Position {
    byte: u32,
    utf16: u32,
}

#[derive(Default)]
struct Index {
    owner: usize,
    byte_len: u32,
    utf16_len: u32,
    cursor: Position,
    checkpoints: Vec<Position>,
}

impl Index {
    fn unit_at(&mut self, bytes: &[u8], idx: usize) -> Option<u16> {
        let mut pos = self.cursor;
        // Nearby forward reads use the cursor, including the second half of
        // an astral character. Other seeks start at the nearest checkpoint.
        if idx < pos.utf16 as usize || idx - pos.utf16 as usize > CHECKPOINT_BYTES {
            let end = self
                .checkpoints
                .partition_point(|p| p.utf16 as usize <= idx);
            pos = end
                .checked_sub(1)
                .map_or(Position::default(), |i| self.checkpoints[i]);
        }
        while (pos.byte as usize) < bytes.len() {
            let last = self.checkpoints.last().copied().unwrap_or_default();
            if pos.byte.saturating_sub(last.byte) as usize >= CHECKPOINT_BYTES {
                self.checkpoints.push(pos);
            }
            let (advance, units, cp) = decode_step(bytes, pos.byte as usize);
            if units > 0 && pos.utf16 as usize + units > idx {
                self.cursor = pos;
                return Some(code_unit(cp, units, idx == pos.utf16 as usize));
            }
            // A truncated tail can advance past byte_len; never save an
            // out-of-payload cursor (or narrow that offset with a wrapping cast).
            pos.byte = (pos.byte as usize).saturating_add(advance).min(bytes.len()) as u32;
            pos.utf16 += units as u32;
        }
        self.cursor = pos;
        None
    }
}

#[inline]
fn code_unit(cp: u32, units: usize, first: bool) -> u16 {
    if units == 2 {
        let v = cp.wrapping_sub(0x10000);
        if first {
            0xD800 + ((v >> 10) & 0x3FF) as u16
        } else {
            0xDC00 + (v & 0x3FF) as u16
        }
    } else {
        cp as u16
    }
}

#[inline]
fn decode_step(bytes: &[u8], i: usize) -> (usize, usize, u32) {
    #[cfg(test)]
    DECODE_STEPS.with(|steps| steps.set(steps.get() + 1));
    wtf8_step(bytes, i)
}

struct IndexCache {
    entries: [Index; CACHE_ENTRIES],
    hot: usize,
    next: usize,
}

impl Default for IndexCache {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| Index::default()),
            hot: 0,
            next: 0,
        }
    }
}

crate::perry_thread_local! {
    static UTF16_INDEX_CACHE: RefCell<IndexCache> = RefCell::new(IndexCache::default());
}

/// Caller has validated the header and UTF-16 index. Small strings bypass the
/// cache: in particular, consuming a character returned by `s[i]` must not evict
/// the source string. ASCII callers retain their existing direct byte access.
pub(super) fn unit_at(s: *const StringHeader, idx: usize) -> Option<u16> {
    let byte_len = unsafe { (*s).byte_len };
    let bytes = unsafe { slice::from_raw_parts(string_data(s), byte_len as usize) };
    if bytes.len() < CHECKPOINT_BYTES || idx == 0 {
        let mut utf16 = 0;
        let mut i = 0;
        while i < bytes.len() {
            let (advance, units, cp) = decode_step(bytes, i);
            if units > 0 && utf16 + units > idx {
                return Some(code_unit(cp, units, utf16 == idx));
            }
            utf16 += units;
            i += advance;
        }
        return None;
    }
    UTF16_INDEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let owner = s as usize;
        let slot = if cache.entries[cache.hot].owner == owner {
            cache.hot
        } else if let Some(slot) = cache.entries.iter().position(|entry| entry.owner == owner) {
            slot
        } else {
            let slot = cache.next;
            cache.next = (slot + 1) % CACHE_ENTRIES;
            slot
        };
        cache.hot = slot;
        let entry = &mut cache.entries[slot];
        let utf16_len = unsafe { (*s).utf16_len };
        if entry.owner != owner || entry.byte_len != byte_len || entry.utf16_len != utf16_len {
            *entry = Index {
                owner,
                byte_len,
                utf16_len,
                ..Index::default()
            };
        }
        entry.unit_at(bytes, idx)
    })
}

pub(crate) fn scan_utf16_index_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    UTF16_INDEX_CACHE.with(|cache| {
        for entry in &mut cache.borrow_mut().entries {
            visitor.visit_metadata_usize_slot(&mut entry.owner);
        }
    });
}

pub(crate) fn prune_dead_utf16_indexes(is_dead_owner: &dyn Fn(usize) -> bool) {
    UTF16_INDEX_CACHE.with(|cache| {
        for entry in &mut cache.borrow_mut().entries {
            if entry.owner != 0 && is_dead_owner(entry.owner) {
                *entry = Index::default();
            }
        }
    });
}

#[cfg(test)]
thread_local! {
    static DECODE_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_utf16_index_entries() -> Vec<(usize, usize)> {
    UTF16_INDEX_CACHE.with(|cache| {
        cache
            .borrow()
            .entries
            .iter()
            .filter(|entry| entry.owner != 0)
            .map(|entry| (entry.owner, entry.checkpoints.len()))
            .collect()
    })
}

#[cfg(test)]
mod tests;
