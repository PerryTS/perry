//! Indexed own-key lookup and its continuations share one tail entrance.
//! The list-only front does not keep the table loop's registers live.
use super::*;

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
    let list = crate::array::array_elements_ptr((*r).keys as usize as *const _) as *const u64;
    let mask = (*table).mask as usize;
    let slots = KeyIndex::buckets(table);
    let mut b = site_key_hash::<LAZY_HASH>(key_bits, hash) as usize & mask;
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
            break (i, entry_load(table, pos, i));
        }
        b = (b + 1) & mask;
    };
    unloaded::finish::<ABSENT, SITE, LAZY_HASH>(dir, obj, key_bits, hash, pos, load, memo)
}
