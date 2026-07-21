//! TypedArray construction: `new TA(...)` runtime dispatch, plain-object /
//! array-like / iterable source materialization, and the typed-array→typed-array
//! copy path. Split out of `typedarray/mod.rs`.

use super::*;

use std::ptr;

use crate::array::ArrayHeader;

/// Allocate a typed array of `length` elements, all zero.
#[no_mangle]
pub extern "C" fn js_typed_array_new_empty(kind: i32, length: i32) -> *mut TypedArrayHeader {
    let len = typed_array_length_or_throw(length as f64);
    typed_array_alloc(kind as u8, len)
}

/// Allocate a typed array from a NaN-boxed JS value. Dispatches at runtime:
/// - POINTER_TAG (0x7FFD) → create from the pointed-to array's elements
/// - INT32_TAG  (0x7FFE) → use the tagged integer as the element count
/// - plain f64 / NaN    → use the numeric value as the element count
/// - anything else      → empty typed array
///
/// Mirrors `js_uint8array_new` for the generic typed-array constructor path.
/// Used when the codegen cannot determine at compile time whether the single
/// constructor argument is a length or a source array.
#[no_mangle]
pub extern "C" fn js_typed_array_new(kind: i32, val: f64) -> *mut TypedArrayHeader {
    let bits = val.to_bits();
    let top16 = (bits >> 48) as u16;
    // `new TA(arg)` with a non-object arg performs ToIndex(arg) = ToNumber(arg)
    // for the length. ToNumber(BigInt) and ToNumber(Symbol) are TypeErrors
    // (§7.1.4), so `new Int8Array(5n)` / `new Int8Array(Symbol())` must throw
    // rather than yielding an empty (BigInt) or garbage-copied (Symbol) array.
    if top16 == 0x7FFA {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    if top16 == 0x7FFD && unsafe { crate::symbol::js_is_symbol(val) } != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
    }
    if top16 == 0x7FFD {
        // POINTER_TAG — existing array pointer; copy its elements.
        let arr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        // Issue #654: a NaN-boxed pointer can also point at a registered
        // typed array (e.g. when the source flowed through a path that
        // re-applied POINTER_TAG). Detect via the registry and copy
        // through `typed_array_to_typed_array` so element values stay
        // numeric instead of being read as f64-NaN-boxed bits.
        let raw_addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if lookup_typed_array_kind(raw_addr).is_some() {
            return typed_array_copy_from_typed_array(
                kind as u8,
                raw_addr as *const TypedArrayHeader,
            );
        }
        if crate::buffer::is_registered_buffer(raw_addr) {
            if crate::buffer::is_any_array_buffer(raw_addr) {
                let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                return crate::typedarray_view::js_typed_array_view(
                    kind, val, undefined, undefined,
                );
            }
            return bigint::copy_from_uint8_buffer(
                kind as u8,
                raw_addr as *const crate::buffer::BufferHeader,
            );
        }
        // A plain object that is neither a typed array nor a buffer is consumed
        // per the spec's `new TypedArray(object)` path: if it exposes a
        // *callable* `@@iterator` it is iterated (InitializeTypedArrayFromList);
        // a non-callable non-nullish `@@iterator` is a TypeError; otherwise it
        // is read as an array-like (`ToLength(Get(obj, "length"))` then each
        // indexed element). Registered Maps/Sets keep the shared `Array.from`
        // materialization (their `@@iterator` is native, not a stored symbol
        // property). Functions are valid array-like/iterable sources too —
        // previously they were reinterpreted as an `ArrayHeader` (crash).
        if crate::map::is_registered_map(raw_addr)
            || crate::set::is_registered_set(raw_addr)
            || crate::array::is_builtin_iterator_class_id(raw_addr)
            || crate::object::js_util_types_is_generator_object(val).to_bits()
                == crate::value::TAG_TRUE
        {
            // Built-in iterables whose `@@iterator` is native (not a stored
            // symbol property): Maps/Sets, builtin iterator objects, and
            // generator objects (Perry generators carry own `next`/`return`
            // closures and no `@@iterator` symbol prop). The shared
            // `Array.from` materialization drives these correctly.
            let materialized = crate::array::js_array_from_value(val);
            return js_typed_array_new_from_array(kind, materialized);
        }
        if crate::closure::is_closure_ptr(raw_addr) {
            return unsafe { typed_array_from_plain_object(kind as u8, val) };
        }
        if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_hdr = (raw_addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if unsafe { (*gc_hdr).obj_type } == crate::gc::GC_TYPE_OBJECT {
                return unsafe { typed_array_from_plain_object(kind as u8, val) };
            }
        }
        return js_typed_array_new_from_array(kind, arr);
    }
    if top16 == 0x7FFE {
        // INT32_TAG — lower 32 bits are the signed length.
        let n = (bits & 0xFFFF_FFFF) as i32;
        let len = typed_array_length_or_throw(n as f64);
        return typed_array_alloc(kind as u8, len);
    }
    if !(0x7FFC..=0x7FFF).contains(&top16) {
        // Issue #654: typed-array sources (`new Float64Array(otherTA)`)
        // arrive as raw `i64 → f64` bitcasts (no NaN-box tag) per the
        // typed-array constructor codegen. Without this arm the address
        // was treated as a numeric length and the result was an empty
        // array. Detect via the registry first; only fall back to the
        // numeric-length interpretation for genuine doubles.
        if top16 == 0 && bits >= 0x10000 {
            let addr = bits as usize;
            if lookup_typed_array_kind(addr).is_some() {
                return typed_array_copy_from_typed_array(
                    kind as u8,
                    addr as *const TypedArrayHeader,
                );
            }
        }
        // Plain IEEE double (including negative, NaN, ±Inf). Node applies
        // ToIndex: NaN → 0, truncate toward zero, and throw a RangeError on a
        // negative / out-of-range length (#3662).
        let len = typed_array_length_or_throw(val);
        return typed_array_alloc(kind as u8, len);
    }
    // Undefined → ToIndex(undefined) = 0. Null / bool / string run through
    // ToNumber then ToIndex, so `new TA(true)` and `new TA('1')` have length
    // 1 (previously all of these built an empty array).
    if bits == crate::value::TAG_UNDEFINED {
        return typed_array_alloc(kind as u8, 0);
    }
    let len = typed_array_length_or_throw(jsvalue_to_f64(val));
    typed_array_alloc(kind as u8, len)
}

/// `new TA(object)` for a plain object / function source (ES2024 §23.2.5.1
/// step 6.b.iii, InitializeTypedArrayFromList / InitializeTypedArrayFromArrayLike).
///
///   - `GetMethod(obj, @@iterator)`: a non-nullish, non-callable value is a
///     TypeError; a callable one drives the iterator protocol (each `next()`
///     may throw — propagate).
///   - Otherwise array-like: `len = ToLength(? Get(obj, "length"))` (a Symbol
///     length is a TypeError, a `valueOf` runs and may throw), then each
///     indexed element is read and coerced per kind (`ToNumber`/`ToBigInt`,
///     both observable / throwing).
///
/// Element values are fully collected BEFORE coercion begins, mirroring the
/// snapshot rule in `js_typed_array_new_from_array`.
unsafe fn typed_array_from_plain_object(kind: u8, val: f64) -> *mut TypedArrayHeader {
    let raw = typed_array_plain_object_values(val);
    typed_array_from_snapshot(kind, raw)
}

/// Collect the raw (uncoerced) element values of a plain-object / function
/// source per the spec's iterator-or-array-like resolution (see
/// `typed_array_from_plain_object` doc above). Observable: the `@@iterator`
/// validation/iteration, the `ToLength(Get(obj, "length"))` coercion, and
/// each indexed `Get` all run here and may throw.
unsafe fn typed_array_plain_object_values(val: f64) -> Vec<f64> {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let iter_wk = crate::symbol::well_known_symbol("iterator");
    let using_iter = if iter_wk.is_null() {
        undefined
    } else {
        let sym = f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
        crate::symbol::js_object_get_symbol_property(val, sym)
    };
    let ub = using_iter.to_bits();
    if ub != crate::value::TAG_UNDEFINED && ub != crate::value::TAG_NULL {
        let fn_raw = crate::value::js_nanbox_get_pointer(using_iter) as usize;
        if fn_raw < 0x10000 || !crate::closure::is_closure_ptr(fn_raw) {
            throw_type_error(b"object is not iterable");
        }
        let bound = crate::closure::clone_closure_rebind_this(using_iter.to_bits(), val);
        let iter = crate::closure::js_native_call_value(f64::from_bits(bound), ptr::null(), 0);
        let mut raw: Vec<f64> = Vec::new();
        while let Some(v) = crate::collection_iter::iterator_next_value(iter) {
            raw.push(v);
        }
        return raw;
    }
    // Array-like path.
    let len_val = object_like_get(val, "length");
    let n = jsvalue_to_f64(len_val);
    // ToLength: NaN / negative → 0, clamp to 2^53-1.
    let len = if n.is_nan() || n <= 0.0 {
        0.0
    } else {
        n.trunc().min(9_007_199_254_740_991.0)
    };
    // AllocateTypedArrayBuffer implementation limit (Node throws RangeError
    // for lengths past the max typed-array size).
    if len > u32::MAX as f64 {
        throw_range_error(format!("Invalid typed array length: {}", len as u64).as_bytes());
    }
    let len = len as u32;
    let mut raw: Vec<f64> = Vec::with_capacity(len as usize);
    for k in 0..len {
        raw.push(object_like_get(val, &k.to_string()));
    }
    raw
}

/// Collect the raw (uncoerced) source values for `%TypedArray%.from(source)`:
/// plain-object / function sources use the spec iterator-or-array-like
/// resolution (so a throwing `length` getter / `ToLength(Symbol)` / a
/// non-callable `@@iterator` propagate); every other shape (arrays, strings,
/// Maps, Sets, iterators, generators, buffers) goes through the shared
/// `Array.from` materialization.
pub(crate) unsafe fn typed_array_from_source_raw_values(val: f64) -> Vec<f64> {
    let bits = val.to_bits();
    if (bits >> 48) == 0x7FFD {
        let raw_addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        let special = crate::map::is_registered_map(raw_addr)
            || crate::set::is_registered_set(raw_addr)
            || crate::array::is_builtin_iterator_class_id(raw_addr)
            || crate::object::js_util_types_is_generator_object(val).to_bits()
                == crate::value::TAG_TRUE
            || lookup_typed_array_kind(raw_addr).is_some()
            || crate::buffer::is_registered_buffer(raw_addr)
            || crate::symbol::js_is_symbol(val) != 0;
        if !special {
            if crate::closure::is_closure_ptr(raw_addr) {
                return typed_array_plain_object_values(val);
            }
            if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
                let gc_hdr = (raw_addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_hdr).obj_type == crate::gc::GC_TYPE_OBJECT {
                    return typed_array_plain_object_values(val);
                }
            }
        }
    }
    let arr = crate::array::js_array_from_value(val);
    let len = crate::array::js_array_length(arr);
    (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i))
        .collect()
}

/// Coerce a snapshot of raw element values per `kind` (observable, may throw)
/// and store them into a freshly allocated typed array.
unsafe fn typed_array_from_snapshot(kind: u8, raw: Vec<f64>) -> *mut TypedArrayHeader {
    let vals: Vec<f64> = raw
        .into_iter()
        .map(|v| bigint::coerce_for_kind(kind, v))
        .collect();
    let ta = typed_array_alloc(kind, vals.len() as u32);
    for (i, v) in vals.iter().enumerate() {
        store_at(ta, i, *v);
    }
    ta
}

/// `Get(obj, name)` for a plain-object or function source value.
unsafe fn object_like_get(val: f64, name: &str) -> f64 {
    let raw = crate::value::js_nanbox_get_pointer(val) as usize;
    if crate::closure::is_closure_ptr(raw) {
        return crate::closure::closure_get_dynamic_prop(raw, name);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let v =
        crate::object::js_object_get_field_by_name(raw as *const crate::object::ObjectHeader, key);
    f64::from_bits(v.bits())
}

/// Copy elements from one typed array into a new typed array of `dst_kind`,
/// reading via `load_at` (so source-element semantics stay correct) and
/// writing via `store_at` (which clamps / truncates / sign-extends per
/// `dst_kind`). Used by both `js_typed_array_new` (constructor copy) and
/// `js_typed_array_new_from_array` when it discovers the source is a
/// typed array rather than an `ArrayHeader`.
fn typed_array_copy_from_typed_array(
    dst_kind: u8,
    src: *const TypedArrayHeader,
) -> *mut TypedArrayHeader {
    let src = clean_ta_ptr(src);
    if src.is_null() {
        return typed_array_alloc(dst_kind, 0);
    }
    unsafe {
        bigint::validate_copy_kinds(dst_kind, (*src).kind);
        let len = (*src).length;
        let out = typed_array_alloc(dst_kind, len);
        for i in 0..len as usize {
            let v = load_at(src, i);
            store_at(out, i, v);
        }
        out
    }
}

/// Allocate a typed array from a Perry array (each element coerced to the
/// per-kind numeric type).
#[no_mangle]
pub extern "C" fn js_typed_array_new_from_array(
    kind: i32,
    arr: *const ArrayHeader,
) -> *mut TypedArrayHeader {
    let kind = kind as u8;
    // Strip NaN-box from the array pointer if needed.
    let arr = {
        let bits = arr as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr
        }
    };
    if arr.is_null() || (arr as usize) < 0x1000 {
        return typed_array_alloc(kind, 0);
    }
    // Issue #654: caller may have handed us a typed-array pointer
    // misaddressed as `*const ArrayHeader`. The two headers differ in
    // layout, so reading element data as raw f64 produces garbage.
    // Detect via the registry and route through the typed-array copy.
    // (This must stay ahead of `clean_arr_ptr`: typed arrays are old-arena
    // allocations that can land below its macOS heap floor, #5484.)
    if lookup_typed_array_kind(arr as usize).is_some() {
        return typed_array_copy_from_typed_array(kind, arr as *const TypedArrayHeader);
    }
    // #6486: an array grown past its capacity by `push` moves, leaving a
    // GC_FLAG_FORWARDED header at the old address — and the caller may
    // still hold that stale pre-grow pointer. `clean_arr_ptr` follows the
    // forwarding chain (#233) exactly like every element read below does
    // via `js_array_get_f64`; raw-dereferencing `(*arr).length` instead
    // read the forwarding pointer's bytes as length/capacity, so
    // `new Float32Array(arr)` saw a garbage element count while
    // `arr.length` and indexed reads (which follow the chain) stayed
    // correct. It also validates the header and materializes lazy arrays.
    let arr = crate::array::clean_arr_ptr(arr);
    if arr.is_null() {
        return typed_array_alloc(kind, 0);
    }
    unsafe {
        let len = (*arr).length;
        // Snapshot the raw source values BEFORE any coercion. Per spec the
        // source list is fully collected first and only THEN are the elements
        // converted (`ToNumber`/`ToBigInt`) and stored. A converting element can
        // run user code (`valueOf`/`Symbol.toPrimitive`) that mutates the source
        // array — `Int32Array.from([0, { valueOf() { src.length = 0; return 100 }}, 2])`
        // must still yield `[0, 100, 2]`, not lose the trailing element. Reading
        // raw values first also keeps the snapshot ahead of the `typed_array_alloc`
        // GC point (#871).
        let raw: Vec<f64> = (0..len)
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect();
        let vals: Vec<f64> = raw
            .into_iter()
            .map(|v| bigint::coerce_for_kind(kind, v))
            .collect();
        let ta = typed_array_alloc(kind, len);
        for (i, v) in vals.iter().enumerate() {
            store_at(ta, i, *v);
        }
        ta
    }
}
