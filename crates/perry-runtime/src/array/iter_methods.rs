//! Higher-order array methods.
use super::*;
use crate::closure::{js_closure_call3, js_closure_call4, ClosureHeader};
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
        self.handle.get_nanbox_f64()
    }

    #[inline(always)]
    fn arr(&self) -> *const ArrayHeader {
        (self.handle.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ArrayHeader
    }

    #[inline(always)]
    unsafe fn present(&self, index: usize) -> Option<f64> {
        present_array_element(array_elements_ptr(self.arr()), index)
    }

    #[inline(always)]
    unsafe fn get_or_undefined(&self, index: usize) -> f64 {
        array_element_get_value(array_elements_ptr(self.arr()), index)
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

/// forEach - call callback(element, index) for each element
/// Returns nothing (void)
#[no_mangle]
pub extern "C" fn js_array_forEach(arr: *const ArrayHeader, callback: *const ClosureHeader) {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return;
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::js_typed_array_for_each(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
        return;
    }
    // #5989: `.forEach` on an unknown-typed receiver is statically fused to
    // this array entry point, but the receiver may be a native Set/Map —
    // react-server-dom iterates `request.abortableTasks` (a Set read back off
    // the request object) exactly this way. Treating a SetHeader as an
    // ArrayHeader feeds hash-table internals to the callback as elements and
    // segfaults on the first property read. `forEach` is the ONLY method name
    // the fused array methods share with Set/Map, so this single reroute —
    // mirroring the typed-array reroute above — covers the hazard class.
    {
        let cb_value = f64::from_bits(crate::value::JSValue::pointer(callback as *const u8).bits());
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        if crate::set::is_registered_set(arr as usize) {
            crate::set::js_set_foreach(arr as *mut crate::set::SetHeader, cb_value, undef);
            return;
        }
        if crate::map::is_registered_map(arr as usize) {
            crate::map::js_map_foreach(arr as *mut crate::map::MapHeader, cb_value, undef);
            return;
        }
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        let _tg = DenseThisGuard::bind_undefined();
        if crate::array::array_iteration_is_exotic(arr) {
            for i in 0..length as usize {
                let arr = rooted.arr();
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                let element = crate::array::array_spec_get(arr, i as u32);
                js_closure_call3(callback, element, i as f64, rooted.receiver());
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
            js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
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
        // Root the callback closure across the iteration. A callback allocated
        // by a frameless caller (arrow/method — #6081) is reachable ONLY via
        // this raw param + the native stack, which an evacuating minor does NOT
        // scan (copied-minor eligibility requires no conservative stack scan).
        // Closures are non-movable, so an unrooted one is swept in place mid-
        // loop → the next dispatch calls freed memory ("object is not a
        // function" / wild-pointer crash). Masked by PERRY_GEN_GC_EVACUATE=0,
        // whose non-moving minor DOES run the conservative scan. See gh #6206.
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
            let mapped = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return;
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        let _tg = DenseThisGuard::bind_undefined();
        if crate::array::array_iteration_is_exotic(arr) {
            for i in 0..length as usize {
                let arr = rooted.arr();
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                let element = crate::array::array_spec_get(arr, i as u32);
                let _ = js_closure_call3(callback, element, i as f64, rooted.receiver());
            }
            return;
        }
        for i in 0..length as usize {
            let Some(element) = rooted.present(i) else {
                continue;
            };
            let _ = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_filter(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
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
            let keep = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_find(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);

        for i in 0..length as usize {
            let element = if exotic {
                crate::array::array_spec_get(rooted.arr(), i as u32)
            } else {
                rooted.get_or_undefined(i)
            };
            let result = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return -1;
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_find_index(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        ) as i32;
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);

        for i in 0..length as usize {
            let element = if exotic {
                crate::array::array_spec_get(rooted.arr(), i as u32)
            } else {
                rooted.get_or_undefined(i)
            };
            let result = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_find_last(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length as usize;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);
        for i in (0..length).rev() {
            let element = if exotic {
                crate::array::array_spec_get(rooted.arr(), i as u32)
            } else {
                rooted.get_or_undefined(i)
            };
            let result = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
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
        let _tg = DenseThisGuard::bind_undefined();
        let exotic = crate::array::array_iteration_is_exotic(arr);
        for i in (0..length).rev() {
            let element = if exotic {
                crate::array::array_spec_get(rooted.arr(), i as u32)
            } else {
                rooted.get_or_undefined(i)
            };
            let result = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_some(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
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
            let result = js_closure_call3(callback, element, i as f64, rooted.receiver());
            if crate::value::js_is_truthy(result) != 0 {
                return f64::from_bits(TAG_TRUE);
            }
        }

        f64::from_bits(TAG_FALSE)
    }
}

/// every - returns true if all elements match callback(element) => true
/// Returns TAG_TRUE or TAG_FALSE as f64
#[no_mangle]
pub extern "C" fn js_array_every(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return f64::from_bits(TAG_TRUE);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_every(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = RootedIterArray::new(&scope, arr);
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
            let result = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
            let mapped = js_closure_call3(callback, element, i as f64, rooted.receiver());
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
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
            let next = js_closure_call4(
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

/// join - Join array elements into a string with a separator
/// Returns pointer to new StringHeader
#[no_mangle]
pub extern "C" fn js_array_join(
    arr: *const ArrayHeader,
    separator: *const crate::string::StringHeader,
) -> *mut crate::string::StringHeader {
    use crate::string::{js_string_from_bytes, StringHeader};
    use crate::value::JSValue;

    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }
    // #3148: TypedArray receiver — join element-typed values (Node formatting).
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_join(
            arr as *const crate::typedarray::TypedArrayHeader,
            separator,
        );
    }
    unsafe {
        let length = (*arr).length;

        // Empty array returns empty string
        if length == 0 {
            return js_string_from_bytes(ptr::null(), 0);
        }

        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let exotic = crate::array::array_iteration_is_exotic(arr);

        // Get separator string
        let sep_str = if separator.is_null() {
            ","
        } else {
            let sep_len = (*separator).byte_len as usize;
            let sep_data = (separator as *const u8).add(std::mem::size_of::<StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(sep_data, sep_len))
        };

        // Build result string
        let mut result = String::new();
        for i in 0..length as usize {
            if i > 0 {
                result.push_str(sep_str);
            }
            let element_bits = if exotic {
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    // absent slot (own or inherited) → empty string per spec
                    continue;
                }
                crate::array::array_spec_get(arr, i as u32).to_bits()
            } else {
                let bits = (*elements_ptr.add(i)).to_bits();
                // Issue #907: `Array(n)` initializes slots to TAG_HOLE; per
                // ES2015 §22.1.3.13 holes stringify to the empty string.
                if bits == crate::value::TAG_HOLE {
                    continue;
                }
                bits
            };
            let jsvalue = JSValue::from_bits(element_bits);

            // Convert element to string based on its type
            if jsvalue.is_string() {
                let str_ptr = jsvalue.as_string_ptr();
                let str_len = (*str_ptr).byte_len as usize;
                let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let s =
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(str_data, str_len));
                result.push_str(s);
            } else if jsvalue.is_short_string() {
                // v0.5.214 SSO — decode inline into a stack buffer
                // and push bytes. No heap roundtrip via
                // materialize_to_heap.
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let n = jsvalue.short_string_to_buf(&mut scratch);
                let s = std::str::from_utf8_unchecked(&scratch[..n]);
                result.push_str(s);
            } else if jsvalue.is_pointer() {
                // POINTER_TAG. Two cases:
                //  1. A genuine string NaN-boxed with POINTER_TAG instead of
                //     STRING_TAG (a cross-module mis-tag) — read its bytes.
                //  2. A real heap object/array/error/buffer — these must go
                //     through the spec `ToString` (`js_jsvalue_to_string`):
                //     Array→nested join, Error→"name: message" (#2135), an
                //     object with a custom `toString`→that result, buffers,
                //     etc. The old code read *every* pointer as a
                //     `StringHeader`, so a non-string's garbage `byte_len`
                //     produced corrupted output (`[err].join()` → empty).
                //     Distinguish via the GcHeader type tag, excluding the
                //     headerless buffer/symbol pointers first.
                let ptr_addr = (element_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
                if ptr_addr >= 0x1000 {
                    let is_string_obj = !crate::buffer::is_registered_buffer(ptr_addr)
                        && !crate::symbol::is_registered_symbol(ptr_addr)
                        && {
                            let gc_header = (ptr_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                                as *const crate::gc::GcHeader;
                            (*gc_header).obj_type == crate::gc::GC_TYPE_STRING
                        };
                    let s_ptr = if is_string_obj {
                        ptr_addr as *const StringHeader
                    } else {
                        crate::value::js_jsvalue_to_string(f64::from_bits(element_bits))
                    };
                    if !s_ptr.is_null() {
                        let str_len = (*s_ptr).byte_len as usize;
                        let str_data =
                            (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        result.push_str(std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                            str_data, str_len,
                        )));
                    }
                } else {
                    result.push_str("[object Object]");
                }
            } else if jsvalue.is_bigint() {
                // BigInt elements are NaN-boxed with BIGINT_TAG (not POINTER_TAG),
                // so they bypass the pointer arm above and previously fell through
                // to the `[object Object]` catch-all. ToString(BigInt) is the plain
                // decimal digits with NO `n` suffix (`[10n].join() === "10"`).
                let s_ptr = crate::bigint::js_bigint_to_string(jsvalue.as_bigint_ptr());
                if !s_ptr.is_null() {
                    let str_len = (*s_ptr).byte_len as usize;
                    let str_data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    result.push_str(std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        str_data, str_len,
                    )));
                }
            } else if jsvalue.is_number() {
                let n = jsvalue.as_number();
                if n.is_nan() {
                    result.push_str("NaN");
                } else if n.is_infinite() {
                    result.push_str(if n > 0.0 { "Infinity" } else { "-Infinity" });
                } else if n == 0.0 {
                    result.push('0');
                } else if n.fract() == 0.0 && n.abs() < 1e15 {
                    result.push_str(&format!("{}", n as i64));
                } else {
                    result.push_str(&format!("{}", n));
                }
            } else if jsvalue.is_null() {
                // null stringifies to empty string in join
            } else if jsvalue.is_undefined() {
                // undefined stringifies to empty string in join
            } else if jsvalue.is_bool() {
                result.push_str(if jsvalue.as_bool() { "true" } else { "false" });
            } else if element_bits > 0x1000
                && element_bits < 0x0001_0000_0000_0000
                && (element_bits & 0x3) == 0
            {
                // Raw pointer fallback — string stored without NaN-box tag
                let str_ptr = element_bits as *const StringHeader;
                let str_len = (*str_ptr).byte_len as usize;
                if str_len < 10_000_000 {
                    let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        str_data, str_len,
                    ));
                    result.push_str(s);
                } else {
                    result.push_str("[object Object]");
                }
            } else {
                // For objects/arrays, just use placeholder
                result.push_str("[object Object]");
            }
        }

        // Create result string - extract ptr/len before passing to avoid
        // potential LLVM reordering of String drop vs copy_nonoverlapping
        let result_ptr = result.as_ptr();
        let result_len = result.len() as u32;
        let ret = js_string_from_bytes(result_ptr, result_len);
        // Ensure result String stays alive until after the copy completes
        std::hint::black_box(&result);
        drop(result);
        ret
    }
}

#[no_mangle]
pub extern "C" fn js_array_join_value(
    arr: *const ArrayHeader,
    separator_value: f64,
) -> *mut crate::string::StringHeader {
    let separator = if separator_value.to_bits() == crate::value::TAG_UNDEFINED {
        ptr::null()
    } else {
        // `ToString(separator)`: a Symbol separator throws a TypeError
        // (§7.1.17) instead of rendering as "Symbol(…)".
        if unsafe { crate::symbol::js_is_symbol(separator_value) } != 0 {
            crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
        }
        crate::value::js_jsvalue_to_string(separator_value) as *const crate::string::StringHeader
    };
    js_array_join(arr, separator)
}

// Symbol retention: codegen lowers `arr.join(sep)` to a call to
// `js_array_join_value`, but its only in-crate caller sits behind a dispatch
// path the auto-optimize whole-program-bitcode build can prove unreachable and
// dead-strip — which broke the default `perry file.ts -o out` link with
// `undefined _js_array_join_value`. The `#[used]` static pins the symbol so it
// survives every link mode. Same pattern as `node_stream_keepalive.rs`.
#[used]
static KEEP_ARRAY_JOIN_VALUE: extern "C" fn(
    *const ArrayHeader,
    f64,
) -> *mut crate::string::StringHeader = js_array_join_value;

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
    let is_typed_array = crate::typedarray::lookup_typed_array_kind(arr as usize).is_some();
    let rendered = if is_typed_array {
        render_callback_plain(cb_boxed)
    } else {
        render_callback_typeof(cb_boxed)
    };
    throw_not_a_function(rendered);
}

#[used]
static KEEP_VALIDATE_ARRAY_MAP_CALLBACK: extern "C" fn(i64, f64) -> i64 =
    js_validate_array_map_callback;
