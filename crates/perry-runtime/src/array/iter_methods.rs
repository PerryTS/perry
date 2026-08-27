//! Higher-order array methods.
use super::*;
use crate::closure::ClosureHeader;
use std::ptr;

/// NaN-box an array header pointer as the JS `array` receiver value passed as
/// the 3rd/4th callback argument (`(element, index, array)` /
/// `(accumulator, currentValue, currentIndex, array)`). Per spec the callback
/// observes the original receiver object.
#[inline(always)]
fn array_receiver_value(arr: *const ArrayHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits())
}

#[inline(always)]
unsafe fn array_elements_ptr(arr: *const ArrayHeader) -> *const f64 {
    (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
}

#[inline(always)]
fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[inline(always)]
unsafe fn present_array_element(elements_ptr: *const f64, index: usize) -> Option<f64> {
    let element = *elements_ptr.add(index);
    (element.to_bits() != crate::value::TAG_HOLE).then_some(element)
}

#[inline(always)]
unsafe fn array_element_get_value(elements_ptr: *const f64, index: usize) -> f64 {
    let element = *elements_ptr.add(index);
    if element.to_bits() == crate::value::TAG_HOLE {
        undefined_value()
    } else {
        element
    }
}

/// Root the receiver for a user-callback loop and re-derive the (possibly
/// moved) header + inline element base on every access. A callback can
/// allocate → trigger a MOVING collection → the array (elements are inline
/// after the header) relocates, and a hoisted `elements_ptr` then reads
/// from-space garbage (2026-07-02 audit, GC deep set). The alloc-point
/// direct minor currently forces a conservative non-moving cycle, which
/// masks this for allocation-triggered GC — but a manual `gc()` inside the
/// callback, and any future safepoint-driven copying cycle, do not.
/// Per-iteration cost is a TLS handle read + an offset add, dwarfed by the
/// callback dispatch itself.
struct RootedIterArray<'s> {
    handle: crate::gc::RuntimeHandle<'s>,
}

impl<'s> RootedIterArray<'s> {
    fn new(scope: &'s crate::gc::RuntimeHandleScope, arr: *const ArrayHeader) -> Self {
        Self {
            handle: scope.root_nanbox_f64(array_receiver_value(arr)),
        }
    }

    /// The live receiver value to pass to the callback (spec: the callback
    /// observes the original receiver object — at its CURRENT address).
    #[inline(always)]
    fn receiver(&self) -> f64 {
        array_receiver_value(self.arr())
    }

    #[inline(always)]
    fn arr(&self) -> *const ArrayHeader {
        let rooted =
            (self.handle.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ArrayHeader;
        // `RootedIterArray` is private and every constructor call receives the
        // non-null, genuine Array result of `normalize_array_receiver` after
        // Buffer/TypedArray dispatch.  The handle is then either rewritten by
        // moving GC to another live Array or still points at an Array-growth
        // forwarding stub.  Therefore the ordinary (non-forwarded) case can
        // read its already-proved header directly instead of re-entering
        // `clean_arr_ptr`'s allocator/registry ownership classifier for every
        // callback argument and element access.
        //
        // Growth is the exceptional case that a GC root cannot heal itself:
        // `js_array_grow` leaves aliases pointing at the old stub.  Keep the
        // full resolver there so forwarding-chain validation and compression
        // retain their existing corruption defenses.
        let live = unsafe {
            let header =
                (rooted as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*header).obj_type == crate::gc::GC_TYPE_ARRAY
                && (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            {
                rooted
            } else {
                clean_arr_ptr(rooted)
            }
        };
        if live != rooted {
            // Array growth and moving GC leave forwarding stubs behind. Keep
            // the root current so subsequent loop iterations do not inspect
            // the stub's overwritten length/capacity word as an ArrayHeader.
            self.handle.set_nanbox_f64(array_receiver_value(live));
        }
        live
    }

    #[inline(always)]
    unsafe fn present(&self, index: usize) -> Option<f64> {
        let arr = self.arr();
        if index >= (*arr).length as usize {
            return None;
        }
        present_array_element(array_elements_ptr(arr), index)
    }

    #[inline(always)]
    unsafe fn get_or_undefined(&self, index: usize) -> f64 {
        let arr = self.arr();
        if index >= (*arr).length as usize {
            return undefined_value();
        }
        array_element_get_value(array_elements_ptr(arr), index)
    }
}

#[cfg(test)]
mod rooted_iter_array_tests {
    use super::*;

    #[test]
    fn forwarded_array_observes_shrunk_length_during_callback_iteration() {
        unsafe {
            let mut arr = js_array_alloc(3);
            arr = js_array_push_f64(arr, 1.0);
            arr = js_array_push_f64(arr, 2.0);
            arr = js_array_push_f64(arr, 3.0);

            let scope = crate::gc::RuntimeHandleScope::new();
            let rooted = RootedIterArray::new(&scope, arr);
            let mut live_arr = js_array_grow(arr, (*arr).capacity + 1);
            assert_ne!(live_arr, arr);
            let _removed = js_array_splice(live_arr, 1, 1, ptr::null(), 0, &mut live_arr);

            assert_eq!((*live_arr).length, 2);
            assert_eq!(rooted.arr(), clean_arr_ptr(live_arr));
            assert_eq!(rooted.get_or_undefined(1), 3.0);
            assert_eq!(
                rooted.get_or_undefined(2).to_bits(),
                crate::value::TAG_UNDEFINED
            );
            assert_eq!(rooted.present(2), None);
        }
    }
}

/// Bind the callback's `this` to `undefined` for the duration of a dense
/// iteration (spec: absent `thisArg` means the callback's `this` is
/// `undefined` — NOT whatever ambient receiver the enclosing call left in
/// IMPLICIT_THIS; test262 some/15.4.4.17-5-25, filter/15.4.4.20-5-30).
/// Explicit-`thisArg` call sites route through the `js_arraylike_*` engine
/// instead of these helpers. Arrow callbacks capture `this` lexically and
/// are unaffected.
struct DenseThisGuard(f64);
impl DenseThisGuard {
    fn bind_undefined() -> Self {
        DenseThisGuard(crate::object::js_implicit_this_set(f64::from_bits(
            crate::value::TAG_UNDEFINED,
        )))
    }
}
impl Drop for DenseThisGuard {
    fn drop(&mut self) {
        crate::object::js_implicit_this_set(self.0);
    }
}

/// #5989/#8117: `.forEach` on a receiver codegen could not prove is a
/// collection is statically fused to the ARRAY entry point below, so a native
/// `Set`/`Map` arrives there. Run the collection's own `forEach` and report
/// `true`; a genuine array-like receiver reports `false` and falls through.
///
/// This MUST run before `normalize_array_receiver`. #8041 made `clean_arr_ptr`
/// — which `normalize_array_receiver` funnels into — reject every *tracked
/// non-array*, where it previously rejected only `GC_TYPE_OBJECT` /
/// `GC_TYPE_CLOSURE`. That is correct for the array layout question, but it
/// nulls a `GC_TYPE_SET` / `GC_TYPE_MAP` receiver, and #5989's reroute sat
/// AFTER the normalize call, behind `if arr.is_null() { return; }`. The reroute
/// therefore became unreachable and every fused `set.forEach(cb)` /
/// `map.forEach(cb)` silently iterated nothing. Same ordering fix #8060 applied
/// to the indexed read and #8090/#8119 applied to the typed-array question.
///
/// Tag-gated exactly as `js_array_get_f64` is (#7765): every registered
/// `Map`/`Set` IS its `arena_alloc_gc(_, _, GC_TYPE_MAP|GC_TYPE_SET)` header,
/// so an ordinary array is excluded by one already-warm header byte and never
/// reaches a registry probe. The registry remains the liveness/layout proof.
#[inline]
fn collection_foreach_reroute(arr: *const ArrayHeader, callback: *const ClosureHeader) -> bool {
    let addr = crate::array::array_receiver_addr(arr as *mut ArrayHeader);
    let tag = crate::array::array_receiver_gc_tag(addr as *const ArrayHeader).0;
    if tag != crate::gc::GC_TYPE_SET && tag != crate::gc::GC_TYPE_MAP {
        return false;
    }
    let cb_value = f64::from_bits(crate::value::JSValue::pointer(callback as *const u8).bits());
    let undef = undefined_value();
    if tag == crate::gc::GC_TYPE_SET && crate::set::is_registered_set(addr) {
        crate::set::js_set_foreach(addr as *mut crate::set::SetHeader, cb_value, undef);
        return true;
    }
    if tag == crate::gc::GC_TYPE_MAP && crate::map::is_registered_map(addr) {
        crate::map::js_map_foreach(addr as *mut crate::map::MapHeader, cb_value, undef);
        return true;
    }
    false
}

/// forEach - call callback(element, index) for each element
/// Returns nothing (void)
#[no_mangle]
pub extern "C" fn js_array_forEach(arr: *const ArrayHeader, callback: *const ClosureHeader) {
    // #5989: a native Set/Map reaching this fused array entry point runs its
    // own `forEach`. Ordered before `normalize_array_receiver` because that
    // funnel nulls every tracked non-array (#8041) — see
    // `collection_foreach_reroute`.
    if collection_foreach_reroute(arr, callback) {
        return;
    }
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    if crate::array::buffer_receiver_dispatch(
        arr,
        "forEach",
        &[crate::array::callback_arg(callback)],
    )
    .is_some()
    {
        return;
    }
    // #7574: `normalize_array_receiver` materializes an array-like OBJECT
    // receiver — a `class X extends Array` instance among them — into a fresh
    // dense snapshot. The spec passes the RECEIVER as the callback's 3rd
    // argument, so without this the callback saw the snapshot and
    // `self === sub` was false (the same "forEach's 3rd argument" obligation
    // #7573 hit for Map/Set). Gated on a one-load `GC_TYPE_OBJECT` header test,
    // so a genuine array pays a compare and never enters the registry probes.
    let self_override = if crate::array::subclass::raw_receiver_is_heap_object(arr) {
        crate::array::subclass::array_object_receiver(arr)
    } else {
        None
    };
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return;
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        crate::typedarray::js_typed_array_for_each(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
        return;
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        // The override is a movable `ObjectHeader` held across user callbacks
        // that allocate — root it for the duration of the loop.
        let self_handle = self_override.map(|recv| scope.root_nanbox_f64(recv));
        let self_value = |rooted: &RootedIterArray| match &self_handle {
            Some(h) => h.get_nanbox_f64(),
            None => rooted.receiver(),
        };
        let _tg = DenseThisGuard::bind_undefined();
        if crate::array::array_iteration_is_exotic(arr) {
            for i in 0..length as usize {
                let arr = rooted.arr();
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                let element = crate::array::array_spec_get(arr, i as u32);
                cb_site.call(callback, element, i as f64, self_value(&rooted));
            }
            return;
        }
        for i in 0..length as usize {
            let Some(element) = rooted.present(i) else {
                continue;
            };
            // JS forEach passes (element, index, array). The callback
            // dispatch path supports call3 safely, so bound native
            // methods like `array.forEach(console.log)` can observe the
            // source array just like Node.
            cb_site.call(callback, element, i as f64, self_value(&rooted));
        }
    }
}

/// map - create new array by calling callback(element) on each element
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_map(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    if let Some(result) =
        crate::array::buffer_receiver_dispatch(arr, "map", &[crate::array::callback_arg(callback)])
    {
        return crate::array::dispatch_result_as_array(result);
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        // Typed-array receiver: read elements per element-kind and return a
        // same-kind TypedArray (mirrors the sort/at/findLast delegation).
        return crate::typedarray::js_typed_array_map(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        // Root the callback closure across the iteration. A callback allocated
        // by a frameless caller (arrow/method — #6081) is reachable ONLY via
        // this raw param + the native stack, which an evacuating minor does NOT
        // scan (copied-minor eligibility requires no conservative stack scan).
        // Closures are non-movable, so an unrooted one is swept in place mid-
        // loop → the next dispatch calls freed memory ("object is not a
        // function" / wild-pointer crash). It used to be masked by
        // the non-copying fallback minor, which DOES run the
        // conservative scan; that knob was deleted in #7611, so there is no
        // longer a configuration in which this rooting is optional. See gh #6206.
        let cb_handle = scope.root_raw_const_ptr(callback);
        let _tg = DenseThisGuard::bind_undefined();

        // ECMA-262 §23.1.3.20 step 5: ArraySpeciesCreate(O, len) runs BEFORE
        // the iteration — it reads `O.constructor` / `@@species` (firing any
        // accessor, propagating a poison throw) and throws TypeError on a
        // non-constructor species, so a bad constructor aborts before the
        // callback is ever invoked. For the common case (plain array whose
        // constructor is the intrinsic `Array`) this returns a fresh plain
        // array, identical to the prior `js_array_alloc_with_length`.
        let result_box =
            crate::array::species::array_species_create(rooted.receiver(), length as usize);
        let is_plain = crate::array::species::species_result_is_plain_array(result_box);
        // Root the result too: it must survive (and be re-derived after)
        // every callback-triggered collection during the fill loop.
        let result_rooted = scope.root_nanbox_f64(result_box);
        let result_arr = |rooted: &crate::gc::RuntimeHandle<'_>| {
            (rooted.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ArrayHeader
        };

        let exotic = crate::array::array_iteration_is_exotic(arr);
        for i in 0..length as usize {
            let element = if exotic {
                let arr = rooted.arr();
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                crate::array::array_spec_get(arr, i as u32)
            } else {
                match rooted.present(i) {
                    Some(e) => e,
                    None => continue,
                }
            };
            // JS .map() callback receives (element, index, array).
            let callback = cb_handle.get_raw_const_ptr::<ClosureHeader>();
            let mapped = cb_site.call(callback, element, i as f64, rooted.receiver());
            if is_plain {
                let result = result_arr(&result_rooted);
                let result_elements =
                    (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
                // GC_STORE_AUDIT(INIT): plain result is unpublished; slot layout noted below.
                ptr::write(result_elements.add(i), mapped);
                let mapped_bits = mapped.to_bits();
                if length <= 64 {
                    note_array_slot_layout_only(result, i, mapped_bits);
                } else {
                    note_array_slot(result, i, mapped_bits);
                }
            } else {
                // Custom species container: CreateDataPropertyOrThrow via [[Set]].
                crate::array::species::species_result_set(
                    result_rooted.get_nanbox_f64(),
                    i,
                    mapped,
                );
            }
        }

        result_arr(&result_rooted)
    }
}

/// map for an unused result: preserve callback evaluation order and side
/// effects without allocating or filling the result array.
#[no_mangle]
pub extern "C" fn js_array_map_discard(arr: *const ArrayHeader, callback: *const ClosureHeader) {
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    // `forEach`, not `map`: this entry point exists precisely to preserve
    // callback evaluation order and side effects WITHOUT allocating a result.
    if crate::array::buffer_receiver_dispatch(
        arr,
        "forEach",
        &[crate::array::callback_arg(callback)],
    )
    .is_some()
    {
        return;
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return;
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        // The callback needs the same root its sibling `js_array_map` gives it
        // (#6081), and for the same reason: a callback allocated by a frameless
        // caller — the arrow in `xs.map(x => …)` — is reachable ONLY through this
        // raw parameter and the native stack, which an evacuating minor does not
        // scan. Every `js_closure_call3` below allocates, so from the second
        // element on the dispatch reads a moved-or-swept closure.
        //
        // This arm was missed when #6081 rooted `js_array_map`, and stayed latent:
        // a stale root only bites when a collection lands inside its window, and
        // nothing put one there. #7533's dense-spread fast path removed ~25
        // allocations per loop iteration from `object_deep_clone`, which moved
        // every subsequent collection and dropped one squarely inside this loop —
        // `PERRY_GC_PROTECT_FROMSPACE=1` then faults here on a retired
        // `obj_type=4` (GC_TYPE_CLOSURE). The kernel faults under the instrument
        // BEFORE that change too, at a different site, so this is a pre-existing
        // defect exposed by new timing, not one introduced by it.
        //
        // NaN-boxed rather than `root_raw_const_ptr`, so the read-back at each
        // callsite is a `get_nanbox_f64` and this module stays out of
        // `scripts/raw_handle_debt.py`'s ledger (same shape as
        // `js_iterator_to_array`'s `next` handle).
        let cb_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(callback as i64));
        let current_callback = || {
            crate::value::js_nanbox_get_pointer(cb_handle.get_nanbox_f64()) as *const ClosureHeader
        };
        let _tg = DenseThisGuard::bind_undefined();
        if crate::array::array_iteration_is_exotic(arr) {
            for i in 0..length as usize {
                let arr = rooted.arr();
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                let element = crate::array::array_spec_get(arr, i as u32);
                let _ = cb_site.call(current_callback(), element, i as f64, rooted.receiver());
            }
            return;
        }
        for i in 0..length as usize {
            let Some(element) = rooted.present(i) else {
                continue;
            };
            let _ = cb_site.call(current_callback(), element, i as f64, rooted.receiver());
        }
    }
}

/// filter - create new array with elements where callback(element) returns truthy
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_filter(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    if let Some(result) = crate::array::buffer_receiver_dispatch(
        arr,
        "filter",
        &[crate::array::callback_arg(callback)],
    ) {
        return crate::array::dispatch_result_as_array(result);
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        return crate::typedarray::js_typed_array_filter(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        // Root the callback across the loop — see js_array_map / gh #6206.
        let cb_handle = scope.root_raw_const_ptr(callback);
        let _tg = DenseThisGuard::bind_undefined();

        // ECMA-262 §23.1.3.7 step 5: ArraySpeciesCreate(O, 0) runs before the
        // iteration (validates `O.constructor` / `@@species`, throwing on a
        // poisoned getter or non-constructor species before the callback runs).
        let result_box = crate::array::species::array_species_create(rooted.receiver(), 0);
        let is_plain = crate::array::species::species_result_is_plain_array(result_box);
        // Root the result across callbacks; a push can also REALLOCATE the
        // plain array, so write the returned pointer back into the handle.
        let result_rooted = scope.root_nanbox_f64(result_box);
        // #854: `js_array_push_f64` already maintains `(*result).length`.
        let mut to = 0usize;

        let exotic = crate::array::array_iteration_is_exotic(arr);
        for i in 0..length as usize {
            let element = if exotic {
                let arr = rooted.arr();
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                crate::array::array_spec_get(arr, i as u32)
            } else {
                match rooted.present(i) {
                    Some(e) => e,
                    None => continue,
                }
            };
            let callback = cb_handle.get_raw_const_ptr::<ClosureHeader>();
            let keep = cb_site.call(callback, element, i as f64, rooted.receiver());
            // Proper truthy check: handles NaN-boxed booleans (TAG_FALSE != 0.0 but is falsy)
            if crate::value::js_is_truthy(keep) != 0 {
                if is_plain {
                    let result = (result_rooted.get_nanbox_u64() & crate::value::POINTER_MASK)
                        as *mut ArrayHeader;
                    let result = js_array_push_f64(result, element);
                    result_rooted.set_nanbox_f64(f64::from_bits(
                        crate::value::JSValue::pointer(result as *const u8).bits(),
                    ));
                } else {
                    crate::array::species::species_result_set(
                        result_rooted.get_nanbox_f64(),
                        to,
                        element,
                    );
                    to += 1;
                }
            }
        }

        (result_rooted.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ArrayHeader
    }
}

/// find - find first element that matches callback(element) => true
/// Returns the element as f64, or undefined if not found.
#[no_mangle]
pub extern "C" fn js_array_find(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    if let Some(result) =
        crate::array::buffer_receiver_dispatch(arr, "find", &[crate::array::callback_arg(callback)])
    {
        return result;
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        return crate::typedarray::js_typed_array_find(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);

        for i in 0..length as usize {
            let element = if exotic {
                crate::array::array_spec_get(rooted.arr(), i as u32)
            } else {
                rooted.get_or_undefined(i)
            };
            let result = cb_site.call(callback, element, i as f64, rooted.receiver());
            // Proper truthy check: handles NaN-boxed booleans
            if crate::value::js_is_truthy(result) != 0 {
                return element;
            }
        }

        // Not found
        undefined_value()
    }
}

/// findIndex - find index of first element that matches callback(element) => true
/// Returns the index as i32, or -1 if not found
#[no_mangle]
pub extern "C" fn js_array_findIndex(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> i32 {
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    if let Some(result) = crate::array::buffer_receiver_dispatch(
        arr,
        "findIndex",
        &[crate::array::callback_arg(callback)],
    ) {
        // The dispatcher answers a raw f64 index (or `-1.0`), never a NaN-box.
        return result as i32;
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return -1;
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        return crate::typedarray::js_typed_array_find_index(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        ) as i32;
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);

        for i in 0..length as usize {
            let element = if exotic {
                crate::array::array_spec_get(rooted.arr(), i as u32)
            } else {
                rooted.get_or_undefined(i)
            };
            let result = cb_site.call(callback, element, i as f64, rooted.receiver());
            // Proper truthy check: handles NaN-boxed booleans
            if crate::value::js_is_truthy(result) != 0 {
                return i as i32;
            }
        }

        // Not found
        -1
    }
}

/// findLast - like find but iterates from the end
#[no_mangle]
pub extern "C" fn js_array_find_last(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        return crate::typedarray::js_typed_array_find_last(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);
        for i in (0..length).rev() {
            let element = if exotic {
                crate::array::array_spec_get(rooted.arr(), i as u32)
            } else {
                rooted.get_or_undefined(i)
            };
            let result = cb_site.call(callback, element, i as f64, rooted.receiver());
            if crate::value::js_is_truthy(result) != 0 {
                return element;
            }
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// findLastIndex - like findIndex but iterates from the end
#[no_mangle]
pub extern "C" fn js_array_find_last_index(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> i32 {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return -1;
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        let r = crate::typedarray::js_typed_array_find_last_index(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
        return r as i32;
    }
    unsafe {
        let length = (*arr).length as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);
        for i in (0..length).rev() {
            let element = if exotic {
                crate::array::array_spec_get(rooted.arr(), i as u32)
            } else {
                rooted.get_or_undefined(i)
            };
            let result = cb_site.call(callback, element, i as f64, rooted.receiver());
            if crate::value::js_is_truthy(result) != 0 {
                return i as i32;
            }
        }
        -1
    }
}

/// at - element access supporting negative indices (arr.at(-1) = last)
#[no_mangle]
pub extern "C" fn js_array_at(arr: *const ArrayHeader, index: f64) -> f64 {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // If this pointer is actually a typed-array, dispatch there. Typed arrays
    // and Uint8Array/Buffer have different layouts than ArrayHeader, and the
    // codegen happily routes their `.at(i)` through this generic helper.
    let addr = arr as usize;
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return crate::typedarray::js_typed_array_at(
            addr as *const crate::typedarray::TypedArrayHeader,
            index,
        );
    }
    if crate::buffer::is_registered_buffer(addr) {
        let buf = addr as *const crate::buffer::BufferHeader;
        unsafe {
            let length = (*buf).length as i64;
            let mut idx = index as i64;
            if idx < 0 {
                idx += length;
            }
            if idx < 0 || idx >= length {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            let data = (buf as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
            return *data.add(idx as usize) as f64;
        }
    }
    unsafe {
        let length = (*arr).length as i64;
        let mut idx = index as i64;
        if idx < 0 {
            idx += length;
        }
        if idx < 0 || idx >= length {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let elements_ptr = array_elements_ptr(arr);
        array_element_get_value(elements_ptr, idx as usize)
    }
}

/// some - returns true if any element matches callback(element) => true
/// Returns TAG_TRUE or TAG_FALSE as f64
#[no_mangle]
pub extern "C" fn js_array_some(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    if let Some(result) =
        crate::array::buffer_receiver_dispatch(arr, "some", &[crate::array::callback_arg(callback)])
    {
        // Already a NaN-boxed boolean, the same shape this function returns.
        return result;
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        return crate::typedarray::js_typed_array_some(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);

        for i in 0..length as usize {
            let element = if exotic {
                let arr = rooted.arr();
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                crate::array::array_spec_get(arr, i as u32)
            } else {
                match rooted.present(i) {
                    Some(e) => e,
                    None => continue,
                }
            };
            let result = cb_site.call(callback, element, i as f64, rooted.receiver());
            if crate::value::js_is_truthy(result) != 0 {
                return f64::from_bits(TAG_TRUE);
            }
        }

        f64::from_bits(TAG_FALSE)
    }
}

/// `Array.prototype.some` for a compiler-proved captureless inline arrow.
///
/// The callback literal is consumed only by `some`, has no observable
/// function identity, and cannot read a closure environment. Passing its code
/// pointer directly avoids the singleton-closure TLS lookup and lets the loop
/// call the body without rebuilding closure dispatch state. Non-Array
/// receivers retain the generic path so Buffer, TypedArray, and array-like
/// semantics remain centralized in [`js_array_some`].
#[no_mangle]
pub extern "C" fn js_array_some_captureless(
    original_arr: *const ArrayHeader,
    callback_func: *const u8,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let arr = normalize_array_receiver(original_arr);
    if arr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
        || super::header::receiver_may_be_registered_exotic(arr)
            && crate::buffer::is_registered_buffer(arr as usize)
    {
        let callback = crate::closure::js_closure_alloc_singleton(callback_func);
        return js_array_some(original_arr, callback);
    }

    let callback: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64 =
        unsafe { std::mem::transmute(callback_func) };
    unsafe {
        let length = (*arr).length;
        // SAFETY: `normalize_array_receiver` returned this live plain-array
        // head and the registry exits above excluded Buffer/TypedArray
        // receivers; nothing allocates before the flag read.
        let exotic = crate::array::array_iteration_is_exotic_resolved(
            arr,
            crate::array::array_object_flags_resolved(arr),
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);

        for i in 0..length as usize {
            // One rooted resolution per element serves the presence test, the
            // slot read and the receiver argument; the callback may move the
            // array, so the next iteration resolves again.
            let arr = rooted.arr();
            let element = if exotic {
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                crate::array::array_spec_get(arr, i as u32)
            } else {
                if i >= (*arr).length as usize {
                    continue;
                }
                let bits = *(array_elements_ptr(arr) as *const u64).add(i);
                if bits == crate::value::TAG_HOLE {
                    continue;
                }
                f64::from_bits(bits)
            };
            let result = callback(
                std::ptr::null(),
                element,
                i as f64,
                array_receiver_value(arr),
            );
            // A predicate callback answers with a boolean box almost always;
            // decide those two bit patterns here and keep the runtime
            // predicate for everything else.
            let result_bits = result.to_bits();
            if result_bits == TAG_TRUE {
                return f64::from_bits(TAG_TRUE);
            }
            if result_bits == TAG_FALSE {
                continue;
            }
            if crate::value::js_is_truthy(result) != 0 {
                return f64::from_bits(TAG_TRUE);
            }
        }
    }

    f64::from_bits(TAG_FALSE)
}

/// every - returns true if all elements match callback(element) => true
/// Returns TAG_TRUE or TAG_FALSE as f64
#[no_mangle]
pub extern "C" fn js_array_every(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    if let Some(result) = crate::array::buffer_receiver_dispatch(
        arr,
        "every",
        &[crate::array::callback_arg(callback)],
    ) {
        return result;
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return f64::from_bits(TAG_TRUE);
    }
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        return crate::typedarray::js_typed_array_every(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);

        for i in 0..length as usize {
            let element = if exotic {
                let arr = rooted.arr();
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                crate::array::array_spec_get(arr, i as u32)
            } else {
                match rooted.present(i) {
                    Some(e) => e,
                    None => continue,
                }
            };
            let result = cb_site.call(callback, element, i as f64, rooted.receiver());
            if crate::value::js_is_truthy(result) == 0 {
                return f64::from_bits(TAG_FALSE);
            }
        }

        f64::from_bits(TAG_TRUE)
    }
}

/// flatMap - map each element to an array, then flatten one level
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_flatMap(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall3::resolve(callback);
        // Root the result across callbacks and pushes (a push both allocates
        // — possibly triggering a moving GC — and may reallocate the array).
        let result_rooted = scope.root_nanbox_f64(f64::from_bits(
            crate::value::JSValue::pointer(js_array_alloc(length) as *const u8).bits(),
        ));
        // Scratch handle for the callback-returned sub-array while the inner
        // push loop allocates.
        let sub_rooted = scope.root_nanbox_f64(undefined_value());
        let push_rooted = |value: f64| {
            let result =
                (result_rooted.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ArrayHeader;
            let result = js_array_push_f64(result, value);
            result_rooted.set_nanbox_f64(f64::from_bits(
                crate::value::JSValue::pointer(result as *const u8).bits(),
            ));
        };
        let _tg = DenseThisGuard::bind_undefined();

        for i in 0..length as usize {
            let Some(element) = rooted.present(i) else {
                continue;
            };
            let mapped = cb_site.call(callback, element, i as f64, rooted.receiver());
            // Root first: detecting a lazy array may materialize it, and a
            // push in the inner loop can move the callback result's target.
            sub_rooted.set_nanbox_f64(mapped);
            let sub_arr = crate::array::flattenable_array_ptr(sub_rooted.get_nanbox_f64());
            if !sub_arr.is_null() {
                let sub_len = (*sub_arr).length;
                for j in 0..sub_len as usize {
                    // Resolve from the rooted value after each allocation so a
                    // moved array (or a proxy's moved array target) is never
                    // read through a stale ArrayHeader pointer.
                    let sub_arr = crate::array::flattenable_array_ptr(sub_rooted.get_nanbox_f64());
                    debug_assert!(!sub_arr.is_null());
                    let sub_elements = (sub_arr as *const u8)
                        .add(std::mem::size_of::<ArrayHeader>())
                        as *const f64;
                    let Some(sub_element) = present_array_element(sub_elements, j) else {
                        continue;
                    };
                    push_rooted(sub_element);
                }
            } else {
                // Not an array — push as single element
                push_rooted(sub_rooted.get_nanbox_f64());
            }
        }

        (result_rooted.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ArrayHeader
    }
}

/// reduce - accumulate values using callback(accumulator, element)
/// initial_ptr is pointer to f64 initial value (null if not provided)
/// Returns the final accumulated value
#[no_mangle]
pub extern "C" fn js_array_reduce(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    // #8137: a Buffer-backed `Uint8Array` receiver. Perry's
    // `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
    // registry, so the `lookup_typed_array_kind` re-dispatch below never
    // answers for it and the `BufferHeader` is read as an `ArrayHeader` —
    // correct `length`, GARBAGE elements. Asked above the funnel; see
    // `array::buffer_receiver`.
    // An ABSENT initial value must stay absent: the dispatcher distinguishes
    // `args.len() >= 2` (seed supplied) from a one-element list (seed is the
    // first element), and a seedless reduce over an empty receiver must THROW
    // rather than answer `undefined`. Passing `initial` unconditionally would
    // silently turn every seedless reduce into a seeded one.
    let reduce_args = [crate::array::callback_arg(callback), initial];
    let reduce_args = &reduce_args[..if has_initial != 0 { 2 } else { 1 }];
    if let Some(result) = crate::array::buffer_receiver_dispatch(arr, "reduce", reduce_args) {
        return result;
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        if has_initial != 0 {
            return initial;
        }
        throw_reduce_of_empty();
    }
    // Typed-array receiver: read elements per element-kind (raw int/float
    // storage is NOT NaN-boxed f64, so the generic ArrayHeader path below would
    // read garbage). Issue #2799.
    if super::header::receiver_may_be_registered_exotic(arr)
        && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some()
    {
        return crate::typedarray::js_typed_array_reduce(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
            has_initial,
            initial,
        );
    }
    unsafe {
        let length = (*arr).length as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall4::resolve(callback);

        if length == 0 {
            if has_initial != 0 {
                return initial;
            }
            // Per spec (ES2015 §22.1.3.18): empty array with no initial value
            // throws `TypeError: Reduce of empty array with no initial value`.
            throw_reduce_of_empty();
        }

        let exotic = crate::array::array_iteration_is_exotic(arr);
        let present = |i: usize| -> Option<f64> {
            if exotic {
                // An exotic index read can run a user getter → GC → move.
                let arr = rooted.arr();
                crate::array::array_spec_has_index(arr, i as u32)
                    .then(|| crate::array::array_spec_get(rooted.arr(), i as u32))
            } else {
                rooted.present(i)
            }
        };

        let (accumulator, start_idx) = if has_initial != 0 {
            (initial, 0)
        } else {
            let mut seed = None;
            for i in 0..length {
                if let Some(element) = present(i) {
                    seed = Some((element, i + 1));
                    break;
                }
            }
            match seed {
                Some(seed) => seed,
                None => throw_reduce_of_empty(),
            }
        };

        // Root the accumulator: it can hold a heap value, and both the
        // callback and an exotic getter can trigger a moving GC between
        // iterations while it sits in this Rust local.
        let acc_rooted = scope.root_nanbox_f64(accumulator);
        for i in start_idx..length {
            let Some(element) = present(i) else {
                continue;
            };
            // Spec callback is `(accumulator, currentValue, currentIndex, array)`.
            let next = cb_site.call(
                callback,
                acc_rooted.get_nanbox_f64(),
                element,
                i as f64,
                rooted.receiver(),
            );
            acc_rooted.set_nanbox_f64(next);
        }

        acc_rooted.get_nanbox_f64()
    }
}

/// Throw `TypeError: Reduce of empty array with no initial value` (ES §22.1.3.18 /
/// §22.2.3.20). Routed through Perry's exception machinery so it can be caught.
pub(crate) fn throw_reduce_of_empty() -> ! {
    let msg = "Reduce of empty array with no initial value";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// `arr.toLocaleString(locales?, options?)` (#2808).
///
/// Per the ECMAScript `Array.prototype.toLocaleString` algorithm: walk the
/// array from `0` to `length - 1`, render `null` / `undefined` elements as the
/// empty string, and for every other element call its own
/// `toLocaleString(locales, options)` method, stringify the result, and join
/// the per-element strings with `","` separators. `locales` / `options` are
/// forwarded verbatim to each element method (omitted args are passed as
/// `undefined`).
#[no_mangle]
pub extern "C" fn js_array_to_locale_string(
    arr: *const ArrayHeader,
    locales: f64,
    options: f64,
) -> *mut crate::string::StringHeader {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }
    let len = unsafe { (*arr).length as usize };
    // Forward (locales, options) to each element's toLocaleString. Both are
    // always passed (undefined when omitted by the caller) so element methods
    // that branch on `arguments.length` still observe two slots, matching V8.
    let elem_args: [f64; 2] = [locales, options];
    let method = b"toLocaleString";
    let mut out = String::new();
    for i in 0..len {
        if i > 0 {
            out.push(',');
        }
        let elem = js_array_get(arr, i as u32);
        if elem.is_null() || elem.is_undefined() {
            // Nullish / hole -> empty field.
            continue;
        }
        let elem_f64 = f64::from_bits(elem.bits());
        let result = unsafe {
            crate::object::js_native_call_method(
                elem_f64,
                method.as_ptr() as *const i8,
                method.len(),
                elem_args.as_ptr(),
                elem_args.len(),
            )
        };
        let sp = crate::value::js_jsvalue_to_string(result);
        if !sp.is_null() {
            unsafe {
                let header = &*(sp as *const crate::string::StringHeader);
                let bytes_ptr =
                    (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
                out.push_str(std::str::from_utf8(slice).unwrap_or(""));
            }
        }
    }
    crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32)
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_TO_LOCALE_STRING: extern "C" fn(
    *const ArrayHeader,
    f64,
    f64,
) -> *mut crate::string::StringHeader = js_array_to_locale_string;

// ---------------------------------------------------------------------------
// #4091: non-callable callback validation for higher-order array / TypedArray
// methods (map/forEach/filter/reduce/find*/some/every/flatMap). Per ECMA-262
// these throw a `TypeError` *before* iterating when the callback is not
// callable. Codegen has already unboxed the closure pointer by the time the
// runtime entry runs, so — mirroring `js_validate_array_comparator` (sort,
// #2796) — the boxed value is threaded into a validator that returns the
// resolved `ClosureHeader*` (as `i64`) or throws.
// ---------------------------------------------------------------------------

/// Read a runtime `StringHeader*` into an owned Rust `String`.
fn header_to_owned_string(sp: *const crate::string::StringHeader) -> String {
    if sp.is_null() {
        return String::new();
    }
    unsafe {
        let header = &*sp;
        let bytes_ptr = (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
        std::str::from_utf8(slice).unwrap_or("").to_string()
    }
}

#[inline]
fn jsvalue_to_owned_string(v: f64) -> String {
    header_to_owned_string(crate::value::js_jsvalue_to_string(v))
}

#[inline]
fn typeof_owned_string(v: f64) -> String {
    header_to_owned_string(crate::builtins::js_value_typeof(v))
}

/// Resolve a higher-order callback argument to its `ClosureHeader*` (as
/// `i64`). Returns `Some(ptr)` only for values the runtime can actually
/// invoke (real closures, bound methods/functions); `None` for any
/// non-callable so the caller can throw the spec `TypeError`.
#[inline]
fn resolve_callback_ptr(cb_boxed: f64) -> Option<i64> {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(cb_boxed.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<ClosureHeader>();
        if !crate::closure::get_valid_func_ptr(ptr).is_null() {
            return Some(ptr as i64);
        }
    }
    None
}

/// Render a non-callable value for the *standard* V8 message used by every
/// `Array.prototype` iteration method and all `%TypedArray%.prototype`
/// methods except `map`: `<typeof> <value>` (e.g. `number 5`, `string "x"`,
/// `object null`, `undefined`, `boolean true`, `object`, `bigint`, `symbol`).
fn render_callback_typeof(cb_boxed: f64) -> String {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(cb_boxed.to_bits());
    let ty = typeof_owned_string(cb_boxed);
    match ty.as_str() {
        "undefined" => "undefined".to_string(),
        "object" if jv.is_null() => "object null".to_string(),
        // Plain objects/arrays render as just the type — no value.
        "object" => "object".to_string(),
        "number" | "boolean" => format!("{} {}", ty, jsvalue_to_owned_string(cb_boxed)),
        "string" => format!("{} \"{}\"", ty, jsvalue_to_owned_string(cb_boxed)),
        // bigint / symbol render as just the type — no value.
        _ => ty,
    }
}

/// Render a non-callable value for `%TypedArray%.prototype.map`, which uses a
/// distinct rendering with no `typeof` prefix (e.g. `5`, `x`, `null`, `true`,
/// `undefined`). Object receivers fall back to V8's `#<Object>`.
fn render_callback_plain(cb_boxed: f64) -> String {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(cb_boxed.to_bits());
    if jv.is_undefined()
        || jv.is_null()
        || jv.is_bool()
        || jv.is_number()
        || jv.is_int32()
        || jv.is_any_string()
        || jv.is_bigint()
    {
        return jsvalue_to_owned_string(cb_boxed);
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>();
        if crate::symbol::is_registered_symbol(ptr as usize) {
            return jsvalue_to_owned_string(cb_boxed);
        }
        return "#<Object>".to_string();
    }
    jsvalue_to_owned_string(cb_boxed)
}

#[cold]
fn throw_not_a_function(rendered: String) -> ! {
    let message = format!("{} is not a function", rendered);
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// Validate a higher-order array/TypedArray callback (#4091). Returns the
/// resolved `ClosureHeader*` (as `i64`) for callable values, or throws a
/// `TypeError` with V8's standard `<typeof> <value> is not a function`
/// message. Used by every iteration method except `map`.
#[no_mangle]
pub extern "C" fn js_validate_array_callback(cb_boxed: f64) -> i64 {
    if let Some(p) = resolve_callback_ptr(cb_boxed) {
        return p;
    }
    throw_not_a_function(render_callback_typeof(cb_boxed));
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_VALIDATE_ARRAY_CALLBACK: extern "C" fn(f64) -> i64 = js_validate_array_callback;

/// Validate a `map` callback (#4091). Identical to
/// [`js_validate_array_callback`] except that, for a typed-array receiver, the
/// non-callable message uses `%TypedArray%.prototype.map`'s distinct rendering
/// (no `typeof` prefix). Takes the receiver handle so it can pick the format.
#[no_mangle]
pub extern "C" fn js_validate_array_map_callback(arr: i64, cb_boxed: f64) -> i64 {
    if let Some(p) = resolve_callback_ptr(cb_boxed) {
        return p;
    }
    let is_typed_array =
        super::header::receiver_may_be_registered_exotic(arr as *const ArrayHeader)
            && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some();
    let rendered = if is_typed_array {
        render_callback_plain(cb_boxed)
    } else {
        render_callback_typeof(cb_boxed)
    };
    throw_not_a_function(rendered);
}

#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_VALIDATE_ARRAY_MAP_CALLBACK: extern "C" fn(i64, f64) -> i64 =
    js_validate_array_map_callback;
