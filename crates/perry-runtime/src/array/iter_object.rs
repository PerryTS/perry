//! #321 (effect `ManagedRuntime` / `Exit.all`): a real array iterator
//! object so a value-level `arr[Symbol.iterator]()` / `arr.values()` /
//! `arr.keys()` / `arr.entries()` call returns a `.next()`-bearing
//! iterator (matching Node), not an eager array clone.
//!
//! Background: Perry's `for...of` over an array is special-cased to an
//! indexed length/[i] loop, and codegen's `Expr::ArrayValues` fast path
//! materializes `arr.values()` as a plain array clone. That covers the
//! common cases. But when an array's `[Symbol.iterator]` is invoked
//! dynamically through the runtime dispatch tower (`js_native_call_method`)
//! — e.g. effect's `Chunk[Symbol.iterator]()` delegates to
//! `backing.array[Symbol.iterator]()`, and `Array.from(chunk)` /
//! `Arr.reduce` then drive `.next()` on the result — the pre-fix tower had
//! no `values`/`keys`/`entries`/`@@iterator` arm, so the call fell through
//! to the object-field scan and returned `undefined`. `Array.from(undefined)`
//! yields nothing (or undefined elements), which surfaced downstream as
//! `Cannot read properties of undefined (reading '_tag')` in effect's
//! `exitZipWith`.
//!
//! Representation mirrors `buffer/iter.rs`: a regular `ObjectHeader` with a
//! dedicated `ARRAY_ITERATOR_CLASS_ID`. Field 0 holds the backing array
//! (NaN-boxed pointer, so the object scanner keeps it alive), field 1 the
//! cursor index, field 2 the iterator kind. Dispatch lives in
//! `object/native_call_method.rs` via the class-id check next to the
//! Buffer iterator one.

use super::*;
use crate::object::{js_object_alloc, js_object_get_field, js_object_set_field, ObjectHeader};
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_UNDEFINED};

/// Class id reserved for array iterators. Sits adjacent to the Buffer
/// iterator id (0xFFFF0005) in the 0xFFFF prefix reserved for
/// runtime-defined classes.
pub const ARRAY_ITERATOR_CLASS_ID: u32 = 0xFFFF_0006;

/// Iterator kind tags — matches the i32 stored in field 2.
const KIND_VALUES: i32 = 0;
const KIND_KEYS: i32 = 1;
const KIND_ENTRIES: i32 = 2;

/// Clean a NaN-boxed array pointer to a raw `*mut ArrayHeader`, or null.
fn unbox_array_ptr(value: f64) -> *mut ArrayHeader {
    let raw = js_nanbox_get_pointer(value);
    if raw < (crate::gc::GC_HEADER_SIZE as i64 + 0x1000) {
        return std::ptr::null_mut();
    }
    raw as *mut ArrayHeader
}

unsafe fn alloc_iterator(arr_ptr: *mut ArrayHeader, kind: i32) -> f64 {
    let obj = js_object_alloc(ARRAY_ITERATOR_CLASS_ID, 3);
    // Field 0: backing array (NaN-boxed pointer so the GC scanner keeps it).
    let arr_nan = js_nanbox_pointer(arr_ptr as i64);
    js_object_set_field(obj, 0, JSValue::from_bits(arr_nan.to_bits()));
    // Field 1: cursor index, starts at 0.
    js_object_set_field(obj, 1, JSValue::number(0.0));
    // Field 2: iterator kind.
    js_object_set_field(obj, 2, JSValue::number(kind as f64));
    // Link `[[Prototype]]` to the shared `%ArrayIteratorPrototype%` singleton so
    // `Object.getPrototypeOf(it)` and the inherited `.next` read resolve.
    crate::object::attach_iterator_prototype(obj, ARRAY_ITERATOR_CLASS_ID);
    js_nanbox_pointer(obj as i64)
}

/// `arr.values()` iterator — yields each element value.
pub fn array_values_iter(arr_f64: f64) -> f64 {
    let arr_ptr = unbox_array_ptr(arr_f64);
    if arr_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(arr_ptr, KIND_VALUES) }
}

/// `arr.keys()` iterator — yields each index `0..length`.
pub fn array_keys_iter(arr_f64: f64) -> f64 {
    let arr_ptr = unbox_array_ptr(arr_f64);
    if arr_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(arr_ptr, KIND_KEYS) }
}

/// `arr.entries()` iterator — yields `[index, value]` pairs.
pub fn array_entries_iter(arr_f64: f64) -> f64 {
    let arr_ptr = unbox_array_ptr(arr_f64);
    if arr_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(arr_ptr, KIND_ENTRIES) }
}

// ---------------------------------------------------------------------------
// #2384: C-ABI entry points for codegen's `Expr::ArrayValues`/`ArrayKeys`/
// `ArrayEntries` fast path. These build a real `.next()`-bearing iterator
// OBJECT (not an eager materialized array), so a value-level
// `const e = arr.entries(); e.next().value` matches Node. Spread
// (`js_array_clone`) and the runtime default-iterator (`js_for_of_to_array`)
// already detect `ARRAY_ITERATOR_CLASS_ID` and drive `.next()`, so
// `[...arr.entries()]` / `for...of` / `Array.from(arr.entries())` keep working.
//
// They take a RAW array pointer (codegen passes the handle through
// `unbox_to_i64`) and return the RAW iterator-object pointer as i64; the
// caller NaN-boxes it via `nanbox_pointer_inline`.

/// GcHeader `obj_type` byte for a receiver, or 0 if the pointer is too low to
/// carry a header. Mirrors `flat_clone::receiver_gc_type` (that fn is private).
unsafe fn receiver_obj_type(arr: *const ArrayHeader) -> u8 {
    let addr = arr as usize;
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return 0;
    }
    let gc_header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*gc_header).obj_type
}

unsafe fn fs_dir_entries_iter_obj(arr: *const ArrayHeader, kind: i32) -> Option<i64> {
    if kind != KIND_ENTRIES || receiver_obj_type(arr) != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj = arr as *const crate::object::ObjectHeader;
    if (*obj).class_id != crate::fs::CLASS_ID_FS_DIR {
        return None;
    }
    let key = crate::string::js_string_from_bytes(b"entries".as_ptr(), b"entries".len() as u32);
    let method = crate::object::js_object_get_field_by_name(obj, key);
    if method.is_undefined() {
        return None;
    }
    let method_f64 = f64::from_bits(method.bits());
    let method_ptr = crate::value::js_nanbox_get_pointer(method_f64);
    if method_ptr == 0 || !crate::closure::is_closure_ptr(method_ptr as usize) {
        return None;
    }
    let result = crate::closure::js_closure_call0(method_ptr as *const _);
    let result_ptr = crate::value::js_nanbox_get_pointer(result);
    (result_ptr != 0).then_some(result_ptr)
}

unsafe fn array_iter_obj_raw(arr: *const ArrayHeader, kind: i32) -> i64 {
    let cleaned = clean_arr_ptr(arr);
    if let Some(iter) = fs_dir_entries_iter_obj(cleaned, kind) {
        return iter;
    }
    // #2384's iterator OBJECT is Array-scoped. A Map or Set reaches the codegen
    // `.entries()`/`.keys()`/`.values()` catch-all when its static type is lost
    // (`any`-typed Map/Set — effect's `FiberRefs.diff`, #321). Those keep the
    // existing eager materialization, which `js_array_{entries,keys,values}`
    // route to the correct Map/Set iterator — building an array-iterator over a
    // Map/Set buffer would reinterpret it as `[index, value]` garbage. Genuine
    // arrays (incl. `GC_TYPE_LAZY_ARRAY`) and everything else get the real
    // iterator object.
    let t = receiver_obj_type(cleaned);
    if t == crate::gc::GC_TYPE_MAP || t == crate::gc::GC_TYPE_SET {
        let materialized = match kind {
            KIND_KEYS => crate::array::js_array_keys(arr),
            KIND_VALUES => crate::array::js_array_values(arr),
            _ => crate::array::js_array_entries(arr),
        };
        return materialized as i64;
    }
    let nanboxed = alloc_iterator(cleaned as *mut ArrayHeader, kind);
    js_nanbox_get_pointer(nanboxed)
}

/// #3148: materialize a TypedArray receiver to a plain Array (element-typed
/// reads) before building the iterator object, so `int32arr.values()` /
/// `.keys()` / `.entries()` yield the numeric elements rather than the raw
/// byte buffer reinterpreted as f64.
#[inline]
fn typed_array_iter_arr(arr: *const ArrayHeader) -> *const ArrayHeader {
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::typed_array_to_array(arr as *const crate::typedarray::TypedArrayHeader)
            as *const ArrayHeader
    } else {
        arr
    }
}

/// `%TypedArray%.prototype.values/keys/entries` begin with `ValidateTypedArray`
/// (spec step 1). When the receiver is a `%TypedArray%.prototype` object itself
/// — `Int8Array.prototype.entries()` / `TypedArray.prototype.values()` — it is
/// NOT a real typed array, so the call must throw a `TypeError`. Codegen lowers
/// `recv.entries()` to the eager `Expr::ArrayEntries` fast path (these C-ABI
/// helpers) regardless of the receiver's static type, so the brand check has to
/// live here rather than in the dynamic dispatch tower.
#[cold]
/// `Array.prototype.{entries,keys,values}` begin with `ToObject(this value)`
/// (ECMA-262 §23.1.3), which throws a TypeError for `undefined` / `null`. The
/// codegen `Expr::Array{Entries,Keys,Values}` lowering unboxes the receiver via
/// `& POINTER_MASK`, so a `null`/`undefined` `this` arrives as the sentinel
/// address `2` / `1` (real heap arrays are always ≥ 0x1000). Throw there instead
/// of silently materializing an empty iterator. (test262
/// Array.prototype.{entries,keys,values}/{return-abrupt-from-this,this-val-non-obj-coercible}.)
unsafe fn throw_non_coercible_this(method: &str) -> ! {
    let _ = method;
    let msg = "Cannot convert undefined or null to object";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ));
}

#[inline]
unsafe fn guard_coercible_this(arr: *const ArrayHeader, method: &str) {
    let a = arr as usize;
    if a == 1 || a == 2 {
        throw_non_coercible_this(method);
    }
}

unsafe fn throw_if_typed_array_proto(arr: *const ArrayHeader, method: &str) {
    if crate::object::is_typed_array_prototype(arr as usize) {
        let msg = format!("Method %TypedArray%.prototype.{method} called on incompatible receiver");
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(s);
        crate::exception::js_throw(f64::from_bits(
            crate::value::JSValue::pointer(err as *const u8).bits(),
        ));
    }
}

/// If `arr` (the already-unwrapped receiver pointer from the
/// `Expr::Array{Values,Keys,Entries}` fold) is actually a Map/Set — registered
/// directly OR a `class X extends Map|Set` instance carrying the hidden backing
/// collection — return the matching COLLECTION iterator object instead of
/// treating it as an array. The any-typed `.values()/.keys()/.entries()` fold
/// (`array_only_methods.rs`, #597) routes every dynamic-receiver call through
/// `js_array_*_iter_obj`; without this a Map/Set (or subclass) receiver was
/// iterated as a (non-)array and yielded an EMPTY iterator — NestJS's
/// `[...modulesContainer.values()]` (a `class ModulesContainer extends Map`)
/// returned 0 entries, so the injector never instantiated controllers/providers
/// and route handlers saw a field-less stub `this` (#wall14). `kind`: 0 =
/// values, 1 = keys, 2 = entries.
unsafe fn collection_iter_obj_for_receiver(arr: *const ArrayHeader, kind: u8) -> Option<i64> {
    let raw = arr as usize;
    if raw < 0x10000 {
        return None;
    }
    // Web Fetch collection handles (Headers / FormData / URLSearchParams) are
    // fetch-band ids, not heap `ArrayHeader`s. An any-typed
    // `.keys()`/`.entries()`/`.values()` on such a handle — the static type
    // erased through an object property or destructure, e.g. an SDK
    // auth-header wrapper's `{ values: Headers }` consumed via
    // `let { values: z } = wrapper; ...z.entries()` — folds to
    // `Expr::Array{Keys,Entries,Values}` (perry-hir #597 any-typed catch-all)
    // and lands here. Reading the handle id as an `ArrayHeader` yields an
    // empty iterator; route through the dynamic method dispatch instead so it
    // reaches the stdlib fetch handlers (`js_headers_{keys,entries,values}`,
    // which return a materialized, iterable array). A fetch handle that lacks
    // the requested iterator method (Response / Request / Blob) returns
    // `undefined`; fall through to the empty-array path then.
    if (crate::value::addr_class::FETCH_HANDLE_BAND_START
        ..crate::value::addr_class::FETCH_HANDLE_BAND_END)
        .contains(&raw)
    {
        let recv = f64::from_bits(JSValue::pointer(arr as *const u8).bits());
        let method: &[u8] = match kind {
            1 => b"keys",
            2 => b"entries",
            _ => b"values",
        };
        let result = crate::object::js_native_call_method(
            recv,
            method.as_ptr() as *const i8,
            method.len(),
            std::ptr::null(),
            0,
        );
        let rv = JSValue::from_bits(result.to_bits());
        if !rv.is_undefined() && !rv.is_null() {
            let ptr = (result.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            if ptr != 0 {
                return Some(ptr);
            }
        }
        return None;
    }
    if crate::map::is_registered_map(raw) {
        let m = raw as *const crate::map::MapHeader;
        return Some(match kind {
            1 => crate::collection_iter_object::js_map_keys_iter_obj(m),
            2 => crate::collection_iter_object::js_map_entries_iter_obj(m),
            _ => crate::collection_iter_object::js_map_values_iter_obj(m),
        });
    }
    if crate::set::is_registered_set(raw) {
        let s = raw as *const crate::set::SetHeader;
        return Some(match kind {
            1 => crate::collection_iter_object::js_set_keys_iter_obj(s),
            2 => crate::collection_iter_object::js_set_entries_iter_obj(s),
            _ => crate::collection_iter_object::js_set_values_iter_obj(s),
        });
    }
    // `class X extends Map|Set` instance — probe the hidden backing field via
    // the reconstructed NaN-boxed pointer value.
    let boxed = f64::from_bits(JSValue::pointer(arr as *const u8).bits());
    match crate::object::map_set_subclass::subclass_backing_of(boxed) {
        Some(crate::object::map_set_subclass::CollectionBacking::Map(m)) => Some(match kind {
            1 => crate::collection_iter_object::js_map_keys_iter_obj(
                m as *const crate::map::MapHeader,
            ),
            2 => crate::collection_iter_object::js_map_entries_iter_obj(
                m as *const crate::map::MapHeader,
            ),
            _ => crate::collection_iter_object::js_map_values_iter_obj(
                m as *const crate::map::MapHeader,
            ),
        }),
        Some(crate::object::map_set_subclass::CollectionBacking::Set(s)) => Some(match kind {
            1 => crate::collection_iter_object::js_set_keys_iter_obj(
                s as *const crate::set::SetHeader,
            ),
            2 => crate::collection_iter_object::js_set_entries_iter_obj(
                s as *const crate::set::SetHeader,
            ),
            _ => crate::collection_iter_object::js_set_values_iter_obj(
                s as *const crate::set::SetHeader,
            ),
        }),
        None => None,
    }
}

#[no_mangle]
pub extern "C" fn js_array_values_iter_obj(arr: *const ArrayHeader) -> i64 {
    unsafe {
        if let Some(it) = collection_iter_obj_for_receiver(arr, 0) {
            return it;
        }
        guard_coercible_this(arr, "values");
        throw_if_typed_array_proto(arr, "values");
        array_iter_obj_raw(typed_array_iter_arr(arr), KIND_VALUES)
    }
}

#[no_mangle]
pub extern "C" fn js_array_keys_iter_obj(arr: *const ArrayHeader) -> i64 {
    unsafe {
        if let Some(it) = collection_iter_obj_for_receiver(arr, 1) {
            return it;
        }
        guard_coercible_this(arr, "keys");
        throw_if_typed_array_proto(arr, "keys");
        array_iter_obj_raw(typed_array_iter_arr(arr), KIND_KEYS)
    }
}

#[no_mangle]
pub extern "C" fn js_array_entries_iter_obj(arr: *const ArrayHeader) -> i64 {
    unsafe {
        if let Some(it) = collection_iter_obj_for_receiver(arr, 2) {
            return it;
        }
        guard_coercible_this(arr, "entries");
        throw_if_typed_array_proto(arr, "entries");
        array_iter_obj_raw(typed_array_iter_arr(arr), KIND_ENTRIES)
    }
}

/// Build the `{ value, done }` iterator-result object. `value` arrives as
/// a NaN-boxed JSValue; `done` is a JS boolean.
unsafe fn make_iter_result(value: JSValue, done: bool) -> f64 {
    let obj = js_object_alloc(0, 2);

    // keys array so destructuring + property reads find named slots.
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let done_key = crate::string::js_string_from_bytes(b"done".as_ptr(), 4);
    let keys = crate::array::js_array_alloc(2);
    crate::array::js_array_push(keys, JSValue::string_ptr(value_key));
    crate::array::js_array_push(keys, JSValue::string_ptr(done_key));
    crate::object::js_object_set_keys(obj, keys);

    js_object_set_field(obj, 0, value);
    js_object_set_field(obj, 1, JSValue::bool(done));
    js_nanbox_pointer(obj as i64)
}

unsafe fn make_pair_array(idx: u32, value: f64) -> f64 {
    let pair = crate::array::js_array_alloc(2);
    (*pair).length = 2;
    let elems = (pair as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    *elems.add(0) = idx as f64;
    *elems.add(1) = value;
    crate::array::note_array_slot(pair, 0, (idx as f64).to_bits());
    crate::array::note_array_slot(pair, 1, value.to_bits());
    js_nanbox_pointer(pair as i64)
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on an array iterator object.
/// Routed from `js_native_call_method`'s class-id check.
pub unsafe fn dispatch_array_iterator_method(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    match method_name {
        "next" => {
            // Field 0: backing array pointer (NaN-boxed).
            let backing_field = js_object_get_field(iter_obj, 0);
            let backing_f64 = f64::from_bits(backing_field.bits());
            // Once the iterator is exhausted the backing array is cleared to
            // `undefined` (spec: `[[IteratedArrayLike]]` set to undefined), so a
            // later `.next()` stays done even if the array grew after exhaustion
            // (test262 Array/prototype/{values,keys,entries}/iteration-mutable:
            // pushing AFTER the iterator reported done must not resurface).
            if JSValue::from_bits(backing_f64.to_bits()).is_undefined() {
                return make_iter_result(JSValue::undefined(), true);
            }
            let arr_ptr = js_nanbox_get_pointer(backing_f64) as *const ArrayHeader;
            // Field 1: current index.
            let idx_field = js_object_get_field(iter_obj, 1);
            let idx = f64::from_bits(idx_field.bits()) as u32;
            // Field 2: iterator kind.
            let kind_field = js_object_get_field(iter_obj, 2);
            let kind = f64::from_bits(kind_field.bits()) as i32;

            let len = if arr_ptr.is_null() {
                0u32
            } else {
                crate::array::js_array_length(arr_ptr)
            };

            if idx >= len {
                js_object_set_field(iter_obj, 0, JSValue::undefined());
                return make_iter_result(JSValue::undefined(), true);
            }

            // Advance the stored cursor before computing the value so a
            // subsequent `.next()` call sees the bumped index.
            js_object_set_field(iter_obj, 1, JSValue::number((idx + 1) as f64));

            let elem = if arr_ptr.is_null() {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                crate::array::js_array_get_f64(arr_ptr, idx)
            };

            let value = match kind {
                KIND_VALUES => JSValue::from_bits(elem.to_bits()),
                KIND_KEYS => JSValue::number(idx as f64),
                KIND_ENTRIES => {
                    let pair = make_pair_array(idx, elem);
                    JSValue::from_bits(pair.to_bits())
                }
                _ => JSValue::undefined(),
            };
            make_iter_result(value, false)
        }
        // Iterators are themselves iterable — `[Symbol.iterator]()` on one
        // returns the same iterator (matches Node, and lets `js_get_iterator`
        // / `for (const v of arr.values())` re-enter without a wrapper).
        "Symbol.iterator" | "@@iterator" | "values" => js_nanbox_pointer(iter_obj as i64),
        // `return`/`throw` are part of the iterator spec; Node's array
        // iterator inherits them from %IteratorPrototype%. Return a
        // `{ value: undefined, done: true }` shape for early-exit code.
        "return" | "throw" => make_iter_result(JSValue::undefined(), true),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
