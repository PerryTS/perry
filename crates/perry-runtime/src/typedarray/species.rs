//! `TypedArraySpeciesCreate` (ES 23.2.4.1) and the supporting
//! `SpeciesConstructor` (7.3.22) for the %TypedArray%.prototype methods that
//! allocate a fresh array — `slice`, `subarray`, `map`, `filter`.
//!
//! Each of those methods must:
//!   1. Read `Get(O, "constructor")` (running any own accessor — observable).
//!   2. Read `Get(C, @@species)` (observable, may throw).
//!   3. Validate the species is a constructor (else TypeError).
//!   4. `Construct(species, args)` and `ValidateTypedArray` the result.
//!
//! When there is no custom species (the common case) we take a fast path that
//! allocates a same-kind `TypedArrayHeader` directly instead of re-entering the
//! intrinsic constructor — observationally identical, since the intrinsic
//! `@@species` returns the intrinsic constructor itself.

use super::{
    bigint, jsvalue_to_f64, load_at, name_for_kind, store_at, typed_array_alloc, TypedArrayHeader,
};
use crate::value::{JSValue, TAG_UNDEFINED};

/// The resolved species for a `TypedArraySpeciesCreate`: either the default
/// intrinsic (fast same-kind allocation) or a user `Construct` target.
pub(crate) enum SpeciesChoice {
    Default,
    Custom(f64),
}

/// `SpeciesConstructor(O, defaultCtor)` for a typed array `owner` of element
/// `kind`. Returns `Default` when the constructor is `undefined` or the
/// `@@species` is `undefined`/`null`; `Custom(S)` when a usable constructor is
/// found. Throws (`!` via `js_throw`) on a non-object constructor or a
/// non-constructor species, and propagates any user getter exception.
pub(crate) unsafe fn species_constructor(owner: usize, kind: u8) -> SpeciesChoice {
    let c = read_constructor(owner, kind);
    let cv = JSValue::from_bits(c.to_bits());
    if cv.is_undefined() {
        return SpeciesChoice::Default;
    }
    if !is_object_value(c) {
        throw_type_error(b"object.constructor is not an object");
    }
    let s = get_species(c);
    let sv = JSValue::from_bits(s.to_bits());
    if sv.is_undefined() || sv.is_null() {
        return SpeciesChoice::Default;
    }
    if !is_constructor(s) {
        throw_type_error(b"object.constructor[Symbol.species] is not a constructor");
    }
    SpeciesChoice::Custom(s)
}

/// `Get(O, "constructor")` — an own expando (data or accessor, the latter
/// runs its getter) wins; otherwise resolves to the intrinsic constructor for
/// this element kind (the default `%Int8Array%` … `%Float64Array%`).
unsafe fn read_constructor(owner: usize, kind: u8) -> f64 {
    if let Some(v) =
        crate::typedarray_props::typed_array_get_property_value_by_name(owner, "constructor")
    {
        return v;
    }
    intrinsic_constructor(kind)
}

/// The intrinsic constructor value for an element kind (`Uint8Array`, …).
pub(crate) fn intrinsic_constructor(kind: u8) -> f64 {
    let name = name_for_kind(kind);
    crate::object::js_get_global_this_builtin_value(name.as_ptr(), name.len())
}

/// `Get(C, @@species)` — runs any species getter, propagating exceptions.
unsafe fn get_species(c: f64) -> f64 {
    let sp = crate::symbol::well_known_symbol("species");
    if sp.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let sym_f64 = f64::from_bits(JSValue::pointer(sp as *const u8).bits());
    crate::symbol::js_object_get_symbol_property(c, sym_f64)
}

/// `Type(value) is Object` — a heap pointer that is not a Symbol (Symbols are
/// pointer-tagged but are primitives). Strings/Numbers/BigInts/booleans/`null`/
/// `undefined` are not pointer-tagged, so they read as non-objects.
fn is_object_value(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    raw >= 0x10000 && !crate::symbol::is_registered_symbol(raw)
}

/// `IsConstructor(value)` — a user `class` ref, or a callable that is not a
/// non-constructable built-in. Mirrors `array::from_concat::is_constructor_value`.
fn is_constructor(value: f64) -> bool {
    if crate::object::class_ref_id(value).is_some() {
        return true;
    }
    crate::collection_iter::is_callable(value)
        && !crate::object::builtin_closure_is_non_constructable_value(value)
}

/// `TypedArrayCreate(constructor, argumentList)` (23.2.4.2): `Construct` then
/// `ValidateTypedArray`. When `single_len` is `Some(n)` (the single-Number
/// argument form), additionally assert `result.[[ArrayLength]] >= n`.
unsafe fn typed_array_create(ctor: f64, args: &[f64], single_len: Option<usize>) -> f64 {
    let result = crate::object::js_new_function_construct(ctor, args.as_ptr(), args.len());
    let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(result) else {
        throw_type_error(b"Species constructor did not return a TypedArray object");
    };
    if let Some(n) = single_len {
        if (crate::typedarray_props::owner_length(addr) as usize) < n {
            throw_type_error(
                b"Derived TypedArray constructor created an array which was too small",
            );
        }
    }
    result
}

/// `TypedArraySpeciesCreate(O, « length »)` — the single-Number form used by
/// `slice`, `map`, and `filter`. Returns the result object as a NaN-boxed
/// pointer value; the caller fills its elements.
pub(crate) unsafe fn species_create_length(choice: &SpeciesChoice, kind: u8, len: usize) -> f64 {
    match choice {
        SpeciesChoice::Default => {
            let out = typed_array_alloc(kind, len as u32);
            f64::from_bits(JSValue::pointer(out as *const u8).bits())
        }
        SpeciesChoice::Custom(c) => typed_array_create(*c, &[len as f64], Some(len)),
    }
}

/// `TypedArraySpeciesCreate(O, « buffer, byteOffset, length »)` — the
/// multi-argument form used by `subarray`. The default path is handled by the
/// caller (it materializes a same-kind copy); here we only build the custom
/// case via `Construct`.
pub(crate) unsafe fn species_create_args(ctor: f64, args: &[f64]) -> f64 {
    typed_array_create(ctor, args, None)
}

/// Element store used to fill a species result of `kind` — `coerce_for_kind`
/// performs `ToNumber`/`ToBigInt` (the latter can throw), then `store_at`.
pub(crate) unsafe fn store_coerced(ta: *mut TypedArrayHeader, index: usize, kind: u8, raw: f64) {
    store_at(ta, index, bigint::coerce_for_kind(kind, raw));
}

/// `ToNumber` for the Uint8Array-buffer element path.
pub(crate) fn to_number(raw: f64) -> f64 {
    jsvalue_to_f64(raw)
}

/// Copy `count` elements of `src[from..]` into the species result `result`
/// (a NaN-boxed pointer). Each element is read with `load_at` and stored via
/// the result's `[[Set]]` (per-kind coercion).
pub(crate) unsafe fn copy_range_into(
    result: f64,
    src: *const TypedArrayHeader,
    from: usize,
    count: usize,
) {
    let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(result) else {
        return;
    };
    for i in 0..count {
        let v = load_at(src, from + i);
        crate::typedarray_props::species_result_store(addr, i, v);
    }
}

#[cold]
fn throw_type_error(msg: &[u8]) -> ! {
    super::throw_type_error(msg)
}

/// Resolve a NaN-boxed pointer value back to a `*mut TypedArrayHeader` (for
/// the legacy return type of the method helpers). Works for both the
/// `TypedArrayHeader` and Uint8Array-buffer representations — the pointer is
/// passed straight back to the caller, which re-NaN-boxes it.
pub(crate) fn result_as_ptr(result: f64) -> *mut TypedArrayHeader {
    let jv = JSValue::from_bits(result.to_bits());
    if jv.is_pointer() {
        return crate::value::js_nanbox_get_pointer(result) as *mut TypedArrayHeader;
    }
    result.to_bits() as *mut TypedArrayHeader
}
