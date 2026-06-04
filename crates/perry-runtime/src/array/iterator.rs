//! Iterator-protocol → array converter.
use super::*;

/// Materialize an arbitrary iterable into a plain Array, used by the
/// `for...of` desugar when the receiver's static type can NOT be proven
/// (an `any`-typed property, an untyped JS-source value, etc.). The HIR
/// loop iterates the returned array by index (`for (i=0; i<arr.length;
/// i++) item = arr[i]`), so this helper must hand back an Array whose
/// elements are exactly what `for...of` would yield in JS:
///
///   * Array / lazy-array  → returned unchanged (no copy; the index
///                           loop reads it directly).
///   * Map                 → array of `[key, value]` pair arrays
///                           (matches `map[Symbol.iterator]()` ===
///                           `map.entries()`), so `for (const [k,v] of
///                           m)` destructures correctly.
///   * Set                 → array of values.
///   * String              → array of code-point substrings (JS spreads
///                           a string by code point, not UTF-16 unit).
///   * anything else        → drive the iterator protocol: obtain the
///                           default iterator via `js_get_iterator`
///                           (custom `[Symbol.iterator]`, perry
///                           generator objects, …) and collect `.value`s
///                           with [`js_iterator_to_array`].
///
/// Returns a NaN-boxed (POINTER_TAG) Array JSValue. Returning the boxed
/// f64 (rather than a raw pointer) keeps the HIR `Stmt::Let` holder typed
/// as a normal array value so `.length` / `arr[i]` lower through the
/// usual array fast paths.
///
/// Refs #321 (effect Context/Layer iterate `for (const [tag, s] of
/// self.unsafeMap)` over an untyped Map).
#[no_mangle]
pub extern "C" fn js_for_of_to_array(val_f64: f64) -> f64 {
    use crate::gc::{
        GcHeader, GC_HEADER_SIZE, GC_TYPE_ARRAY, GC_TYPE_LAZY_ARRAY, GC_TYPE_MAP, GC_TYPE_SET,
    };
    use crate::value::{js_nanbox_pointer, JSValue};

    let jsv = JSValue::from_bits(val_f64.to_bits());
    if let Some(entries) = entries_array_for_small_handle_value(val_f64) {
        return js_nanbox_pointer(entries as i64);
    }

    // Strings: iterate by code point. `is_any_string` covers both heap
    // STRING_TAG and inline SSO short strings. `js_get_string_pointer_unified`
    // returns a real `*const StringHeader` for either representation
    // (materializing SSO onto the heap); re-box with STRING_TAG so
    // `js_string_to_char_array` (which masks POINTER_MASK off the bits)
    // reads it correctly. The resulting array yields single-char
    // substrings exactly like `for (const c of "abc")`.
    if jsv.is_any_string() {
        let str_ptr = crate::value::js_get_string_pointer_unified(val_f64);
        let str_bits = crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK);
        let arr_i64 = crate::string::js_string_to_char_array(str_bits as i64);
        return js_nanbox_pointer(arr_i64);
    }

    // Non-pointer scalars (number/bool/null/undefined) are not iterable;
    // hand back an empty array so the loop runs zero times rather than
    // dereferencing a non-pointer.
    let raw_ptr = crate::value::js_nanbox_get_pointer(val_f64);
    if raw_ptr == 0 {
        return js_nanbox_pointer(js_array_alloc(0) as i64);
    }

    // Inspect the GC header's object kind to dispatch Array / Map / Set
    // without consulting any static type.
    let obj_type = unsafe {
        let gc_header = (raw_ptr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
        (*gc_header).obj_type
    };

    match obj_type {
        // Already an array: return unchanged — the index loop reads it in
        // place, no allocation. Lazy arrays are arrays from the iterator's
        // perspective and `js_array_length` / indexing materialize lazily.
        t if t == GC_TYPE_ARRAY || t == GC_TYPE_LAZY_ARRAY => val_f64,
        // Map → `[k, v]` pair array (=== `map.entries()` spread).
        GC_TYPE_MAP => {
            let arr = js_map_entries_for_for_of(raw_ptr);
            js_nanbox_pointer(arr as i64)
        }
        // Set → values array.
        GC_TYPE_SET => {
            let arr = js_set_to_array_for_for_of(raw_ptr);
            js_nanbox_pointer(arr as i64)
        }
        // Generic objects / generator objects / anything carrying a
        // custom `[Symbol.iterator]` or a `.next()`: walk the iterator
        // protocol. `js_get_iterator` returns the operand's
        // `Symbol.iterator()` result when iterable, or the operand
        // unchanged when it already is an iterator (perry generators).
        //
        // `for await` currently lowers through this same materializer.
        // Classic Node Readables are async-iterable only, so when no
        // sync iterator was found, prefer `[Symbol.asyncIterator]()` if
        // present and synchronously unwrap already-settled `next()`
        // promises. This keeps Readable iterator helpers like `take(0)`
        // from being mis-driven as sync iterators.
        _ => {
            let iter = crate::symbol::js_get_iterator(val_f64);
            let arr = if iter.to_bits() != val_f64.to_bits() {
                js_iterator_to_array(iter)
            } else if let Some(async_iter) = call_symbol_async_iterator(val_f64) {
                js_async_iterator_to_array(async_iter)
            } else if has_named_next(iter) {
                js_iterator_to_array(iter)
            } else {
                js_iterator_to_array(iter)
            };
            js_nanbox_pointer(arr as i64)
        }
    }
}

pub(crate) fn entries_array_for_small_handle_value(value: f64) -> Option<*mut ArrayHeader> {
    let bits = value.to_bits();
    if (bits >> 48) != 0x7FFD {
        return None;
    }
    entries_array_for_small_handle_id((bits & crate::value::POINTER_MASK) as i64)
}

pub(crate) fn entries_array_for_small_handle_id(id: i64) -> Option<*mut ArrayHeader> {
    if id <= 0 || id >= 0x100000 {
        return None;
    }
    let dispatch = crate::object::handle_method_dispatch()?;
    let prop = b"entries";
    let entries = unsafe { dispatch(id, prop.as_ptr(), prop.len(), std::ptr::null(), 0) };
    if entries.to_bits() == crate::value::TAG_UNDEFINED {
        return None;
    }
    if js_array_is_array(entries).to_bits() != crate::value::TAG_TRUE {
        return None;
    }
    let ptr = crate::value::js_nanbox_get_pointer(entries) as *mut ArrayHeader;
    (!ptr.is_null()).then_some(ptr)
}

/// Thin wrappers so this module can reach the Map/Set materializers
/// without importing their concrete header types (they live in sibling
/// runtime modules and take typed pointers). `raw_ptr` is the cleaned
/// payload pointer already extracted by `js_nanbox_get_pointer`.
#[inline]
fn js_map_entries_for_for_of(raw_ptr: i64) -> *mut ArrayHeader {
    crate::map::js_map_entries(raw_ptr as *const crate::map::MapHeader)
}

#[inline]
fn js_set_to_array_for_for_of(raw_ptr: i64) -> *mut ArrayHeader {
    crate::set::js_set_to_array(raw_ptr as *const crate::set::SetHeader)
}

fn is_callable_value(value: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(value);
    raw >= 0x10000 && crate::closure::is_closure_ptr(raw as usize)
}

fn named_field(value: f64, name: &[u8]) -> f64 {
    use crate::object::{js_object_get_field_by_name, ObjectHeader};
    use crate::string::js_string_from_bytes;
    use crate::value::{js_nanbox_get_pointer, TAG_UNDEFINED};

    let ptr = js_nanbox_get_pointer(value);
    if ptr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let field = js_object_get_field_by_name(ptr as *const ObjectHeader, key);
    unsafe { f64::from_bits(std::mem::transmute::<_, u64>(field)) }
}

fn has_named_next(value: f64) -> bool {
    is_callable_value(named_field(value, b"next"))
}

#[cold]
fn throw_not_iterable(value: f64) -> ! {
    let label = if value.to_bits() == crate::value::TAG_NULL {
        "null"
    } else if value.to_bits() == crate::value::TAG_UNDEFINED {
        "undefined"
    } else {
        "value"
    };
    let msg = format!("{label} is not iterable");
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

#[cold]
fn throw_iterator_method_not_callable() -> ! {
    let msg = b"object is not iterable";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

fn object_like_iterator_result(value: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    raw >= 0x10000
}

pub(crate) fn array_from_spread_value(value: f64) -> *mut ArrayHeader {
    use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, POINTER_MASK};

    let jsv = JSValue::from_bits(value.to_bits());
    if jsv.is_null() || jsv.is_undefined() {
        throw_not_iterable(value);
    }
    if jsv.is_any_string() {
        let str_ptr = crate::value::js_get_string_pointer_unified(value);
        let str_bits = crate::value::STRING_TAG | (str_ptr as u64 & POINTER_MASK);
        return crate::string::js_string_to_char_array(str_bits as i64) as *mut ArrayHeader;
    }

    let raw_ptr = js_nanbox_get_pointer(value) as usize;
    if raw_ptr == 0 {
        throw_not_iterable(value);
    }
    if let Some(entries) = entries_array_for_small_handle_id(raw_ptr as i64) {
        return entries;
    }
    if crate::buffer::is_registered_buffer(raw_ptr) {
        return crate::buffer::buffer_to_array(raw_ptr as *const crate::buffer::BufferHeader);
    }
    if crate::set::is_registered_set(raw_ptr) {
        return crate::set::js_set_to_array(raw_ptr as *const crate::set::SetHeader);
    }
    if crate::map::is_registered_map(raw_ptr) {
        return crate::map::js_map_entries(raw_ptr as *const crate::map::MapHeader);
    }
    if crate::typedarray::lookup_typed_array_kind(raw_ptr).is_some() {
        return crate::typedarray::typed_array_to_array(
            raw_ptr as *const crate::typedarray::TypedArrayHeader,
        );
    }
    if raw_ptr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        let obj_type = unsafe {
            let hdr =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*hdr).obj_type
        };
        if obj_type == crate::gc::GC_TYPE_OBJECT {
            let obj = raw_ptr as *mut crate::object::ObjectHeader;
            if crate::url::try_read_as_search_params(obj).is_some() {
                let boxed = crate::url::js_url_search_params_entries_arr(obj);
                let ptr = crate::value::js_nanbox_get_pointer(boxed) as *mut ArrayHeader;
                if !ptr.is_null() {
                    return ptr;
                }
            }
        }
    }
    // A built-in iterator object (`arr.values()`, `map.entries()`, a String
    // iterator, …) IS already an iterator: drive `.next()` via the class-id
    // tower directly. These now inherit `[Symbol.iterator]` from the shared
    // `%IteratorPrototype%`, so the symbol-method read below would resolve the
    // inherited thunk and call it WITHOUT binding `this` — which yields a bad
    // result. Short-circuit here to keep `Array.from(arr.values())` / `[...it]`
    // working.
    if is_builtin_iterator_class_id(raw_ptr) {
        return js_iterator_to_array(value);
    }
    // Arguments objects spread like arrays (spec:
    // `arguments[Symbol.iterator] === Array.prototype.values`).
    if crate::object::is_arguments_object(raw_ptr as *const crate::object::ObjectHeader) {
        if let Some(arr) = unsafe {
            crate::object::arguments_object_to_array(raw_ptr as *const crate::object::ObjectHeader)
        } {
            return arr;
        }
    }

    let iter_wk = crate::symbol::well_known_symbol("iterator");
    if !iter_wk.is_null() {
        let sym_f64 = f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
        let method = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_f64) };
        if method.to_bits() != crate::value::TAG_UNDEFINED {
            if !is_callable_value(method) {
                throw_iterator_method_not_callable();
            }
            let rebound = crate::closure::clone_closure_rebind_this(method.to_bits(), value);
            let call_target = f64::from_bits(rebound);
            let fn_ptr = js_nanbox_get_pointer(call_target) as *const crate::closure::ClosureHeader;
            if fn_ptr.is_null() {
                throw_iterator_method_not_callable();
            }
            let iter = crate::closure::js_closure_call0(fn_ptr);
            if crate::array::js_array_is_array(iter).to_bits() == crate::value::TAG_TRUE {
                return js_iterator_to_array(crate::array::array_values_iter(iter));
            }
            if !object_like_iterator_result(iter) {
                let msg = b"Result of the Symbol.iterator method is not an object";
                let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
                let err = crate::error::js_typeerror_new(msg_str);
                crate::exception::js_throw(js_nanbox_pointer(err as i64));
            }
            return js_iterator_to_array(iter);
        }
    }

    if crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE {
        return js_iterator_to_array(crate::array::array_values_iter(value));
    }
    if has_named_next(value) {
        return js_iterator_to_array(value);
    }
    throw_not_iterable(value);
}

#[no_mangle]
pub extern "C" fn js_array_spread_append(dest: *mut ArrayHeader, source: f64) -> *mut ArrayHeader {
    let arr = array_from_spread_value(source);
    js_array_concat(dest, arr)
}

/// `true` when `raw_ptr` is a heap `GC_TYPE_OBJECT` whose class id is one of the
/// built-in iterator families (array / map / set / string / buffer / iterator-
/// helper). These dispatch `.next()` via the class-id tower in
/// `js_native_call_method`, so they should be driven directly rather than via the
/// (now inherited) `[Symbol.iterator]` method.
pub(crate) fn is_builtin_iterator_class_id(raw_ptr: usize) -> bool {
    if raw_ptr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let gc =
            (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT {
            return false;
        }
        let class_id = (*(raw_ptr as *const crate::object::ObjectHeader)).class_id;
        matches!(
            class_id,
            crate::array::ARRAY_ITERATOR_CLASS_ID
                | crate::collection_iter_object::MAP_ITERATOR_CLASS_ID
                | crate::collection_iter_object::SET_ITERATOR_CLASS_ID
                | crate::buffer::BUFFER_ITERATOR_CLASS_ID
                | crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID
                | crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID
        ) || class_id == crate::string::STRING_ITERATOR_CLASS_ID
    }
}

fn is_object_like_value(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        let bits = value.to_bits();
        return bits != 0
            && bits <= 0x0000_FFFF_FFFF_FFFF
            && bits > 0x10000
            && crate::closure::is_closure_ptr(bits as usize);
    }
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    raw >= 0x10000 && !crate::symbol::is_registered_symbol(raw)
}

#[cold]
fn throw_iterator_result_not_object() -> ! {
    let msg = b"Iterator result is not an object";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// `IteratorNext(iterator)` for assignment destructuring lowering.
#[no_mangle]
pub extern "C" fn js_iterator_next_result(iter_f64: f64) -> f64 {
    let next = named_field(iter_f64, b"next");
    if !is_callable_value(next) {
        crate::closure::throw_not_callable();
    }
    let prev_this = crate::object::js_implicit_this_set(iter_f64);
    let result = unsafe { crate::closure::js_native_call_value(next, std::ptr::null(), 0) };
    crate::object::js_implicit_this_set(prev_this);
    if !is_object_like_value(result) {
        throw_iterator_result_not_object();
    }
    result
}

/// `IteratorClose(iterator)` when destructuring exits before the iterator is done.
#[no_mangle]
pub extern "C" fn js_iterator_close_if_not_done(iter_f64: f64, done_f64: f64) -> f64 {
    if crate::value::js_is_truthy(done_f64) != 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    let ret = named_field(iter_f64, b"return");
    if ret.to_bits() == crate::value::TAG_UNDEFINED {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if !is_callable_value(ret) {
        crate::closure::throw_not_callable();
    }

    let prev_this = crate::object::js_implicit_this_set(iter_f64);
    let result = unsafe { crate::closure::js_native_call_value(ret, std::ptr::null(), 0) };
    crate::object::js_implicit_this_set(prev_this);
    if !is_object_like_value(result) {
        throw_iterator_result_not_object();
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Issue #1572 — node:stream uses this from `node_stream::ns_iter_flat_map`
/// to drive an async-iterable mapper result (an `async function*` return
/// value) without re-deriving the `Symbol.asyncIterator` lookup +
/// implicit-this dance.
pub(crate) fn call_symbol_async_iterator_for_flat_map(value: f64) -> Option<f64> {
    call_symbol_async_iterator(value)
}

/// Issue #1572 — same as `js_async_iterator_to_array` but reachable from
/// the node_stream crate path so flatMap can flatten an `async function*`
/// mapper result without duplicating the next()/done/value loop.
pub(crate) fn async_iterator_to_array_for_flat_map(iter_f64: f64) -> *mut ArrayHeader {
    js_async_iterator_to_array(iter_f64)
}

/// Issue #1572 — true when `value` is itself an iterator object (has a
/// callable `.next()` own field). Used by flatMap to recognise a bare
/// generator object that doesn't carry `[Symbol.asyncIterator]`.
pub(crate) fn has_iterator_next(value: f64) -> bool {
    has_named_next(value)
}

pub(crate) fn sync_iterator_to_array_if_not_async(iter_f64: f64) -> Option<*mut ArrayHeader> {
    use crate::closure;
    use crate::object::{js_object_get_field_by_name, ObjectHeader};
    use crate::string::js_string_from_bytes;
    use crate::value::{js_nanbox_get_pointer, TAG_UNDEFINED};

    let arr = js_array_alloc(8);
    let iter_ptr = js_nanbox_get_pointer(iter_f64);
    if iter_ptr == 0 {
        return Some(arr);
    }
    let _iter_obj = iter_ptr as *const ObjectHeader;

    // OWN-field only: an inherited built-in `.next` (now provided by the shared
    // `%...IteratorPrototype%`) needs `this` bound by the class-id tower, so it
    // must take the method-dispatch path, not the raw closure call.
    let next_val = crate::object::js_object_get_own_field_or_undef(iter_f64, b"next".as_ptr(), 4);
    let next_val = crate::value::JSValue::from_bits(next_val.to_bits());
    let next_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(next_val)) };
    let next_ptr = if next_val.is_undefined() {
        std::ptr::null::<closure::ClosureHeader>()
    } else {
        js_nanbox_get_pointer(next_f64) as *const closure::ClosureHeader
    };
    let use_method_dispatch = next_ptr.is_null();

    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let mut result = arr;

    for _ in 0..100_000 {
        let step = if use_method_dispatch {
            unsafe {
                crate::object::js_native_call_method(
                    iter_f64,
                    b"next".as_ptr() as *const i8,
                    4,
                    std::ptr::null(),
                    0,
                )
            }
        } else {
            closure::js_closure_call1(next_ptr, f64::from_bits(TAG_UNDEFINED))
        };
        if crate::promise::js_value_is_promise(step) != 0 {
            return None;
        }
        let result_ptr = js_nanbox_get_pointer(step);
        if result_ptr == 0 {
            break;
        }
        let result_obj = result_ptr as *const ObjectHeader;
        let done_val = js_object_get_field_by_name(result_obj, done_key);
        let done_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(done_val)) };
        if crate::value::js_is_truthy(done_f64) != 0 {
            break;
        }

        let val = js_object_get_field_by_name(result_obj, value_key);
        let val_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(val)) };
        result = js_array_push_f64(result, val_f64);
    }

    Some(result)
}

fn call_symbol_async_iterator(value: f64) -> Option<f64> {
    let sym = crate::symbol::well_known_symbol("asyncIterator");
    if sym.is_null() {
        return None;
    }
    let sym_f64 = f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits());
    let method = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_f64) };
    if !is_callable_value(method) {
        return None;
    }
    let prev_this = crate::object::js_implicit_this_set(value);
    let iterator = unsafe { crate::closure::js_native_call_value(method, std::ptr::null(), 0) };
    crate::object::js_implicit_this_set(prev_this);
    if iterator.to_bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(iterator)
    }
}

fn settled_promise_value(value: f64) -> Option<f64> {
    if crate::promise::js_value_is_promise(value) == 0 {
        return Some(value);
    }
    let promise = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    if promise.is_null() {
        return None;
    }
    for _ in 0..10_000 {
        if unsafe { (*promise).state } != crate::promise::PromiseState::Pending {
            break;
        }
        if crate::promise::js_promise_run_microtasks() == 0 {
            break;
        }
    }
    unsafe {
        match (*promise).state {
            crate::promise::PromiseState::Fulfilled => Some((*promise).value),
            crate::promise::PromiseState::Pending | crate::promise::PromiseState::Rejected => None,
        }
    }
}

/// Convert any iterator-protocol object (has `.next()` method) to an array.
/// Used by spread on generators, Array.from on generators, etc.
/// Calls `.next()` in a loop until `.done` is true, collecting `.value` entries.
#[no_mangle]
pub extern "C" fn js_iterator_to_array(iter_f64: f64) -> *mut ArrayHeader {
    use crate::closure;
    use crate::object::{js_object_get_field_by_name, ObjectHeader};
    use crate::string::js_string_from_bytes;
    use crate::value::{js_nanbox_get_pointer, TAG_UNDEFINED};

    let arr = js_array_alloc(8); // start with capacity 8

    // Get the iterator object pointer
    let _iter_bits = iter_f64.to_bits();
    let iter_ptr = js_nanbox_get_pointer(iter_f64);
    if iter_ptr == 0 {
        return arr;
    }
    let _iter_obj = iter_ptr as *const ObjectHeader;

    // Look up the "next" method on the iterator object as a stored closure
    // FIELD (the common case for generator objects / effect's `SingleShotGen`,
    // which store `next` as an own callable property). Use the OWN-field getter:
    // built-in iterators (array/map/set/string) now inherit `.next` from their
    // shared `%...IteratorPrototype%` singleton, and that inherited thunk relies
    // on `this` being bound by the class-id method tower — so an INHERITED
    // `.next` must take the method-dispatch path below, not this raw
    // closure-call (which doesn't bind `this`).
    let next_val = crate::object::js_object_get_own_field_or_undef(iter_f64, b"next".as_ptr(), 4);
    let next_val = crate::value::JSValue::from_bits(next_val.to_bits());
    let next_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(next_val)) };
    let next_ptr = if next_val.is_undefined() {
        std::ptr::null::<closure::ClosureHeader>()
    } else {
        js_nanbox_get_pointer(next_f64) as *const closure::ClosureHeader
    };
    // #321: some iterators (perry's runtime array iterator with
    // `ARRAY_ITERATOR_CLASS_ID`, Buffer iterators) dispatch `.next()` through
    // the class-id method tower in `js_native_call_method` rather than storing
    // a `next` closure field, so the field lookup above misses. Fall back to a
    // method-call dispatch in that case instead of bailing with an empty array.
    let use_method_dispatch = next_ptr.is_null();

    // Iterate: call next() until done
    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let mut result = arr;

    for _ in 0..100_000 {
        // safety limit
        // Call next() — stored-closure fast path, or class-id method dispatch.
        let result_f64 = if use_method_dispatch {
            unsafe {
                crate::object::js_native_call_method(
                    iter_f64,
                    b"next".as_ptr() as *const i8,
                    4,
                    std::ptr::null(),
                    0,
                )
            }
        } else {
            closure::js_closure_call1(next_ptr, f64::from_bits(TAG_UNDEFINED))
        };
        let result_ptr = js_nanbox_get_pointer(result_f64);
        if result_ptr == 0 {
            break;
        }
        let result_obj = result_ptr as *const ObjectHeader;

        // Check .done
        let done_val = js_object_get_field_by_name(result_obj, done_key);
        let done_bits = unsafe { std::mem::transmute::<_, u64>(done_val) };
        // done is true when it's TAG_TRUE (0x7FFC_0000_0000_0004) or truthy number
        if done_bits == 0x7FFC_0000_0000_0004 {
            break;
        } // TAG_TRUE

        // Get .value and push to array
        let val = js_object_get_field_by_name(result_obj, value_key);
        let val_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(val)) };
        result = js_array_push_f64(result, val_f64);
    }

    result
}

fn js_async_iterator_to_array(iter_f64: f64) -> *mut ArrayHeader {
    use crate::closure;
    use crate::object::{js_object_get_field_by_name, ObjectHeader};
    use crate::string::js_string_from_bytes;
    use crate::value::{js_nanbox_get_pointer, TAG_TRUE, TAG_UNDEFINED};

    let arr = js_array_alloc(8);
    let iter_ptr = js_nanbox_get_pointer(iter_f64);
    if iter_ptr == 0 {
        return arr;
    }
    let _ = iter_ptr;
    // OWN-field only (see sync variant): inherited built-in `.next` needs the
    // class-id method tower to bind `this`.
    let next_val = crate::object::js_object_get_own_field_or_undef(iter_f64, b"next".as_ptr(), 4);
    let next_val = crate::value::JSValue::from_bits(next_val.to_bits());
    let next_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(next_val)) };
    let next_ptr = if next_val.is_undefined() {
        std::ptr::null::<closure::ClosureHeader>()
    } else {
        js_nanbox_get_pointer(next_f64) as *const closure::ClosureHeader
    };
    let use_method_dispatch = next_ptr.is_null();
    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let mut result = arr;

    for _ in 0..100_000 {
        let step = if use_method_dispatch {
            unsafe {
                crate::object::js_native_call_method(
                    iter_f64,
                    b"next".as_ptr() as *const i8,
                    4,
                    std::ptr::null(),
                    0,
                )
            }
        } else {
            closure::js_closure_call1(next_ptr, f64::from_bits(TAG_UNDEFINED))
        };
        let Some(step_result) = settled_promise_value(step) else {
            break;
        };
        let result_ptr = js_nanbox_get_pointer(step_result);
        if result_ptr == 0 {
            break;
        }
        let result_obj = result_ptr as *const ObjectHeader;
        let done_val = js_object_get_field_by_name(result_obj, done_key);
        let done_bits = unsafe { std::mem::transmute::<_, u64>(done_val) };
        if done_bits == TAG_TRUE {
            break;
        }
        let val = js_object_get_field_by_name(result_obj, value_key);
        let val_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(val)) };
        result = js_array_push_f64(result, val_f64);
    }

    result
}
