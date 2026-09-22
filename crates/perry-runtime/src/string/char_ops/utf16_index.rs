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
    /// Locate the code point containing UTF-16 index `idx`, returning its
    /// position along with the decoded step. Shared by `unit_at` and
    /// `boundary_at` so both pay the same amortised seek and both maintain the
    /// same cursor and checkpoints.
    fn seek(&mut self, bytes: &[u8], idx: usize) -> Option<(Position, usize, u32)> {
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
                return Some((pos, units, cp));
            }
            // A truncated tail can advance past byte_len; never save an
            // out-of-payload cursor (or narrow that offset with a wrapping cast).
            pos.byte = (pos.byte as usize).saturating_add(advance).min(bytes.len()) as u32;
            pos.utf16 += units as u32;
        }
        self.cursor = pos;
        None
    }

    fn unit_at(&mut self, bytes: &[u8], idx: usize) -> Option<u16> {
        let (pos, units, cp) = self.seek(bytes, idx)?;
        Some(code_unit(cp, units, idx == pos.utf16 as usize))
    }

    /// Byte offset of the code point containing `idx`, and whether `idx` is
    /// its low surrogate half — i.e. `slice_range::Boundary` in its raw parts.
    fn boundary_at(&mut self, bytes: &[u8], idx: usize) -> Option<(usize, bool)> {
        let (pos, units, _) = self.seek(bytes, idx)?;
        Some((pos.byte as usize, units == 2 && idx != pos.utf16 as usize))
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

/// #10688: an owner-keyed map rather than a fixed array of slots.
///
/// The array held `CACHE_ENTRIES` indexes and evicted round-robin, so a
/// program interleaving indexed access across more strings than that evicted
/// the entry it was about to need on every single access and rebuilt from
/// scratch forever — measured at 1,224x once K exceeded the slot count, with
/// no gradual degradation. Capacity is the defect, so there is no capacity:
/// entries live until their string dies, and `prune_dead_utf16_indexes`
/// (already driven by the collector) reclaims them.
///
/// The map is keyed by a string identity the GC *rewrites* when it relocates
/// an object, so `scan_utf16_index_roots_mut` must rehash after the visitor
/// runs — see there.
type IndexCache = crate::fast_hash::PtrHashMap<usize, Index>;

crate::perry_thread_local! {
    static UTF16_INDEX_CACHE: RefCell<IndexCache> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Caller has validated the header and UTF-16 index. Small strings bypass the
/// cache: in particular, consuming a character returned by `s[i]` must not evict
/// the source string. ASCII callers retain their existing direct byte access.
/// Byte offset (and low-surrogate-half flag) for UTF-16 index `idx`, through
/// the same cache `unit_at` uses. #10685: `slice_range::copy_utf16_range`
/// resolved its start boundary with `advance(bytes, Boundary::default(), start)`
/// — a walk from byte 0 on every call — so slicing a non-ASCII string at
/// increasing offsets was O(n^2), which is the shape TypeScript's scanner has.
pub(super) fn boundary_at(s: *const StringHeader, idx: usize) -> Option<(usize, bool)> {
    let byte_len = unsafe { (*s).byte_len };
    let bytes = unsafe { slice::from_raw_parts(string_data(s), byte_len as usize) };
    if bytes.len() < CHECKPOINT_BYTES || idx == 0 {
        // Short strings and a zero start do not need the cache: the caller's
        // own walk is already O(1)-ish, and consuming a slice must not evict
        // the source string from a four-entry cache.
        return None;
    }
    UTF16_INDEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let owner = s as usize;
        let utf16_len = unsafe { (*s).utf16_len };
        let entry = cache.entry(owner).or_insert_with(|| Index {
            owner,
            byte_len,
            utf16_len,
            ..Index::default()
        });
        // A uniquely owned string can be appended to in place, which
        // invalidates every recorded offset.
        if entry.byte_len != byte_len || entry.utf16_len != utf16_len {
            *entry = Index {
                owner,
                byte_len,
                utf16_len,
                ..Index::default()
            };
        }
        entry.boundary_at(bytes, idx)
    })
}

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
        let utf16_len = unsafe { (*s).utf16_len };
        let entry = cache.entry(owner).or_insert_with(|| Index {
            owner,
            byte_len,
            utf16_len,
            ..Index::default()
        });
        // A uniquely owned string can be appended to in place, which
        // invalidates every recorded offset.
        if entry.byte_len != byte_len || entry.utf16_len != utf16_len {
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
        let mut cache = cache.borrow_mut();
        // The visitor may relocate the string each entry describes, which
        // changes the very address the map is keyed by. Drain first, let the
        // owners be rewritten, then reinsert so the keys and the `owner`
        // fields agree again.
        let mut moved: Vec<(usize, Index)> = cache.drain().collect();
        for (key, index) in &mut moved {
            visitor.visit_metadata_usize_slot(key);
            index.owner = *key;
        }
        for (key, index) in moved {
            cache.insert(key, index);
        }
    });
}

pub(crate) fn prune_dead_utf16_indexes(is_dead_owner: &dyn Fn(usize) -> bool) {
    UTF16_INDEX_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .retain(|&owner, _| owner != 0 && !is_dead_owner(owner));
    });
}

#[cfg(test)]
thread_local! {
    static DECODE_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_utf16_index_entries() -> Vec<(usize, usize)> {
    UTF16_INDEX_CACHE.with(|cache| {
        let mut entries: Vec<(usize, usize)> = cache
            .borrow()
            .iter()
            .filter(|(&owner, _)| owner != 0)
            .map(|(&owner, index)| (owner, index.checkpoints.len()))
            .collect();
        // HashMap iteration order is not stable; callers compare snapshots.
        entries.sort_unstable();
        entries
    })
}

#[cfg(test)]
mod tests;
