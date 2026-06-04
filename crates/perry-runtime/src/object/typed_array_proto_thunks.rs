//! `%TypedArray%.prototype` method thunks with `this` brand checks.
//!
//! Every `%TypedArray%.prototype` method begins (spec step 1) with
//! `ValidateTypedArray(this)` — it must throw a `TypeError` when invoked on a
//! receiver that is not a TypedArray. The receiver-typed fast path
//! (`new Int8Array([…]).map(…)`) is lowered straight to the element-typed
//! `js_typed_array_*` helpers by codegen and never touches these thunks. They
//! fire only on the *value* path:
//!
//!   const m = Int8Array.prototype.map;
//!   m.call(plainArray, fn);   // must throw — plainArray is NOT a TypedArray
//!
//! Pre-fix the per-kind prototypes installed the shared
//! `global_this_builtin_noop_thunk` for these methods. A `.call`/`.apply` on
//! that no-op routed through `try_dispatch_value_called_proto_method`, which
//! re-dispatched by *method name* against the new receiver — landing on the
//! regular Array helper, which (after the array-like-receiver change) silently
//! accepted a plain array and returned a wrong-but-non-throwing result.
//!
//! These thunks instead read the `IMPLICIT_THIS` receiver (set by the
//! `.call`/`.apply` dispatch), brand-check it via `lookup_typed_array_kind`,
//! throw a `TypeError` on a non-TypedArray receiver, and otherwise delegate to
//! the existing `dispatch_typed_array_method` tower (the same code the fast
//! path's runtime sibling uses) — so reflective TypedArray-prototype calls now
//! also *work* on a real TypedArray receiver.
//!
//! Installed onto each per-kind typed-array `.prototype` by
//! `global_this::populate_builtin_prototype_methods`.

use super::*;

/// The TypedArray prototype methods whose receiver must be brand-checked. The
/// `u32` is the spec `.length` (own-property arity), matching what Node reports
/// for `Int8Array.prototype.<m>.length`. Iterator/data methods that don't take
/// a callback are included too — they all share the same brand requirement.
///
/// Mutators (`set`/`fill`/`copyWithin`/`sort`) are intentionally included: the
/// brand check is the only behavioral change for them, and on a real TypedArray
/// receiver they still reach the existing mutator impls via the dispatch tower.
pub(super) const TYPED_ARRAY_PROTO_METHODS: &[(&str, u32)] = &[
    ("at", 1),
    ("copyWithin", 2),
    ("entries", 0),
    ("every", 1),
    ("fill", 1),
    ("filter", 1),
    ("find", 1),
    ("findIndex", 1),
    ("findLast", 1),
    ("findLastIndex", 1),
    ("forEach", 1),
    ("includes", 1),
    ("indexOf", 1),
    ("join", 1),
    ("keys", 0),
    ("lastIndexOf", 1),
    ("map", 1),
    ("reduce", 1),
    ("reduceRight", 1),
    ("reverse", 0),
    ("set", 1),
    ("slice", 2),
    ("some", 1),
    ("sort", 1),
    ("subarray", 2),
    ("toLocaleString", 0),
    ("toReversed", 0),
    ("toSorted", 1),
    ("values", 0),
    ("with", 2),
];

/// Install the brand-checking `%TypedArray%.prototype` methods onto a per-kind
/// typed-array prototype object. Each method gets a DISTINCT thunk func_ptr so
/// the per-func-ptr arity registry (and the no-op-thunk filter in
/// `try_dispatch_value_called_proto_method`) can tell them apart — and, because
/// these are not the shared no-op thunk, a `.call`/`.apply` on the value flows
/// through the normal closure-dispatch path straight into the thunk, where the
/// brand check runs.
pub(super) fn install_typed_array_proto_methods(proto_obj: *mut ObjectHeader) {
    use super::global_this::install_proto_method as ipm;
    for &(name, arity) in TYPED_ARRAY_PROTO_METHODS {
        let func_ptr = thunk_for(name);
        ipm(proto_obj, name, func_ptr, arity);
    }
}

/// Map a method name to its dedicated brand-checking thunk. Unknown names fall
/// back to a generic dispatcher keyed off the closure's recorded `.name` — but
/// every entry in `TYPED_ARRAY_PROTO_METHODS` has a concrete thunk so the lookup
/// is exhaustive in practice.
fn thunk_for(name: &str) -> *const u8 {
    match name {
        "at" => ta_at_thunk as *const u8,
        "copyWithin" => ta_copy_within_thunk as *const u8,
        "entries" => ta_entries_thunk as *const u8,
        "every" => ta_every_thunk as *const u8,
        "fill" => ta_fill_thunk as *const u8,
        "filter" => ta_filter_thunk as *const u8,
        "find" => ta_find_thunk as *const u8,
        "findIndex" => ta_find_index_thunk as *const u8,
        "findLast" => ta_find_last_thunk as *const u8,
        "findLastIndex" => ta_find_last_index_thunk as *const u8,
        "forEach" => ta_for_each_thunk as *const u8,
        "includes" => ta_includes_thunk as *const u8,
        "indexOf" => ta_index_of_thunk as *const u8,
        "join" => ta_join_thunk as *const u8,
        "keys" => ta_keys_thunk as *const u8,
        "lastIndexOf" => ta_last_index_of_thunk as *const u8,
        "map" => ta_map_thunk as *const u8,
        "reduce" => ta_reduce_thunk as *const u8,
        "reduceRight" => ta_reduce_right_thunk as *const u8,
        "reverse" => ta_reverse_thunk as *const u8,
        "set" => ta_set_thunk as *const u8,
        "slice" => ta_slice_thunk as *const u8,
        "some" => ta_some_thunk as *const u8,
        "sort" => ta_sort_thunk as *const u8,
        "subarray" => ta_subarray_thunk as *const u8,
        "toLocaleString" => ta_to_locale_string_thunk as *const u8,
        "toReversed" => ta_to_reversed_thunk as *const u8,
        "toSorted" => ta_to_sorted_thunk as *const u8,
        "values" => ta_values_thunk as *const u8,
        "with" => ta_with_thunk as *const u8,
        _ => ta_generic_thunk as *const u8,
    }
}

/// Throw `TypeError: <fn> called on incompatible receiver` (Test262's brand
/// checks assert only the error *type*; the wording is informational). Never
/// returns.
fn throw_not_typed_array(method: &str) -> ! {
    let msg = format!("Method %TypedArray%.prototype.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

/// Read the `IMPLICIT_THIS` receiver and brand-check it as a real TypedArray.
/// Returns the cleaned `TypedArrayHeader` pointer, or throws a `TypeError`.
#[inline]
unsafe fn ta_receiver_or_throw(method: &str) -> *mut crate::typedarray::TypedArrayHeader {
    let bits = IMPLICIT_THIS.with(|c| c.get());
    // A TypedArray receiver reaches here in either of two boxings: a NaN-boxed
    // `POINTER_TAG` value (top16 >= 0x7FF8) or a *raw* heap pointer whose top16
    // is 0 (the receiver-typed fast path threads the bare pointer — see the
    // raw-pointer arm in `native_call_method`). Resolve both to a clean address
    // and brand-check it against the typed-array registry.
    let top16 = bits >> 48;
    let addr = if top16 >= 0x7FF8 {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 && bits >= 0x10000 {
        bits as usize
    } else {
        throw_not_typed_array(method)
    };
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return addr as *mut crate::typedarray::TypedArrayHeader;
    }
    throw_not_typed_array(method)
}

/// Brand-check, then delegate to the shared dispatch tower with the supplied
/// argument slice. `dispatch_typed_array_method` handles every method name in
/// `TYPED_ARRAY_PROTO_METHODS`; the `unwrap_or(undefined)` guard never fires in
/// practice (the name set is kept in sync) but avoids a panic on drift.
#[inline]
unsafe fn brand_then_dispatch(method: &str, args: &[f64]) -> f64 {
    let ta = ta_receiver_or_throw(method);
    let args_ptr = if args.is_empty() {
        std::ptr::null()
    } else {
        args.as_ptr()
    };
    match super::native_call_method::dispatch_typed_array_method(ta, method, args_ptr, args.len()) {
        // Brand check passed and the tower handled the method.
        Some(r) => r,
        // The only `TYPED_ARRAY_PROTO_METHODS` entry the tower doesn't yet
        // resolve is `toLocaleString` (a separate formatting gap, out of scope
        // for this brand-check fix). The brand check already ran, so a
        // non-TypedArray receiver has thrown; a real receiver simply gets
        // `undefined` here rather than a wrong value.
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

// Every thunk takes a uniform `(closure, f64, f64, f64)` signature — the
// closure-dispatch path (`js_native_call_value`) transmutes the func_ptr to a
// per-arity signature using `max(registered_arity, supplied_args)`; a 3-arg
// signature safely covers all real TypedArray-method call shapes (the widest
// real methods — `copyWithin`/`fill` — take 3). Extra supplied args beyond the
// 3 we declare are dropped, which is fine: the brand check (the point of these
// thunks) runs before any argument is read, and no spec TypedArray method
// consumes more than 3 positional arguments. Each method slices off only the
// args it needs before delegating.

macro_rules! ta_thunk {
    ($name:ident, $method:literal, $argc:literal) => {
        pub(super) extern "C" fn $name(
            _c: *const crate::closure::ClosureHeader,
            a: f64,
            b: f64,
            d: f64,
        ) -> f64 {
            let all = [a, b, d];
            unsafe { brand_then_dispatch($method, &all[..$argc]) }
        }
    };
}

ta_thunk!(ta_at_thunk, "at", 1);
ta_thunk!(ta_copy_within_thunk, "copyWithin", 3);
ta_thunk!(ta_entries_thunk, "entries", 0);
ta_thunk!(ta_every_thunk, "every", 1);
ta_thunk!(ta_fill_thunk, "fill", 3);
ta_thunk!(ta_filter_thunk, "filter", 1);
ta_thunk!(ta_find_thunk, "find", 1);
ta_thunk!(ta_find_index_thunk, "findIndex", 1);
ta_thunk!(ta_find_last_thunk, "findLast", 1);
ta_thunk!(ta_find_last_index_thunk, "findLastIndex", 1);
ta_thunk!(ta_for_each_thunk, "forEach", 1);
ta_thunk!(ta_includes_thunk, "includes", 2);
ta_thunk!(ta_index_of_thunk, "indexOf", 2);
ta_thunk!(ta_join_thunk, "join", 1);
ta_thunk!(ta_keys_thunk, "keys", 0);
ta_thunk!(ta_last_index_of_thunk, "lastIndexOf", 2);
ta_thunk!(ta_map_thunk, "map", 1);
ta_thunk!(ta_reduce_thunk, "reduce", 2);
ta_thunk!(ta_reduce_right_thunk, "reduceRight", 2);
ta_thunk!(ta_reverse_thunk, "reverse", 0);
ta_thunk!(ta_set_thunk, "set", 2);
ta_thunk!(ta_slice_thunk, "slice", 2);
ta_thunk!(ta_some_thunk, "some", 1);
ta_thunk!(ta_sort_thunk, "sort", 1);
ta_thunk!(ta_subarray_thunk, "subarray", 2);
ta_thunk!(ta_to_locale_string_thunk, "toLocaleString", 0);
ta_thunk!(ta_to_reversed_thunk, "toReversed", 0);
ta_thunk!(ta_to_sorted_thunk, "toSorted", 1);
ta_thunk!(ta_values_thunk, "values", 0);
ta_thunk!(ta_with_thunk, "with", 2);

/// Fallback thunk for any method name not given a dedicated thunk above.
/// Recovers the method name from the closure's recorded `.name`, brand-checks,
/// then dispatches. Never reached for the names in `TYPED_ARRAY_PROTO_METHODS`,
/// but keeps the install path total.
pub(super) extern "C" fn ta_generic_thunk(
    c: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
    d: f64,
) -> f64 {
    unsafe {
        let name_val = crate::closure::closure_get_dynamic_prop(c as usize, "name");
        let name_hdr = crate::builtins::js_string_coerce(name_val);
        let name =
            super::has_own_helpers::str_from_string_header(name_hdr).unwrap_or("TypedArray method");
        let all = [a, b, d];
        brand_then_dispatch(name, &all)
    }
}
