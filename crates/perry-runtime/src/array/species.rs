//! `ArraySpeciesCreate` (ECMA-262 §23.1.5.1) and its `SpeciesConstructor`
//! reads for the `Array.prototype` methods that allocate a fresh result —
//! `map`, `filter`, `slice`, `splice`, `concat`.
//!
//! Each of those methods must, before populating the result:
//!   1. `Get(O, "constructor")` — runs any own accessor (observable, may throw).
//!   2. If that is an object, `Get(C, @@species)` (observable, may throw).
//!   3. Validate the resolved species is a constructor (else **TypeError**).
//!   4. `Construct(species, « length »)` for the result container.
//!
//! When there is no custom species (the overwhelmingly common case — a plain
//! array whose `constructor` resolves to the intrinsic `Array`) we take a fast
//! path that allocates a plain `ArrayHeader` directly: observationally
//! identical, since `Array[@@species]` returns `Array` itself.
//!
//! Only that default arm hands the caller a container it may fill by raw slot
//! writes. Whatever a species constructor returns — a typed array, a Number
//! object (`new Object(count)`), a frozen or pre-filled Array, an object with
//! accessors — is populated through the object's own `[[DefineOwnProperty]]`
//! ([`species_result_define`], CreateDataPropertyOrThrow) and, for the methods
//! that end with `Set(A, "length", n, true)`, [`species_result_set_length`].

use super::ArrayHeader;
use crate::value::{JSValue, TAG_NULL, TAG_UNDEFINED};

/// The resolved species for an `ArraySpeciesCreate`: either the default
/// intrinsic (fast plain-array allocation) or a user `Construct` target.
pub(crate) enum SpeciesChoice {
    Default,
    Custom(f64),
}

/// `Type(value) is Object` — a heap pointer that is not a Symbol.
fn is_object_value(value: f64) -> bool {
    crate::builtins::arithmetic::js_value_is_object(value)
}

/// `IsConstructor(value)` — a user `class` ref, or a callable that is not a
/// non-constructable built-in. Mirrors the typed-array species check.
fn is_constructor(value: f64) -> bool {
    if crate::object::class_ref_id(value).is_some() {
        return true;
    }
    crate::collection_iter::is_callable(value)
        && !crate::object::builtin_closure_is_non_constructable_value(value)
}

/// `Get(originalArray, "constructor")` — fires an own accessor and walks the
/// prototype chain (resolving to `Array.prototype.constructor` = the intrinsic
/// `Array` for an ordinary array). Propagates a poisoned-getter exception.
unsafe fn read_constructor(original: f64) -> f64 {
    // An own `constructor` ACCESSOR installed directly on the array
    // (`Object.defineProperty(a, 'constructor', { get })`) lives in the
    // descriptor side table, which the generic property read below does not
    // consult for array receivers — fire it here (its throw propagates;
    // test262 {map,filter,splice,concat}/create-ctor-poisoned).
    {
        let raw = crate::value::js_nanbox_get_pointer(original) as usize;
        if raw != 0 {
            if let Some(acc) = crate::object::get_accessor_descriptor(raw, "constructor") {
                if acc.get != 0 {
                    return f64::from_bits(
                        crate::object::invoke_accessor_getter(acc.get, original).bits(),
                    );
                }
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }
    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    let key_v = f64::from_bits(JSValue::string_ptr(key).bits());
    crate::object::js_object_get_property_key(original, key_v)
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

/// The intrinsic `Array` constructor value (for the default fast-path check).
fn intrinsic_array() -> f64 {
    crate::object::builtin_constructor_value("Array")
}

/// The allocation-free part of the species resolution: `true` when `original`
/// is an ordinary Array that owns no `constructor` and inherits the intrinsic
/// one as a data property, so `ArraySpeciesCreate` is `ArrayCreate` and no
/// user code can run. Header peeks only: `false` just means "ask
/// [`array_species_create`]", which may run getters and a constructor.
#[inline]
pub(crate) unsafe fn default_species_is_certain(original: f64) -> bool {
    let jv = JSValue::from_bits(original.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let raw = crate::value::js_nanbox_get_pointer(original) as usize;
    let arr = raw as *const crate::array::ArrayHeader;
    // Proxy check FIRST: a masked proxy id is not a heap pointer, so the
    // GcHeader deref below would read unmapped memory for one.
    if crate::array::array_ptr_as_proxy(arr).is_some() || raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    crate::gc_type_is_array!((*hdr).obj_type)
        && (*hdr)._reserved & crate::gc::GC_ARRAY_CUSTOM_PROTO == 0
        && !super::named_props::array_named_property_has_by_name(arr, "constructor")
        && crate::object::get_accessor_descriptor(
            crate::array::array_prototype_addr_if_resolved(),
            "constructor",
        )
        .is_none()
        && !crate::object::descriptor_state::owner_key_is_accessor(
            crate::array::object_prototype_addr_if_resolved(),
            b"constructor",
        )
}

/// `SpeciesConstructor` portion of `ArraySpeciesCreate`: resolve the result
/// constructor, returning `Default` for the intrinsic / undefined case and
/// `Custom(S)` for a usable user constructor. Throws on a non-constructor
/// species; propagates any user getter exception.
unsafe fn resolve_species(original: f64) -> SpeciesChoice {
    // ECMA-262 §23.1.5.1: only arrays consult `constructor`; a non-array
    // receiver (the generic `.call(arrayLike)` form) always gets a plain array.
    if crate::value::js_is_truthy(crate::array::js_array_is_array(original)) == 0 {
        return SpeciesChoice::Default;
    }
    if default_species_is_certain(original) {
        return SpeciesChoice::Default;
    }
    // step 3: C = Get(O, "constructor"). step 5: if Type(C) is Object,
    // C = Get(C, @@species); a null species → undefined.
    let mut c = read_constructor(original);
    if is_object_value(c) {
        let s = get_species(c);
        c = if s.to_bits() == TAG_NULL {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            s
        };
    }
    // step 6: undefined → default ArrayCreate.
    if JSValue::from_bits(c.to_bits()).is_undefined() {
        return SpeciesChoice::Default;
    }
    // Fast path: the intrinsic Array constructor → plain allocation
    // (observationally identical to Construct(%Array%, « len »)).
    if c.to_bits() == intrinsic_array().to_bits() {
        return SpeciesChoice::Default;
    }
    // step 7: a non-constructor (number/string/null/non-callable) → TypeError.
    if !is_constructor(c) {
        throw_not_constructor();
    }
    SpeciesChoice::Custom(c)
}

#[cold]
fn throw_not_constructor() -> ! {
    crate::collection_iter::throw_type_error("Array species constructor is not a constructor");
}

/// `ArraySpeciesCreate(originalArray, length)` — returns the result container
/// as a NaN-boxed value and whether the DEFAULT species arm produced it (see
/// [`array_species_create_with_capacity`]): only then is it a fresh plain
/// array of `length` holes the caller may fill with raw slot writes. Any other
/// result is filled with [`species_result_define`]. May throw (poisoned
/// constructor/@@species getter, a non-constructor species, the constructor).
pub(crate) unsafe fn array_species_create(original: f64, length: usize) -> (f64, bool) {
    array_species_create_with_capacity(original, length, 0)
}

/// [`array_species_create`] with a result-capacity hint (#6386). Capacity is
/// unobservable, so when the default species applies the plain result can be
/// allocated at its final size up front — sparing the concat/slice-style
/// callers the grow-doubling allocations and copies of populating a
/// `MIN_ARRAY_CAPACITY` array element-by-element. The hint must come from
/// pure header peeks (no user code); a custom species constructor ignores it.
///
/// The second return is `true` only when the DEFAULT species branch ran —
/// i.e. the result is a freshly allocated, empty, unfrozen plain array. A
/// custom `@@species` constructor can RETURN a plain-typed array too (frozen,
/// sealed, or pre-populated), so callers wanting raw-write access must gate
/// on this flag, not on the result's GC type.
pub(crate) unsafe fn array_species_create_with_capacity(
    original: f64,
    length: usize,
    capacity_hint: u32,
) -> (f64, bool) {
    match resolve_species(original) {
        SpeciesChoice::Default => {
            let out = default_species_array(length, capacity_hint);
            (
                f64::from_bits(JSValue::pointer(out as *const u8).bits()),
                true,
            )
        }
        SpeciesChoice::Custom(c) => {
            let args = [length as f64];
            (
                crate::object::js_new_function_construct(c, args.as_ptr(), args.len()),
                false,
            )
        }
    }
}

/// The default species arm's result: `ArrayCreate(length)` as a fresh plain
/// array of `length` holes, allocated at `capacity_hint` slots when that is
/// larger (capacity is unobservable).
#[inline]
pub(crate) unsafe fn default_species_array(length: usize, capacity_hint: u32) -> *mut ArrayHeader {
    if capacity_hint > length as u32 {
        let arr = crate::array::js_array_alloc(capacity_hint);
        (*arr).length = length as u32;
        arr
    } else {
        crate::array::js_array_alloc_with_length(length as u32)
    }
}

/// `CreateDataPropertyOrThrow(A, ToString(index), value)` on the result of a
/// species constructor (never the default arm's plain array, which callers
/// fill directly). The property is defined through the result's own
/// `[[DefineOwnProperty]]`, so an accessor or an inherited setter at the index
/// is not called, an Array result grows its `length`, and a **TypeError** is
/// thrown when the definition is rejected: a frozen, sealed or non-extensible
/// result, a non-configurable property in the way, a typed array without that
/// index.
///
/// May allocate, run a proxy trap and throw: the caller keeps `result`,
/// `value` and its own receiver rooted across the call.
pub(crate) unsafe fn species_result_define(result: f64, index: usize, value: f64) {
    // An elements-backed Array-subclass instance (`class V extends Array`,
    // the usual species): an in-bounds or appending store into its elements
    // array is that definition. It declines for a frozen, sealed or
    // non-extensible instance and for an index past the end.
    if index <= u32::MAX as usize
        && crate::array::array_subclass_fast_index_set(result, index as u32, value)
    {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let result_h = scope.root_nanbox_f64(result);
    let key_h = scope.root_nanbox_f64(index as f64);
    if !crate::proxy::create_data_property(result, index as f64, value) {
        let key = crate::object::js_to_property_key(key_h.get_nanbox_f64());
        key_h.set_nanbox_f64(key);
        crate::object::object_ops::throw_definition_rejected(&scope, &result_h, &key_h);
    }
}

/// `Set(A, "length", length, true)` on the result of a species constructor:
/// the last step of `slice`, `splice` and `concat` (`map`, `filter`, `flat`
/// and `flatMap` have none). A `length` setter runs; a getter-only or
/// non-writable `length` and a frozen result throw a **TypeError**, as does a
/// typed array, whose `length` is a getter of `%TypedArray%.prototype`.
///
/// May allocate, run user code and throw, as [`species_result_define`].
pub(crate) unsafe fn species_result_set_length(result: f64, length: usize) {
    if crate::proxy_hooks::proxy_is_proxy(result) != 0 {
        super::push_pop::proxy_set_str_key(result, b"length", length as f64);
        return;
    }
    let raw = crate::value::js_nanbox_get_pointer(result) as usize;
    if crate::typedarray_hook_slots::lookup_typed_array_kind(raw).is_some()
        || crate::buffer::is_registered_buffer(raw)
    {
        crate::collection_iter::throw_type_error(
            "Cannot set property length of [object Object] which has only a getter",
        );
    }
    super::generic_object::al_set_length(result, length as i64);
}
