//! Collection prototype-method thunks (#3662).
//!
//! `Set.prototype.add` & friends are reachable as plain values (e.g.
//! `Set.prototype.add.call(x, v)`, `Reflect.apply`, method extraction). The
//! fast path `s.add(v)` is lowered directly to `js_set_add` by codegen and
//! never touches these thunks, so they only fire on the reflective path —
//! which previously resolved to `global_this_builtin_noop_thunk` and silently
//! did nothing. Per spec these methods must perform a `this` brand check and
//! throw a `TypeError` when called on an incompatible receiver. The thunks
//! below read the `IMPLICIT_THIS` receiver (set by the `.call`/`.apply`
//! dispatch), brand-check it, throw on mismatch, and otherwise dispatch to the
//! real runtime helper — so reflective collection calls now also *work*.
//!
//! Installed onto each collection's `.prototype` by
//! `global_this::populate_builtin_prototype_methods`.

use super::*;

thread_local! {
    static BUILTIN_MAP_SET_VALUE_BITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static BUILTIN_SET_ADD_VALUE_BITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(crate) fn is_builtin_map_set_value(value: f64) -> bool {
    is_remembered_builtin_collection_method(value, &BUILTIN_MAP_SET_VALUE_BITS)
}

pub(crate) fn is_builtin_set_add_value(value: f64) -> bool {
    is_remembered_builtin_collection_method(value, &BUILTIN_SET_ADD_VALUE_BITS)
}

fn is_remembered_builtin_collection_method(
    value: f64,
    cell: &'static std::thread::LocalKey<std::cell::Cell<u64>>,
) -> bool {
    let ptr = normalized_collection_method_ptr(value);
    ptr != 0 && cell.with(|remembered| remembered.get() == ptr)
}

fn remember_builtin_collection_method(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
    value: f64,
    cell: &'static std::thread::LocalKey<std::cell::Cell<u64>>,
) {
    let value = installed_collection_method_value(proto_obj, method_name).unwrap_or(value);
    let ptr = normalized_collection_method_ptr(value);
    cell.with(|remembered| remembered.set(ptr));
}

fn normalized_collection_method_ptr(value: f64) -> u64 {
    (crate::value::js_nanbox_get_pointer(value) as u64) & !0x7
}

fn installed_collection_method_value(
    proto_obj: *mut ObjectHeader,
    method_name: &str,
) -> Option<f64> {
    if proto_obj.is_null() {
        return None;
    }
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    unsafe {
        super::own_data_field_by_name(proto_obj, key).map(|value| f64::from_bits(value.bits()))
    }
}

/// Resolve a collection prototype method to the SAME brand-checking thunk value
/// installed on `<Builtin>.prototype`. Used so reading a method off an *instance*
/// (`const m = s.forEach`) yields the identical function object as
/// `Set.prototype.forEach` — which (a) makes `m === Set.prototype.forEach` hold
/// and (b) routes `m.call(badReceiver)` through the brand check that throws a
/// `TypeError` on an incompatible `this`. Previously the instance read returned a
/// `js_class_method_bind` closure pre-bound to the instance, so `.call(0)` kept
/// the bound Set and silently skipped the check (test262
/// `Set/prototype/forEach/this-not-object-throw-*`, and the Map/Set families).
///
/// `builtin_name` is one of `Map`/`Set`/`WeakMap`/`WeakSet`. Returns `None` for
/// unknown names/methods so callers fall back to their existing path.
pub(crate) fn collection_proto_method_value(builtin_name: &str, method_name: &str) -> Option<f64> {
    let proto = super::global_this::builtin_prototype_value(builtin_name);
    if proto.to_bits() == crate::value::TAG_UNDEFINED {
        return None;
    }
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *mut ObjectHeader;
    installed_collection_method_value(proto_ptr, method_name)
        .filter(|v| v.to_bits() != crate::value::TAG_UNDEFINED)
}

/// Install the brand-checking `.prototype` methods for the collection named
/// `builtin_name` (`Map`/`Set`/`WeakMap`/`WeakSet`). Returns `true` when
/// `builtin_name` is one of those collections — the caller then adds the
/// shared `OBJECT_PROTO_METHODS` — and `false` otherwise. Lives here rather
/// than inline in `global_this::populate_builtin_prototype_methods` to keep
/// that file under the 2000-line size gate. #3662.
pub(super) fn install_collection_proto_methods(
    builtin_name: &str,
    proto_obj: *mut ObjectHeader,
) -> bool {
    use super::global_this::install_proto_method as ipm;
    match builtin_name {
        "Map" => {
            // #4099: install the `size` *accessor* BEFORE the data methods.
            // Installing an accessor descriptor onto a prototype that already
            // holds data properties corrupts one of those data slots (the
            // accessor/data-field bookkeeping desyncs), which left
            // `Map.prototype.set` reading back as a garbage number — a later
            // `Map.prototype.set.call(...)` then dereferenced it and crashed
            // (SIGBUS). Installing the lone accessor first sidesteps the desync.
            install_collection_size_getter(
                proto_obj,
                "size",
                map_proto_size_getter_thunk as *const u8,
            );
            ipm(proto_obj, "clear", map_proto_clear_thunk as *const u8, 0);
            ipm(proto_obj, "delete", map_proto_delete_thunk as *const u8, 1);
            let entries_value = ipm(
                proto_obj,
                "entries",
                map_proto_entries_thunk as *const u8,
                0,
            );
            ipm(
                proto_obj,
                "forEach",
                map_proto_foreach_thunk as *const u8,
                1,
            );
            ipm(proto_obj, "get", map_proto_get_thunk as *const u8, 1);
            ipm(proto_obj, "has", map_proto_has_thunk as *const u8, 1);
            ipm(proto_obj, "keys", map_proto_keys_thunk as *const u8, 0);
            let set_value = ipm(proto_obj, "set", map_proto_set_thunk as *const u8, 2);
            ipm(proto_obj, "values", map_proto_values_thunk as *const u8, 0);
            install_collection_iterator_symbol(proto_obj, entries_value);
            remember_builtin_collection_method(
                proto_obj,
                "set",
                set_value,
                &BUILTIN_MAP_SET_VALUE_BITS,
            );
        }
        "Set" => {
            // #4099: install the `size` accessor first (see the Map arm).
            install_collection_size_getter(
                proto_obj,
                "size",
                set_proto_size_getter_thunk as *const u8,
            );
            let add_value = ipm(proto_obj, "add", set_proto_add_thunk as *const u8, 1);
            ipm(proto_obj, "clear", set_proto_clear_thunk as *const u8, 0);
            ipm(proto_obj, "delete", set_proto_delete_thunk as *const u8, 1);
            ipm(
                proto_obj,
                "entries",
                set_proto_entries_thunk as *const u8,
                0,
            );
            ipm(
                proto_obj,
                "forEach",
                set_proto_foreach_thunk as *const u8,
                1,
            );
            ipm(proto_obj, "has", set_proto_has_thunk as *const u8, 1);
            ipm(proto_obj, "keys", set_proto_keys_thunk as *const u8, 0);
            let values_value = ipm(proto_obj, "values", set_proto_values_thunk as *const u8, 0);
            install_collection_iterator_symbol(proto_obj, values_value);
            remember_builtin_collection_method(
                proto_obj,
                "add",
                add_value,
                &BUILTIN_SET_ADD_VALUE_BITS,
            );
        }
        "WeakMap" => {
            ipm(
                proto_obj,
                "delete",
                weakmap_proto_delete_thunk as *const u8,
                1,
            );
            ipm(proto_obj, "get", weakmap_proto_get_thunk as *const u8, 1);
            ipm(proto_obj, "has", weakmap_proto_has_thunk as *const u8, 1);
            ipm(proto_obj, "set", weakmap_proto_set_thunk as *const u8, 2);
        }
        "WeakSet" => {
            ipm(proto_obj, "add", weakset_proto_add_thunk as *const u8, 1);
            ipm(
                proto_obj,
                "delete",
                weakset_proto_delete_thunk as *const u8,
                1,
            );
            ipm(proto_obj, "has", weakset_proto_has_thunk as *const u8, 1);
        }
        _ => return false,
    }
    true
}

fn install_collection_iterator_symbol(proto_obj: *mut ObjectHeader, method_value: f64) {
    if proto_obj.is_null() || method_value.to_bits() == crate::value::TAG_UNDEFINED {
        return;
    }
    let iter = crate::symbol::well_known_symbol("iterator");
    if iter.is_null() {
        return;
    }
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(proto_obj as i64),
            f64::from_bits(crate::value::JSValue::pointer(iter as *const u8).bits()),
            method_value,
        );
    }
    crate::symbol::set_symbol_property_attrs(
        proto_obj as usize,
        iter as usize,
        super::PropertyAttrs::new(true, false, true),
    );
}

fn install_collection_size_getter(proto_obj: *mut ObjectHeader, name: &str, func_ptr: *const u8) {
    if proto_obj.is_null() {
        return;
    }
    unsafe {
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return;
        }
        super::native_module::set_bound_native_closure_name(closure, "get size");
        super::native_module::set_builtin_closure_length(closure as usize, 0);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        super::object_ops::ensure_key_in_keys_array(proto_obj, key);
        // #6809: gate-neutral BUILTIN install. `.size` reads on live Map/Set
        // receivers are gc-type-routed inside the generic getter (they never
        // dispatch through the gated accessor tables), so this entry exists
        // for reflection only — and this installer runs inside
        // `populate_global_this_builtins`, where the user-install variant
        // flipped the process-wide descriptor gates at startup for every
        // program touching a builtin global.
        super::set_builtin_accessor_descriptor(
            proto_obj as usize,
            name.to_string(),
            super::AccessorDescriptor {
                get: crate::value::js_nanbox_pointer(closure as i64).to_bits(),
                set: 0,
            },
            super::PropertyAttrs::new(true, false, true),
        );
        super::set_builtin_property_attrs(
            closure as usize,
            "name".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
        super::set_builtin_property_attrs(
            closure as usize,
            "length".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
    }
}

/// Owned UTF-8 copy of a heap `StringHeader`'s bytes.
fn string_header_owned(sp: *const crate::string::StringHeader) -> String {
    if sp.is_null() {
        return String::new();
    }
    unsafe {
        let bytes = (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let len = (*sp).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(bytes, len))
            .unwrap_or("")
            .to_string()
    }
}

/// Render the brand-check receiver the way V8's `NoSideEffectsToString` does —
/// node APPENDS it to the incompatible-receiver TypeError (`Method
/// Set.prototype.add called on incompatible receiver #<Object>` /
/// `... receiver undefined` / `... receiver [object Array]`), and #6658's
/// parity fixtures assert the full message. V8 never CALLS a potentially
/// side-effecting user `toString` here: primitives print their ToString
/// (`undefined`, `null`, `5.5`, the raw string chars, `Symbol(desc)`, bigint
/// digits); objects whose `toString` is the default `Object.prototype.toString`
/// print `#<CtorName>` (`#<Object>`, `#<Set>`, `#<Map>`, `#<Promise>`,
/// `#<Foo>`); objects that override it (Array, Date, RegExp, an own user
/// `toString`) print the `Object.prototype.toString` tag instead; errors print
/// `Name: message`. node prints a function receiver's SOURCE — perry keeps no
/// source, so the native-code form is the closest stable rendering.
/// Unclassifiable heap values fall back to `#<Object>`.
fn render_incompatible_receiver(bits: u64) -> String {
    use crate::value::JSValue;
    let value = f64::from_bits(bits);
    let jv = JSValue::from_bits(bits);
    if !jv.is_pointer() {
        // undefined / null / bool / number / bigint / heap+SSO string: plain
        // side-effect-free ToString.
        return string_header_owned(crate::value::js_jsvalue_to_string(value));
    }
    let ptr = (bits & crate::value::POINTER_MASK) as usize;
    if crate::symbol::is_registered_symbol(ptr) {
        // "Symbol(desc)" — Symbol.prototype.toString is side-effect free.
        return string_header_owned(unsafe { crate::symbol::js_symbol_to_string(value) }
            as *const crate::string::StringHeader);
    }
    if crate::set::is_registered_set(ptr) {
        return "#<Set>".to_string();
    }
    if crate::map::is_registered_map(ptr) {
        return "#<Map>".to_string();
    }
    match crate::weakref::weak_class_id_from_receiver(value) {
        Some(crate::weakref::CLASS_ID_WEAKSET) => return "#<WeakSet>".to_string(),
        Some(crate::weakref::CLASS_ID_WEAKMAP) => return "#<WeakMap>".to_string(),
        _ => {}
    }
    if crate::date::is_registered_date_bits(bits) {
        return "[object Date]".to_string();
    }
    if crate::regex::is_registered_regex(ptr) {
        return "[object RegExp]".to_string();
    }
    // Heap-header classification — only a real heap pointer above the
    // synthetic handle band may be dereferenced (mirrors `as_real_array`).
    if crate::value::addr_class::is_above_handle_band(ptr)
        && crate::object::is_valid_obj_ptr(ptr as *const u8)
    {
        let obj_type = unsafe {
            (*((ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader))
                .obj_type
        };
        match obj_type {
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY => {
                return "[object Array]".to_string();
            }
            crate::gc::GC_TYPE_PROMISE => return "#<Promise>".to_string(),
            crate::gc::GC_TYPE_ERROR => {
                let err = ptr as *mut crate::error::ErrorHeader;
                let name = string_header_owned(crate::error::js_error_get_name(err));
                let message = string_header_owned(crate::error::js_error_get_message(err));
                return if message.is_empty() {
                    name
                } else {
                    format!("{name}: {message}")
                };
            }
            crate::gc::GC_TYPE_CLOSURE => {
                return "function () { [native code] }".to_string();
            }
            crate::gc::GC_TYPE_OBJECT => {
                // An OWN user `toString` overrides the default — V8 refuses to
                // call it and falls back to the Object.prototype.toString tag.
                // js_string_from_bytes allocates and can trigger a GC that
                // relocates the receiver: root it and re-derive the pointer
                // through the (rewritten) handle after the allocation.
                let scope = crate::gc::RuntimeHandleScope::new();
                let handle = scope.root_nanbox_f64(f64::from_bits(bits));
                let key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
                let obj = crate::value::JSValue::from_bits(handle.get_nanbox_f64().to_bits())
                    .as_pointer::<u8>() as *mut ObjectHeader;
                if unsafe { super::own_data_field_by_name(obj, key) }.is_some() {
                    return "[object Object]".to_string();
                }
                let cid = unsafe { (*obj).class_id };
                if cid != 0 {
                    if let Some(name) = crate::object::class_name_for_id(cid) {
                        if !name.is_empty() {
                            return format!("#<{name}>");
                        }
                    }
                }
                return "#<Object>".to_string();
            }
            _ => {}
        }
    }
    "#<Object>".to_string()
}

/// Throw `TypeError: Method <proto>.<method> called on incompatible receiver
/// <receiver>`. Matches V8/node's wording INCLUDING the appended receiver
/// rendering (#6658 asserts the full message; Test262's brand-check tests
/// assert only the error *type*). Never returns.
fn throw_incompatible_receiver(proto: &str, method: &str, receiver_bits: u64) -> ! {
    let msg = format!(
        "Method {proto}.{method} called on incompatible receiver {}",
        render_incompatible_receiver(receiver_bits)
    );
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

#[inline]
fn set_receiver_or_throw(method: &str) -> *mut crate::set::SetHeader {
    let bits = IMPLICIT_THIS.with(|c| c.get());
    match crate::set::set_ptr_from_receiver_bits(bits) {
        Some(p) => p,
        None => throw_incompatible_receiver("Set.prototype", method, bits),
    }
}

#[inline]
fn map_receiver_or_throw(method: &str) -> *mut crate::map::MapHeader {
    let bits = IMPLICIT_THIS.with(|c| c.get());
    match crate::map::map_ptr_from_receiver_bits(bits) {
        Some(p) => p,
        None => throw_incompatible_receiver("Map.prototype", method, bits),
    }
}

#[inline]
fn weak_receiver_or_throw(expected: u32, proto: &str, method: &str) -> f64 {
    let receiver = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    match crate::weakref::weak_class_id_from_receiver(receiver) {
        Some(cid) if cid == expected => receiver,
        _ => throw_incompatible_receiver(proto, method, receiver.to_bits()),
    }
}

pub(super) extern "C" fn set_proto_add_thunk(
    _c: *const crate::closure::ClosureHeader,
    v: f64,
) -> f64 {
    let set = set_receiver_or_throw("add");
    let r = crate::set::js_set_add(set, v);
    f64::from_bits(crate::value::JSValue::pointer(r as *mut u8).bits())
}

pub(super) extern "C" fn set_proto_has_thunk(
    _c: *const crate::closure::ClosureHeader,
    v: f64,
) -> f64 {
    let set = set_receiver_or_throw("has");
    f64::from_bits(crate::value::JSValue::bool(crate::set::js_set_has(set, v) != 0).bits())
}

pub(super) extern "C" fn set_proto_size_getter_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let set = set_receiver_or_throw("size");
    crate::set::js_set_size(set) as f64
}

pub(super) extern "C" fn set_proto_delete_thunk(
    _c: *const crate::closure::ClosureHeader,
    v: f64,
) -> f64 {
    let set = set_receiver_or_throw("delete");
    f64::from_bits(crate::value::JSValue::bool(crate::set::js_set_delete(set, v) != 0).bits())
}

pub(super) extern "C" fn set_proto_clear_thunk(
    _c: *const crate::closure::ClosureHeader,
    _v: f64,
) -> f64 {
    let set = set_receiver_or_throw("clear");
    crate::set::js_set_clear(set);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(super) extern "C" fn set_proto_foreach_thunk(
    _c: *const crate::closure::ClosureHeader,
    cb: f64,
    this_arg: f64,
) -> f64 {
    let set = set_receiver_or_throw("forEach");
    crate::set::js_set_foreach(set, cb, this_arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(super) extern "C" fn set_proto_values_thunk(
    _c: *const crate::closure::ClosureHeader,
    _v: f64,
) -> f64 {
    let set = set_receiver_or_throw("values");
    f64::from_bits(
        crate::value::JSValue::pointer(
            crate::collection_iter_object::js_set_values_iter_obj(set) as *mut u8
        )
        .bits(),
    )
}

pub(super) extern "C" fn set_proto_keys_thunk(
    _c: *const crate::closure::ClosureHeader,
    _v: f64,
) -> f64 {
    let set = set_receiver_or_throw("keys");
    f64::from_bits(
        crate::value::JSValue::pointer(
            crate::collection_iter_object::js_set_values_iter_obj(set) as *mut u8
        )
        .bits(),
    )
}

pub(super) extern "C" fn set_proto_entries_thunk(
    _c: *const crate::closure::ClosureHeader,
    _v: f64,
) -> f64 {
    let set = set_receiver_or_throw("entries");
    f64::from_bits(
        crate::value::JSValue::pointer(
            crate::collection_iter_object::js_set_entries_iter_obj(set) as *mut u8
        )
        .bits(),
    )
}

pub(super) extern "C" fn map_proto_get_thunk(
    _c: *const crate::closure::ClosureHeader,
    k: f64,
) -> f64 {
    let map = map_receiver_or_throw("get");
    crate::map::js_map_get(map, k)
}

pub(super) extern "C" fn map_proto_set_thunk(
    _c: *const crate::closure::ClosureHeader,
    k: f64,
    v: f64,
) -> f64 {
    let map = map_receiver_or_throw("set");
    let r = crate::map::js_map_set(map, k, v);
    f64::from_bits(crate::value::JSValue::pointer(r as *mut u8).bits())
}

pub(super) extern "C" fn map_proto_has_thunk(
    _c: *const crate::closure::ClosureHeader,
    k: f64,
) -> f64 {
    let map = map_receiver_or_throw("has");
    f64::from_bits(crate::value::JSValue::bool(crate::map::js_map_has(map, k) != 0).bits())
}

pub(super) extern "C" fn map_proto_size_getter_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let map = map_receiver_or_throw("size");
    crate::map::js_map_size(map) as f64
}

pub(super) extern "C" fn map_proto_delete_thunk(
    _c: *const crate::closure::ClosureHeader,
    k: f64,
) -> f64 {
    let map = map_receiver_or_throw("delete");
    f64::from_bits(crate::value::JSValue::bool(crate::map::js_map_delete(map, k) != 0).bits())
}

pub(super) extern "C" fn map_proto_clear_thunk(
    _c: *const crate::closure::ClosureHeader,
    _v: f64,
) -> f64 {
    let map = map_receiver_or_throw("clear");
    crate::map::js_map_clear(map);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(super) extern "C" fn map_proto_foreach_thunk(
    _c: *const crate::closure::ClosureHeader,
    cb: f64,
    this_arg: f64,
) -> f64 {
    let map = map_receiver_or_throw("forEach");
    crate::map::js_map_foreach(map, cb, this_arg);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(super) extern "C" fn map_proto_keys_thunk(
    _c: *const crate::closure::ClosureHeader,
    _v: f64,
) -> f64 {
    let map = map_receiver_or_throw("keys");
    f64::from_bits(
        crate::value::JSValue::pointer(
            crate::collection_iter_object::js_map_keys_iter_obj(map) as *mut u8
        )
        .bits(),
    )
}

pub(super) extern "C" fn map_proto_values_thunk(
    _c: *const crate::closure::ClosureHeader,
    _v: f64,
) -> f64 {
    let map = map_receiver_or_throw("values");
    f64::from_bits(
        crate::value::JSValue::pointer(
            crate::collection_iter_object::js_map_values_iter_obj(map) as *mut u8
        )
        .bits(),
    )
}

pub(super) extern "C" fn map_proto_entries_thunk(
    _c: *const crate::closure::ClosureHeader,
    _v: f64,
) -> f64 {
    let map = map_receiver_or_throw("entries");
    f64::from_bits(
        crate::value::JSValue::pointer(
            crate::collection_iter_object::js_map_entries_iter_obj(map) as *mut u8
        )
        .bits(),
    )
}

pub(super) extern "C" fn weakset_proto_add_thunk(
    _c: *const crate::closure::ClosureHeader,
    v: f64,
) -> f64 {
    let r = weak_receiver_or_throw(crate::weakref::CLASS_ID_WEAKSET, "WeakSet.prototype", "add");
    crate::weakref::js_weakset_add(r, v)
}

pub(super) extern "C" fn weakset_proto_has_thunk(
    _c: *const crate::closure::ClosureHeader,
    v: f64,
) -> f64 {
    let r = weak_receiver_or_throw(crate::weakref::CLASS_ID_WEAKSET, "WeakSet.prototype", "has");
    crate::weakref::js_weakmap_has(r, v)
}

pub(super) extern "C" fn weakset_proto_delete_thunk(
    _c: *const crate::closure::ClosureHeader,
    v: f64,
) -> f64 {
    let r = weak_receiver_or_throw(
        crate::weakref::CLASS_ID_WEAKSET,
        "WeakSet.prototype",
        "delete",
    );
    crate::weakref::js_weakmap_delete(r, v)
}

pub(super) extern "C" fn weakmap_proto_get_thunk(
    _c: *const crate::closure::ClosureHeader,
    k: f64,
) -> f64 {
    let r = weak_receiver_or_throw(crate::weakref::CLASS_ID_WEAKMAP, "WeakMap.prototype", "get");
    crate::weakref::js_weakmap_get(r, k)
}

pub(super) extern "C" fn weakmap_proto_set_thunk(
    _c: *const crate::closure::ClosureHeader,
    k: f64,
    v: f64,
) -> f64 {
    let r = weak_receiver_or_throw(crate::weakref::CLASS_ID_WEAKMAP, "WeakMap.prototype", "set");
    crate::weakref::js_weakmap_set(r, k, v)
}

pub(super) extern "C" fn weakmap_proto_has_thunk(
    _c: *const crate::closure::ClosureHeader,
    k: f64,
) -> f64 {
    let r = weak_receiver_or_throw(crate::weakref::CLASS_ID_WEAKMAP, "WeakMap.prototype", "has");
    crate::weakref::js_weakmap_has(r, k)
}

pub(super) extern "C" fn weakmap_proto_delete_thunk(
    _c: *const crate::closure::ClosureHeader,
    k: f64,
) -> f64 {
    let r = weak_receiver_or_throw(
        crate::weakref::CLASS_ID_WEAKMAP,
        "WeakMap.prototype",
        "delete",
    );
    crate::weakref::js_weakmap_delete(r, k)
}
