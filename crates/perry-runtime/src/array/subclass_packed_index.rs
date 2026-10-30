//! The packed array-like numeric read codegen's guarded typed-array miss
//! block calls. It holds no site state.
//!
//! Child module of `subclass.rs`, split out to stay under the 2,000-line file
//! gate; `use super::*` keeps the parent's private layout helpers reachable.

use super::*;

/// Unknown-receiver numeric read used by codegen's guarded typed-array miss
/// block. Stable real arrays and Array subclasses terminate here; every other
/// receiver/key keeps the established tag-aware dispatcher as a cold side
/// exit. Keeping that call behind this ABI boundary removes `js_dyn_index_get`
/// from the emitted hot-loop artifact without weakening its semantics.
///
#[no_mangle]
pub extern "C" fn js_packed_arraylike_index_get(receiver: f64, index: f64) -> f64 {
    if let Some(index_u32) = canonical_u32_index(index) {
        let js = JSValue::from_bits(receiver.to_bits());
        if js.is_pointer() {
            let raw = js.as_pointer::<u8>();
            if let Some(header) = crate::value::addr_class::pointer_value_header(js.bits()) {
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
                // #10515: an admitted owning byte view answers from the
                // inline-access cache before the dispatcher's registry probes.
                if crate::gc::is_buffer_family_type(header.obj_type) {
                    if let Some(byte) = cached_u8_packed_get(raw as usize, index_u32) {
                        return byte;
                    }
                }
                if header.obj_type == crate::gc::GC_TYPE_ARRAY
                    && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                {
                    let arr = raw as *const crate::array::ArrayHeader;
                    if unsafe { (*arr).length <= (*arr).capacity } {
                        // SAFETY: the header proved a live dense array.
                        return unsafe {
                            crate::array::indexing::array_get_resolved(arr, index_u32)
                        };
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
                if crate::gc_type_is_object!(header.obj_type)
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
/// #10515: an admitted owning byte view's element, out of line so the Array
/// receivers this helper mostly serves pay only the brand compare.
#[inline(never)]
fn cached_u8_packed_get(addr: usize, index: u32) -> Option<f64> {
    let idx = i32::try_from(index).ok()?;
    crate::buffer::admitted_u8_read(addr, idx).map(f64::from)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_PACKED_ARRAYLIKE_INDEX_GET: extern "C" fn(f64, f64) -> f64 =
    js_packed_arraylike_index_get;
