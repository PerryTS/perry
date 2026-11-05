//! The complete load when a probe entry does not prove an inline slot.
//! Re-derive the record here to keep the tail entrance within six integer
//! arguments. The probe is a GC leaf, so its receiver and shape stay live.
use super::*;
use crate::codegen_abi::LayoutWord;

#[inline(never)]
pub(super) unsafe fn get<const ABSENT: bool, const SITE: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    pos: u32,
    memo: *const site_memo::ReadSiteMemo,
) -> f64 {
    let r = record_of(dir, LayoutWord((*obj).parent_class_id));
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

/// Finish a proved inline load without retaining a ShapeRecord through the
/// lookup loop. The unproved and hole cases derive their record at the exit.
#[inline(always)]
pub(super) unsafe fn finish<const ABSENT: bool, const SITE: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    pos: u32,
    load: u32,
    memo: *const site_memo::ReadSiteMemo,
) -> f64 {
    if load == NO_LOAD {
        return get::<ABSENT, SITE, LAZY_HASH>(dir, obj, key_bits, hash, pos, memo);
    }
    let v = *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + load as usize * 8)
        as *const f64);
    if v.to_bits() != TAG_HOLE {
        return v;
    }
    hole_at::<ABSENT, SITE, LAZY_HASH>(dir, obj, key_bits, hash, pos, memo)
}

#[inline(never)]
unsafe fn hole_at<const ABSENT: bool, const SITE: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    pos: u32,
    memo: *const site_memo::ReadSiteMemo,
) -> f64 {
    if SITE {
        return site_memo::miss_hole::<ABSENT, LAZY_HASH>(dir, obj, pos, key_bits, hash, memo);
    }
    let r = record_of(dir, LayoutWord((*obj).parent_class_id));
    own_hole::<ABSENT>(
        dir,
        obj,
        r,
        pos,
        key_bits,
        site_key_hash::<LAZY_HASH>(key_bits, hash),
    )
}

/// Entries of [`underived`], read by the indexed lookup's tests.
#[cfg(test)]
pub(super) static UNDERIVED_ENTRIES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Keep the indexed loop's text exit from retaining the record and word.
#[inline(never)]
pub(super) unsafe fn underived<const ABSENT: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
) -> f64 {
    #[cfg(test)]
    UNDERIVED_ENTRIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let word = (*obj).parent_class_id;
    probe_get_underived::<ABSENT>(
        dir,
        obj,
        LayoutWord(word),
        record_of(dir, LayoutWord(word)),
        key_bits,
        site_key_hash::<LAZY_HASH>(key_bits, hash),
    )
}
