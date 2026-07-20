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
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    raw >= 0x10000 && !crate::symbol::is_registered_symbol(raw)
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
    if crate::object::descriptors_in_use() {
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
    crate::object::js_get_global_this_builtin_value(b"Array".as_ptr(), 5)
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
    // #6386 fast path: a plain dense `ArrayHeader` (not a proxy / subclass
    // instance) whose own-`constructor` cannot exist — no `"constructor"`
    // accessor was ever installed process-wide and the array's named-props
    // side table has no `constructor` entry — resolves through the by-name
    // walk to the intrinsic `Array`, i.e. `Default`. Behavior-identical to
    // the walk: the array property walk does not model prototype-level
    // `Array.prototype.constructor` mutation (verified against pre-change
    // main), and both own-`constructor` stores land in the two tables
    // consulted here. Skips the per-call key-string allocation and the
    // full property walk.
    {
        let jv = JSValue::from_bits(original.to_bits());
        if jv.is_pointer() {
            let raw = crate::value::js_nanbox_get_pointer(original) as usize;
            let arr = raw as *const crate::array::ArrayHeader;
            // Proxy check FIRST: a masked proxy id is not a heap pointer, so
            // the GcHeader deref below would read unmapped memory for one.
            if crate::array::array_ptr_as_proxy(arr).is_none()
                && raw >= crate::gc::GC_HEADER_SIZE + 0x1000
            {
                let hdr =
                    (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*hdr).obj_type == crate::gc::GC_TYPE_ARRAY
                    && !crate::object::constructor_accessor_ever_installed()
                    && crate::array::array_named_property_get_by_name(arr, "constructor").is_none()
                {
                    return SpeciesChoice::Default;
                }
            }
        }
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
/// as a NaN-boxed value (a plain array for the default case, or the
/// `Construct(species, « length »)` result). The caller populates its
/// elements (via [[Set]] / CreateDataProperty for the custom case). May throw
/// (poisoned constructor/@@species getter, or a non-constructor species).
pub(crate) unsafe fn array_species_create(original: f64, length: usize) -> f64 {
    array_species_create_with_capacity(original, length, 0).0
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
            let out = if capacity_hint > length as u32 {
                let arr = crate::array::js_array_alloc(capacity_hint);
                (*arr).length = length as u32;
                arr
            } else {
                crate::array::js_array_alloc_with_length(length as u32)
            };
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

/// `true` when `array_species_create` would take the default fast path — used
/// by callers that only need to *validate* the constructor (throwing on a bad
/// one) while keeping their existing plain-array result building, and want to
/// know whether a custom container must instead be populated element-by-element.
pub(crate) unsafe fn species_is_default(original: f64) -> bool {
    matches!(resolve_species(original), SpeciesChoice::Default)
}

/// `true` when a species `result` (NaN-boxed) is an ordinary `ArrayHeader` —
/// i.e. the default fast path — so the caller can use direct slot writes.
pub(crate) unsafe fn species_result_is_plain_array(result: f64) -> bool {
    let jv = JSValue::from_bits(result.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let raw = crate::value::js_nanbox_get_pointer(result) as usize;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let obj_type = {
        let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*hdr).obj_type
    };
    obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
}

/// Store `value` at `index` on a species `result` (NaN-boxed). A plain array
/// gets a direct slot write (+ length bump + GC slot note); any other object
/// gets a polymorphic indexed [[Set]] / data-property define.
pub(crate) unsafe fn species_result_set(result: f64, index: usize, value: f64) {
    let jv = JSValue::from_bits(result.to_bits());
    if jv.is_pointer() {
        let raw = crate::value::js_nanbox_get_pointer(result) as usize;
        if raw >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let obj_type = {
                let hdr =
                    (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                (*hdr).obj_type
            };
            if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let arr = raw as *mut ArrayHeader;
                super::js_array_set_f64(arr, index as u32, value);
                return;
            }
        }
        crate::object::js_object_set_index_polymorphic(raw as i64, index as f64, value);
    }
}
