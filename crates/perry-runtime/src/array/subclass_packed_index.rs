//! The packed array-like numeric read codegen's guarded typed-array miss
//! block calls, with its per-site index cache (#9708: allocated on the first
//! prime, behind an emitted pointer slot).
//!
//! Child module of `subclass.rs`, split out to stay under the 2,000-line file
//! gate; `use super::*` keeps the parent's private layout helpers reachable.

use super::*;

/// Words in a per-site packed array-like index cache: `(identity, length
/// slot, element base, dense prefix, inline bound)`.
pub const ARRAYLIKE_PIC_WORDS: usize = 5;
/// A per-site packed array-like index cache, as the emitted slot resolves it.
pub type ArrayLikePicCache = [u64; ARRAYLIKE_PIC_WORDS];
/// The emitted `@perry_ic_N = private global ptr null` for such a site: null
/// until the site's first priming read (#9708).
pub type ArrayLikePicCacheSlot = *mut ArrayLikePicCache;

/// Unknown-receiver numeric read used by codegen's guarded typed-array miss
/// block. Stable real arrays and Array subclasses terminate here; every other
/// receiver/key keeps the established tag-aware dispatcher as a cold side
/// exit. Keeping that call behind this ABI boundary removes `js_dyn_index_get`
/// from the emitted hot-loop artifact without weakening its semantics.
///
/// The five optional IC words are
/// scalar layout facts, never heap pointers:
/// `(class_id, ShapeId)`, length slot, element base, dense prefix, inline bound.
/// The emitted hit path reloads the live object/meta/spill pointers, so moving
/// GC never has to trace or rewrite this cache.
///
/// `cache_slot` is the site's [`ArrayLikePicCacheSlot`] address; the cache is
/// allocated on the first prime (#9708), so a site whose receivers are plain
/// Arrays or elements-backed instances never allocates one.
#[no_mangle]
pub extern "C" fn js_packed_arraylike_index_get(
    receiver: f64,
    index: f64,
    cache_slot: *mut ArrayLikePicCacheSlot,
) -> f64 {
    if let Some(index_u32) = canonical_u32_index(index) {
        let js = JSValue::from_bits(receiver.to_bits());
        if js.is_pointer() {
            let raw = js.as_pointer::<u8>();
            if let Some(header) =
                unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) }
            {
                // #10114's lazy-JSON-array tier. It used to be three emitted
                // blocks and a direct `js_lazy_array_index_probe` call at
                // EVERY indexed read site; it lives here now, so a site pays
                // nothing for it and a `JSON.parse` result still skips the
                // `js_array_get_f64` -> `lazy_get` chain (~227 retired
                // instructions for `rows[7].id`). `TAG_HOLE` is the probe's
                // "this read needs the rooted accessor" signal — unambiguous,
                // because a hole is never a value a read yields — and covers
                // cold elements, descriptors, out-of-bounds, growth-forwarding
                // stubs and a stale `cached_length` mirror, all of which fall
                // through to the unchanged accessor below.
                //
                // The `GC_FLAG_FORWARDED` test is load-bearing, not defensive:
                // the probe's contract is "a live, UNFORWARDED
                // `GC_TYPE_LAZY_ARRAY` pointer -- the caller proves that from
                // the GC header", and since #10098 a lazy array is movable and
                // nursery-resident, so a forwarded one is reachable here. When
                // this tier was three emitted blocks that proof came from
                // `arrlike.ic.header`, which dominated them; folding the tier
                // into this helper moved the obligation here with it. A
                // forwarded receiver falls through to `js_array_get_f64`,
                // whose `clean_arr_ptr` resolves the forwarding pointer.
                //
                // Nothing between the header read and the probe can collect
                // (the probe is `CannotCollect` and `try_read_gc_header` only
                // reads), so `raw` cannot go stale across this window.
                if header.obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
                    && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                {
                    let probed = unsafe {
                        crate::json_tape::js_lazy_array_index_probe(
                            raw as i64,
                            i64::from(index_u32),
                        )
                    };
                    if probed.to_bits() != crate::value::TAG_HOLE {
                        return probed;
                    }
                }
                if matches!(
                    header.obj_type,
                    crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY
                ) {
                    return crate::array::js_array_get_f64(
                        raw as *const crate::array::ArrayHeader,
                        index_u32,
                    );
                }
                if header.obj_type == crate::gc::GC_TYPE_OBJECT
                    && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                {
                    let obj = raw.cast::<ObjectHeader>();
                    // Elements-backed instance: an in-bounds non-hole element
                    // answers directly; a hole continues to the complete
                    // dispatcher (prototype chain).
                    let elements = unsafe { crate::array::subclass_elements::elements_of(obj) };
                    if !elements.is_null() {
                        if let Some(value) =
                            crate::array::subclass_elements::elements_index_get(elements, index_u32)
                        {
                            return value;
                        }
                    } else if let Some(layout) = dense_layout_for_validated_object(obj) {
                        // The codegen hit path handles both inline and
                        // object-owned spill slots.  In spill mode, publish a
                        // class-wide dense-tail identity when the owner has
                        // proved one.  Exact push/pop transitions preserve
                        // that move-stable token, so a lifecycle loop does not
                        // miss once for every historical tail ShapeId.  The
                        // cached dense-prefix word remains the admitted high
                        // water mark: a generic `length` grow beyond it still
                        // side-exits and re-establishes the complete proof.
                        if !cache_slot.is_null() && crate::object::object_spill_enabled() {
                            // SAFETY: a non-null slot is the emitted pointer
                            // global or a test's stack slot.
                            let cache =
                                unsafe { crate::object::pic_slot_resolve(cache_slot) } as *mut u64;
                            let family_token = unsafe {
                                array_subclass_named_prefix_token_for_slot(
                                    obj,
                                    layout.length_slot as usize,
                                )
                            };
                            unsafe {
                                // GC_STORE_AUDIT(POINTER_FREE): generated IC
                                // words are scalar layout facts, not heap edges.
                                cache.add(1).write(layout.length_slot as u64);
                                cache.add(2).write(layout.element_base as u64);
                                cache.add(3).write(layout.dense_prefix_len as u64);
                                cache.add(4).write(layout.live_inline_slots as u64);
                                cache.write(if family_token != 0 {
                                    family_token
                                } else {
                                    dense_cache_key((*obj).class_id, (*obj).parent_class_id)
                                });
                            }
                        }
                        if let Some(value) = dense_index_get_with_layout(obj, layout, index_u32) {
                            return value;
                        }
                    }
                }
            }
        }
    }
    crate::value::js_dyn_index_get(receiver, index)
}
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_PACKED_ARRAYLIKE_INDEX_GET: extern "C" fn(
    f64,
    f64,
    *mut ArrayLikePicCacheSlot,
) -> f64 = js_packed_arraylike_index_get;
