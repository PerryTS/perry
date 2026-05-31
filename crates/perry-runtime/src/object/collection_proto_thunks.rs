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
            ipm(proto_obj, "clear", map_proto_clear_thunk as *const u8, 0);
            ipm(proto_obj, "delete", map_proto_delete_thunk as *const u8, 1);
            ipm(
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
            ipm(proto_obj, "set", map_proto_set_thunk as *const u8, 2);
            ipm(proto_obj, "values", map_proto_values_thunk as *const u8, 0);
        }
        "Set" => {
            ipm(proto_obj, "add", set_proto_add_thunk as *const u8, 1);
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
            ipm(proto_obj, "values", set_proto_values_thunk as *const u8, 0);
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

/// Throw `TypeError: Method <proto>.<method> called on incompatible receiver`.
/// Mirrors V8's wording closely; Test262's brand-check tests assert only the
/// error *type*, so the exact message is informational. Never returns.
fn throw_incompatible_receiver(proto: &str, method: &str) -> ! {
    let msg = format!("Method {proto}.{method} called on incompatible receiver");
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
        None => throw_incompatible_receiver("Set.prototype", method),
    }
}

#[inline]
fn map_receiver_or_throw(method: &str) -> *mut crate::map::MapHeader {
    let bits = IMPLICIT_THIS.with(|c| c.get());
    match crate::map::map_ptr_from_receiver_bits(bits) {
        Some(p) => p,
        None => throw_incompatible_receiver("Map.prototype", method),
    }
}

#[inline]
fn weak_receiver_or_throw(expected: u32, proto: &str, method: &str) -> f64 {
    let receiver = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    match crate::weakref::weak_class_id_from_receiver(receiver) {
        Some(cid) if cid == expected => receiver,
        _ => throw_incompatible_receiver(proto, method),
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
