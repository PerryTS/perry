//! The existing indexed lookup starts with one bucket and tail-continues
//! collisions. A hit need not retain the collision loop's mask and index.
use super::*;

/// Use both halves of the atom hash when selecting a shape-index bucket.
/// Construction and every lookup share this compression; the hash itself
/// remains unchanged for key identity and TEXT-entry comparisons.
#[inline(always)]
pub(super) fn initial_bucket(hash: u64, mask: usize) -> usize {
    (hash ^ (hash >> 32)) as usize & mask
}

#[inline(never)]
pub(super) unsafe fn get<const ABSENT: bool, const SITE: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    r: *const ShapeRecord,
    key_bits: u64,
    hash: u64,
    memo: *const site_memo::ReadSiteMemo,
) -> f64 {
    let table = ((*r).probe & !PROBE_FLAGS) as *const KeyIndex;
    let b = initial_bucket(
        site_key_hash::<LAZY_HASH>(key_bits, hash),
        (*table).mask as usize,
    );
    let pos = *KeyIndex::buckets(table).add(b);
    if pos == u32::MAX {
        return if SITE {
            site_memo::miss_absent::<ABSENT, LAZY_HASH>(dir, obj, key_bits, hash, memo)
        } else {
            absent_from::<ABSENT>(
                dir,
                obj,
                key_bits,
                site_key_hash::<LAZY_HASH>(key_bits, hash),
            )
        };
    }
    // The complete text path performs the hash/content comparison and
    // continues collisions. No text hash need stay live on an atom hit.
    if pos & TEXT_ENTRY != 0 {
        return unloaded::underived::<ABSENT, LAZY_HASH>(dir, obj, key_bits, hash);
    }
    // Select the key storage before retaining its slot index. This keeps
    // the list header arithmetic from extending the index's live range.
    let keys = (*r).keys;
    let words = if pos & ALIAS_ENTRY != 0 {
        (*table).words.as_ptr()
    } else {
        crate::array::array_elements_ptr(keys as usize as *const _) as *const u64
    };
    let i = pos & POS_BITS;
    if *words.add(i as usize) != key_bits {
        // A matching memo proves the entire live chain, including any
        // own key in a later bucket. An unanswered proof still continues
        // this same index; the first bucket is not treated as own absence.
        if SITE {
            return site_memo::collision::<ABSENT, LAZY_HASH>(dir, obj, key_bits, hash, memo);
        }
        return continued::<ABSENT, SITE, LAZY_HASH>(dir, obj, key_bits, hash, memo);
    }
    // This existing shape fact proves a public accessor. The leaf must
    // decline it; the collecting entrance reloads the live getter pair.
    let load = entry_load(table, pos, i);
    if load == NO_LOAD && pos & ACCESSOR_ENTRY != 0 {
        return hole();
    }
    unloaded::finish::<ABSENT, SITE, LAZY_HASH>(dir, obj, key_bits, hash, i, load, memo)
}

#[inline(never)]
pub(super) unsafe fn continued<const ABSENT: bool, const SITE: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: *const site_memo::ReadSiteMemo,
) -> f64 {
    // Reconstruct only on an unanswered collision. Keeping this context
    // out of the front avoids saving registers on every indexed own hit.
    // No collection or user code occurs between the first bucket and here.
    let r = record_of(dir, (*obj).parent_class_id);
    let table = ((*r).probe & !PROBE_FLAGS) as *const KeyIndex;
    let list = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
    let mask = (*table).mask as usize;
    let slots = KeyIndex::buckets(table);
    let mut b =
        initial_bucket(site_key_hash::<LAZY_HASH>(key_bits, hash), mask).wrapping_add(1) & mask;
    let (pos, load) = loop {
        let pos = *slots.add(b);
        if pos == u32::MAX {
            return if SITE {
                site_memo::miss_absent::<ABSENT, LAZY_HASH>(dir, obj, key_bits, hash, memo)
            } else {
                absent_from::<ABSENT>(
                    dir,
                    obj,
                    key_bits,
                    site_key_hash::<LAZY_HASH>(key_bits, hash),
                )
            };
        }
        let i = pos & POS_BITS;
        if pos & TEXT_ENTRY != 0 {
            if KeyIndex::word(table, i) != site_key_hash::<LAZY_HASH>(key_bits, hash) {
                b = (b + 1) & mask;
                continue;
            }
            return unloaded::underived::<ABSENT, LAZY_HASH>(dir, obj, key_bits, hash);
        }
        let w = if pos & ALIAS_ENTRY != 0 {
            KeyIndex::word(table, i)
        } else {
            *list.add(i as usize)
        };
        if w == key_bits {
            let load = entry_load(table, pos, i);
            if load == NO_LOAD && pos & ACCESSOR_ENTRY != 0 {
                return hole();
            }
            break (i, load);
        }
        b = (b + 1) & mask;
    };
    unloaded::finish::<ABSENT, SITE, LAZY_HASH>(dir, obj, key_bits, hash, pos, load, memo)
}
