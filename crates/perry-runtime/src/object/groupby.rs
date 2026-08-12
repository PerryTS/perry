//! `Object.groupBy` / `Map.groupBy` (#2777/#2779) — split out of
//! `object_ops.rs` to keep it under the 2000-line cap. Pure relocation;
//! `use super::*` gives the same visibility the parent module has.
//!
//! ## Rooting (#7949)
//!
//! Every JS value this module holds between two calls that can allocate lives
//! in a [`RootedValues`] or a [`RuntimeHandle`], never in a bare `f64` local or
//! a `Vec<f64>`. Three separate windows made that mandatory here:
//!
//! * `group_by_collect` calls a **user callback** once per element. That runs
//!   arbitrary JS, so it can evacuate — while the materialized input array, the
//!   closure, and every already-collected `(key, item)` pair are live.
//! * `Object.groupBy` coerces each non-Symbol key with `js_string_coerce`
//!   (allocates for every shape except an already-heap string), builds one
//!   result array per group, and interns one key string per group — all while
//!   the result object and every remaining group's items are live.
//! * `Map.groupBy` allocates a result array per group and inserts it into the
//!   Map, while the Map, the keys, and the remaining groups are live.
//!
//! The Symbol coalescing key is `SymbolHeader::id`, not the symbol's address,
//! for the same reason `#7246` interns descriptions by id: an evacuation copies
//! the id verbatim, so a symbol that moves mid-loop still lands in its own
//! group instead of starting a duplicate one.

use super::*;
use crate::gc::{RootedValues, RuntimeHandle, RuntimeHandleScope};

/// `Object.groupBy(items, callback)` — Node 22+ static method.
///
/// Consumes any iterable `items`, calls `callback(value, index)` to compute
/// a key per element, converts the key with ToPropertyKey (Symbols stay
/// Symbol keys, everything else is coerced to a String), and returns a new
/// **null-prototype** object whose keys are the distinct callback results
/// and whose values are arrays of the items that produced each key.
/// Insertion order of first-seen keys is preserved.
///
/// Throws `TypeError` for nullish `items` or a non-callable `callback`.
/// Returns the result object as a NaN-boxed POINTER_TAG f64.
#[no_mangle]
pub extern "C" fn js_object_group_by(items_value: f64, callback: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let (keys, items) = group_by_collect(&scope, items_value, callback, b"Object.groupBy");

        // Coalesce by ToPropertyKey. String keys group by string contents;
        // Symbol keys group by Symbol identity (`SymbolHeader::id`, which
        // survives evacuation — the address does not).
        use std::collections::HashMap;
        enum GroupKey<'scope> {
            Str(String),
            Sym(RuntimeHandle<'scope>),
        }
        let mut str_index: HashMap<String, usize> = HashMap::new();
        let mut sym_index: HashMap<u64, usize> = HashMap::new();
        let mut order: Vec<GroupKey<'_>> = Vec::new();
        let mut groups: Vec<RootedValues<'_>> = Vec::new();

        for i in 0..keys.len() {
            // Re-read both words on every use: `js_string_coerce` below can run
            // a user `toString`/`valueOf`, and the handles are what survive it.
            if let Some(symbol_id) = group_by_symbol_identity(keys.get(i)) {
                let idx = match sym_index.get(&symbol_id) {
                    Some(idx) => *idx,
                    None => {
                        let idx = groups.len();
                        sym_index.insert(symbol_id, idx);
                        order.push(GroupKey::Sym(scope.root_nanbox_f64(keys.get(i))));
                        groups.push(RootedValues::new(&scope));
                        idx
                    }
                };
                groups[idx].push(items.get(i));
            } else {
                let key_string = group_by_key_string(keys.get(i));
                let idx = match str_index.get(&key_string) {
                    Some(idx) => *idx,
                    None => {
                        let idx = groups.len();
                        str_index.insert(key_string.clone(), idx);
                        order.push(GroupKey::Str(key_string));
                        groups.push(RootedValues::new(&scope));
                        idx
                    }
                };
                groups[idx].push(items.get(i));
            }
        }

        // Null-prototype result object (Node: getPrototypeOf === null).
        let obj = js_object_alloc_null_proto(0, order.len() as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // The result object is reachable from nothing else until we return it,
        // so this handle is its only root across the per-group allocations
        // below.
        let obj_handle = scope.root_nanbox_f64(group_by_box_pointer(obj as usize));
        for (idx, key) in order.iter().enumerate() {
            let arr_handle = scope
                .root_nanbox_f64(group_by_box_pointer(
                    group_by_make_array(&scope, &groups[idx]) as usize,
                ));
            match key {
                GroupKey::Str(s) => {
                    // Intern the key string FIRST; it is the last allocation in
                    // this iteration, so the receiver and value reads after it
                    // are the post-collection addresses.
                    let key_str_ptr =
                        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                    let obj_ptr = crate::value::js_nanbox_get_pointer(obj_handle.get_nanbox_f64())
                        as *mut ObjectHeader;
                    js_object_set_field_by_name(obj_ptr, key_str_ptr, arr_handle.get_nanbox_f64());
                }
                GroupKey::Sym(sym_handle) => {
                    crate::symbol::js_object_set_symbol_property(
                        obj_handle.get_nanbox_f64(),
                        sym_handle.get_nanbox_f64(),
                        arr_handle.get_nanbox_f64(),
                    );
                }
            }
        }
        obj_handle.get_nanbox_f64()
    }
}

/// `Map.groupBy(items, callback)` — Node 22+ static method.
///
/// Consumes any iterable `items`, calls `callback(value, index)` per element,
/// and groups elements into a new `Map` keyed by the callback results
/// **without coercion** (numbers, objects, and Symbols all retain identity
/// via SameValueZero). Values are arrays of the grouped items, in first-seen
/// key order.
///
/// Throws `TypeError` for nullish `items` or a non-callable `callback`.
/// Returns the result Map as a NaN-boxed POINTER_TAG f64.
#[no_mangle]
pub extern "C" fn js_map_group_by(items_value: f64, callback: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let (keys, items) = group_by_collect(&scope, items_value, callback, b"Map.groupBy");

        let map = crate::map::js_map_alloc(0);
        if map.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let map_handle = scope.root_nanbox_f64(group_by_box_pointer(map as usize));

        // Coalesce by SameValueZero. Collect groups in first-seen order, then
        // materialize into the Map at the end so per-push array reallocation
        // never invalidates a value stored inside the Map.
        let mut order = RootedValues::new(&scope);
        let mut groups: Vec<RootedValues<'_>> = Vec::new();

        'outer: for i in 0..keys.len() {
            for idx in 0..order.len() {
                if crate::value::js_jsvalue_same_value_zero(order.get(idx), keys.get(i)) != 0 {
                    groups[idx].push(items.get(i));
                    continue 'outer;
                }
            }
            order.push(keys.get(i));
            let mut group = RootedValues::new(&scope);
            group.push(items.get(i));
            groups.push(group);
        }

        for idx in 0..order.len() {
            // `group_by_make_array` allocates, so read the Map and the key back
            // out of their roots afterwards, not before.
            let arr_value =
                group_by_box_pointer(group_by_make_array(&scope, &groups[idx]) as usize);
            let map_ptr = crate::value::js_nanbox_get_pointer(map_handle.get_nanbox_f64())
                as *mut crate::map::MapHeader;
            crate::map::js_map_set(map_ptr, order.get(idx), arr_value);
        }

        map_handle.get_nanbox_f64()
    }
}

/// Keepalive anchors: these `#[no_mangle]` helpers are only called from
/// codegen-emitted `.o`. The auto-optimize whole-program LLVM rebuild
/// dead-strips unreferenced `#[no_mangle]` symbols (see #3320), so pin them.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_OBJECT_GROUP_BY: extern "C" fn(f64, f64) -> f64 = js_object_group_by;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_MAP_GROUP_BY: extern "C" fn(f64, f64) -> f64 = js_map_group_by;

/// NaN-box a heap address with `POINTER_TAG`.
#[inline]
fn group_by_box_pointer(addr: usize) -> f64 {
    f64::from_bits((addr as u64) | 0x7FFD_0000_0000_0000)
}

/// Returns true if `value` is a Symbol (registered SymbolHeader pointer).
unsafe fn group_by_value_is_symbol(value: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(value);
    raw != 0 && crate::symbol::is_registered_symbol(raw as usize)
}

/// Move-stable coalescing identity for a Symbol key, or `None` if `value` is
/// not a Symbol.
///
/// `SymbolHeader::id` is a monotonic `u64` that an evacuation copies verbatim,
/// so it stays a valid `HashMap` key across a collection. The symbol's
/// *address* does not: a fresh `Symbol(desc)` is a GC allocation, and keying on
/// its address would start a second group for the same symbol the first time a
/// collection moved it mid-loop.
unsafe fn group_by_symbol_identity(value: f64) -> Option<u64> {
    if !group_by_value_is_symbol(value) {
        return None;
    }
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    let sym = raw as *const crate::symbol::SymbolHeader;
    if (*sym).magic != crate::symbol::SYMBOL_MAGIC {
        // Registered but not header-shaped: fall back to the address. Nothing
        // in the tree produces this today; the branch exists so an unexpected
        // shape degrades to the previous behaviour instead of panicking.
        return Some(raw as u64);
    }
    Some((*sym).id)
}

/// ToPropertyKey a non-Symbol group key into an owned Rust `String`.
///
/// The returned `String` is Rust-heap and therefore move-immune, which is the
/// point: the *key* stops being a GC value the instant it is coerced, so the
/// `HashMap` that coalesces groups holds no heap addresses at all.
unsafe fn group_by_key_string(key_val: f64) -> String {
    let key_ptr = crate::builtins::js_string_coerce(key_val);
    if key_ptr.is_null() {
        return "undefined".to_string();
    }
    let len = (*key_ptr).byte_len as usize;
    let data = (key_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

/// Shared grouping core for `Object.groupBy` / `Map.groupBy`.
///
/// Validates `items` (nullish → TypeError) and `callback` (non-callable →
/// TypeError), materializes `items` through the iterator protocol (so Sets,
/// strings, and custom iterables all work), then calls
/// `callback(value, index)` for each element. Returns the per-element keys and
/// items as two parallel [`RootedValues`] — index `i` of each is one pair. The
/// caller decides how to coalesce keys (ToPropertyKey for Object,
/// SameValueZero for Map).
///
/// #7949: the callback is user JS, so every iteration is a collection point.
/// The materialized array, the closure, and every pair collected so far are
/// rooted across it; the array and closure pointers are re-derived from their
/// handles on each iteration rather than hoisted out of the loop.
unsafe fn group_by_collect<'scope>(
    scope: &'scope RuntimeHandleScope,
    items_value: f64,
    callback: f64,
    callee_name: &[u8],
) -> (RootedValues<'scope>, RootedValues<'scope>) {
    let items_jv = crate::value::JSValue::from_bits(items_value.to_bits());
    if items_jv.is_null() || items_jv.is_undefined() {
        // Match Node: "X.groupBy called on null or undefined"
        let mut msg = callee_name.to_vec();
        msg.extend_from_slice(b" called on null or undefined");
        throw_group_by_type_error(&msg);
    }
    if !group_by_value_is_callable(callback) {
        throw_group_by_type_error(b"callback is not a function");
    }

    let callback_handle = scope.root_nanbox_f64(callback);
    // Materialize any iterable into an Array via the iterator protocol. This
    // can run a user iterator, so root the callback before it.
    let array_handle = scope.root_nanbox_f64(crate::array::js_for_of_to_array(items_value));

    let length = {
        let raw = crate::value::js_nanbox_get_pointer(array_handle.get_nanbox_f64())
            as *const ArrayHeader;
        if raw.is_null() {
            0
        } else {
            crate::array::js_array_length(raw) as usize
        }
    };

    let mut keys = RootedValues::with_capacity(scope, length);
    let mut items = RootedValues::with_capacity(scope, length);
    for i in 0..length {
        let raw = crate::value::js_nanbox_get_pointer(array_handle.get_nanbox_f64())
            as *const ArrayHeader;
        // Root the item BEFORE the callback runs — `items.get(i)` below is the
        // post-collection word, and the pair stays in sync because keys are
        // pushed only after a successful call.
        items.push(crate::array::js_array_get_f64(raw, i as u32));
        let cb_ptr = crate::value::js_nanbox_get_pointer(callback_handle.get_nanbox_f64())
            as *const crate::closure::ClosureHeader;
        let key_val = crate::closure::js_closure_call2(cb_ptr, items.get(i), i as f64);
        keys.push(key_val);
    }
    (keys, items)
}

/// Build an Array<f64> from a rooted group of element values.
///
/// The element words are read out of their handles only after `js_array_alloc`
/// has run, and the array itself is rooted across `rebuild_array_layout_from_slots`
/// so the returned pointer is never a pre-collection address.
unsafe fn group_by_make_array(
    scope: &RuntimeHandleScope,
    items_for_key: &RootedValues<'_>,
) -> *mut ArrayHeader {
    let len = items_for_key.len();
    let arr = crate::array::js_array_alloc(len as u32);
    (*arr).length = len as u32;
    let arr_data = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    for i in 0..len {
        // GC_STORE_AUDIT(INIT): groupBy result array is unpublished; layout is rebuilt before publication.
        std::ptr::write(arr_data.add(i), items_for_key.get(i));
    }
    let arr_handle = scope.root_nanbox_f64(group_by_box_pointer(arr as usize));
    super::rebuild_array_layout_from_slots(arr);
    crate::value::js_nanbox_get_pointer(arr_handle.get_nanbox_f64()) as *mut ArrayHeader
}

fn throw_group_by_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// Returns true if `value` is a callable closure.
unsafe fn group_by_value_is_callable(value: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(value);
    raw >= 0x10000
        && !crate::closure::get_valid_func_ptr(raw as *const crate::closure::ClosureHeader)
            .is_null()
}
