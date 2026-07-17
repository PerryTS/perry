//! `Array.from` API-level validation + mapped-call semantics (#2773) and the
//! spec-complete variadic, non-mutating `Array.prototype.concat` (#2805).
//!
//! These are the JS-API entry points. The low-level materialization helpers
//! (`js_array_clone`, `js_array_from_arraylike`, ...) stay in their own
//! modules; this module layers the spec validation (`TypeError` for nullish
//! sources, callability checks, `Symbol.isConcatSpreadable`) on top of them.
use super::{
    clean_arr_ptr, js_array_alloc, js_array_clone, js_array_is_array, js_array_push_f64,
    js_array_set_f64_extend, ArrayHeader,
};
use crate::closure::ClosureHeader;
use crate::value::JSValue;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

/// Throw `TypeError: <receiver> is not iterable` (matches Node's wording for
/// nullish `Array.from` / `Uint8Array.from` sources).
#[cold]
fn throw_not_iterable(receiver: &str) -> ! {
    let msg = format!(
        "{} is not iterable (cannot read property Symbol(Symbol.iterator))",
        receiver
    );
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value));
}

/// Throw `TypeError: <value> is not a function` for a non-callable mapFn,
/// matching Node's `Array.from([1], 1)` → "number 1 is not a function".
#[cold]
fn throw_map_fn_not_callable(map_fn: f64) -> ! {
    let value_str = {
        let sp = crate::value::js_jsvalue_to_string(map_fn);
        if sp.is_null() {
            String::new()
        } else {
            unsafe {
                let header = &*(sp as *const crate::string::StringHeader);
                let bytes_ptr =
                    (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
                std::str::from_utf8(slice).unwrap_or("").to_string()
            }
        }
    };
    let jv = JSValue::from_bits(map_fn.to_bits());
    let type_name = if jv.is_null() || jv.is_pointer() {
        "object"
    } else if map_fn.to_bits() >> 48 == 0x7FFF {
        "string"
    } else {
        "number"
    };
    let msg = format!("{} {} is not a function", type_name, value_str);
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value));
}

/// `Array.from(source)` — the non-mapped form. Per ECMA-262 §23.1.2.1,
/// `null`/`undefined` sources throw `TypeError` (they have no
/// `Symbol.iterator`), while numbers/booleans/symbols are non-iterable
/// non-objects that materialize to an empty array. All valid object/iterable
/// sources delegate to the existing `js_array_clone` materialization (which
/// covers arrays, sets, maps, strings, typed arrays, buffers, iterators, and
/// array-likes).
///
/// Takes the raw NaN-boxed f64 value (NOT a pre-unboxed pointer) so it can
/// inspect the tag bits before stripping.
#[no_mangle]
pub extern "C" fn js_array_from_value(boxed: f64) -> *mut ArrayHeader {
    let bits = boxed.to_bits();
    if bits == TAG_UNDEFINED {
        throw_not_iterable("undefined");
    }
    if bits == TAG_NULL {
        throw_not_iterable("object null");
    }
    // #6454: `Array.from(SomeClass)` where the class DECLARATION (an
    // INT32-tagged ClassRef) carries a — possibly inherited, #36/#321 —
    // `[Symbol.iterator]`: drive it. A class WITHOUT one falls through to the
    // non-iterable branch below (node: `Array.from(class C {})` → `[]` via the
    // array-like path, not a throw — unlike spread/for-of).
    if crate::object::class_ref_id(boxed).is_some()
        && crate::symbol::class_ref_resolves_iterator(boxed)
    {
        let iter = crate::symbol::js_get_iterator(boxed);
        return crate::array::js_iterator_to_array(iter);
    }
    // Numbers / booleans / strings handled inside js_array_clone:
    //  - numbers/booleans aren't pointers → empty array.
    //  - strings → per-codepoint materialization.
    // Pointers (objects/arrays/iterables) materialize via js_array_clone.
    let ptr_bits = if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    unsafe {
        if let Some(arr) =
            crate::object::arguments_object_to_array(ptr_bits as *const crate::object::ObjectHeader)
        {
            return arr;
        }
    }
    // A raw ArrayBuffer / SharedArrayBuffer is NOT array-like: it has no
    // `length` property and no [[Symbol.iterator]], so `Array.from(arrayBuffer)`
    // takes the array-like branch with length 0 and yields an empty array,
    // rather than materializing its bytes (test262
    // Array/from/items-is-arraybuffer). Node Buffers and typed arrays ARE
    // array-like / iterable and still materialize their elements via
    // js_array_clone below — only the backing ArrayBuffer is registered here,
    // and Array.from receives the typed-array/Buffer pointer, not the backing
    // buffer. (Spread `[...arrayBuffer]` goes through js_array_clone_for_spread,
    // not this path, so its separate semantics are unaffected.)
    if ptr_bits != 0
        && (crate::buffer::is_array_buffer(ptr_bits)
            || crate::buffer::is_shared_array_buffer(ptr_bits))
    {
        return js_array_alloc(0);
    }
    js_array_clone(ptr_bits as *const ArrayHeader)
}

#[used]
static KEEP_ARRAY_FROM_VALUE: extern "C" fn(f64) -> *mut ArrayHeader = js_array_from_value;

/// `Array.from(source, mapFn, thisArg)` — the mapped form. Throws for nullish
/// sources (like the non-mapped form), validates that `mapFn` is callable,
/// then materializes the source and calls `mapFn(value, index)` for each
/// element with `thisArg` bound as the function's `this`.
///
/// All three arguments are raw NaN-boxed f64 values. `this_arg` may be
/// `undefined` (no binding).
#[no_mangle]
pub extern "C" fn js_array_from_mapped(
    src_boxed: f64,
    map_fn: f64,
    this_arg: f64,
) -> *mut ArrayHeader {
    // The `this` value for a direct `Array.from(items, mapFn)` call is the
    // `%Array%` intrinsic, so the result is always a plain Array — drive the
    // spec algorithm with no constructor and unbox the array pointer.
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let result = array_from_full(undefined, src_boxed, map_fn, this_arg);
    clean_arr_ptr(crate::value::js_nanbox_get_pointer(result) as *const ArrayHeader)
        as *mut ArrayHeader
}

#[used]
static KEEP_ARRAY_FROM_MAPPED: extern "C" fn(f64, f64, f64) -> *mut ArrayHeader =
    js_array_from_mapped;

/// Validate a mapFn argument is callable, returning its `ClosureHeader*`.
/// Throws `TypeError` for any non-callable value.
fn resolve_callable(map_fn: f64) -> *const ClosureHeader {
    let jv = JSValue::from_bits(map_fn.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<ClosureHeader>();
        if !ptr.is_null() && crate::closure::is_closure_ptr(ptr as usize) {
            return ptr;
        }
    }
    throw_map_fn_not_callable(map_fn);
}

/// `Array.prototype.concat(...args)` — spec-complete, non-mutating.
///
/// Returns a NEW array; the receiver is never mutated. Each argument is
/// appended in order, spreading per ECMA-262 §23.1.3.1 / IsConcatSpreadable:
///   - the receiver and array arguments spread by default,
///   - an array with `Symbol.isConcatSpreadable === false` is a single element,
///   - a non-array object with `Symbol.isConcatSpreadable === true` is spread
///     as an array-like (`length` + indexed reads),
///   - every other value (primitives, plain objects) is a single element.
///
/// `args_ptr` points at `count` raw NaN-boxed f64 argument values (alloca buffer
/// built by codegen / passed straight from the dynamic dispatcher).
#[no_mangle]
pub extern "C" fn js_array_concat_variadic(
    recv: *const ArrayHeader,
    args_ptr: *const f64,
    count: i32,
) -> *mut ArrayHeader {
    // Defensive receiver re-dispatch: a variable whose STATIC type was
    // inferred `Array` but was reassigned to a plain object at runtime
    // (`var x = [0]; … x = {0: 0}; x.concat()`, test262 S15.4.4.4_A3_T1)
    // reaches this dense entry with an ObjectHeader — reading it as an
    // ArrayHeader spreads garbage. Route plain objects/closures through the
    // generic array-like engine instead.
    {
        let raw = recv as usize;
        if raw >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let obj_type = unsafe {
                let hdr =
                    (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                (*hdr).obj_type
            };
            if obj_type == crate::gc::GC_TYPE_OBJECT || obj_type == crate::gc::GC_TYPE_CLOSURE {
                let recv_value = f64::from_bits(JSValue::pointer(recv as *const u8).bits());
                let result = crate::array::js_arraylike_concat(recv_value, args_ptr, count);
                return crate::value::js_nanbox_get_pointer(result) as *mut ArrayHeader;
            }
        }
    }
    // ECMA-262 §23.1.3.5 step 2: ArraySpeciesCreate(O, 0) runs BEFORE any
    // element is read — it fires the receiver's `constructor` / `@@species`
    // accessors (propagating a poisoned-getter throw) and throws TypeError on
    // a non-constructor species (test262 concat/create-ctor-poisoned,
    // create-ctor-non-object, create-non-array).
    let recv_value = f64::from_bits(JSValue::pointer(recv as *const u8).bits());
    // #6386: size the result once. The hint is pure header peeks
    // (unobservable), so it runs before the observable species resolution
    // without reordering anything the spec sequences.
    let cap_hint = unsafe { concat_capacity_hint(recv, args_ptr, count) };
    let (result_box, species_was_default) = unsafe {
        crate::array::species::array_species_create_with_capacity(recv_value, 0, cap_hint)
    };
    let result_is_plain = species_was_default
        || unsafe { crate::array::species::species_result_is_plain_array(result_box) };
    let result = if result_is_plain {
        crate::value::js_nanbox_get_pointer(result_box) as *mut ArrayHeader
    } else {
        // Custom species container: build the elements in a plain staging
        // array first, then CreateDataProperty them onto the container below.
        js_array_alloc(cap_hint)
    };
    // #6386 all-dense bulk path: when the DEFAULT species ran (so the result
    // is guaranteed fresh, empty, and unfrozen — a custom `@@species` ctor
    // can return a plain-typed array that is none of those), the spreadable
    // gate is closed, and every source is a plain dense hole-free array (or
    // a non-pointer single value), fill the pre-sized result with one copy
    // pass and ONE layout/barrier rebuild — no per-source rebuild, no
    // spreadable reads, no growth. Falls through (result still empty) when
    // anything exotic shows up.
    if species_was_default && !crate::symbol::concat_spreadable_symbol_ever_set() {
        if let Some(out) = unsafe { try_concat_all_dense(result, recv, args_ptr, count) } {
            return out;
        }
    }
    // The receiver itself is always spread (it's the array on which `.concat`
    // was invoked). Materialize a clone to read its elements safely.
    let result = append_spread_array(result, recv as *const ArrayHeader);
    let mut result = result;
    if !args_ptr.is_null() && count > 0 {
        for i in 0..count as usize {
            let value = unsafe { *args_ptr.add(i) };
            result = append_concat_arg(result, value);
        }
    }
    if !result_is_plain {
        unsafe {
            let len = (*result).length as usize;
            let elems = (result as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            for i in 0..len {
                let v = *elems.add(i);
                if v.to_bits() != crate::value::TAG_HOLE {
                    crate::array::species::species_result_set(result_box, i, v);
                }
            }
            return crate::value::js_nanbox_get_pointer(result_box) as *mut ArrayHeader;
        }
    }
    result
}

#[used]
static KEEP_ARRAY_CONCAT_VARIADIC: extern "C" fn(
    *const ArrayHeader,
    *const f64,
    i32,
) -> *mut ArrayHeader = js_array_concat_variadic;

/// Append a single concat argument to `result`, applying spreadability rules.
pub(crate) fn append_concat_arg(result: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    let bits = value.to_bits();
    let jv = JSValue::from_bits(bits);
    if !jv.is_pointer() {
        // Primitive (number / bool / undefined / null / string-by-tag): one element.
        return js_array_push_f64(result, value);
    }
    let raw_addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;

    // Is the spreadable flag explicitly set?
    let spreadable = read_concat_spreadable(value);

    // `class X extends Array` — the instance is object-backed, so the dense
    // `ArrayHeader` spread below would misread it. `js_array_is_array` reports
    // true for it, so intercept before that branch and spread a dense snapshot
    // of its elements (honoring an explicit `@@isConcatSpreadable === false`).
    if crate::array::is_array_subclass_instance(value) {
        if spreadable == Some(false) {
            return js_array_push_f64(result, value);
        }
        let snap = crate::array::array_subclass_dense_snapshot(value);
        let snap_ptr = (snap.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader;
        return append_spread_array(result, snap_ptr);
    }

    // Arrays (and set/map/typed-array/buffer that concat treats array-like via
    // js_array_concat) spread by default, unless @@isConcatSpreadable === false.
    let is_array = js_array_is_array(value).to_bits() == 0x7FFC_0000_0000_0004;
    if is_array {
        if spreadable == Some(false) {
            return js_array_push_f64(result, value);
        }
        return append_spread_array(result, raw_addr as *const ArrayHeader);
    }
    // Non-array object explicitly marked spreadable → spread as array-like.
    if spreadable == Some(true) {
        let arr = unsafe {
            super::js_array_from_arraylike(raw_addr as *const crate::object::ObjectHeader)
        };
        return append_spread_array(result, arr as *const ArrayHeader);
    }
    // Everything else (plain object, function, etc.) is a single element.
    js_array_push_f64(result, value)
}

/// Read `value[Symbol.isConcatSpreadable]`. Returns `Some(true)`/`Some(false)`
/// when the property is a defined boolean (using JS truthiness), or `None` when
/// the property is absent/undefined (→ default behavior).
fn read_concat_spreadable(value: f64) -> Option<bool> {
    // #6386 fast path: while no `Symbol.isConcatSpreadable` property has
    // ever been installed process-wide (monotonic gate over every symbol
    // install funnel), the lookup below is guaranteed to produce undefined
    // for any non-proxy value — and to run no user code — so skip the
    // symbol-table ladder (a mutex acquisition per argument). Proxies are
    // excluded: their `get` trap can materialize the property without any
    // install having happened.
    if !crate::symbol::concat_spreadable_symbol_ever_set()
        && crate::proxy::js_proxy_is_proxy(value) == 0
    {
        return None;
    }
    let sym = crate::symbol::well_known_symbol("isConcatSpreadable");
    if sym.is_null() {
        return None;
    }
    let sym_f64 = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    let flag = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_f64) };
    let fbits = flag.to_bits();
    if fbits == TAG_UNDEFINED {
        return None;
    }
    Some(crate::value::js_is_truthy(flag) != 0)
}

const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

#[inline]
fn jsv_is_array(value: f64) -> bool {
    js_array_is_array(value).to_bits() == TAG_TRUE
}

/// `IsConstructor(value)` — true for any callable that is not a built-in
/// non-constructable function (arrow/method semantics are not tracked for
/// user closures, so a plain `function` declaration reads as a constructor,
/// matching how Perry models user functions).
fn is_constructor_value(value: f64) -> bool {
    // A user `class` is an INT32-tagged class reference, not a closure pointer,
    // so `is_callable` misses it; recognize it directly.
    if crate::object::class_ref_id(value).is_some() {
        return true;
    }
    crate::collection_iter::is_callable(value)
        && !crate::object::builtin_closure_is_non_constructable_value(value)
}

/// Build a fresh `{value, writable, enumerable, configurable}` (all true)
/// data descriptor object, returned NaN-boxed — the shape
/// `CreateDataProperty` hands to `[[DefineOwnProperty]]`.
unsafe fn data_property_descriptor(value: f64) -> f64 {
    let desc = crate::object::js_object_alloc(0, 4);
    let set = |name: &[u8], v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(desc, key, v);
    };
    set(b"value", value);
    set(b"writable", f64::from_bits(TAG_TRUE));
    set(b"enumerable", f64::from_bits(TAG_TRUE));
    set(b"configurable", f64::from_bits(TAG_TRUE));
    f64::from_bits(JSValue::pointer(desc as *const u8).bits())
}

/// `CreateDataPropertyOrThrow(A, index, value)` — returns `true` on success.
/// A freshly-`ArrayCreate`d result takes the fast array-set path (never
/// fails); a `Construct`-produced object routes through `[[DefineOwnProperty]]`
/// (`Reflect.defineProperty` semantics), which reports `false` for a
/// non-extensible target or a non-configurable existing index.
unsafe fn create_index_data_property(result: f64, index: usize, value: f64) -> bool {
    if jsv_is_array(result) {
        let arr = crate::value::js_nanbox_get_pointer(result) as *mut ArrayHeader;
        js_array_set_f64_extend(arr, index as u32, value);
        return true;
    }
    let key_str = index.to_string();
    let key = crate::string::js_string_from_bytes(key_str.as_ptr(), key_str.len() as u32);
    let key_f64 = f64::from_bits(JSValue::string_ptr(key).bits());
    // `CreateDataProperty` installs a *fresh* fully-defaulted data property,
    // fully replacing a configurable existing one (even a non-writable one).
    // Perry's ordinary `[[DefineOwnProperty]]` merges rather than replaces, so
    // drop a configurable-but-non-writable existing descriptor first; the new
    // definition then lands writable. A non-configurable existing property is
    // left in place so the define correctly reports failure.
    let obj_addr = crate::value::js_nanbox_get_pointer(result) as usize;
    if let Some(attrs) = crate::object::get_property_attrs(obj_addr, &key_str) {
        if !attrs.writable() && attrs.configurable() {
            // Reset the existing (configurable) descriptor to a fully-default
            // writable data property so the subsequent define overwrites both
            // its value and attributes — Perry's `[[DefineOwnProperty]]` merges
            // onto an existing property and would otherwise keep `writable:false`
            // and the stale value.
            crate::object::set_property_attrs(
                obj_addr,
                key_str.clone(),
                crate::object::PropertyAttrs::new(true, true, true),
            );
        }
    }
    let desc = data_property_descriptor(value);
    crate::proxy::js_reflect_define_property(result, key_f64, desc).to_bits() == TAG_TRUE
}

/// `Set(A, "length", len, true)`. Arrays take the fast length set; a
/// `Construct`-produced object goes through `OrdinarySet` (honoring an
/// inherited `length` accessor — a poisoned setter throws and propagates,
/// a failed write throws in the strict path).
unsafe fn set_result_length(result: f64, len: usize) {
    if jsv_is_array(result) {
        let arr = crate::value::js_nanbox_get_pointer(result) as *mut ArrayHeader;
        crate::array::js_array_set_length(arr, len as f64);
        return;
    }
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let key_f64 = f64::from_bits(JSValue::string_ptr(key).bits());
    crate::proxy::js_put_value_set(result, key_f64, len as f64, result, 1);
}

#[cold]
fn throw_cannot_define_property(index: usize) -> ! {
    let msg = format!("Cannot define property {}, object is not extensible", index);
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value));
}

/// Call `mapfn(value, index)` with `this_arg` bound. Restores the implicit
/// `this` even on a thrown completion (then re-throws). Used by the
/// array-like / snapshot branches where there is no live iterator to close.
fn call_map_fn(mapfn: f64, this_arg: f64, value: f64, index: usize) -> f64 {
    match crate::collection_iter::call_with_this_capturing_throw(
        mapfn,
        this_arg,
        &[value, index as f64],
    ) {
        Ok(v) => v,
        Err(e) => crate::exception::js_throw(e),
    }
}

/// `ToLength(? Get(arrayLike, "length"))` for the array-like branch.
fn array_like_length(items: f64) -> usize {
    let raw = crate::value::js_nanbox_get_pointer(items);
    if raw == 0 {
        return 0;
    }
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let length_val = crate::object::js_object_get_field_by_name_f64(
        raw as *const crate::object::ObjectHeader,
        key,
    );
    let n = crate::builtins::js_number_coerce(length_val);
    if n.is_nan() || n <= 0.0 {
        return 0;
    }
    let n = n.floor();
    let max = (1u64 << 53) as f64 - 1.0;
    if n > max {
        // 2^53 - 1 (JS max safe integer). usize can't represent this on 32-bit
        // targets (arm64_32 watchOS, wasm32), so saturate to usize::MAX there.
        return usize::try_from((1u64 << 53) - 1).unwrap_or(usize::MAX);
    }
    n as usize
}

enum IterSourceKind {
    /// A plain Array — iterate by live index read so a `mapfn` that mutates
    /// the source mid-iteration observes the updated elements (spec array
    /// iterator semantics).
    LiveArray,
    /// Strings / Sets / Maps / typed-arrays / buffers / `arguments` — no
    /// abrupt-close or live-mutation test exercises these, so materialize a
    /// snapshot (matches the pre-existing `js_array_clone` behavior).
    Snapshot,
    /// A generic object iterator — drive the iterator protocol element by
    /// element so a `mapfn` throw (or a failed `CreateDataProperty`) triggers
    /// `IteratorClose`.
    Generic,
}

/// `GetMethod(items, @@iterator) is not undefined` — the branch test for
/// ECMA-262 §23.1.2.1 step 5. Beyond `collection_iter::is_iterable` (which
/// keys on a side-table `@@iterator`), this also catches built-in iterator
/// objects and bare iterators/generators, whose `@@iterator` dispatches via
/// class id (so a direct symbol read returns `undefined`) but which still
/// drive `.next()`. Mirrors the iterable detection in `js_array_clone`.
fn items_is_iterable(items: f64) -> bool {
    if crate::collection_iter::is_iterable(items) {
        return true;
    }
    let raw = crate::value::js_nanbox_get_pointer(items) as usize;
    if raw == 0 {
        return false;
    }
    if crate::array::is_builtin_iterator_class_id(raw) {
        return true;
    }
    // A bare iterator / generator object exposes a callable `next`.
    let next_key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
    let next_val = unsafe {
        crate::object::js_object_get_field_by_name(
            raw as *const crate::object::ObjectHeader,
            next_key,
        )
    };
    if next_val.is_undefined() {
        return false;
    }
    let next_ptr = crate::value::js_nanbox_get_pointer(f64::from_bits(next_val.bits())) as usize;
    crate::closure::is_closure_ptr(next_ptr)
}

fn classify_iter_source(items: f64) -> IterSourceKind {
    if jsv_is_array(items) {
        return IterSourceKind::LiveArray;
    }
    let jv = JSValue::from_bits(items.to_bits());
    if jv.is_any_string() {
        return IterSourceKind::Snapshot;
    }
    let raw = crate::value::js_nanbox_get_pointer(items) as usize;
    if raw != 0 {
        if crate::set::is_registered_set(raw)
            || crate::map::is_registered_map(raw)
            || crate::typedarray::lookup_typed_array_kind(raw).is_some()
            || crate::buffer::is_registered_buffer(raw)
        {
            return IterSourceKind::Snapshot;
        }
        if crate::object::is_arguments_object(raw as *const crate::object::ObjectHeader) {
            return IterSourceKind::Snapshot;
        }
    }
    IterSourceKind::Generic
}

/// Populate `result` from an iterable `items`, applying `mapfn` when
/// `mapping`. Implements the iterator branch of ECMA-262 §23.1.2.1 step 6
/// (including `IteratorClose` on an abrupt `mapfn` / `CreateDataProperty`).
unsafe fn populate_from_iterable(
    result: f64,
    items: f64,
    mapping: bool,
    mapfn: f64,
    this_arg: f64,
) {
    match classify_iter_source(items) {
        IterSourceKind::LiveArray => {
            let arr = crate::value::js_nanbox_get_pointer(items) as *const ArrayHeader;
            let mut k = 0usize;
            loop {
                let live = clean_arr_ptr(arr);
                if live.is_null() || k >= (*live).length as usize {
                    break;
                }
                let value = crate::array::js_array_get_f64(live, k as u32);
                let mapped = if mapping {
                    call_map_fn(mapfn, this_arg, value, k)
                } else {
                    value
                };
                if !create_index_data_property(result, k, mapped) {
                    throw_cannot_define_property(k);
                }
                k += 1;
            }
            set_result_length(result, k);
        }
        IterSourceKind::Snapshot => {
            let snapshot = clean_arr_ptr(js_array_clone(
                crate::value::js_nanbox_get_pointer(items) as *const ArrayHeader,
            ));
            let len = if snapshot.is_null() {
                0
            } else {
                (*snapshot).length as usize
            };
            for k in 0..len {
                let value = crate::array::js_array_get_f64(snapshot, k as u32);
                let mapped = if mapping {
                    call_map_fn(mapfn, this_arg, value, k)
                } else {
                    value
                };
                if !create_index_data_property(result, k, mapped) {
                    throw_cannot_define_property(k);
                }
            }
            set_result_length(result, len);
        }
        IterSourceKind::Generic => {
            let iter = crate::symbol::js_get_iterator(items);
            let mut k = 0usize;
            loop {
                match crate::collection_iter::iterator_next_value(iter) {
                    None => {
                        set_result_length(result, k);
                        break;
                    }
                    Some(value) => {
                        let mapped = if mapping {
                            match crate::collection_iter::call_with_this_capturing_throw(
                                mapfn,
                                this_arg,
                                &[value, k as f64],
                            ) {
                                Ok(v) => v,
                                Err(e) => {
                                    crate::collection_iter::iterator_close(iter);
                                    crate::exception::js_throw(e);
                                }
                            }
                        } else {
                            value
                        };
                        if !create_index_data_property(result, k, mapped) {
                            crate::collection_iter::iterator_close(iter);
                            throw_cannot_define_property(k);
                        }
                        k += 1;
                    }
                }
            }
        }
    }
}

/// Spec-complete `Array.from ( items [ , mapfn [ , thisArg ] ] )`
/// (ECMA-262 §23.1.2.1), returning a NaN-boxed result value. `c` is the
/// `this` value: when it `IsConstructor`, the result is `Construct(C)` (the
/// iterator branch) or `Construct(C, «len»)` (the array-like branch);
/// otherwise a plain Array is created. Abrupt completions everywhere
/// propagate, and the iterator branch closes the iterator on an abrupt
/// `mapfn` / `CreateDataProperty`.
pub fn array_from_full(c: f64, items: f64, mapfn: f64, this_arg: f64) -> f64 {
    // Steps 1-3: validate `mapfn`.
    // Only `undefined` means "no mapfn"; any other value (including `null`)
    // must be callable or it is a TypeError.
    let mapping = if mapfn.to_bits() == TAG_UNDEFINED {
        false
    } else {
        resolve_callable(mapfn);
        true
    };

    // Nullish sources have no `@@iterator` and throw before anything else.
    let item_bits = items.to_bits();
    if item_bits == TAG_UNDEFINED {
        throw_not_iterable("undefined");
    }
    if item_bits == TAG_NULL {
        throw_not_iterable("object null");
    }

    let is_ctor = is_constructor_value(c);

    if items_is_iterable(items) {
        // Step 5: iterable. Construct the result BEFORE creating the iterator
        // (Construct can throw, e.g. a custom constructor) so the throw
        // propagates without a dangling open iterator.
        let result = if is_ctor {
            unsafe { crate::object::js_new_function_construct(c, std::ptr::null(), 0) }
        } else {
            let arr = js_array_alloc(0);
            f64::from_bits(JSValue::pointer(arr as *const u8).bits())
        };
        unsafe {
            populate_from_iterable(result, items, mapping, mapfn, this_arg);
        }
        result
    } else {
        // Step 6+: array-like.
        let len = array_like_length(items);
        let result = if is_ctor {
            let len_arg = [len as f64];
            unsafe { crate::object::js_new_function_construct(c, len_arg.as_ptr(), 1) }
        } else {
            let arr = js_array_alloc(len as u32);
            unsafe {
                (*arr).length = 0;
            }
            f64::from_bits(JSValue::pointer(arr as *const u8).bits())
        };
        let mut k = 0usize;
        while k < len {
            let kvalue = crate::object::js_object_get_index_polymorphic(item_bits as i64, k as f64);
            let mapped = if mapping {
                call_map_fn(mapfn, this_arg, kvalue, k)
            } else {
                kvalue
            };
            if !unsafe { create_index_data_property(result, k, mapped) } {
                throw_cannot_define_property(k);
            }
            k += 1;
        }
        unsafe {
            set_result_length(result, len);
        }
        result
    }
}

/// Spec-complete `Array.of ( ...items )` (ECMA-262 §23.1.2.3), returning a
/// NaN-boxed result value. `c` is the `this` value: when it `IsConstructor`,
/// the result is `Construct(C, «len»)`; otherwise a plain Array is created.
/// Each element is installed via `CreateDataPropertyOrThrow` and the final
/// `Set(A, "length", len, true)` runs — every abrupt completion (a throwing
/// constructor, a failed define, a poisoned length setter) propagates.
pub fn array_of_full(c: f64, vals: &[f64]) -> f64 {
    let len = vals.len();
    let is_ctor = is_constructor_value(c);
    let result = if is_ctor {
        let len_arg = [len as f64];
        unsafe { crate::object::js_new_function_construct(c, len_arg.as_ptr(), 1) }
    } else {
        let arr = js_array_alloc(len as u32);
        unsafe {
            (*arr).length = 0;
        }
        f64::from_bits(JSValue::pointer(arr as *const u8).bits())
    };
    for (k, &v) in vals.iter().enumerate() {
        if !unsafe { create_index_data_property(result, k, v) } {
            throw_cannot_define_property(k);
        }
    }
    unsafe {
        set_result_length(result, len);
    }
    result
}

/// Peek a source's dense length for the concat capacity estimate: `Some(len)`
/// for a genuine plain `ArrayHeader`, `Some(0)` for null, `None` for anything
/// whose element count this can't cheaply know (proxy, lazy array, set/map/
/// typed-array/buffer reading as array-typed). Pure header reads — never runs
/// user code.
unsafe fn peek_plain_array_len(arr: *const ArrayHeader) -> Option<u32> {
    if crate::array::array_ptr_as_proxy(arr).is_some() {
        return None;
    }
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return Some(0);
    }
    let raw = arr as usize;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*hdr).obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    Some((*arr).length)
}

/// Capacity hint for the concat result (#6386): sum of the receiver's and the
/// array arguments' dense lengths (1 slot for anything un-peekable). Purely
/// a hint — under-estimates are backstopped by `js_array_grow`, so exotic
/// cases (array-likes, species mutation from a poisoned getter) stay correct,
/// merely unpre-sized.
unsafe fn concat_capacity_hint(recv: *const ArrayHeader, args_ptr: *const f64, count: i32) -> u32 {
    let mut total: u64 = peek_plain_array_len(recv).unwrap_or(0) as u64;
    if !args_ptr.is_null() && count > 0 {
        for i in 0..count as usize {
            let bits = (*args_ptr.add(i)).to_bits();
            if JSValue::from_bits(bits).is_pointer() {
                let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader;
                total += peek_plain_array_len(ptr).unwrap_or(1) as u64;
            } else {
                total += 1;
            }
        }
    }
    total.min(16_000_000) as u32
}

/// Validate one concat source for the all-dense bulk path: a genuine plain
/// dense hole-free `ArrayHeader`. `Some((ptr, len))` on success (null →
/// `Some((null, 0))`), `None` for anything the bulk path must not touch.
/// Pure reads — runs no user code, allocates nothing.
unsafe fn dense_concat_array_source(src: *const ArrayHeader) -> Option<(*const ArrayHeader, u32)> {
    if crate::array::array_ptr_as_proxy(src).is_some() {
        return None;
    }
    let src = clean_arr_ptr(src);
    if src.is_null() {
        return Some((src, 0));
    }
    let raw = src as usize;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*hdr).obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    if crate::set::is_registered_set(raw)
        || crate::map::is_registered_map(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
        || crate::buffer::is_registered_buffer(raw)
    {
        return None;
    }
    let len = (*src).length;
    if len > (*src).capacity {
        return None;
    }
    let elems = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    for i in 0..len as usize {
        if (*elems.add(i)).to_bits() == crate::value::TAG_HOLE {
            return None;
        }
    }
    Some((src, len))
}

/// #6386 all-dense bulk concat. Preconditions established by the caller: the
/// result is a freshly allocated plain array (length 0) and the
/// `isConcatSpreadable` gate is closed (so skipping the per-argument
/// spreadable reads is unobservable). Validates every source in a first
/// pass (pure reads), then — only if the pre-sized result can hold the total
/// WITHOUT growing, so no allocation and hence no GC can occur mid-copy —
/// copies everything and performs a single exact layout/barrier rebuild.
/// Returns `None` with the result untouched (length still 0) when any
/// precondition fails; the caller's spec-shaped per-source flow takes over.
unsafe fn try_concat_all_dense(
    result: *mut ArrayHeader,
    recv: *const ArrayHeader,
    args_ptr: *const f64,
    count: i32,
) -> Option<*mut ArrayHeader> {
    // Small fixed classification buffer: pass 1's validated (ptr, len) pairs
    // feed pass 2 directly, so pass 2 has NO bail-out point — no side effect
    // (string shared-demote) can be applied and then re-applied by the
    // fallback path. Wider argument lists take the per-source flow.
    const MAX_DENSE_ARGS: usize = 8;
    let count = count.max(0) as usize;
    if count > MAX_DENSE_ARGS {
        return None;
    }
    let args: &[f64] = if args_ptr.is_null() || count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, count)
    };
    // Pass 1: validate every source and total the lengths. `None` in a slot
    // marks a single-value (non-pointer) argument occupying one result slot.
    let (recv_src, recv_len) = dense_concat_array_source(recv)?;
    let mut arg_sources: [Option<(*const ArrayHeader, u32)>; MAX_DENSE_ARGS] =
        [None; MAX_DENSE_ARGS];
    let mut total: u64 = recv_len as u64;
    for (i, &arg) in args.iter().enumerate() {
        let bits = arg.to_bits();
        if bits >> 48 == 0x7FFD {
            let (src, len) =
                dense_concat_array_source((bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader)?;
            arg_sources[i] = Some((src, len));
            total += len as u64;
        } else {
            // Non-pointer value (number / string-by-tag / bool / undefined /
            // null / bigint): exactly one result slot.
            total += 1;
        }
    }
    if total > 16_000_000 {
        return None;
    }
    let total = total as u32;
    if total > (*result).capacity {
        return None;
    }
    // Pass 2: copy. Nothing below allocates or bails, so no GC can move a
    // source or the result mid-copy and no shared-demote runs twice — which
    // is what makes the single deferred rebuild sound.
    let dst = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    let mut off: usize = 0;
    let mut copy_array = |src: *const ArrayHeader, len: u32, off: &mut usize| {
        if len == 0 {
            return;
        }
        let elems = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        // GC_STORE_AUDIT(BARRIERED): all-dense concat bulk copy; one exact
        // layout/barrier rebuild follows after all sources are copied.
        std::ptr::copy_nonoverlapping(elems, dst.add(*off), len as usize);
        // Same shared-demote `js_array_push_f64` performs per element, so a
        // later in-place mutation of a source string local can't edit the
        // stored element.
        for i in *off..*off + len as usize {
            let v = *dst.add(i);
            if v.to_bits() >> 48 == 0x7FFF {
                crate::string::js_string_addref_if_heap_string(v);
            }
        }
        *off += len as usize;
    };
    copy_array(recv_src, recv_len, &mut off);
    for (i, &arg) in args.iter().enumerate() {
        if let Some((src, len)) = arg_sources[i] {
            copy_array(src, len, &mut off);
        } else {
            if arg.to_bits() >> 48 == 0x7FFF {
                crate::string::js_string_addref_if_heap_string(arg);
            }
            std::ptr::write(dst.add(off), arg);
            off += 1;
        }
    }
    (*result).length = total;
    crate::array::rebuild_array_layout_exact(result);
    Some(result)
}

/// Bulk fast path for `append_spread_array` (#6386): a plain, dense,
/// hole-free `ArrayHeader` source appended onto a plain result with one
/// pre-grow + bulk element copy, mirroring `js_array_concat`'s audited
/// bulk-copy pattern (`concat_reverse.rs`). Replaces a full `js_array_clone`
/// of the source plus per-element `js_array_push_f64` (each doing proxy /
/// frozen / capacity / barrier work) — the dominant cost of `a.concat(b)` on
/// dense arrays. Returns `None` when any precondition fails so the caller
/// falls back to the spec-shaped loop below.
unsafe fn try_append_spread_array_dense(
    result: *mut ArrayHeader,
    src: *const ArrayHeader,
) -> Option<*mut ArrayHeader> {
    // A masked proxy id is not a dereferenceable ArrayHeader.
    if crate::array::array_ptr_as_proxy(src).is_some() {
        return None;
    }
    let src = clean_arr_ptr(src);
    if src.is_null() {
        return Some(result);
    }
    let raw = src as usize;
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    // Only a genuine dense array: sets/maps/typed-arrays/buffers/lazy arrays
    // materialize element values through `js_array_clone` on the slow path.
    let hdr = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*hdr).obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    if crate::set::is_registered_set(raw)
        || crate::map::is_registered_map(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
        || crate::buffer::is_registered_buffer(raw)
    {
        return None;
    }
    let src_len = (*src).length;
    if src_len == 0 {
        return Some(result);
    }
    if src_len > (*src).capacity {
        return None;
    }
    let result = crate::array::clean_arr_ptr_mut(result);
    if result.is_null() || std::ptr::eq(result as *const ArrayHeader, src) {
        return None;
    }
    // The result is freshly allocated by the concat entry points, but a
    // sealed/frozen dest would make `js_array_grow` return it un-grown and
    // the bulk copy would overflow its capacity — keep the guard explicit.
    if crate::array::array_is_frozen(result) || crate::array::array_is_sealed_or_no_extend(result) {
        return None;
    }
    // Validation pass, no side effects: holes need the spec
    // `HasProperty`/`Get` reads (inherited elements, side-table entries) —
    // punt those to the slow path. String addrefs happen only after the copy
    // has committed below, so a mid-scan bail can't leave the fallback path
    // double-retaining an already-addref'd string.
    let src_elems = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    for i in 0..src_len as usize {
        if (*src_elems.add(i)).to_bits() == crate::value::TAG_HOLE {
            return None;
        }
    }
    let dest_len = (*result).length;
    let new_len = dest_len.checked_add(src_len)?;
    let (result, src) = if new_len > (*result).capacity {
        // Growing can allocate → GC can run. `result` is rooted inside
        // `js_array_grow`; root `src` too (it may be an unrooted snapshot,
        // e.g. from `array_subclass_dense_snapshot`) and re-resolve both.
        let scope = crate::gc::RuntimeHandleScope::new();
        let src_handle = scope.root_raw_const_ptr(src);
        let grown = crate::array::js_array_grow(result, new_len);
        (
            grown,
            clean_arr_ptr(src_handle.get_raw_const_ptr::<ArrayHeader>()),
        )
    } else {
        (result, src)
    };
    if result.is_null() || src.is_null() {
        return None;
    }
    let src_elems = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    let dst_elems = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    // GC_STORE_AUDIT(BARRIERED): concat bulk copy is followed by exact layout/barrier rebuild.
    std::ptr::copy_nonoverlapping(
        src_elems,
        dst_elems.add(dest_len as usize),
        src_len as usize,
    );
    // The copy has committed — apply the same shared-demote
    // `js_array_push_f64` performs per element, so a later in-place mutation
    // of a source string local can't edit the stored element. Runs after
    // every bail-out point so a fallback re-append can't double-retain.
    for i in dest_len as usize..new_len as usize {
        let v = *dst_elems.add(i);
        if v.to_bits() >> 48 == 0x7FFF {
            crate::string::js_string_addref_if_heap_string(v);
        }
    }
    (*result).length = new_len;
    crate::array::rebuild_array_layout_exact(result);
    Some(result)
}

/// Append every element of the (already-materializable) source array `src`
/// into `result`, returning the (possibly reallocated) result. `src` is
/// materialized via `js_array_clone` so sets/maps/typed-arrays/buffers spread
/// to their element values, matching `[...x]`.
fn append_spread_array(result: *mut ArrayHeader, src: *const ArrayHeader) -> *mut ArrayHeader {
    if let Some(out) = unsafe { try_append_spread_array_dense(result, src) } {
        return out;
    }
    let materialized = js_array_clone(src);
    let materialized = clean_arr_ptr(materialized);
    if materialized.is_null() {
        return result;
    }
    unsafe {
        let len = (*materialized).length;
        let elems =
            (materialized as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let mut out = result;
        // ECMA-262 §23.1.3.5 step 5.c.iii: each index goes through
        // `HasProperty(E, k)` / `Get(E, k)` — a hole filled by an inherited
        // `Array.prototype[k]` element lands as an OWN property of the result
        // (test262 concat/S15.4.4.4_A3_T1); a genuinely absent index stays a
        // hole. The dense raw read only differs when the clone has holes, so
        // gate the spec reads on the per-element hole check.
        for i in 0..len as usize {
            let v = *elems.add(i);
            if v.to_bits() == crate::value::TAG_HOLE
                && crate::array::array_spec_has_index(materialized, i as u32)
            {
                out = js_array_push_f64(out, crate::array::array_spec_get(materialized, i as u32));
                continue;
            }
            out = js_array_push_f64(out, v);
        }
        out
    }
}
