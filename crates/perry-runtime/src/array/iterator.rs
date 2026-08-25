//! Iterator-protocol → array converter.
use super::*;
use crate::value::nanbox_string_key;

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
        GC_HEADER_SIZE, GC_TYPE_ARRAY, GC_TYPE_LAZY_ARRAY, GC_TYPE_MAP, GC_TYPE_SET, GcHeader,
    };
    use crate::value::{JSValue, js_nanbox_pointer};

    let jsv = JSValue::from_bits(val_f64.to_bits());
    if let Some(entries) = entries_array_for_small_handle_value(val_f64) {
        return js_nanbox_pointer(entries as i64);
    }

    // `class X extends Map | Set` instance — iterate its hidden backing
    // collection (`for (const [k,v] of mapSubclass)` / `for (const v of
    // setSubclass)`). Map yields `[k, v]` pairs (=== `.entries()`), Set yields
    // values, matching the builtins' default `[Symbol.iterator]`. Skipped when
    // the subclass overrides `[Symbol.iterator]` so the override drives `for…of`.
    match crate::object::map_set_subclass::subclass_backing_for_default_iteration(val_f64) {
        Some(crate::object::map_set_subclass::CollectionBacking::Map(m)) => {
            let arr = js_map_entries_for_for_of(m as i64);
            return js_nanbox_pointer(arr as i64);
        }
        Some(crate::object::map_set_subclass::CollectionBacking::Set(s)) => {
            let arr = js_set_to_array_for_for_of(s as i64);
            return js_nanbox_pointer(arr as i64);
        }
        None => {}
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

    // #6454: a class DECLARATION is an INT32-tagged ClassRef whose low bits are
    // the class id — `js_nanbox_get_pointer` below would misread that id as a
    // heap address and the GC-header sniff would dereference `id - 8`. Resolve
    // its (possibly inherited, #36/#321) `[Symbol.iterator]` and drive it;
    // a class with none is not iterable, exactly like node
    // (`for (const x of Plain) {}` → TypeError). Must run BEFORE the raw-pointer
    // logic below.
    if crate::object::class_ref_id(val_f64).is_some() {
        if crate::symbol::class_ref_resolves_iterator(val_f64) {
            let iter = crate::symbol::js_get_iterator(val_f64);
            let arr = js_iterator_to_array(iter);
            return js_nanbox_pointer(arr as i64);
        }
        throw_not_iterable(val_f64);
    }

    // Non-pointer scalars (number/bool/null/undefined/symbol) are not
    // iterable. Per ECMA-262 §13.7.5.13 (ForIn/OfHeadEvaluation →
    // GetIterator → ToObject/GetMethod) these MUST throw a TypeError:
    // `for (x of null)`, `for (x of 37)`, `for (x of false)` all reject
    // (language/statements/for-of/head-expr-to-obj,
    // head-expr-primitive-iterator-method). Web Streams are async-iterable
    // only, so plain `for...of` rejects them here too.
    let raw_ptr = crate::value::js_nanbox_get_pointer(val_f64);
    if raw_ptr == 0 {
        throw_not_iterable(val_f64);
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
        // custom `[Symbol.iterator]` or a `.next()`: walk the synchronous
        // iterator protocol. `js_get_iterator` returns the operand's
        // `Symbol.iterator()` result when iterable, or the operand unchanged
        // when it already is an iterator (perry generators). Plain `for...of`
        // must not fall back to `Symbol.asyncIterator`; async-only stream
        // values belong to the dedicated `for await...of` lowering.
        _ => {
            let iter = crate::symbol::js_get_iterator(val_f64);
            let arr = if iter.to_bits() != val_f64.to_bits() {
                js_iterator_to_array(iter)
            } else if is_builtin_iterator_class_id(raw_ptr as usize) {
                js_iterator_to_array(iter)
            } else if has_named_next(iter) {
                js_iterator_to_array(iter)
            } else {
                throw_not_iterable(val_f64);
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
    if id <= 0 || !crate::value::addr_class::is_small_handle(id as usize) {
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
    use crate::object::{ObjectHeader, js_object_get_field_by_name};
    use crate::string::js_string_from_bytes;
    use crate::value::{TAG_UNDEFINED, js_nanbox_get_pointer};

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

fn boxed_promise_value(promise: *mut crate::promise::Promise) -> f64 {
    crate::value::js_nanbox_pointer(promise as i64)
}

/// PERRY_ITER_BT diagnostic: dump which throw site fired, the offending
/// value's decoded shape, and a native backtrace. Gated on env var so it has
/// zero cost in normal builds. Used to localize the bundle's
/// "Iterator result is not an object" wall.
pub(crate) fn iter_bt_dump(tag: &str, value: f64) {
    if std::env::var("PERRY_ITER_BT").is_err() {
        return;
    }
    let bits = value.to_bits();
    let jv = crate::value::JSValue::from_bits(bits);
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    let kind = if jv.is_number() {
        "number"
    } else if jv.is_int32() {
        "int32"
    } else if jv.is_bool() {
        "bool"
    } else if jv.is_undefined() {
        "undefined"
    } else if jv.is_null() {
        "null"
    } else if jv.is_any_string() {
        "string"
    } else if jv.is_bigint() {
        "bigint"
    } else if jv.is_pointer() {
        "pointer"
    } else {
        "other"
    };
    let in_handle_band = crate::value::addr_class::is_handle_band(raw);
    eprintln!(
        "[PERRY_ITER_BT] site={tag} bits={bits:#018x} kind={kind} raw={raw:#x} handle_band={in_handle_band}",
    );
    let bt = std::backtrace::Backtrace::force_capture();
    eprintln!("{bt}");
}

fn async_from_sync_type_error(message: &[u8]) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

fn async_from_sync_rejected(message: &[u8]) -> f64 {
    boxed_promise_value(crate::promise::js_promise_rejected(
        async_from_sync_type_error(message),
    ))
}

fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn async_from_sync_iter_result(value: f64, done: bool) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let done_key = crate::string::js_string_from_bytes(b"done".as_ptr(), 4);
    crate::object::js_object_set_field_by_name(obj, value_key, value);
    crate::object::js_object_set_field_by_name(
        obj,
        done_key,
        if done {
            f64::from_bits(crate::value::TAG_TRUE)
        } else {
            f64::from_bits(crate::value::TAG_FALSE)
        },
    );
    crate::value::js_nanbox_pointer(obj as i64)
}

extern "C" fn async_from_sync_fulfilled(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let promise =
        crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    let done = crate::closure::js_closure_get_capture_f64(closure, 1) != 0.0;
    if !promise.is_null() {
        crate::promise::js_promise_resolve(promise, async_from_sync_iter_result(value, done));
    }
    0.0
}

extern "C" fn async_from_sync_rejected_value(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let promise =
        crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    let iter = crate::closure::js_closure_get_capture_f64(closure, 1);
    let close_on_rejection = crate::closure::js_closure_get_capture_f64(closure, 2) != 0.0;
    if close_on_rejection {
        async_from_sync_close(iter);
    }
    if !promise.is_null() {
        crate::promise::js_promise_reject(promise, reason);
    }
    0.0
}

fn async_from_sync_continue(iter: f64, step_result: f64, close_on_rejection: bool) -> f64 {
    let ptr = crate::value::js_nanbox_get_pointer(step_result);
    if ptr == 0 {
        iter_bt_dump("async_from_sync_continue", step_result);
        return async_from_sync_rejected(b"Iterator result is not an object");
    }

    let result_obj = ptr as *const crate::object::ObjectHeader;
    let done_key = crate::string::js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let done = {
        let done_val = crate::object::js_object_get_field_by_name(result_obj, done_key);
        let done_f64 = f64::from_bits(done_val.bits());
        crate::value::js_is_truthy(done_f64) != 0
    };
    let value = {
        let value_val = crate::object::js_object_get_field_by_name(result_obj, value_key);
        f64::from_bits(value_val.bits())
    };

    let outer = crate::promise::js_promise_new();
    let on_fulfilled = crate::closure::js_closure_alloc(async_from_sync_fulfilled as *const u8, 2);
    let on_rejected =
        crate::closure::js_closure_alloc(async_from_sync_rejected_value as *const u8, 3);
    crate::closure::js_closure_set_capture_ptr(on_fulfilled, 0, outer as i64);
    crate::closure::js_closure_set_capture_f64(on_fulfilled, 1, if done { 1.0 } else { 0.0 });
    crate::closure::js_closure_set_capture_ptr(on_rejected, 0, outer as i64);
    crate::closure::js_closure_set_capture_f64(on_rejected, 1, iter);
    crate::closure::js_closure_set_capture_f64(
        on_rejected,
        2,
        if close_on_rejection { 1.0 } else { 0.0 },
    );

    let value_promise = match crate::promise::js_promise_resolved_catching(value) {
        Ok(promise) => promise,
        Err(reason) => {
            if close_on_rejection {
                async_from_sync_close(iter);
            }
            crate::promise::js_promise_reject(outer, reason);
            return boxed_promise_value(outer);
        }
    };
    crate::promise::js_promise_then(value_promise, on_fulfilled, on_rejected);
    boxed_promise_value(outer)
}

fn async_from_sync_rest_args(rest: f64) -> (usize, f64) {
    let ptr = crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader;
    if ptr.is_null() {
        return (0, undefined_value());
    }
    let len = crate::array::js_array_length(ptr) as usize;
    let first = if len == 0 {
        undefined_value()
    } else {
        crate::array::js_array_get_f64(ptr, 0)
    };
    (len, first)
}

fn async_from_sync_call_raw(iter: f64, method: &[u8], args: &[f64]) -> Result<Option<f64>, f64> {
    let method_value = named_field(iter, method);
    // Spec `%AsyncFromSyncIteratorPrototype%.{return,throw}` (and the sync
    // `yield *` close) do `GetMethod(syncIterator, name)` ONCE and then
    // `Call(method, syncIterator, args)` on that captured value. Re-dispatching
    // by NAME here re-read the property a second time, firing a `get return` /
    // `get throw` accessor twice and diverging from Node's operation order
    // (test262 yield-star-sync-return). So when a callable method was found,
    // invoke the already-fetched value with `this = iter`.
    let callable = if method_value.to_bits() == crate::value::TAG_UNDEFINED {
        let raw = crate::value::js_nanbox_get_pointer(iter) as usize;
        if method != b"next" || !is_builtin_iterator_class_id(raw) {
            return Ok(None);
        }
        // A builtin iterator that exposes no readable `next` property: fall back
        // to method dispatch (string/typed-array iterators tower their `next`
        // through the class-id method table).
        false
    } else if crate::JSValue::from_bits(method_value.to_bits()).is_null() {
        // ECMA-262 §27.1.4.2.2: GetMethod treats null the same as undefined —
        // a null `return`/`throw` method means the method is absent → Ok(None).
        return Ok(None);
    } else if !is_callable_value(method_value) {
        return Err(async_from_sync_type_error(
            b"Async-from-sync iterator method is not callable",
        ));
    } else {
        true
    };

    let prev_this = if callable {
        Some(crate::object::js_implicit_this_set(iter))
    } else {
        None
    };
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut std::os::raw::c_int) };
    let result = if jumped == 0 {
        let args_ptr = if args.is_empty() {
            std::ptr::null()
        } else {
            args.as_ptr()
        };
        let value = if callable {
            unsafe { crate::closure::js_native_call_value(method_value, args_ptr, args.len()) }
        } else {
            unsafe {
                crate::object::js_native_call_method(
                    iter,
                    method.as_ptr() as *const i8,
                    method.len(),
                    args_ptr,
                    args.len(),
                )
            }
        };
        Ok(Some(value))
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(exc)
    };
    if let Some(prev) = prev_this {
        crate::object::js_implicit_this_set(prev);
    }
    crate::exception::js_try_end();
    result
}

/// Invoke a pre-fetched method value with `this` = `iter`. Mirrors
/// [`async_from_sync_call_raw`] but skips the per-call property read — used for
/// the `next` method, whose `[[NextMethod]]` the spec captures ONCE at
/// CreateAsyncFromSyncIterator time and reuses for every step (ECMA-262
/// §27.1.4.2). Re-reading `next` per call re-ran the sync iterator's `get next`
/// accessor on every pull, diverging from Node's operation order
/// (test262 yield-star-sync-next).
fn async_from_sync_call_cached_raw(
    iter: f64,
    method_value: f64,
    args: &[f64],
) -> Result<Option<f64>, f64> {
    if !is_callable_value(method_value) {
        return Err(async_from_sync_type_error(
            b"Async-from-sync iterator method is not callable",
        ));
    }
    let prev_this = crate::object::js_implicit_this_set(iter);
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut std::os::raw::c_int) };
    let result = if jumped == 0 {
        let args_ptr = if args.is_empty() {
            std::ptr::null()
        } else {
            args.as_ptr()
        };
        let value =
            unsafe { crate::closure::js_native_call_value(method_value, args_ptr, args.len()) };
        Ok(Some(value))
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(exc)
    };
    crate::object::js_implicit_this_set(prev_this);
    crate::exception::js_try_end();
    result
}

fn async_from_sync_close(iter: f64) {
    let _ = async_from_sync_call_raw(iter, b"return", &[]);
}

fn async_from_sync_call(iter: f64, method: &[u8], args: &[f64], close_on_rejection: bool) -> f64 {
    match async_from_sync_call_raw(iter, method, args) {
        Ok(Some(step)) => async_from_sync_continue(iter, step, close_on_rejection),
        Ok(None) => async_from_sync_rejected(b"Async-from-sync iterator method is not callable"),
        Err(reason) => boxed_promise_value(crate::promise::js_promise_rejected(reason)),
    }
}

extern "C" fn async_from_sync_next(
    closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let iter = crate::closure::js_closure_get_capture_f64(closure, 0);
    let cached_next = crate::closure::js_closure_get_capture_f64(closure, 1);
    let (argc, first) = async_from_sync_rest_args(rest);
    let single = [first];
    let args: &[f64] = if argc == 0 { &[] } else { &single };
    // Use the captured `[[NextMethod]]` when it is a readable callable (the
    // observable-getter case). Builtin iterators (array/map/set/string) expose
    // no readable own `next` and dispatch through the class-id method tower, so
    // fall back to the by-name call for them.
    if is_callable_value(cached_next) {
        return match async_from_sync_call_cached_raw(iter, cached_next, args) {
            Ok(Some(step)) => async_from_sync_continue(iter, step, true),
            Ok(None) => async_from_sync_call(iter, b"next", args, true),
            Err(reason) => boxed_promise_value(crate::promise::js_promise_rejected(reason)),
        };
    }
    async_from_sync_call(iter, b"next", args, true)
}

extern "C" fn async_from_sync_return(
    closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let iter = crate::closure::js_closure_get_capture_f64(closure, 0);
    let (argc, first) = async_from_sync_rest_args(rest);
    let single = [first];
    let args: &[f64] = if argc == 0 { &[] } else { &single };
    match async_from_sync_call_raw(iter, b"return", args) {
        Ok(Some(step)) => async_from_sync_continue(iter, step, false),
        Ok(None) => {
            let value = if argc == 0 { undefined_value() } else { first };
            let done = async_from_sync_iter_result(value, true);
            boxed_promise_value(crate::promise::js_promise_resolved(done))
        }
        Err(reason) => boxed_promise_value(crate::promise::js_promise_rejected(reason)),
    }
}

extern "C" fn async_from_sync_throw(
    closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let iter = crate::closure::js_closure_get_capture_f64(closure, 0);
    let (argc, first) = async_from_sync_rest_args(rest);
    let single = [first];
    let args: &[f64] = if argc == 0 { &[] } else { &single };
    match async_from_sync_call_raw(iter, b"throw", args) {
        Ok(Some(step)) => async_from_sync_continue(iter, step, true),
        Ok(None) => {
            async_from_sync_close(iter);
            async_from_sync_rejected(b"The iterator does not provide a 'throw' method.")
        }
        Err(reason) => boxed_promise_value(crate::promise::js_promise_rejected(reason)),
    }
}

extern "C" fn async_from_sync_async_iterator(closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

fn register_async_from_sync_thunks_once() {
    crate::perry_thread_local! {
        static REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    REGISTERED.with(|flag| {
        if flag.get() {
            return;
        }
        crate::closure::js_register_closure_rest(async_from_sync_next as *const u8, 0);
        crate::closure::js_register_closure_rest(async_from_sync_return as *const u8, 0);
        crate::closure::js_register_closure_rest(async_from_sync_throw as *const u8, 0);
        crate::closure::js_register_closure_arity(async_from_sync_async_iterator as *const u8, 0);
        flag.set(true);
    });
}

fn install_async_from_sync_method(
    obj: *mut crate::object::ObjectHeader,
    name: &[u8],
    func: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64,
    iter: f64,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(func as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, iter);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
    value
}

/// Install the wrapper's `next` method with TWO captures: the sync iterator
/// (slot 0) and its pre-fetched `[[NextMethod]]` (slot 1, see
/// [`async_from_sync_call_cached_raw`]).
fn install_async_from_sync_next(
    obj: *mut crate::object::ObjectHeader,
    iter: f64,
    cached_next: f64,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(async_from_sync_next as *const u8, 2);
    crate::closure::js_closure_set_capture_f64(closure, 0, iter);
    crate::closure::js_closure_set_capture_f64(closure, 1, cached_next);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    let key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
    crate::object::js_object_set_field_by_name(obj, key, value);
    value
}

pub(crate) fn async_from_sync_wrap_iterator(iter: f64) -> f64 {
    register_async_from_sync_thunks_once();
    let obj = crate::object::js_object_alloc(0, 0);
    let wrapper = crate::value::js_nanbox_pointer(obj as i64);
    // Spec (CreateAsyncFromSyncIterator): the sync iterator record's
    // `[[NextMethod]]` is read once, here, and reused for every `next()` step.
    let cached_next = named_field(iter, b"next");
    install_async_from_sync_next(obj, iter, cached_next);
    install_async_from_sync_method(obj, b"return", async_from_sync_return, iter);
    install_async_from_sync_method(obj, b"throw", async_from_sync_throw, iter);
    let async_iter =
        crate::closure::js_closure_alloc(async_from_sync_async_iterator as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(async_iter, 0, wrapper);
    let sym = crate::symbol::well_known_symbol("asyncIterator");
    if !sym.is_null() {
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                wrapper,
                f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits()),
                crate::value::js_nanbox_pointer(async_iter as i64),
            );
        }
    }
    wrapper
}

#[no_mangle]
pub extern "C" fn js_get_async_iterator(value: f64) -> f64 {
    // GetIterator(value, async) — ECMA-262 §7.4.3.
    //
    // Spec ordering matters (test262 yield-star-getiter-async-*): consult
    // @@asyncIterator with GetMethod semantics FIRST. A method that is present
    // but not callable is a TypeError; a callable method whose result is not an
    // Object is a TypeError. Only an ABSENT (undefined/null) @@asyncIterator
    // falls back to the sync iterator wrapped via CreateAsyncFromSyncIterator —
    // so e.g. `yield* { [Symbol.asyncIterator]() { return undefined } }` throws
    // instead of (wrongly) reaching the object's `[Symbol.iterator]`.
    let sym = crate::symbol::well_known_symbol("asyncIterator");
    if !sym.is_null() {
        let sym_f64 = f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits());
        let method = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_f64) };
        let mb = method.to_bits();
        if mb != crate::value::TAG_UNDEFINED && mb != crate::value::TAG_NULL {
            // @@asyncIterator is present: GetMethod requires it be callable.
            if !is_callable_value(method) {
                throw_iterator_method_not_callable();
            }
            let prev_this = crate::object::js_implicit_this_set(value);
            let iterator =
                unsafe { crate::closure::js_native_call_value(method, std::ptr::null(), 0) };
            crate::object::js_implicit_this_set(prev_this);
            // GetIterator step 5: the result must be an Object.
            if !is_async_iterator_object(iterator) {
                throw_iterator_result_not_object();
            }
            return iterator;
        }
        // @@asyncIterator absent → fall through to the sync-iterator path.
    }

    let iter = crate::symbol::js_get_iterator(value);
    let raw = crate::value::js_nanbox_get_pointer(iter) as usize;
    if iter.to_bits() == value.to_bits()
        && !is_builtin_iterator_class_id(raw)
        && !has_named_next(iter)
    {
        throw_not_iterable(value);
    }

    async_from_sync_wrap_iterator(iter)
}

/// `Type(x) is Object` for the GetIterator(async) result check: heap
/// pointer-tagged values that are not registered Symbols (strings, numbers,
/// booleans, null/undefined, symbols are all NOT objects).
fn is_async_iterator_object(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    jv.is_pointer()
        && !crate::symbol::is_registered_symbol(crate::value::js_nanbox_get_pointer(value) as usize)
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

/// #7542: does this value carry its OWN `[Symbol.iterator]`? An own method
/// shadows the prototype, so the patched-prototype shortcut must stand aside
/// for it and let the ordinary `@@iterator` walk resolve it.
fn array_has_own_iterator(value: f64) -> bool {
    let iter_sym = crate::symbol::well_known_symbol("iterator");
    if iter_sym.is_null() {
        return false;
    }
    let sym_value = f64::from_bits(crate::value::JSValue::pointer(iter_sym as *const u8).bits());
    unsafe { crate::symbol::has_own_symbol_property(value, sym_value) }
}

pub(crate) fn array_from_spread_value(value: f64) -> *mut ArrayHeader {
    use crate::value::{JSValue, POINTER_MASK, js_nanbox_get_pointer, js_nanbox_pointer};

    // #7498: the spread receiver is a GC-managed value, and this function
    // carries it across a dozen classification probes AND the whole
    // `[Symbol.iterator]` prototype walk. That walk allocates a key string on
    // every hop (`array_prototype_property_value` →
    // `default_object_prototype_property_value` → `js_object_get_field_by_name`
    // is where `PERRY_GC_PROTECT_FROMSPACE=1` faults), so the copying minor can
    // move the receiver while it exists only in this bare `value` argument —
    // which the collector cannot see and therefore never rewrites.
    //
    // The uses AFTER the walk are the consequential ones:
    //   * `clone_closure_rebind_this(method, value)` would bind a from-space
    //     `this`, so a user `[Symbol.iterator]()` factory reads a dead
    //     receiver and yields nothing;
    //   * `js_implicit_this_set(value)` publishes the same dead receiver to
    //     the canonical bound-method path;
    //   * the `js_array_is_array(value)` fallback reads a recycled GcHeader
    //     and reports a live array as "not iterable".
    //
    // Root it FIRST — before anything here allocates — and SHADOW the argument
    // with readers, so the pre-collection address is not nameable below. Every
    // handle is NaN-boxed, so a read-back is `get_nanbox_f64` and this module
    // stays out of `scripts/raw_handle_debt.py`'s ledger.
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_h = scope.root_nanbox_f64(value);
    let value = || value_h.get_nanbox_f64();
    let raw_ptr = || js_nanbox_get_pointer(value_h.get_nanbox_f64()) as usize;

    let jsv = JSValue::from_bits(value().to_bits());
    if jsv.is_null() || jsv.is_undefined() {
        throw_not_iterable(value());
    }
    if jsv.is_any_string() {
        let str_ptr = crate::value::js_get_string_pointer_unified(value());
        let str_bits = crate::value::STRING_TAG | (str_ptr as u64 & POINTER_MASK);
        return crate::string::js_string_to_char_array(str_bits as i64) as *mut ArrayHeader;
    }

    // #6454: `[...SomeClass]` / `fn(...SomeClass)` on a class DECLARATION — an
    // INT32-tagged ClassRef. Drive its (possibly inherited) `[Symbol.iterator]`;
    // with none it is not iterable, like node. Must run before the raw-pointer
    // reads below, which would misread the class id as a heap address.
    if crate::object::class_ref_id(value()).is_some() {
        if crate::symbol::class_ref_resolves_iterator(value()) {
            let iter = crate::symbol::js_get_iterator(value());
            return js_iterator_to_array(iter);
        }
        throw_not_iterable(value());
    }

    if raw_ptr() == 0 {
        throw_not_iterable(value());
    }

    // An Array Proxy is a small registry id, not an `ArrayHeader`. Route it
    // through the full GetIterator path before any raw-address classifiers.
    // `array_values_iter` stores the proxy value as a dedicated live backing,
    // so the default Array iterator performs `Get(length)` / `Get(index)`
    // through traps instead of dereferencing the id or unwrapping the target.
    if crate::proxy::js_proxy_is_proxy(value()) != 0 {
        return js_iterator_to_array(crate::symbol::js_get_iterator(value()));
    }

    // #7533: the overwhelmingly common spread — an ordinary dense array — is a
    // straight element copy that nobody can observe as anything else. Take it
    // before the classification probes and long before the `@@iterator` walk
    // below: on the `object_deep_clone` app-pattern kernel that walk plus the
    // `.next()` drain it feeds was 90% of the whole process, and the identical
    // copy through `Array.from`'s memcpy was ~66x cheaper.
    //
    // `dense_spread_source` proves ordinariness (see its doc comment for each
    // gate); anything it cannot prove falls through to the unchanged protocol.
    // It runs before `entries_array_for_small_handle_id` / `is_registered_buffer`
    // only because it never dereferences an unvalidated address itself —
    // `try_read_gc_header` rejects the handle band and the header-less
    // small-buffer slab without touching memory.
    if crate::array::dense_spread_source(value()).is_some() {
        return crate::array::dense_spread_copy(value());
    }

    // #7542: `Array.prototype[Symbol.iterator] = fn` must drive the spread, as
    // it already drives `for…of` over a literal and destructuring.
    //
    // This has to run BEFORE the generic `[Symbol.iterator]` walk below, not
    // after it. The walk does NOT miss for a plain array — contrary to how this
    // looked from the outside, `js_object_get_symbol_property` synthesizes the
    // BUILT-IN `Symbol.iterator` for an array receiver (a `js_class_method_bind`
    // by name) rather than reading the prototype slot, so the walk resolves the
    // builtin, calls it, and returns the element copy. A guard placed after the
    // walk is dead code: measured, it is never reached.
    //
    // `js_get_iterator` is the one implementation that consults the patched
    // prototype under this flag (per GetIterator: read the method off
    // `Array.prototype`, call it with `this === val`, TypeError when deleted or
    // non-callable). Delegate rather than restate it — two copies of a spec
    // sequence that must agree is how this diverged in the first place.
    //
    // Placed after the dense fast path, which already declines when the flag is
    // set (`flat_clone.rs`), and after the string/class-ref arms above, which
    // are not arrays. The unpatched path is untouched: the flag is sticky-false
    // until user code writes the prototype slot.
    // An OWN `arr[Symbol.iterator]` still wins over the prototype, so this must
    // not preempt the walk below for that case: `js_get_iterator`'s patched
    // branch reads the PROTOTYPE only, and would throw "not iterable" for an
    // array carrying its own method once the prototype slot has been deleted.
    if crate::array::array_proto_iterator_modified()
        && crate::array::js_array_is_array(value()).to_bits() == crate::value::TAG_TRUE
        && !array_has_own_iterator(value())
    {
        return js_iterator_to_array(crate::symbol::js_get_iterator(value()));
    }

    if let Some(entries) = entries_array_for_small_handle_id(raw_ptr() as i64) {
        return entries;
    }
    if crate::buffer::is_registered_buffer(raw_ptr()) {
        return crate::buffer::buffer_to_array(raw_ptr() as *const crate::buffer::BufferHeader);
    }
    if crate::set::is_registered_set(raw_ptr()) {
        return crate::set::js_set_to_array(raw_ptr() as *const crate::set::SetHeader);
    }
    if crate::map::is_registered_map(raw_ptr()) {
        return crate::map::js_map_entries(raw_ptr() as *const crate::map::MapHeader);
    }
    // `class X extends Map | Set` instance — spread (`[...container]`,
    // `Array.from(container)`, `fn(...container)`) over the hidden backing
    // collection's default iterator (Map → entries, Set → values), matching
    // the builtins. The `is_registered_*` checks above only match a real
    // Map/Set value, so a subclass instance (a plain object with a backing
    // field) falls through to here. Skipped when the subclass overrides
    // `[Symbol.iterator]` so the override drives the spread.
    match crate::object::map_set_subclass::subclass_backing_for_default_iteration(value()) {
        Some(crate::object::map_set_subclass::CollectionBacking::Map(m)) => {
            return crate::map::js_map_entries(m as *const crate::map::MapHeader);
        }
        Some(crate::object::map_set_subclass::CollectionBacking::Set(s)) => {
            return crate::set::js_set_to_array(s as *const crate::set::SetHeader);
        }
        None => {}
    }
    // `class X extends Array` instance — object-backed; spread (`[...sub]`,
    // `fn(...sub)`) over a dense snapshot of its indexed elements. The generic
    // `[Symbol.iterator]` lookup below would resolve the inherited array
    // iterator, which misreads the plain object as a dense `ArrayHeader`.
    // Matches the Map/Set-subclass branch above. Skipped when the subclass
    // declared its own `[Symbol.iterator]`, so the override drives the spread
    // via the generic symbol lookup below.
    if crate::array::is_array_subclass_instance(value())
        && !crate::array::array_subclass_has_iterator_override(value())
    {
        let snap = crate::array::array_subclass_dense_snapshot(value());
        return crate::value::js_nanbox_get_pointer(snap) as *mut ArrayHeader;
    }
    if crate::typedarray::lookup_typed_array_kind(raw_ptr()).is_some() {
        return crate::typedarray::typed_array_to_array(
            raw_ptr() as *const crate::typedarray::TypedArrayHeader
        );
    }
    if raw_ptr() >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        let obj_type = unsafe {
            let hdr = (raw_ptr() as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            (*hdr).obj_type
        };
        if obj_type == crate::gc::GC_TYPE_OBJECT {
            // `try_read_as_search_params` interns its own key string, so the
            // receiver address is re-read from the root for the entries call
            // rather than reused from before that probe.
            if crate::url::try_read_as_search_params(raw_ptr() as *mut crate::object::ObjectHeader)
                .is_some()
            {
                let boxed = crate::url::js_url_search_params_entries_arr(
                    raw_ptr() as *mut crate::object::ObjectHeader
                );
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
    if is_builtin_iterator_class_id(raw_ptr()) {
        return js_iterator_to_array(value());
    }
    // Arguments objects spread like arrays (spec:
    // `arguments[Symbol.iterator] === Array.prototype.values`).
    if crate::object::is_arguments_object(raw_ptr() as *const crate::object::ObjectHeader) {
        if let Some(arr) = unsafe {
            crate::object::arguments_object_to_array(raw_ptr() as *const crate::object::ObjectHeader)
        } {
            return arr;
        }
    }

    let iter_wk = crate::symbol::well_known_symbol("iterator");
    if !iter_wk.is_null() {
        // The well-known symbol is itself a heap object, and the lookup it is
        // about to key can collect, so it gets a root of its own rather than a
        // raw `iter_wk` carried across the call.
        let sym_h = scope.root_nanbox_f64(f64::from_bits(
            crate::value::JSValue::pointer(iter_wk as *const u8).bits(),
        ));
        let method = unsafe {
            crate::symbol::js_object_get_symbol_property(value(), sym_h.get_nanbox_f64())
        };
        // The resolved method is a fresh closure value that must survive
        // `clone_closure_rebind_this` (an allocation) and the factory call.
        let method_h = scope.root_nanbox_f64(method);
        if method_h.get_nanbox_f64().to_bits() != crate::value::TAG_UNDEFINED {
            if !is_callable_value(method_h.get_nanbox_f64()) {
                throw_iterator_method_not_callable();
            }
            let rebound =
                crate::closure::clone_closure_rebind_this(method_h.get_nanbox_u64(), value());
            let rebound_h = scope.root_nanbox_u64(rebound);
            if js_nanbox_get_pointer(rebound_h.get_nanbox_f64()) == 0 {
                throw_iterator_method_not_callable();
            }
            // Spec `GetIterator(obj)` → `Call(method, obj)`: the
            // `[Symbol.iterator]()` factory runs with `this === obj`. A canonical
            // bound class method (#5128's `@@iterator` wrapper) reads its receiver
            // from IMPLICIT_THIS, so set it here too — mirroring `js_get_iterator`.
            // Without this the wrapper saw a stale `this` and the generator
            // yielded nothing (empty spread).
            let prev_this = crate::object::js_implicit_this_set(value());
            // The DISPLACED receiver rides through arbitrary user code before
            // being republished, so it is rooted too — republishing a from-space
            // `this` is the same defect one frame out.
            let prev_this_h = scope.root_nanbox_f64(prev_this);
            let trap_buf = crate::exception::js_try_push();
            let jumped =
                unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut std::os::raw::c_int) };
            // `js_try_push` captured the handle-stack depth AFTER these roots
            // were pushed, so the `longjmp` restore below leaves them intact and
            // reading them here is sound.
            let iter = if jumped == 0 {
                crate::closure::js_closure_call0(js_nanbox_get_pointer(rebound_h.get_nanbox_f64())
                    as *const crate::closure::ClosureHeader)
            } else {
                // Factory threw: restore the receiver and unwind the trap frame
                // before re-propagating, so IMPLICIT_THIS can't leak into later
                // calls (mirrors `async_from_sync_call_cached_raw` above).
                let exc = crate::exception::js_get_exception();
                crate::exception::js_clear_exception();
                crate::object::js_implicit_this_set(prev_this_h.get_nanbox_f64());
                crate::exception::js_try_end();
                crate::exception::js_throw(exc)
            };
            let iter_h = scope.root_nanbox_f64(iter);
            crate::object::js_implicit_this_set(prev_this_h.get_nanbox_f64());
            crate::exception::js_try_end();
            if crate::array::js_array_is_array(iter_h.get_nanbox_f64()).to_bits()
                == crate::value::TAG_TRUE
            {
                return js_iterator_to_array(crate::array::array_values_iter(
                    iter_h.get_nanbox_f64(),
                ));
            }
            if !object_like_iterator_result(iter_h.get_nanbox_f64()) {
                let msg = b"Result of the Symbol.iterator method is not an object";
                let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
                let err = crate::error::js_typeerror_new(msg_str);
                crate::exception::js_throw(js_nanbox_pointer(err as i64));
            }
            return js_iterator_to_array(iter_h.get_nanbox_f64());
        }
    }

    if crate::array::js_array_is_array(value()).to_bits() == crate::value::TAG_TRUE {
        return js_iterator_to_array(crate::array::array_values_iter(value()));
    }
    if has_named_next(value()) {
        return js_iterator_to_array(value());
    }
    throw_not_iterable(value());
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
    // Native handle ids (Web-Fetch Headers/Request/Response, streams, ws, DB,
    // …) are NaN-boxed POINTER values in the small-handle band (see
    // `value::addr_class`): registry indices, NOT heap pointers. Dereferencing
    // `raw_ptr - 8` as a GcHeader for one of them reads unmapped memory and
    // segfaults — e.g. `for (const [k, v] of response.headers)` (#4800), where
    // the lazy `for…of` protocol (#4786) routes the Headers handle
    // (id >= 0x40000) through `js_get_iterator`, which calls this check.
    // Reject the whole handle band, matching `Array.isArray` and
    // `try_dispatch_instance_method_value`. A real built-in iterator is always
    // a heap object well above this floor, so this never loses a true match.
    if crate::value::addr_class::is_handle_band(raw_ptr) {
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

/// #7562: the drain hit [`MAX_ITERATOR_DRAIN`] without the iterator finishing.
///
/// Throwing is the point. The previous behaviour was to fall out of the loop
/// and return whatever had accumulated, so `[...it]` handed back a short array
/// that looked entirely valid — a `RangeError` is recoverable and visible,
/// silent truncation is neither.
#[cold]
fn throw_iterator_too_long() -> ! {
    let msg = b"Iterator produced more than the maximum supported number of elements";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_rangeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cold]
fn throw_iterator_result_not_object() -> ! {
    iter_bt_dump(
        "array_throw_iterator_result_not_object",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
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
        iter_bt_dump("js_iterator_next_result", result);
        throw_iterator_result_not_object();
    }
    result
}

/// Convert any iterator-protocol object (has `.next()` method) to an array.
/// #7562: the drain loops below were bounded at a hardcoded 100,000 and
/// **silently returned a short array** when a longer iterator hit it —
/// `[...m.values()]` on a 250,000-entry Map produced 100,000 elements with no
/// error, no warning, and a plausible-looking result. Wrong data a caller
/// would ship, which is strictly worse than either hanging or throwing.
///
/// Node applies no such limit: `[...it]` runs until the iterator finishes or
/// the process runs out of memory. Matching that exactly would trade silent
/// truncation for an unbounded loop, so the bound stays — but it is raised to
/// JavaScript's own maximum array length and **throws** on exhaustion instead
/// of truncating. Every real workload is unaffected; a runaway iterator now
/// reports itself rather than corrupting a result.
const MAX_ITERATOR_DRAIN: usize = u32::MAX as usize - 1;

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
    use crate::object::{ObjectHeader, js_object_get_field_by_name};
    use crate::string::js_string_from_bytes;
    use crate::value::{TAG_UNDEFINED, js_nanbox_get_pointer};

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

    for drained in 0..MAX_ITERATOR_DRAIN {
        if drained == MAX_ITERATOR_DRAIN - 1 {
            // Reached the cap with the iterator still producing: refuse rather
            // than return a short array (#7562).
            throw_iterator_too_long();
        }
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

/// Resolve `Symbol.asyncIterator` and invoke it with the iterable as `this`.
pub(crate) fn call_symbol_async_iterator(value: f64) -> Option<f64> {
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

/// Used by spread on generators, Array.from on generators, etc.
/// Calls `.next()` in a loop until `.done` is true, collecting `.value` entries.
#[no_mangle]
pub extern "C" fn js_iterator_to_array(iter_f64: f64) -> *mut ArrayHeader {
    use crate::closure;
    use crate::object::{ObjectHeader, js_object_get_field_by_name};
    use crate::string::js_string_from_bytes;
    use crate::value::{TAG_UNDEFINED, js_nanbox_get_pointer};

    // #7475: EVERY value this loop carries across a `.next()` call is a
    // GC-managed object, and `.next()` allocates the `{ value, done }` result
    // — so any of the four can be moved by the copying minor that allocation
    // triggers. Before this scope they lived in bare Rust locals, which the
    // collector cannot see and therefore never rewrites:
    //
    //   * the iterator object itself. A moved iterator leaves the pre-move
    //     copy in retired from-space; the next `.next()` dispatch reads its
    //     STALE field 0 (an array iterator's backing array), and
    //     `dispatch_array_iterator_method` then calls `js_array_length` on a
    //     from-space address. That is the exact fault
    //     `PERRY_GC_PROTECT_FROMSPACE=1` reports for `[...arr]` at scale.
    //   * the accumulator array, re-read on every push.
    //   * the `next` closure (non-movable, but sweepable while unreferenced).
    //   * the two interned property keys.
    //
    // The per-iteration result object gets ONE reusable scratch slot rather
    // than a fresh handle each turn — the loop runs up to 100k times and a
    // push-per-iteration would grow the handle stack without bound.
    //
    // Every handle here is NaN-boxed rather than `root_raw_*_ptr`, so reading
    // one back is a `get_nanbox_f64` at the point of use and the module stays
    // out of `scripts/raw_handle_debt.py`'s ledger.
    let scope = crate::gc::RuntimeHandleScope::new();

    // The iterator is rooted FIRST, before anything in this function allocates:
    // `js_array_alloc` below can trigger a copying minor, and until the value is
    // in a scope slot the collector has nothing to rewrite. Rooting a
    // non-pointer (`undefined`/`null`) is harmless — the visitor ignores it —
    // so the null check reads back through the handle rather than gating it.
    let iter_h = scope.root_nanbox_f64(iter_f64);

    let arr = js_array_alloc(8); // start with capacity 8
    let result_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(arr as i64));
    let read_result = || js_nanbox_get_pointer(result_h.get_nanbox_f64()) as *mut ArrayHeader;

    // Get the iterator object pointer
    if js_nanbox_get_pointer(iter_h.get_nanbox_f64()) == 0 {
        return read_result();
    }

    // Look up the "next" method on the iterator object as a stored closure
    // FIELD (the common case for generator objects / effect's `SingleShotGen`,
    // which store `next` as an own callable property). Use the OWN-field getter:
    // built-in iterators (array/map/set/string) now inherit `.next` from their
    // shared `%...IteratorPrototype%` singleton, and that inherited thunk relies
    // on `this` being bound by the class-id method tower — so an INHERITED
    // `.next` must take the method-dispatch path below, not this raw
    // closure-call (which doesn't bind `this`).
    let next_val = crate::object::js_object_get_own_field_or_undef(
        iter_h.get_nanbox_f64(),
        b"next".as_ptr(),
        4,
    );
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
    // `next_f64` is already the NaN-boxed closure value (or `undefined`, which
    // the root scanner ignores), so it roots directly.
    let next_h = scope.root_nanbox_f64(next_f64);

    // Iterate: call next() until done
    let done_key_h =
        scope.root_nanbox_f64(nanbox_string_key(js_string_from_bytes(b"done".as_ptr(), 4)));
    let value_key_h = scope.root_nanbox_f64(nanbox_string_key(js_string_from_bytes(
        b"value".as_ptr(),
        5,
    )));
    // Reusable scratch slot for the `{ value, done }` object `.next()` returns.
    let step_h = scope.root_nanbox_f64(f64::from_bits(TAG_UNDEFINED));

    for drained in 0..MAX_ITERATOR_DRAIN {
        if drained == MAX_ITERATOR_DRAIN - 1 {
            // Reached the cap with the iterator still producing: refuse rather
            // than return a short array (#7562).
            throw_iterator_too_long();
        }
        // safety limit
        // Call next() — stored-closure fast path, or class-id method dispatch.
        // Both addresses are read fresh from their roots at the callsite: the
        // previous iteration's `.next()` may have moved either one.
        let result_f64 = if use_method_dispatch {
            unsafe {
                crate::object::js_native_call_method(
                    iter_h.get_nanbox_f64(),
                    b"next".as_ptr() as *const i8,
                    4,
                    std::ptr::null(),
                    0,
                )
            }
        } else {
            closure::js_closure_call1(
                js_nanbox_get_pointer(next_h.get_nanbox_f64()) as *const closure::ClosureHeader,
                f64::from_bits(TAG_UNDEFINED),
            )
        };
        // IteratorNext (ECMA-262 §7.4.2 step 3): if Type(result) is not
        // Object, throw a TypeError. `is_pointer()` is true only for
        // POINTER_TAG heap objects/arrays — strings, numbers, booleans,
        // null and undefined all fail it (and would otherwise be silently
        // treated as "done"). Symbols are pointer-tagged but are NOT objects,
        // so exclude registered symbols too.
        // language/statements/for-of/iterator-next-result-type.
        let result_is_object = crate::value::JSValue::from_bits(result_f64.to_bits()).is_pointer()
            && !crate::symbol::is_registered_symbol(js_nanbox_get_pointer(result_f64) as usize);
        if !result_is_object {
            throw_iterator_result_not_object();
        }
        // Root the result object before touching it: the two field reads below
        // can allocate (key interning / shape lookup), and the push certainly
        // can.
        step_h.set_nanbox_f64(result_f64);
        let result_obj = js_nanbox_get_pointer(result_f64) as *const ObjectHeader;

        // Check .done. `across_nanbox` runs the (allocating) read and hands
        // back the POST-collection address of the result object, so the pre-
        // call copy is never nameable afterwards.
        let (done_val, result_after) = step_h.across_nanbox(|| {
            js_object_get_field_by_name(
                result_obj,
                js_nanbox_get_pointer(done_key_h.get_nanbox_f64()) as *const crate::StringHeader,
            )
        });
        let result_obj = js_nanbox_get_pointer(result_after) as *const ObjectHeader;
        let done_bits = unsafe { std::mem::transmute::<_, u64>(done_val) };
        // done is true when it's TAG_TRUE (0x7FFC_0000_0000_0004) or truthy number
        if done_bits == 0x7FFC_0000_0000_0004 {
            break;
        } // TAG_TRUE

        // Get .value and push to array
        let val = js_object_get_field_by_name(
            result_obj,
            js_nanbox_get_pointer(value_key_h.get_nanbox_f64()) as *const crate::StringHeader,
        );
        let val_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(val)) };
        let pushed = js_array_push_f64(read_result(), val_f64);
        result_h.set_nanbox_f64(crate::value::js_nanbox_pointer(pushed as i64));
    }

    read_result()
}

/// `BindingRestElement` / `AssignmentRestElement` iterator drain for
/// destructuring (`let [...rest] = src`, `method([...rest]) {}`). Spec §8.5.3
/// ArrayBindingPattern step for a rest element: if the iterator is already
/// done, the rest is an empty array; otherwise drain the remaining values into
/// a fresh array (which leaves the iterator exhausted). `done_f64` carries the
/// destructuring `[[Done]]` flag so a rest after an exhausted iterator
/// (`let [a, b, ...r] = [1]`) yields `[]` without re-invoking `next()`.
#[no_mangle]
pub extern "C" fn js_iterator_rest_to_array(iter_f64: f64, done_f64: f64) -> f64 {
    if crate::value::js_is_truthy(done_f64) != 0 {
        let arr = js_array_alloc(0);
        return crate::value::js_nanbox_pointer(arr as i64);
    }
    let arr = js_iterator_to_array(iter_f64);
    crate::value::js_nanbox_pointer(arr as i64)
}

fn js_async_iterator_to_array(iter_f64: f64) -> *mut ArrayHeader {
    use crate::closure;
    use crate::object::{ObjectHeader, js_object_get_field_by_name};
    use crate::string::js_string_from_bytes;
    use crate::value::{TAG_TRUE, TAG_UNDEFINED, js_nanbox_get_pointer};

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

    for drained in 0..MAX_ITERATOR_DRAIN {
        if drained == MAX_ITERATOR_DRAIN - 1 {
            // Reached the cap with the iterator still producing: refuse rather
            // than return a short array (#7562).
            throw_iterator_too_long();
        }
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
