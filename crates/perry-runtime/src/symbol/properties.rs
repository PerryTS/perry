//! Symbol-keyed property side tables: data properties, descriptor attrs,
//! accessor definitions, deletion, has-own checks, computed function-name
//! inference, and the class-static symbol registry.

use super::*;
use crate::string::{js_string_from_bytes, StringHeader};
use std::collections::HashMap;

/// Look up the cached pointer for the registered `util.inspect.custom` symbol
/// (description `"nodejs.util.inspect.custom"`). Returns 0 if the symbol has
/// not been allocated yet — which means no user code has touched
/// `util.inspect.custom` so no object can possibly hold it as a key.
/// Used by the inspect formatter to detect the hook without iterating every
/// symbol entry. Refs #1201.
pub(crate) fn inspect_custom_symbol_ptr() -> usize {
    let guard = SYMBOL_REGISTRY.lock().unwrap();
    if let Some(map) = guard.as_ref() {
        if let Some(&ptr) = map.get("nodejs.util.inspect.custom") {
            return ptr;
        }
    }
    0
}

pub(crate) fn clone_symbol_entries_for_obj_ptr(src_obj_ptr: usize) -> Vec<(usize, u64)> {
    if src_obj_ptr == 0 {
        return Vec::new();
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard
        .as_ref()
        .and_then(|m| m.get(&src_obj_ptr))
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn symbol_property_root_bits(owner: usize, sym_key: usize) -> Option<u64> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard.as_ref().and_then(|map| {
        map.get(&owner)
            .and_then(|entries| entries.iter().find(|(key, _)| *key == sym_key))
            .map(|(_, value_bits)| *value_bits)
    })
}

pub(crate) fn get_symbol_property_attrs(
    owner: usize,
    sym_key: usize,
) -> Option<crate::object::PropertyAttrs> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
    guard
        .as_ref()
        .and_then(|map| map.get(&(owner, sym_key)).copied())
}

pub(crate) fn set_symbol_property_attrs(
    owner: usize,
    sym_key: usize,
    attrs: crate::object::PropertyAttrs,
) {
    if owner == 0 || sym_key == 0 {
        return;
    }
    super::note_symbol_key_installed(sym_key);
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert((owner, sym_key), attrs);
}

pub(crate) unsafe fn js_object_delete_symbol_property(obj_f64: f64, sym_f64: f64) -> i32 {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return 1;
    }
    if get_symbol_property_attrs(obj_key, sym_key).is_some_and(|attrs| !attrs.configurable()) {
        return 0;
    }
    // `delete Array.prototype[Symbol.iterator]` — the builtin iterator is
    // virtual (native dispatch, not in the side table), so the delete must
    // still flip the modified flag for `js_get_iterator` to throw per spec.
    crate::array::note_array_proto_iterator_write(obj_key, sym_key);

    accessors::clear_symbol_accessor_property(obj_key, sym_key);
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if let Some(map) = guard.as_mut() {
            let should_remove_owner = if let Some(entries) = map.get_mut(&obj_key) {
                entries.retain(|(key, _)| *key != sym_key);
                entries.is_empty()
            } else {
                false
            };
            if should_remove_owner {
                map.remove(&obj_key);
            }
        }
    }
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
        if let Some(map) = guard.as_mut() {
            map.remove(&(obj_key, sym_key));
        }
    }
    1
}

/// #6710: drop ALL symbol-keyed props (data + attrs + accessors) owned by
/// `obj_key`. Used when a handle id is recycled so the reused id starts with a
/// clean symbol side table — mirrors `handle_expando_clear` for the string
/// table. The three tables are keyed by `obj_key` (`SYMBOL_PROPERTIES`) or
/// `(obj_key, sym_key)` (attrs / accessors), so a single object drops in one
/// `remove` and two `retain`s.
pub(crate) fn clear_all_symbol_properties_for_object(obj_key: usize) {
    if obj_key == 0 {
        return;
    }
    accessors::clear_all_symbol_accessor_properties_for_object(obj_key);
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if let Some(map) = guard.as_mut() {
            map.remove(&obj_key);
        }
    }
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
        if let Some(map) = guard.as_mut() {
            map.retain(|(o, _), _| *o != obj_key);
        }
    }
}

/// #6710: clear every per-handle JS-property side table for a recycled handle
/// id (the string expando table AND the symbol tables). Called on the MAIN
/// (JS-owning) thread from perry-ext-http-server just before a recycled
/// `IncomingMessage`/`ServerResponse` id is handed to a new request's handler,
/// so no request inherits a prior request's `req.__rid` / `isRSCRequest` /
/// `NextInternalRequestMeta`. The `handle` id equals `obj_key_from_f64` of the
/// handle's NaN-boxed pointer, so one id keys all four tables.
#[no_mangle]
pub extern "C" fn js_handle_clear_side_tables(handle: i64) {
    if handle == 0 {
        return;
    }
    crate::object::handle_expando::handle_expando_clear(handle);
    clear_all_symbol_properties_for_object(handle as usize);
}

pub(crate) fn symbol_property_is_enumerable(owner: usize, sym_key: usize) -> bool {
    get_symbol_property_attrs(owner, sym_key)
        .map(|attrs| attrs.enumerable())
        .unwrap_or(true)
}

pub(crate) fn symbol_accessor_descriptor_bits(owner: usize, sym_key: usize) -> Option<(u64, u64)> {
    accessors::symbol_accessor_property_by_key(owner, sym_key).map(|acc| (acc.get, acc.set))
}

pub(crate) unsafe fn reflect_symbol_getter_closure_bits(obj_f64: f64, sym_f64: f64) -> Option<u64> {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return None;
    }
    let acc = accessors::symbol_accessor_property_by_key(obj_key, sym_key)?;
    if acc.get != 0 {
        Some(acc.get)
    } else {
        Some(0)
    }
}

pub(crate) unsafe fn js_object_has_own_symbol_property(obj_f64: f64, sym_f64: f64) -> bool {
    let bits = obj_f64.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        return class_static_symbol_lookup(class_id, sym_f64).is_some();
    }
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return false;
    }
    accessors::has_own_symbol_accessor(obj_key, sym_key)
        || object_symbol_data_property_exists(obj_key, sym_key)
}

/// Define (or merge) a symbol-keyed accessor on an object literal, delegating
/// to the shared symbol-accessor side table. Separate `get`/`set` definitions
/// for the same key accumulate, matching `Object.defineProperty` semantics.
pub(crate) unsafe fn js_object_define_symbol_accessor(
    obj_f64: f64,
    sym_f64: f64,
    getter: f64,
    setter: f64,
) -> f64 {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return obj_f64;
    }
    let existing = accessors::symbol_accessor_property(obj_f64, sym_f64);
    let undef = crate::value::TAG_UNDEFINED;
    let get_bits = if getter.to_bits() == undef {
        existing.map(|a| a.get).unwrap_or(0)
    } else {
        crate::closure::clone_closure_rebind_this(getter.to_bits(), obj_f64)
    };
    let set_bits = if setter.to_bits() == undef {
        existing.map(|a| a.set).unwrap_or(0)
    } else {
        crate::closure::clone_closure_rebind_this(setter.to_bits(), obj_f64)
    };
    accessors::set_symbol_accessor_property(obj_f64, sym_f64, get_bits, set_bits);
    obj_f64
}

/// Set a closure value's `.name` (if not already named) given its NaN-boxed
/// bits. Returns silently for non-closure values. Shared by the symbol-key and
/// string-key computed-name inference paths.
unsafe fn register_closure_name_if_absent(val_bits: u64, name: &str) {
    let val_tag = val_bits & 0xFFFF_0000_0000_0000;
    if val_tag != POINTER_TAG {
        return;
    }
    let val_addr = (val_bits & POINTER_MASK) as usize;
    // #6320: the old `<= 0x10000` floor sits an order of magnitude below
    // `HANDLE_BAND_MAX`, so a registry id NaN-boxed under POINTER_TAG passed it
    // and the `*(addr - 8)` GcHeader read faulted on unmapped low memory:
    // `{ [Symbol.toPrimitive]: new Proxy(fn, {}) }` routes the proxy VALUE
    // through here for `fn.name` inference (proxy id 0xF000D → read at 0xF0005).
    // `try_read_gc_header` owns the band + heap-range + slab checks.
    let Some(gc_header) = crate::value::addr_class::try_read_gc_header(val_addr) else {
        return;
    };
    if gc_header.obj_type != crate::gc::GC_TYPE_CLOSURE {
        return;
    }
    let val_ptr = val_addr as *const u8;
    let closure_ptr = val_ptr as *const crate::closure::ClosureHeader;
    let func_ptr = (*closure_ptr).func_ptr;
    if func_ptr.is_null() {
        return;
    }
    crate::builtins::register_function_name_if_absent(func_ptr as usize, name);
}

unsafe fn infer_symbol_function_name(sym_key: usize, val_bits: u64) {
    let sym_ptr = sym_key as *const SymbolHeader;
    // Spec: a symbol key with an *undefined* description names the function the
    // empty string `""`; a symbol with a (possibly empty) string description
    // names it `"[" + description + "]"`. Distinguish "no description" (→ `""`)
    // from `Symbol("")` (→ `"[]"`).
    let desc = registered_symbol_description(sym_ptr as usize)
        .map(|s| s.as_ref().to_string())
        .or_else(|| str_from_header((*sym_ptr).description));
    let inferred = match desc {
        Some(d) => format!("[{}]", d),
        None => String::new(),
    };
    register_closure_name_if_absent(val_bits, &inferred);
}

/// Resolve (and cache) the registered `Symbol.for("NextInternalRequestMeta")`
/// pointer used by Next.js to stash per-request metadata on the underlying
/// IncomingMessage handle. Returns the symbol's stable `sym_key` (its leaked
/// `SymbolHeader*`), or 0 if it can't be resolved. The undefined-write wipe
/// guard below is narrowed to THIS symbol so ordinary handle symbol writes —
/// including clearing a non-metadata symbol with `undefined` — behave normally.
fn next_request_meta_sym_key() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // 0 = "not resolved yet", usize::MAX would be a sentinel we never need
    // because the registered symbol pointer is always a real, non-zero address.
    static CACHED: AtomicUsize = AtomicUsize::new(0);
    let cached = CACHED.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    const KEY: &[u8] = b"NextInternalRequestMeta";
    let sym_key = unsafe {
        let kh = js_string_from_bytes(KEY.as_ptr(), KEY.len() as u32);
        let key_f64 = crate::value::js_nanbox_string(kh as i64);
        let sym = super::constructors::js_symbol_for(key_f64);
        sym_key_from_f64(sym)
    };
    if sym_key != 0 {
        CACHED.store(sym_key, Ordering::Relaxed);
    }
    sym_key
}

unsafe fn set_symbol_property(obj_f64: f64, sym_f64: f64, value_f64: f64) -> f64 {
    if let Some(acc) = accessors::symbol_accessor_property(obj_f64, sym_f64) {
        if acc.set != 0 {
            let closure =
                (acc.set & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
            if !closure.is_null() {
                crate::closure::js_closure_call1(closure, value_f64);
            }
        }
        return value_f64;
    }
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return value_f64;
    }
    super::note_symbol_key_installed(sym_key);
    // #5437 (Next.js): a native HANDLE (small-id NaN-boxed POINTER, e.g. the
    // node:http IncomingMessage) carries per-request metadata in the symbol
    // side table keyed by its handle id. Node shares one metadata object by
    // reference across every wrapper that re-`new`s around the same
    // IncomingMessage, so a wrapper write-back like
    // `this._req[NEXT_REQUEST_META] = this[NEXT_REQUEST_META]` is harmless when
    // `this[...]` is the shared object. In Perry a late-bundled wrapper can
    // reach that write-back with an *undefined* `this[...]`, which would CLOBBER
    // the handle's existing (non-undefined) metadata — wiping
    // `resolvedPathname` and tripping Next's `resolvedPathname must be set`
    // invariant. Treat an `undefined` write onto a handle-band receiver that
    // already holds a non-undefined entry as a no-op: it never *adds*
    // information, and the by-reference object the handle still points at is
    // exactly what Node would keep.
    //
    // Gated narrowly so it only protects the Next request-metadata flow:
    //   (1) the symbol is `Symbol.for("NextInternalRequestMeta")`,
    //   (2) the receiver is a handle-band value (id below HANDLE_BAND_MAX and
    //       not a real heap object),
    //   (3) the write value is `undefined`, and
    //   (4) a non-undefined entry already exists.
    // Any OTHER symbol on a handle — including legitimately clearing it by
    // writing `undefined` — falls through to the normal store path below.
    {
        let raw = (obj_f64.to_bits() & crate::value::POINTER_MASK) as usize;
        let meta_key = next_request_meta_sym_key();
        if meta_key != 0
            && sym_key == meta_key
            && (obj_f64.to_bits() >> 48) == 0x7FFD
            && crate::value::addr_class::is_small_handle(raw)
            && !crate::object::is_valid_obj_ptr(raw as *const u8)
            && value_f64.to_bits() == TAG_UNDEFINED
        {
            if let Some(existing) = symbol_property_root_bits(obj_key, sym_key) {
                if existing != TAG_UNDEFINED {
                    return value_f64;
                }
            }
        }
    }
    // `Array.prototype[Symbol.iterator] = fn` disables the array fast path in
    // `js_get_iterator` so destructuring / GetIterator see the patched method.
    crate::array::note_array_proto_iterator_write(obj_key, sym_key);
    let has_own_data = object_symbol_data_property_exists(obj_key, sym_key);
    // Frozen / sealed / non-extensible receivers reject symbol-keyed writes
    // like string-keyed ones: an existing prop is non-writable when frozen
    // (or its per-symbol attrs say so), a new prop is forbidden when
    // non-extensible. Only heap receivers carry the GC flag word.
    if (obj_f64.to_bits() >> 48) == 0x7FFD
        && obj_key >= 0x10000
        && crate::object::is_valid_obj_ptr(obj_key as *const u8)
    {
        let gc = (obj_key - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let flags = (*gc)._reserved;
        if has_own_data {
            if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
                return value_f64;
            }
            if let Some(attrs) = get_symbol_property_attrs(obj_key, sym_key) {
                if !attrs.writable() {
                    return value_f64;
                }
            }
        } else if flags & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            return value_f64;
        }
    }
    if !has_own_data {
        let bits = obj_f64.to_bits();
        if (bits >> 48) == 0x7FFE {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            if crate::object::class_symbol_setter_apply(class_id, sym_key, obj_f64, value_f64, true)
            {
                return value_f64;
            }
        } else {
            let jsval = crate::value::JSValue::from_bits(bits);
            if jsval.is_pointer() {
                let ptr = jsval.as_pointer::<crate::object::ObjectHeader>();
                if !ptr.is_null() && crate::object::is_valid_obj_ptr(ptr as *const u8) {
                    let class_id = crate::object::js_object_get_class_id(ptr);
                    if class_id != 0
                        && crate::object::class_symbol_setter_apply(
                            class_id, sym_key, obj_f64, value_f64, false,
                        )
                    {
                        return value_f64;
                    }
                }
            }
        }
    }
    accessors::clear_symbol_accessor_property(obj_key, sym_key);
    store_object_symbol_property_root(obj_key, sym_key, value_f64.to_bits());
    value_f64
}

fn object_symbol_data_property_exists(obj_key: usize, sym_key: usize) -> bool {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard.as_ref().is_some_and(|map| {
        map.get(&obj_key)
            .is_some_and(|entries| entries.iter().any(|&(sk, _)| sk == sym_key))
    })
}

/// True when an existing symbol-keyed own data property is non-writable — the
/// receiver was frozen (`Object.freeze`), or the per-symbol attrs recorded via
/// `Object.defineProperty(obj, sym, {writable:false})` say so. Mirrors the
/// frozen / non-writable rejection in `set_symbol_property`, but as a query so
/// the ordinary-`[[Set]]` walk (`own_set_descriptor`) can report the slot as
/// read-only and let a strict write throw. `obj_f64` / `sym_f64` are the same
/// NaN-boxed values passed to the symbol setters.
pub(crate) fn symbol_property_is_non_writable(obj_f64: f64, sym_f64: f64) -> bool {
    let obj_key = unsafe { obj_key_from_f64(obj_f64) };
    let sym_key = unsafe { sym_key_from_f64(sym_f64) };
    if obj_key == 0 || sym_key == 0 {
        return false;
    }
    // Only heap receivers carry the GC integrity flag word.
    if (obj_f64.to_bits() >> 48) == 0x7FFD
        && obj_key >= 0x10000
        && crate::object::is_valid_obj_ptr(obj_key as *const u8)
    {
        let gc = (obj_key - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let flags = unsafe { (*gc)._reserved };
        if flags & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return true;
        }
    }
    get_symbol_property_attrs(obj_key, sym_key).is_some_and(|attrs| !attrs.writable())
}

/// `obj[sym] = value` where `sym` is a Symbol. Stores into the side table.
/// Returns the value (NaN-boxed) for chained assignment semantics.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_symbol_property(
    obj_f64: f64,
    sym_f64: f64,
    value_f64: f64,
) -> f64 {
    // #6160: a runtime symbol assignment onto a class constructor
    // (`(C as any)[sym] = v`) targets an INT32-tagged class ref, which has no
    // heap address — `set_symbol_property` keys the own-symbol side table by
    // `obj_key_from_f64`, which returns 0 for a non-pointer receiver, so the
    // write was silently dropped and `sym in C` / `C[sym]` came back undefined.
    // Store it as a static Symbol-keyed member (CLASS_STATIC_SYMBOLS), the same
    // table `static [sym] = v` uses and that the class-ref arms of
    // `js_object_get_symbol_property` / `js_object_has_property` already read.
    if let Some(class_id) = crate::object::class_ref_id(obj_f64) {
        let sym_key = sym_key_from_f64(sym_f64);
        if sym_key != 0 {
            super::store_class_static_symbol_root(class_id, sym_key, value_f64.to_bits());
            return value_f64;
        }
    }
    set_symbol_property(obj_f64, sym_f64, value_f64)
}

/// Computed-key object literal function-name inference. Storage stays on the
/// normal IndexSet path, but object literals get Node's `[symbol.description]`
/// name for anonymous functions assigned under symbol keys.
#[no_mangle]
pub unsafe extern "C" fn js_object_literal_infer_computed_function_name(
    key_f64: f64,
    value_f64: f64,
) -> f64 {
    let sym_key = sym_key_from_f64(key_f64);
    if sym_key != 0 {
        infer_symbol_function_name(sym_key, value_f64.to_bits());
        return value_f64;
    }
    // A computed *string* (or stringified numeric) key names the function after
    // the key itself: `{ ["sk"]: function(){} }.sk.name === "sk"`,
    // `{ [1]: () => {} }[1].name === "1"`. The key arriving here has already
    // passed through ToPropertyKey, so a non-symbol key is a string value.
    let key_ptr = crate::value::js_get_string_pointer_unified(key_f64) as *const StringHeader;
    if let Some(name) = str_from_header(key_ptr) {
        register_closure_name_if_absent(value_f64.to_bits(), &name);
    }
    value_f64
}

unsafe fn js_object_set_symbol_property_infer_name(
    obj_f64: f64,
    sym_f64: f64,
    value_f64: f64,
) -> f64 {
    let stored = set_symbol_property(obj_f64, sym_f64, value_f64);
    js_object_literal_infer_computed_function_name(sym_f64, value_f64);
    stored
}

/// Register a static Symbol-keyed field on a class. Called once per
/// class + static computed-key field at module init.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_symbol(class_id: u32, sym: f64, value: f64) {
    let sym_key = sym_key_from_f64(sym);
    if class_id == 0 {
        return;
    }
    if sym_key == 0 {
        // Computed STATIC field whose key evaluated to a non-symbol —
        // ToPropertyKey makes it a string. A "prototype"-named static field
        // is a TypeError per ClassDefinitionEvaluation; anything else
        // becomes an ordinary own static data property (numeric keys, a
        // computed "constructor", drizzle-style `static [name] = v`).
        let key_str = crate::builtins::js_string_coerce(sym);
        if key_str.is_null() {
            return;
        }
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) else {
            return;
        };
        if name == "prototype" {
            let msg = "Classes may not have a static property named 'prototype'";
            let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err = crate::error::js_typeerror_new(s);
            crate::exception::js_throw(f64::from_bits(
                crate::value::JSValue::pointer(err as *const u8).bits(),
            ));
        }
        crate::object::class_dynamic_prop_root_store(class_id, name.to_string(), value);
        return;
    }
    store_class_static_symbol_root(class_id, sym_key, value.to_bits());
}

/// Look up a static Symbol-keyed property on a class by class_id.
/// Returns the stored value bits or `None` if no entry. Refs #420.
pub fn class_static_symbol_lookup(class_id: u32, sym_f64: f64) -> Option<u64> {
    unsafe {
        let sym_key = sym_key_from_f64(sym_f64);
        if class_id == 0 || sym_key == 0 {
            return None;
        }
        let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
        guard
            .as_ref()
            .and_then(|m| m.get(&(class_id, sym_key)).copied())
    }
}

pub(crate) fn class_static_symbol_keys_for_class(class_id: u32) -> Vec<usize> {
    let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    guard
        .as_ref()
        .map(|map| {
            map.keys()
                .filter_map(|&(cid, sym_key)| (cid == class_id).then_some(sym_key))
                .collect()
        })
        .unwrap_or_default()
}

/// `Object.prototype.hasOwnProperty.call(obj, sym)` for Symbol keys.
/// Refs #420 — drizzle's `is(value, type)` checks entityKind which is a Symbol.
///
/// When `obj` is an INT32-tagged class ref, also consult
/// `CLASS_STATIC_SYMBOLS` for static-Symbol-keyed declarations.
#[no_mangle]
pub unsafe extern "C" fn js_object_has_own_symbol(obj_f64: f64, sym_f64: f64) -> bool {
    let bits = obj_f64.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        return class_static_symbol_lookup(class_id, sym_f64).is_some();
    }
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return false;
    }
    if accessors::has_own_symbol_accessor(obj_key, sym_key) {
        return true;
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    if let Some(map) = guard.as_ref() {
        if let Some(entries) = map.get(&obj_key) {
            for &(sk, _) in entries.iter() {
                if sk == sym_key {
                    return true;
                }
            }
        }
    }
    false
}

/// Set a method on an object keyed by a symbol. Mirrors
/// `js_object_set_symbol_property` but ALSO binds the closure's reserved
/// `this` slot to `obj_f64` so `[Symbol.toPrimitive](hint) { return this.value }`
/// reads the container when called from `js_to_primitive` at runtime.
///
/// Layout assumption: the last capture slot is the reserved `this` slot
/// (matches `lower_object_literal`'s patching for static-key methods).
/// Only used by HIR for computed-key method props with `captures_this=true`.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_symbol_method(
    obj_f64: f64,
    sym_f64: f64,
    closure_f64: f64,
) -> f64 {
    bind_reserved_this_slot(closure_f64, obj_f64);
    js_object_set_symbol_property_infer_name(obj_f64, sym_f64, closure_f64)
}

/// Patch the closure's reserved (LAST) capture slot with `obj_f64` so a
/// `this`-reading object-literal method reads its container. No-op for any
/// value that is not a real heap `ClosureHeader` — shared by
/// [`js_object_set_symbol_method`] and [`js_object_set_method_by_name`].
///
/// #6320: both call sites hand-rolled the CLOSURE_MAGIC probe behind an
/// `0x1000` floor — an order of magnitude below `HANDLE_BAND_MAX`, so a
/// registry handle (revocable-proxy id, fetch/zlib stream, stdlib id) NaN-boxed
/// under POINTER_TAG passed the floor and the `*(addr + 12)` read faulted on
/// unmapped low memory. `closure::is_closure_ptr` owns the check: it rejects the
/// handle band, non-heap addresses and misalignment before any dereference. A
/// non-closure value (e.g. a Proxy of a function) has no capture array to patch,
/// so it is simply stored as-is; the call site then dispatches it through the
/// proxy `[[Call]]` path.
unsafe fn bind_reserved_this_slot(closure_f64: f64, obj_f64: f64) {
    let c_bits = closure_f64.to_bits();
    if c_bits & 0xFFFF_0000_0000_0000 != POINTER_TAG {
        return;
    }
    let c_addr = (c_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(c_addr) {
        return;
    }
    let c_ptr = c_addr as *mut crate::closure::ClosureHeader;
    let real_count = crate::closure::real_capture_count((*c_ptr).capture_count);
    if real_count >= 1 {
        let captures_ptr = (c_ptr as *mut u8)
            .add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *mut f64;
        *captures_ptr.add((real_count - 1) as usize) = obj_f64;
    }
}

/// #809: string-key analog of [`js_object_set_symbol_method`]. Sets
/// `obj[key] = closure` by NAME (not the symbol side-table) and ALSO binds
/// the closure's reserved `this` slot to `obj_f64` so a method written
/// AFTER a `...spread` in an object literal still reads the right receiver.
///
/// Used by the ordered-IIFE lowering of object literals that interleave a
/// spread with `this`-binding methods (Effect `HashRing.ts` `Proto`). The
/// non-spread fast path patches `this` post-build in codegen; this helper
/// is the runtime equivalent for the ordered path where the closure flows
/// in as a call argument.
///
/// Layout assumption (identical to `js_object_set_symbol_method`): the
/// LAST capture slot is the reserved `this` slot.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_method_by_name(
    obj_f64: f64,
    key_f64: f64,
    closure_f64: f64,
) -> f64 {
    // 1) Patch the closure's reserved (last) `this` capture slot with obj.
    //    No-op for anything that is not a real heap closure (#6320).
    bind_reserved_this_slot(closure_f64, obj_f64);

    // 2) Set the field by name. `js_object_set_field_by_name` strips the
    //    NaN-box tag off `obj` itself, so passing the raw bits is fine; the
    //    key must be a real `StringHeader*` (tag stripped).
    let key_bits = key_f64.to_bits();
    let key_ptr = (key_bits & POINTER_MASK) as *const StringHeader;
    let obj_ptr = obj_f64.to_bits() as *mut crate::object::ObjectHeader;
    if crate::value::addr_class::is_above_handle_band(key_ptr as usize) {
        crate::object::js_object_set_field_by_name(obj_ptr, key_ptr, closure_f64);
    }
    obj_f64
}
