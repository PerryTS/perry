//! Array.prototype.reduceRight.
use super::*;
use crate::array::throw_reduce_of_empty;
use crate::closure::ClosureHeader;

#[inline(always)]
unsafe fn array_elements_ptr(arr: *const ArrayHeader) -> *const f64 {
    (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
}

#[inline(always)]
unsafe fn present_array_element(elements_ptr: *const f64, index: usize) -> Option<f64> {
    let element = *elements_ptr.add(index);
    (element.to_bits() != crate::value::TAG_HOLE).then_some(element)
}

/// `arr.reduceRight(callback, initial?)` — reduce from right to left
#[no_mangle]
pub extern "C" fn js_array_reduce_right(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    // #8137: a Buffer-backed `Uint8Array` receiver reads as an `ArrayHeader`
    // below — correct `length`, GARBAGE elements. `reduceRight` is the widest
    // case in the family: it is wrong even for a STATICALLY typed
    // `const u = new Uint8Array([3,1,2])`, because codegen folds that call
    // straight to this helper rather than routing it through
    // `dispatch_buffer_method`. Measured `z|6.36e-314|5.09e-315|4.29e-315`
    // against node's `z|2|1|3`.
    //
    // An ABSENT initial value must stay absent — see `js_array_reduce`.
    let reduce_args = [crate::array::callback_arg(callback), initial];
    let reduce_args = &reduce_args[..if has_initial != 0 { 2 } else { 1 }];
    if let Some(result) = crate::array::buffer_receiver_dispatch(arr, "reduceRight", reduce_args) {
        return result;
    }
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        if has_initial != 0 {
            return initial;
        }
        throw_reduce_of_empty();
    }
    // Typed-array receiver: read elements per element-kind. Issue #2799.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_reduce_right(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
            has_initial,
            initial,
        );
    }
    unsafe {
        let length = (*arr).length as usize;
        // Root the receiver: the callback (and an exotic user getter) can
        // trigger a moving GC, invalidating a hoisted elements pointer
        // (2026-07-02 audit — mirrors iter_methods' RootedIterArray).
        let scope = crate::gc::RuntimeHandleScope::new();
        let rooted = scope.root_nanbox_f64(f64::from_bits(
            crate::value::JSValue::pointer(arr as *const u8).bits(),
        ));
        let live_arr =
            || (rooted.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ArrayHeader;

        if length == 0 {
            if has_initial != 0 {
                return initial;
            }
            // Per spec (ES2015 §22.1.3.19): empty array with no initial value
            // throws `TypeError: Reduce of empty array with no initial value`.
            throw_reduce_of_empty();
        }

        let exotic = crate::array::array_iteration_is_exotic(arr);
        let present = |i: usize| -> Option<f64> {
            if exotic {
                crate::array::array_spec_has_index(live_arr(), i as u32)
                    .then(|| crate::array::array_spec_get(live_arr(), i as u32))
            } else {
                present_array_element(array_elements_ptr(live_arr()), i)
            }
        };

        let (accumulator, start_idx) = if has_initial != 0 {
            (initial, length)
        } else {
            let mut seed = None;
            for i in (0..length).rev() {
                if let Some(element) = present(i) {
                    seed = Some((element, i));
                    break;
                }
            }
            match seed {
                Some(seed) => seed,
                None => throw_reduce_of_empty(),
            }
        };

        // Root the accumulator too: it can hold a heap value while a GC runs
        // between iterations.
        // #8180: resolve the callback's dispatch ONCE. It is invariant for a
        // fixed closure (see closure/dispatch/direct.rs), and this loop calls
        // exactly one.
        let cb_site = crate::closure::DirectCall4::resolve(callback);
        let acc_rooted = scope.root_nanbox_f64(accumulator);
        if start_idx > 0 {
            for i in (0..start_idx).rev() {
                let Some(element) = present(i) else {
                    continue;
                };
                // Spec callback `(accumulator, currentValue, currentIndex, array)`.
                let next = cb_site.call(
                    callback,
                    acc_rooted.get_nanbox_f64(),
                    element,
                    i as f64,
                    rooted.get_nanbox_f64(),
                );
                acc_rooted.set_nanbox_f64(next);
            }
        }

        acc_rooted.get_nanbox_f64()
    }
}
