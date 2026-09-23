//! The miss entry of the generated static-key store (`o.k = v`).
//!
//! # What the emitted hit path proves, and what this entry must therefore publish
//!
//! `perry-codegen/src/expr/put_value_store_ic.rs` emits ONE inline hit for an
//! existing own data property: a receiver-tag test, one compare of the
//! receiver's ShapeId word against the low half of the site's compact word
//! (`@perry_ic_N_packed_set`), the per-object receiver-kind test, and a store
//! at the slot the word's high half names. Every other case calls this entry.
//!
//! The word is a MEMO of a pure function of one ShapeId: "key `k` is an own,
//! writable data property of every receiver carrying ShapeId `S`, at inline
//! slot `s`". It never duplicates mutable shape data, because nothing about
//! `S` is mutable: ShapeIds are process-unique and never reused, and a
//! receiver whose keys, descriptors or integrity level change is re-stamped
//! with a different id. So the word cannot go stale; a receiver that no longer
//! satisfies it simply carries a different id and misses. This is the same
//! device, and the same argument, as the read path's `@perry_ic_N_packed_get`
//! (`field_get_set/ic_miss/packed_get.rs`).
//!
//! What has to be true of `S` for the publication to be sound is therefore a
//! property of the SHAPE, established here once, at prime time, from a live
//! receiver that carries it:
//!
//! * **own, inline, at slot `s`** — the key is found in the shape's own key
//!   list below its logical count, at an index below `live_inline_slot_count`.
//!   Spill-located keys are never published to the word (they keep the
//!   runtime-validated ways below).
//! * **a data property, writable** — rule 1 (#10824/#10287): every descriptor
//!   install, removal or bulk clear transitions the ShapeId, and a data
//!   descriptor's generation is a pure function of (predecessor, key,
//!   attributes). So the receiver's per-key descriptor summary, vetted here
//!   (`own_descriptors_skip_key`), holds for every carrier of `S`.
//! * **not frozen / sealed / non-extensible** — `set_integrity_flags` mints a
//!   counter-unique semantic generation when it sets any of the three flags,
//!   so an integrity-restricted receiver carries a private lineage that this
//!   entry refuses to publish.
//! * **not a class object, not a dictionary** — both are the shape's
//!   `object_kind` (`object_is_regular`).
//!
//! What is NOT a property of the shape is re-tested by the emitted hit on
//! every store, and refused here: the receiver-kind admission
//! (`write_fast_path_receiver_kind_ok` — a native-module receiver, and a
//! class-less receiver that no birth site marked ordinary: `URL`,
//! `Object.prototype`, the typed-array prototypes) and the Array-subclass
//! numeric proof (`OBJ_FLAG_PACKED_NUMERIC_PROOF`). The emitted code reads them
//! from the header; this entry refuses a receiver that fails them so the
//! word's first carrier is always one the hit path would have admitted.
//!
//! # Polymorphic sites
//!
//! The word holds the most recently PRIMED shape. The site's lazily allocated
//! way cache (`@perry_ic_N`, a [`PackedSetWays`]) holds up to eight more, in
//! the word's own format: ways 0..4 are compared by the emitted code right
//! after a word miss (the read path's structure, #7753), ways 4..8 by this
//! entry, which then stores without the full `[[Set]]` walk. A spill-located
//! key's way holds its ShapeId with `PACKED_SPILL_FLIP` flipped into it, so no
//! emitted compare can match it, and this entry serves it through the audited
//! overflow store. Ways are filled in order and never evicted, so a site with
//! more than eight stable shapes settles instead of cycling its entries, and a
//! way hit never rewrites the word.
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

/// The value `@perry_ic_N_packed_set` holds before its first prime.
///
/// **Must equal `PACKED_SET_EMPTY` in
/// `perry-codegen/src/expr/put_value_store_ic.rs`.** Not zero: the hit path
/// compares the receiver's ShapeId word against the low half, and a receiver
/// that was never shape-stamped carries a small `parent_class_id` there (0 for
/// an anonymous literal), which a zero sentinel would match. `0xFFFF_FFFF` is
/// above every ShapeId and every class id (`u32::MAX` is reserved, see
/// `class_guard_shape.rs`), so the compare refuses an unprimed site by itself.
pub const PACKED_SET_EMPTY: u64 = 0xFFFF_FFFF;

/// Ways in a site's cache. The first [`PACKED_SET_INLINE_WAYS`] are compared by
/// the emitted code (**must equal `PACKED_SET_INLINE_WAYS` in
/// `perry-codegen/src/expr/put_value_store_ic.rs`**); the rest by this entry.
pub const PACKED_SET_WAYS: usize = 8;
pub const PACKED_SET_INLINE_WAYS: usize = 4;

/// A site's way cache: packed words in the compact word's format.
pub type PackedSetWays = [u64; PACKED_SET_WAYS];
/// The emitted `@perry_ic_N = private global ptr null` for a store site.
pub type PackedSetWaysSlot = *mut PackedSetWays;

/// A spill-located key's way: the ShapeId with the read path's flip applied,
/// which no receiver's `+4` word can equal (see `PACKED_SPILL_FLIP`).
const SPILL_FLIP: u32 = crate::object::field_get_set::PACKED_SPILL_FLIP;

/// Blocking header flags for a receiver this entry may PUBLISH.
///
/// Integrity flags would already have moved the receiver to a private lineage
/// (see the module doc); they are refused here as well because a refused
/// receiver costs one miss, while a published one is served forever. The
/// typed-array-prototype flag and the numeric proof are per-object facts the
/// hit path re-tests; refusing them here keeps the word's first carrier one the
/// hit path admits.
const PACKED_SET_PRIME_BLOCKING: u16 = crate::gc::OBJ_FLAG_FROZEN
    | crate::gc::OBJ_FLAG_SEALED
    | crate::gc::OBJ_FLAG_NO_EXTEND
    | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO
    | crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF;

/// Miss entry for the generated static-key store. Performs the full
/// strict-aware `[[Set]]` (or a validated way store) and publishes what it
/// learned.
///
/// * `cache_slot` — the site's [`PackedSetWaysSlot`] (ways; allocated on the
///   first way prime).
/// * `packed` — the site's compact word; null in a build that emits no inline
///   hit (the full-outline form).
#[no_mangle]
pub extern "C" fn js_put_value_set_packed_miss(
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
    strict: i32,
    cache_slot: *mut PackedSetWaysSlot,
    packed: *const AtomicU64,
) -> f64 {
    // The ways the emitted code does not compare. Nothing here allocates or
    // runs user code, so `target` and `value` are still the caller's values
    // when the full walk below needs them.
    unsafe {
        let cache = crate::object::pic_slot_peek(cache_slot);
        if !cache.is_null() {
            // A full-outline site (null `packed`) emits no compare at all, so
            // every way is this entry's to serve.
            let first_way = if packed.is_null() {
                0
            } else {
                PACKED_SET_INLINE_WAYS
            };
            let ways = &*(cache as *const [AtomicU64; PACKED_SET_WAYS]);
            if let Some(stored) = packed_ways_store(ways, first_way, target, value) {
                return stored;
            }
        }
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_f64(target);
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(value);
    let key_value = if key.is_null() {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits())
    };
    // #7341: the allocating call and the re-read are paired, so the pre-call
    // `key` address is never nameable afterwards.
    let (result, key) = key_handle.across_const::<crate::StringHeader, _>(|| {
        js_put_value_set(
            target_handle.get_nanbox_f64(),
            key_value,
            value_handle.get_nanbox_f64(),
            target_handle.get_nanbox_f64(),
            strict,
        )
    });
    unsafe {
        prime_packed_set(target_handle.get_nanbox_f64(), key, cache_slot, packed);
    }
    result
}

/// Serve `target` from the runtime-compared ways (and any spill way), with
/// the same per-object tests and barriers as the emitted hit.
///
/// # Safety
/// `ways` is a live site cache.
unsafe fn packed_ways_store(
    ways: &[AtomicU64; PACKED_SET_WAYS],
    first_way: usize,
    target: f64,
    value: f64,
) -> Option<f64> {
    let bits = target.to_bits();
    // The emitted receiver test: POINTER tag and a payload above the handle
    // band, so the `+4` load below is of a heap cell (rule 3 then makes a
    // ShapeId match prove a live, non-forwarded GC_TYPE_OBJECT).
    if (bits & !POINTER_MASK) != POINTER_TAG
        || (bits & POINTER_MASK) < crate::value::addr_class::HANDLE_BAND_MAX as u64
    {
        return None;
    }
    let obj = (bits & POINTER_MASK) as *mut crate::ObjectHeader;
    let sid = crate::object::shapes::object_shape_stamp(obj);
    if !crate::object::shapes::is_shape_id(sid) {
        return None;
    }
    for (way, word) in ways.iter().enumerate() {
        let word = word.load(Ordering::Relaxed);
        let stamp = word as u32;
        let index = (word >> 32) as u32;
        if stamp == sid && way >= first_way {
            if !packed_hit_receiver_ok(obj) {
                return None;
            }
            crate::object::store_object_field_slot(obj, index as usize, value.to_bits());
            return Some(value);
        }
        if stamp ^ SPILL_FLIP == sid {
            let token = crate::object::shapes::PIC_ID_TOKEN_BIT | sid as u64;
            return dyn_ic_try_store(target, token, index | IC_SLOT_OVERFLOW_BIT, value);
        }
    }
    None
}

/// The per-object half of the emitted hit, for a receiver whose ShapeId
/// already matched: no numeric proof, and a class instance or a class-less
/// receiver marked ordinary that is not a typed-array prototype.
///
/// # Safety
/// `obj` passed the receiver test and matched a published ShapeId.
unsafe fn packed_hit_receiver_ok(obj: *mut crate::ObjectHeader) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return false;
    };
    let reserved = header._reserved;
    if reserved & crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF != 0 {
        return false;
    }
    let class_id = (*obj).class_id;
    if class_id.wrapping_add(2) > 2 {
        return true;
    }
    class_id == 0
        && reserved & (crate::gc::OBJ_FLAG_PLAIN_ORDINARY | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            == crate::gc::OBJ_FLAG_PLAIN_ORDINARY
}

/// Publish `(ShapeId, slot)` for `key` on `target`, if `target` is a receiver
/// the emitted hit path may serve. Allocation-free except for the way cache's
/// first allocation (an IC-arena block, never a GC object).
///
/// # Safety
/// `target` is a live value read after the last collection point; `key` is
/// null or a live string; `cache_slot` / `packed` are null or live site words.
unsafe fn prime_packed_set(
    target: f64,
    key: *const crate::StringHeader,
    cache_slot: *mut PackedSetWaysSlot,
    packed: *const AtomicU64,
) {
    let target_bits = target.to_bits();
    if (target_bits & !POINTER_MASK) != POINTER_TAG || key.is_null() {
        return;
    }
    let obj_addr = (target_bits & POINTER_MASK) as usize;
    let Some(gc_header) = crate::value::addr_class::try_read_gc_header(obj_addr) else {
        return;
    };
    if gc_header.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || gc_header._reserved & PACKED_SET_PRIME_BLOCKING != 0
    {
        return;
    }
    let obj = obj_addr as *mut crate::ObjectHeader;
    if !crate::object::object_is_regular(obj)
        || !write_fast_path_receiver_kind_ok(obj, gc_header._reserved)
    {
        return;
    }
    // The key is matched by CONTENT below, so any live heap string will do.
    // Only the pointer-keyed read-plan memo needs an interned (never-recycled)
    // key: a non-interned key's address could later name a different string.
    let Some(key_gc) = crate::value::addr_class::try_read_gc_header(key as usize) else {
        return;
    };
    if key_gc.obj_type != crate::gc::GC_TYPE_STRING
        || key_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return;
    }
    let key_interned = key_gc.gc_flags & crate::gc::GC_FLAG_INTERNED != 0;
    // #10287 / rule 1: a descriptor on THIS key is a shape fact only through
    // the per-key summary; a descriptor on another key leaves `key` plain data
    // for every carrier of the same (deterministically minted) ShapeId.
    if gc_header._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        && !crate::object::own_descriptors_skip_key(
            obj_addr,
            f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits()),
        )
    {
        return;
    }
    let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
        return;
    };
    // #10969 (step 2.5): the shape owns the key COUNT; the keys array may be a
    // canonical backing shared along a growth chain, whose header length is
    // the longest list's. Every lookup is bounded by the shape's count.
    let keys = shape.keys as usize as *mut crate::array::ArrayHeader;
    if keys.is_null() || (keys as u64) >> 48 != 0 {
        return;
    }
    let Some(keys_gc) = crate::value::addr_class::try_read_gc_header(keys as usize) else {
        return;
    };
    if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY
        || keys_gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return;
    }
    let key_count = shape.logical_key_count;
    let mut own_idx = if key_interned {
        crate::object::prop_plan::read_plan_lookup(keys as usize, key as usize, key_count)
    } else {
        None
    };
    if own_idx.is_none() {
        if key_count > 4096 {
            return;
        }
        if let Some(i) = crate::object::keys_find_slot_by_key_ptr(keys, key_count, key) {
            if key_interned {
                crate::object::prop_plan::read_plan_record(keys as usize, key as usize, i);
            }
            own_idx = Some(i);
        }
    }
    let Some(idx) = own_idx else {
        return;
    };
    let inline = idx < shape.live_inline_slot_count;
    if !inline && !(idx < key_count && idx < IC_SLOT_OVERFLOW_BIT) {
        return;
    }
    let stamp = crate::object::shapes::object_shape_stamp(obj);
    if !crate::object::shapes::is_shape_id(stamp) {
        return;
    }
    // One word format for the MRU word and every way: `(index << 32) | key`,
    // `key` the ShapeId for an inline slot, the flipped ShapeId for a spill
    // one. Relaxed suffices — each is one atomic 64-bit store of two numbers,
    // never a GC address.
    let (key32, index) = if inline {
        (stamp, idx)
    } else {
        (stamp ^ SPILL_FLIP, idx)
    };
    let entry = (u64::from(index) << 32) | u64::from(key32);

    // The way cache: fill the first empty way, never evict.
    if !cache_slot.is_null() {
        let cache = crate::object::pic_slot_resolve_init(cache_slot, |fresh| {
            for w in (*fresh).iter_mut() {
                *w = PACKED_SET_EMPTY;
            }
        });
        if !cache.is_null() {
            let ways = &*(cache as *const [AtomicU64; PACKED_SET_WAYS]);
            for way in ways.iter() {
                let current = way.load(Ordering::Relaxed);
                if current == entry {
                    break;
                }
                if current == PACKED_SET_EMPTY {
                    way.store(entry, Ordering::Relaxed);
                    break;
                }
            }
        }
    }
    // The compact word: inline slots only.
    if inline && !packed.is_null() {
        (*packed).store(entry, Ordering::Relaxed);
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_PUT_VALUE_SET_PACKED_MISS: extern "C" fn(
    f64,
    *const crate::StringHeader,
    f64,
    i32,
    *mut PackedSetWaysSlot,
    *const AtomicU64,
) -> f64 = js_put_value_set_packed_miss;

#[cfg(test)]
#[path = "packed_set_tests.rs"]
mod tests;
