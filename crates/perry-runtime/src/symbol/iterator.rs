//! Iterator-protocol entry points (`js_get_iterator`,
//! `js_iterator_result_validate`), `Object.getOwnPropertySymbols`, and
//! `ToPrimitive` (`[Symbol.toPrimitive]`) dispatch.

use super::*;
use crate::string::{js_string_from_bytes, StringHeader};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// `Object.getOwnPropertySymbols(obj)` — returns an array of symbol keys on
/// the object. Looks up the side table populated by
/// `js_object_set_symbol_property`.
///
/// Returns a raw `*mut ArrayHeader` as i64 (unboxed). Callers should NaN-box
/// with POINTER_TAG before handing the result to user code.
#[no_mangle]
pub unsafe extern "C" fn js_object_get_own_property_symbols(obj_f64: f64) -> i64 {
    // #2818: ToObject(null/undefined) throws TypeError, matching Node. Other
    // primitives box successfully and enumerate no own symbols (empty array).
    let jv = crate::JSValue::from_bits(obj_f64.to_bits());
    if jv.is_null() || jv.is_undefined() {
        crate::object::has_own_helpers::throw_to_object_nullish_type_error();
    }
    // A Proxy is a small registered id — route through the `ownKeys` trap
    // (symbol subset) before the heap-object paths below.
    if crate::proxy::js_proxy_is_proxy(obj_f64) != 0 {
        let arr = crate::proxy::proxy_own_property_symbols(obj_f64);
        return (arr.to_bits() & POINTER_MASK) as i64;
    }
    if let Some(class_id) = crate::object::class_ref_id(obj_f64) {
        let mut entries = if crate::object::class_prototype_ref_id(obj_f64).is_some() {
            crate::object::class_own_symbol_member_keys(class_id, false)
        } else {
            let mut keys = crate::object::class_own_symbol_member_keys(class_id, true);
            for sym_key in class_static_symbol_keys_for_class(class_id) {
                if !keys.contains(&sym_key) {
                    keys.push(sym_key);
                }
            }
            keys.sort_by_key(|sym_key| {
                let ptr = *sym_key as *const SymbolHeader;
                if ptr.is_null() {
                    u64::MAX
                } else {
                    (*ptr).id
                }
            });
            keys
        };
        let mut arr = crate::array::js_array_alloc(entries.len() as u32);
        for sym_ptr_usize in entries.drain(..) {
            let boxed = f64::from_bits(POINTER_TAG | (sym_ptr_usize as u64 & POINTER_MASK));
            arr = crate::array::js_array_push_f64(arr, boxed);
        }
        return arr as i64;
    }
    let obj_key = obj_key_from_f64(obj_f64);
    if obj_key == 0 {
        return crate::array::js_array_alloc(0) as i64;
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    let mut entries = guard
        .as_ref()
        .and_then(|m| m.get(&obj_key))
        .cloned()
        .unwrap_or_default();
    drop(guard);
    // `entries[..data_len]` are the data-valued symbol properties from
    // `SYMBOL_PROPERTIES`, already in their true insertion order. Everything
    // appended after `data_len` is an accessor-only symbol.
    let data_len = entries.len();
    for sym_key in accessors::owner_symbol_accessor_keys(obj_key) {
        if !entries.iter().any(|(existing, _)| *existing == sym_key) {
            entries.push((sym_key, 0));
        }
    }
    if entries.is_empty() {
        return crate::array::js_array_alloc(0) as i64;
    }
    // `[[OwnPropertyKeys]]` reports symbol keys in property-creation order.
    // Data-valued symbols already arrive in insertion order, so we must NOT
    // reorder them (an unconditional sort by creation id would reorder e.g.
    // `obj[b]=…; obj[a]=…` when `a` was created before `b`). Accessor-only
    // symbols, however, are appended from a HashMap (`owner_symbol_accessor_keys`)
    // in nondeterministic order, so a `defineProperty(o, sym, {get})` pair came
    // out unstable (test262 assign/strings-and-symbol-order,
    // getOwnPropertyDescriptors/order-after-define-property). Sort ONLY that
    // appended accessor-only tail by the symbol's monotonic creation id (the
    // convention the class-ref symbol path already uses), leaving the data-symbol
    // insertion order intact.
    entries[data_len..].sort_by_key(|(sym_ptr_usize, _)| {
        let ptr = *sym_ptr_usize as *const SymbolHeader;
        if ptr.is_null() {
            u64::MAX
        } else {
            (*ptr).id
        }
    });
    let mut arr = crate::array::js_array_alloc(entries.len() as u32);
    for (sym_ptr_usize, _val_bits) in entries.iter() {
        // Re-NaN-box each symbol pointer with POINTER_TAG so the array
        // contains JSValues that round-trip to user code as Symbols.
        let boxed = f64::from_bits(POINTER_TAG | (*sym_ptr_usize as u64 & POINTER_MASK));
        arr = crate::array::js_array_push_f64(arr, boxed);
    }
    arr as i64
}

fn is_object_value(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    raw >= 0x10000 && !is_registered_symbol(raw)
}

#[cold]
fn throw_iterator_result_not_object() -> ! {
    let msg = b"Result of the Symbol.iterator method is not an object";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

fn throw_value_not_iterable() -> ! {
    let msg = b"is not iterable";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// Spec IteratorNext / IteratorClose step "If innerResult is not an Object,
/// throw a TypeError". The for-of lazy-loop desugar wraps each `__iter.next()`
/// / guarded `__iter.return()` call in this validator. Returns the result
/// unchanged when it is an object.
// #1561-style force-keep: only generated IR calls this.
#[used]
static KEEP_JS_ITERATOR_RESULT_VALIDATE: extern "C" fn(f64) -> f64 = js_iterator_result_validate;

#[no_mangle]
pub extern "C" fn js_iterator_result_validate(result: f64) -> f64 {
    if !is_object_value(result) {
        crate::array::iter_bt_dump("js_iterator_result_validate", result);
        let msg = b"Iterator result is not an object";
        let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(msg_str);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    result
}

/// #1831: resolve the iterator for a `yield*` operand.
///
/// `yield* X` must drive `X[Symbol.iterator]()` — for a generator **call** the
/// result already *is* its iterator (perry's generator object is
/// `{next,return,throw}` with no `Symbol.iterator`), but for an arbitrary
/// iterable (effect's `EffectPrimitive`, custom `[Symbol.iterator]` objects)
/// the iterator must first be obtained by invoking the well-known-symbol
/// method. This helper returns that iterator, or `val` unchanged when `val` is
/// already an iterator / not iterable.
///
/// Arrays now route through `array_values_iter` — the runtime has a real
/// `.next`-bearing iterator (`ARRAY_ITERATOR_CLASS_ID`) since #321's
/// `arr.values()` dispatch landed, so `yield* [..]` and any other consumer
/// that drives `js_get_iterator(...).next()` works on a plain array. The
/// for-of and spread fast paths still special-case arrays earlier (in the
/// array-memcpy / index-loop arms) so they don't reach this helper.
#[no_mangle]
pub extern "C" fn js_get_iterator(val_f64: f64) -> f64 {
    if crate::array::js_array_is_array(val_f64).to_bits() == crate::value::TAG_TRUE {
        if !crate::array::array_proto_iterator_modified() {
            return crate::array::array_values_iter(val_f64);
        }
        // `Array.prototype[Symbol.iterator]` was replaced or deleted. Per
        // GetIterator, read the (patched) method off the prototype and call it
        // with `this === val`; a deleted/non-callable method is a TypeError.
        // The generic symbol lookup below reads OWN symbol props only, so the
        // prototype is consulted explicitly here.
        let proto_addr = crate::array::array_prototype_addr();
        if proto_addr != 0 {
            let iter_wk = well_known_symbol("iterator");
            if !iter_wk.is_null() {
                let proto_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(proto_addr as *const u8).bits());
                let sym_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
                let iter_fn = unsafe { own_symbol_property(proto_f64, sym_f64) }
                    .unwrap_or(f64::from_bits(TAG_UNDEFINED));
                let fn_ptr = crate::value::js_nanbox_get_pointer(iter_fn)
                    as *const crate::closure::ClosureHeader;
                if iter_fn.to_bits() == TAG_UNDEFINED || fn_ptr.is_null() {
                    throw_value_not_iterable();
                }
                let prev_this = crate::object::js_implicit_this_set(val_f64);
                let rebound = crate::closure::clone_closure_rebind_this(iter_fn.to_bits(), val_f64);
                let rebound_ptr = crate::value::js_nanbox_get_pointer(f64::from_bits(rebound))
                    as *const crate::closure::ClosureHeader;
                let iter = crate::closure::js_closure_call0(rebound_ptr);
                crate::object::js_implicit_this_set(prev_this);
                if !is_object_value(iter) {
                    throw_iterator_result_not_object();
                }
                return iter;
            }
        }
        return crate::array::array_values_iter(val_f64);
    }
    // Arguments objects iterate like arrays (spec:
    // `arguments[Symbol.iterator] === Array.prototype.values`). They are plain
    // objects with no @@iterator slot, so route them through the array iterator
    // so `for…of`, destructuring, and Array.from drive `.next()` correctly.
    {
        let jsv = crate::value::JSValue::from_bits(val_f64.to_bits());
        if jsv.is_pointer() {
            let ptr = jsv.as_pointer::<crate::object::ObjectHeader>();
            if crate::object::is_arguments_object(ptr) {
                if let Some(arr) = unsafe { crate::object::arguments_object_to_array(ptr) } {
                    let arr_f64 =
                        f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits());
                    return crate::array::array_values_iter(arr_f64);
                }
            }
        }
    }
    // A built-in iterator object (array/map/set/string/buffer/iterator-helper)
    // IS already an iterator and returns itself from `[Symbol.iterator]`. It now
    // INHERITS `[Symbol.iterator]` from the shared `%IteratorPrototype%`, but
    // that inherited thunk relies on the caller binding `this`; reading + calling
    // it here would not, yielding a bad result. Return the iterator unchanged.
    {
        let jsv = crate::value::JSValue::from_bits(val_f64.to_bits());
        if jsv.is_pointer() {
            let raw = jsv.as_pointer::<u8>() as usize;
            if crate::array::is_builtin_iterator_class_id(raw) {
                return val_f64;
            }
        }
    }
    // `class X extends Map | Set` instance — its default `[Symbol.iterator]`
    // yields the hidden backing collection's entries (Map) / values (Set),
    // returned as a real iterator object so the lazy `for…of` protocol can
    // drive `.next()`. Matches the builtins' default iterator. Skipped when the
    // subclass overrides `[Symbol.iterator]`, so we fall through to the generic
    // symbol lookup below (which resolves the user's `@@iterator` method).
    match crate::object::map_set_subclass::subclass_backing_for_default_iteration(val_f64) {
        Some(crate::object::map_set_subclass::CollectionBacking::Map(m)) => {
            return crate::value::js_nanbox_pointer(
                crate::collection_iter_object::js_map_entries_iter_obj(m),
            );
        }
        Some(crate::object::map_set_subclass::CollectionBacking::Set(s)) => {
            return crate::value::js_nanbox_pointer(
                crate::collection_iter_object::js_set_values_iter_obj(s),
            );
        }
        None => {}
    }
    // A primitive number / boolean / null / undefined is not iterable. Per
    // GetIterator this is a TypeError; bail before the `[Symbol.iterator]`
    // lookup, which would otherwise dereference a raw (non-NaN-boxed) double as
    // an object pointer and crash (`for (x of 37) {}`). Strings ARE iterable, so
    // they fall through to the symbol lookup below.
    {
        let jsv = crate::value::JSValue::from_bits(val_f64.to_bits());
        if !jsv.is_pointer() && !jsv.is_any_string() {
            throw_value_not_iterable();
        }
    }
    // A string PRIMITIVE (heap STRING_TAG or inline SSO short string) iterates
    // over its Unicode code points per `String.prototype[Symbol.iterator]`
    // (ECMA-262 §22.1.3.36). The generic `[Symbol.iterator]` lookup below only
    // resolves the method off an OBJECT — for a string primitive
    // `js_object_get_symbol_property` finds nothing, so `js_get_iterator` used
    // to return the string UNCHANGED, and the lazy `for…of` loop then called
    // `.next()` on the string itself → `(string).next is not a function`
    // (#4892). This only bit the dynamic path (`for (c of v)` where `v: any`,
    // or a segmenter-/destructure-derived value); statically-typed string
    // for-of never routes through here. Build the real String iterator object
    // directly, mirroring the array short-circuit at the top.
    {
        let jsv = crate::value::JSValue::from_bits(val_f64.to_bits());
        if jsv.is_any_string() {
            let sptr =
                crate::value::js_get_string_pointer_unified(val_f64) as *const crate::StringHeader;
            return crate::string::string_values_iter(sptr);
        }
    }
    let iter_wk = well_known_symbol("iterator");
    if !iter_wk.is_null() {
        let sym_f64 = f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
        let iter_fn = unsafe { js_object_get_symbol_property(val_f64, sym_f64) };
        if iter_fn.to_bits() != TAG_UNDEFINED {
            // #321: the `[Symbol.iterator]` method may be INHERITED from a
            // prototype object literal (effect's `EffectPrototype`), in which
            // case codegen baked `this` to the prototype object at definition
            // time (CAPTURES_THIS_FLAG). Per spec `iterable[Symbol.iterator]()`
            // must run with `this === iterable`, so the method reads the real
            // receiver — effect's body is `new SingleShotGen(new YieldWrap(this))`
            // and wraps the wrong value if `this` stays the prototype. Rebind
            // `this` to the original value; a no-op for closures that don't
            // capture `this`.
            let rebound = crate::closure::clone_closure_rebind_this(iter_fn.to_bits(), val_f64);
            let call_target = f64::from_bits(rebound);
            let fn_ptr = crate::value::js_nanbox_get_pointer(call_target)
                as *const crate::closure::ClosureHeader;
            if !fn_ptr.is_null() {
                // Spec `GetIterator(obj)` → `Call(method, obj)`: the
                // `[Symbol.iterator]()` factory runs with `this === obj`. The
                // `clone_closure_rebind_this` above covers a closure that
                // *captures* `this` (effect's prototype method); a plain
                // `function(){ …this… }` factory reads `this` dynamically off
                // IMPLICIT_THIS, so set it here too (test262 yield-star-sync-*
                // asserts the `[Symbol.iterator]` call's thisValue === obj).
                let prev_this = crate::object::js_implicit_this_set(val_f64);
                let iter = crate::closure::js_closure_call0(fn_ptr);
                crate::object::js_implicit_this_set(prev_this);
                // Several Perry host-backed collections expose iterator
                // helpers as eager arrays for direct `.entries()` parity. When
                // the same function is reached through `Symbol.iterator`, wrap
                // that array in the runtime array iterator so generic protocol
                // consumers can drive `.next()`.
                if crate::array::js_array_is_array(iter).to_bits() == crate::value::TAG_TRUE {
                    return crate::array::array_values_iter(iter);
                }
                if !is_object_value(iter) {
                    throw_iterator_result_not_object();
                }
                return iter;
            }
        }
    }
    // We reach here only when NO `[Symbol.iterator]` method resolved. A
    // pointer-tagged value whose payload lies in the small-handle band
    // (`< HANDLE_BAND_MAX`, e.g. a near-null `POINTER_TAG | 1`) is NOT a
    // dereferenceable heap object, and with no iterator method it cannot be
    // iterable. Returning it `val_f64` below would manufacture the bogus value
    // as its own "iterator"; the lazy for-of then calls `.next()` on it, gets
    // `undefined`, and throws a misleading late "Iterator result is not an
    // object" far from the real fault. Throw the correct "not iterable" here
    // instead. Genuinely-iterable handle-backed values (fetch `Headers`,
    // proxies, …) resolve their `@@iterator` via the small-handle dispatch in
    // `js_object_get_symbol_property` above and already returned — only a
    // corrupt/non-iterable handle reaches this point.
    {
        let jsv = crate::value::JSValue::from_bits(val_f64.to_bits());
        if jsv.is_pointer()
            && crate::value::addr_class::is_handle_band(jsv.as_pointer::<u8>() as usize)
        {
            throw_value_not_iterable();
        }
    }
    val_f64
}

/// `ToPrimitive(value, hint)` — if `value` is an object with a
/// `[Symbol.toPrimitive]` method registered in the symbol side-table, call
/// it with the appropriate hint string ("number" / "string" / "default")
/// and return the primitive result. Otherwise returns `value` unchanged.
///
/// `hint`: 0 = default, 1 = number, 2 = string.
///
/// Used by `js_number_coerce` (unary `+`, binary `+` numeric coercion),
/// `js_jsvalue_to_string` (template literals, String(x)), and the
/// lower_string_coerce_concat path.
#[no_mangle]
pub unsafe extern "C" fn js_to_primitive(value: f64, hint: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let value = value_handle.get_nanbox_f64();
    let bits = value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag != POINTER_TAG {
        return value;
    }
    let obj_ptr = (bits & POINTER_MASK) as usize;
    if obj_ptr < 0x1000 {
        return value;
    }
    // Skip symbols / buffers / arrays — they have their own coercion rules.
    if is_registered_symbol(obj_ptr) {
        return value;
    }
    // A `Temporal.*` value is a cell, NOT an `ObjectHeader`: looking up
    // `[Symbol.toPrimitive]` below would deref the boxed payload as an object
    // and segfault. Temporal's own `[Symbol.toPrimitive]` throws a TypeError for
    // the `"number"` hint and returns the canonical ISO string for
    // `"string"`/`"default"` — which is exactly what `"x" + plainDateTime` and
    // template interpolation need. (Direct `String(x)` already brand-checks; the
    // `+`/template coercion routed here did not.)
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(value) {
        if hint == 1 {
            crate::object::throw_object_type_error(b"Cannot convert a Temporal value to a number");
        }
        if let Some(s) = crate::temporal::temporal_iso_string(value) {
            let p = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            return crate::value::js_nanbox_string(p as i64);
        }
    }
    // Look up obj[Symbol.toPrimitive].
    let wk_ptr = well_known_symbol("toPrimitive");
    let sym_f64 = f64::from_bits(POINTER_TAG | (wk_ptr as u64 & POINTER_MASK));
    let current_value = value_handle.get_nanbox_f64();
    let method = js_object_get_symbol_property(current_value, sym_f64);
    if method.to_bits() == TAG_UNDEFINED {
        return current_value;
    }
    // Method must be a closure pointer.
    let method_bits = method.to_bits();
    let method_tag = method_bits & 0xFFFF_0000_0000_0000;
    if method_tag != POINTER_TAG {
        return value_handle.get_nanbox_f64();
    }
    let method_handle = scope.root_nanbox_f64(method);
    let closure_ptr = (method_bits & POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure_ptr.is_null() || (closure_ptr as usize) < 0x1000 {
        return value_handle.get_nanbox_f64();
    }
    // Validate CLOSURE_MAGIC before calling.
    let type_tag = std::ptr::read_volatile((closure_ptr as *const u8).add(12) as *const u32);
    if type_tag != crate::closure::CLOSURE_MAGIC {
        return value_handle.get_nanbox_f64();
    }
    let hint_str: &[u8] = match hint {
        1 => b"number",
        2 => b"string",
        _ => b"default",
    };
    let hint_ptr = js_string_from_bytes(hint_str.as_ptr(), hint_str.len() as u32);
    let hint_handle = scope.root_string_ptr(hint_ptr);
    let hint_f64 = f64::from_bits(
        STRING_TAG | (hint_handle.get_raw_const_ptr::<StringHeader>() as u64 & POINTER_MASK),
    );
    let method_bits = method_handle.get_nanbox_f64().to_bits();
    let closure_ptr = (method_bits & POINTER_MASK) as *const crate::closure::ClosureHeader;

    // Spec says the return value must be a primitive; if it's still an
    // object pointer, that's a TypeError in JS, but we just return it
    // as-is and let the caller fall back.
    crate::closure::js_closure_call1(closure_ptr, hint_f64)
}
