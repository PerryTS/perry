use super::*;

/// `pop`/`shift`/`push`/`unshift` on a frozen array perform a `Set`/`Delete`
/// with `Throw = true` internally (ECMA-262 §23.1.3.*), so a non-writable
/// `length` / non-extensible receiver makes them throw a **TypeError** — they
/// must not silently no-op. Used by the frozen guards below.
#[cold]
pub(super) fn throw_frozen_array_mutation() -> ! {
    crate::collection_iter::throw_type_error("Cannot mutate a frozen array");
}

/// `push`/`pop`/`shift`/`unshift` always perform `Set(O, "length", …, true)`
/// (ECMA-262 §23.1.3.*), so an array whose `length` was made non-writable via
/// `Object.defineProperty(arr, "length", { writable: false })` makes them throw
/// a **TypeError** — even when the call would otherwise be a no-op (empty array,
/// zero-arg). A *frozen* array is caught by `array_is_frozen` first (same throw);
/// this covers the non-writable-`length`-only case. (test262
/// Array.prototype.{push,pop,shift,unshift}/set-length-*-non-writable.)
#[inline]
pub(crate) fn array_length_is_non_writable(arr: *const ArrayHeader) -> bool {
    // A non-writable `length` is a key of the array's named layout.
    array_has_element_descriptors(arr)
        && crate::object::get_property_attrs(arr as usize, "length")
            .map(|a| !a.writable())
            .unwrap_or(false)
}

/// §23.1.3.21 push performs `Set(O, len, value, true)`; on a non-extensible
/// receiver `CreateDataProperty` for the new index fails, and `Throw=true`
/// makes that a TypeError. Node words it exactly this way for
/// `preventExtensions`, `seal` AND `freeze`.
///
/// The failing index is the receiver's CURRENT length, so this takes the
/// receiver and reads it through the resolved head itself: a caller holding
/// the address it was handed (which a growth may have left as a forwarding
/// stub) must not be able to report a stale word as the index.
#[cold]
pub(crate) fn throw_non_extensible_array_push(arr: *const ArrayHeader) -> ! {
    let head = clean_arr_ptr(arr);
    let index = if head.is_null() {
        0
    } else {
        // SAFETY: `clean_arr_ptr` returned the live resolved head.
        unsafe { (*head).length }
    };
    crate::collection_iter::throw_type_error(&format!(
        "Cannot add property {index}, object is not extensible"
    ));
}

#[cold]
pub(super) fn throw_non_writable_length() -> ! {
    crate::collection_iter::throw_type_error(
        "Cannot assign to read only property 'length' of object '[object Array]'",
    );
}

#[cold]
pub(in super::super) fn throw_cannot_delete_array_index(index: u32) -> ! {
    crate::collection_iter::throw_type_error(&format!(
        "Cannot delete property '{index}' of [object Array]"
    ));
}
