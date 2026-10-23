//! The complete load when a probe entry does not prove an inline slot.
//! Re-derive the record here to keep the tail entrance within six integer
//! arguments. The probe is a GC leaf, so its receiver and shape stay live.
use super::*;

#[inline(never)]
pub(super) unsafe fn get<const ABSENT: bool, const SITE: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    pos: u32,
    memo: *const site_memo::ReadSiteMemo,
) -> f64 {
    let r = record_of(dir, (*obj).parent_class_id);
    let v = value_at(obj, r, pos);
    if v.to_bits() != TAG_HOLE {
        return v;
    }
    if SITE {
        return site_memo::miss_hole::<ABSENT, LAZY_HASH>(dir, obj, pos, key_bits, hash, memo);
    }
    own_hole::<ABSENT>(
        dir,
        obj,
        r,
        pos,
        key_bits,
        site_key_hash::<LAZY_HASH>(key_bits, hash),
    )
}
