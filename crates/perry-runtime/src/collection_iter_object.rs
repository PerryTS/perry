//! Real Map/Set iterator objects (#2856).
//!
//! Node's `Map.prototype.{entries,keys,values}` and
//! `Set.prototype.{entries,keys,values}` return iterator OBJECTS — not
//! arrays. Each is `Array.isArray(...) === false`, exposes a `.next()`
//! method returning `{ value, done }`, is iterable via `Symbol.iterator`,
//! and is recognized by `util.types.isMapIterator()` / `isSetIterator()`.
//!
//! Representation mirrors `array/iter_object.rs`: a regular `ObjectHeader`
//! with a dedicated class id. Field 0 holds the backing Map/Set (NaN-boxed
//! pointer, so the object scanner keeps it alive), field 1 the cursor
//! index, field 2 the iterator kind. The collection is read LIVE at each
//! `.next()` via `js_map_entry_key_at` / `js_map_entry_value_at` /
//! `js_set_value_at`, so insertion-order-after-delete (#2831) is honored.
//!
//! Dispatch lives in `object/native_call_method.rs` via the class-id check
//! next to the array iterator one; `flat_clone.rs` detects the class id so
//! `[...m.entries()]` / `Array.from(s.values())` drive `.next()`.

use crate::array::ArrayHeader;
use crate::map::MapHeader;
use crate::object::{js_object_alloc, js_object_get_field, js_object_set_field, ObjectHeader};
use crate::set::SetHeader;
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_UNDEFINED};

/// Class id reserved for Map iterators. Sits just past the array iterator
/// id (0xFFFF0006) in the 0xFFFF prefix reserved for runtime-defined
/// classes.
pub const MAP_ITERATOR_CLASS_ID: u32 = 0xFFFF_0007;
/// Class id reserved for Set iterators.
pub const SET_ITERATOR_CLASS_ID: u32 = 0xFFFF_0008;

/// Iterator kind tags — matches the i32 stored in field 2.
const KIND_KEYS: i32 = 1;
const KIND_VALUES: i32 = 0;
const KIND_ENTRIES: i32 = 2;

/// Methods implemented intrinsically by the Map/Set iterator class-id
/// dispatcher. `return` and `throw` deliberately are not here: ordinary
/// collection iterators do not define them, and a user-installed own or
/// inherited method must flow through ordinary method lookup (#9098).
#[inline]
pub(crate) fn is_intrinsic_iterator_method(method_name: &str) -> bool {
    matches!(method_name, "next" | "Symbol.iterator" | "@@iterator")
}

/// `true` when `addr` carries a Map iterator object's class id.
pub fn is_map_iterator_addr(addr: usize) -> bool {
    iterator_class_id(addr) == Some(MAP_ITERATOR_CLASS_ID)
}

/// `true` when `addr` carries a Set iterator object's class id.
pub fn is_set_iterator_addr(addr: usize) -> bool {
    iterator_class_id(addr) == Some(SET_ITERATOR_CLASS_ID)
}

fn iterator_class_id(addr: usize) -> Option<u32> {
    // `util.types.isMapIterator(v)` / `isSetIterator(v)` hand any value's
    // candidate address here. The canonical header read rejects the handle
    // band and out-of-window bits before touching memory (#10479: the old
    // `addr < GC_HEADER_SIZE + 0x1000` floor let a proxy/fetch id through).
    unsafe {
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        Some((*(addr as *const ObjectHeader)).class_id)
    }
}

unsafe fn alloc_iterator(class_id: u32, coll_nanboxed: f64, kind: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let coll_h = scope.root_nanbox_f64(coll_nanboxed);
    let obj_h = scope.root_raw_mut_ptr(js_object_alloc(class_id, 4));
    // Field 0: backing collection (NaN-boxed pointer so the GC scanner keeps it).
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| {
        js_object_set_field(
            obj,
            0,
            JSValue::from_bits(coll_h.get_nanbox_f64().to_bits()),
        )
    });
    // Field 1: cursor index (index just past the last-returned entry), starts at 0.
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| js_object_set_field(obj, 1, JSValue::number(0.0)));
    // Field 2: iterator kind.
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| {
        js_object_set_field(obj, 2, JSValue::number(kind as f64))
    });
    // Field 3: the backing collection's compaction epoch this iterator last
    // synchronised with (starts at 0 — a cursor of 0 rebases to 0 through any
    // history). `next()` rebases the cursor through every squeeze recorded
    // since, so a compaction below the cursor can never skip an entry (#6075,
    // #6165 — and the multi-hole squeeze the key-based re-derive got wrong).
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| js_object_set_field(obj, 3, JSValue::number(0.0)));
    // Link `[[Prototype]]` to the shared `%MapIteratorPrototype%` /
    // `%SetIteratorPrototype%` singleton so `Object.getPrototypeOf(it)` and the
    // inherited `.next` read resolve.
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| {
        crate::object::attach_iterator_prototype(obj, class_id)
    });
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| js_nanbox_pointer(obj as i64))
}

/// Build a fresh Map iterator object for `map` (raw pointer) of the given
/// kind. Returns the RAW iterator-object pointer as i64 (caller NaN-boxes).
unsafe fn map_iter_obj_raw(map: *const MapHeader, kind: i32) -> i64 {
    // #7570: these entries are reached from the DECLARED-type lowering of
    // `m.entries()`/`.keys()`/`.values()`, so `map` can be a `class X extends
    // Map` instance (a plain ObjectHeader) rather than a `MapHeader`. Every
    // `next()` would then read `keys_array` as the entries pointer (#8113 moved
    // the confusable word; the hazard is unchanged). Resolve onto the hidden backing before the iterator captures it.
    // Unlike the `js_map_*` entries this is not a `clean_map_ptr` caller — it
    // stores the raw pointer into the iterator object, so the redirect has to
    // happen here.
    let map = crate::map::resolve_map_receiver(map);
    if map.is_null() {
        return 0;
    }
    let nanboxed = alloc_iterator(MAP_ITERATOR_CLASS_ID, js_nanbox_pointer(map as i64), kind);
    js_nanbox_get_pointer(nanboxed)
}

unsafe fn set_iter_obj_raw(set: *const SetHeader, kind: i32) -> i64 {
    // #7570 — see `map_iter_obj_raw`.
    let set = crate::set::resolve_set_receiver(set);
    if set.is_null() {
        return 0;
    }
    let nanboxed = alloc_iterator(SET_ITERATOR_CLASS_ID, js_nanbox_pointer(set as i64), kind);
    js_nanbox_get_pointer(nanboxed)
}

// ---------------------------------------------------------------------------
// C-ABI entry points for codegen / runtime dispatch. Each takes a RAW
// Map/Set pointer (the handle from `unbox_to_i64`) and returns the RAW
// iterator-object pointer as i64; the caller NaN-boxes it.

#[no_mangle]
pub extern "C" fn js_map_entries_iter_obj(map: *const MapHeader) -> i64 {
    unsafe { map_iter_obj_raw(map, KIND_ENTRIES) }
}

#[no_mangle]
pub extern "C" fn js_map_keys_iter_obj(map: *const MapHeader) -> i64 {
    unsafe { map_iter_obj_raw(map, KIND_KEYS) }
}

#[no_mangle]
pub extern "C" fn js_map_values_iter_obj(map: *const MapHeader) -> i64 {
    unsafe { map_iter_obj_raw(map, KIND_VALUES) }
}

#[no_mangle]
pub extern "C" fn js_set_values_iter_obj(set: *const SetHeader) -> i64 {
    unsafe { set_iter_obj_raw(set, KIND_VALUES) }
}

#[no_mangle]
pub extern "C" fn js_set_keys_iter_obj(set: *const SetHeader) -> i64 {
    unsafe { set_iter_obj_raw(set, KIND_KEYS) }
}

#[no_mangle]
pub extern "C" fn js_set_entries_iter_obj(set: *const SetHeader) -> i64 {
    unsafe { set_iter_obj_raw(set, KIND_ENTRIES) }
}

// These are only invoked from generated LLVM IR (codegen emits the
// `.entries()`/`.keys()`/`.values()` call), so they have zero internal
// Rust callers. The whole-program auto-optimize bitcode link would
// otherwise internalize + dead-strip the `#[no_mangle]` exports and break
// the default compile path (see project_auto_optimize_keepalive).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_MAP_ENTRIES_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_entries_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_MAP_KEYS_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_keys_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_MAP_VALUES_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_values_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_VALUES_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_values_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_KEYS_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_keys_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_ENTRIES_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_entries_iter_obj;

// #7564: the `{ value, done }` constructor was a local five-allocation copy
// with every intermediate in a bare Rust local — see `crate::iter_result` for
// what that cost and why it was a stale-from-space hazard. Since the fused
// driver's recycling moved there too, this module reaches results only through
// `iter_result::emit_iter_result_cached` and imports no constructor of its own.

/// `[key, value]` pair array for Map entries / Set entries (`[v, v]`).
unsafe fn make_pair_array(a: f64, b: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let a = scope.root_nanbox_f64(a);
    let b = scope.root_nanbox_f64(b);
    let pair = scope.root_raw_mut_ptr(crate::array::js_array_alloc(2));
    pair.with_mut_ptr::<ArrayHeader, _>(|pair| {
        crate::array::store_array_slot(pair, 0, a.get_nanbox_u64());
        crate::array::store_array_slot(pair, 1, b.get_nanbox_u64());
        (*pair).length = 2;
        crate::array::rebuild_array_layout_exact(pair);
    });
    pair.with_mut_ptr::<ArrayHeader, _>(|pair| js_nanbox_pointer(pair as i64))
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on a Map iterator object.
pub unsafe fn dispatch_map_iterator_method(iter_obj: *mut ObjectHeader, method_name: &str) -> f64 {
    dispatch_map_iterator_method_emit(
        iter_obj,
        method_name,
        crate::iter_result::IterResultTarget::Object,
        true,
    )
}

/// Builtin advance only — the canonical prototype thunk's entry (#9019).
/// `%MapIteratorPrototype%.next.call(it)` (including a `.bind(it)` taken
/// before a patch was installed) must run the builtin algorithm even when
/// the instance carries an own patched `next`: honoring the override there
/// would make a patch that delegates to the bound original re-enter itself
/// forever, and it is also not what the spec function does.
pub(crate) unsafe fn dispatch_map_iterator_method_builtin(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    dispatch_map_iterator_method_emit(
        iter_obj,
        method_name,
        crate::iter_result::IterResultTarget::Object,
        false,
    )
}

unsafe fn dispatch_map_iterator_method_emit(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
    target: crate::iter_result::IterResultTarget,
    honor_override: bool,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter_h = scope.root_nanbox_f64(js_nanbox_pointer(iter_obj as i64));
    let iter_obj = || js_nanbox_get_pointer(iter_h.get_nanbox_f64()) as *mut ObjectHeader;
    match method_name {
        "next" => {
            if honor_override {
                if let Some(result) =
                    crate::object::call_overridden_iterator_next(iter_obj(), MAP_ITERATOR_CLASS_ID)
                {
                    return result;
                }
            }
            let backing = f64::from_bits(js_object_get_field(iter_obj(), 0).bits());
            let map_h = scope.root_nanbox_f64(backing);
            let map = || js_nanbox_get_pointer(map_h.get_nanbox_f64()) as *const MapHeader;
            let kind = f64::from_bits(js_object_get_field(iter_obj(), 2).bits()) as i32;
            if map().is_null() {
                return emit_iter_result(target, JSValue::undefined(), true);
            }
            let cursor = f64::from_bits(js_object_get_field(iter_obj(), 1).bits()) as u32;
            // Field 3: the backing Map's compaction epoch this iterator last
            // synchronised with. `map_cursor_next_raw` rebases the cursor
            // through every squeeze since (exactly — by removed-slot count,
            // not by re-finding a key that may itself be gone), then steps
            // over tombstones.
            let epoch = f64::from_bits(js_object_get_field(iter_obj(), 3).bits());
            let epoch = if epoch > 0.0 { epoch as u32 } else { 0 };
            let used = crate::map::map_used_entries(map());
            let next = crate::map::map_cursor_next_raw(map(), cursor, epoch);
            js_object_set_field(
                iter_obj(),
                3,
                JSValue::number(crate::map::map_compaction_epoch(map()) as f64),
            );
            let Some(idx) = next else {
                js_object_set_field(iter_obj(), 1, JSValue::number(used as f64));
                // Once a collection iterator is exhausted it stays exhausted,
                // even if entries are appended later.
                js_object_set_field(iter_obj(), 0, JSValue::undefined());
                return emit_iter_result(target, JSValue::undefined(), true);
            };

            let entry_key = crate::map::map_entry_key_raw(map(), idx);
            // Record the cursor BEFORE any allocation below.
            js_object_set_field(iter_obj(), 1, JSValue::number((idx + 1) as f64));

            let value = match kind {
                KIND_KEYS => JSValue::from_bits(entry_key.to_bits()),
                KIND_VALUES => {
                    JSValue::from_bits(crate::map::map_entry_value_raw(map(), idx).to_bits())
                }
                _ => {
                    let val = crate::map::map_entry_value_raw(map(), idx);
                    JSValue::from_bits(make_pair_array(entry_key, val).to_bits())
                }
            };
            emit_iter_result(target, value, false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj() as i64),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on a Set iterator object.
pub unsafe fn dispatch_set_iterator_method(iter_obj: *mut ObjectHeader, method_name: &str) -> f64 {
    dispatch_set_iterator_method_emit(
        iter_obj,
        method_name,
        crate::iter_result::IterResultTarget::Object,
        true,
    )
}

/// Builtin advance only — see [`dispatch_map_iterator_method_builtin`].
pub(crate) unsafe fn dispatch_set_iterator_method_builtin(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    dispatch_set_iterator_method_emit(
        iter_obj,
        method_name,
        crate::iter_result::IterResultTarget::Object,
        false,
    )
}

unsafe fn dispatch_set_iterator_method_emit(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
    target: crate::iter_result::IterResultTarget,
    honor_override: bool,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter_h = scope.root_nanbox_f64(js_nanbox_pointer(iter_obj as i64));
    let iter_obj = || js_nanbox_get_pointer(iter_h.get_nanbox_f64()) as *mut ObjectHeader;
    match method_name {
        "next" => {
            if honor_override {
                if let Some(result) =
                    crate::object::call_overridden_iterator_next(iter_obj(), SET_ITERATOR_CLASS_ID)
                {
                    return result;
                }
            }
            let backing = f64::from_bits(js_object_get_field(iter_obj(), 0).bits());
            let set_h = scope.root_nanbox_f64(backing);
            let set = || js_nanbox_get_pointer(set_h.get_nanbox_f64()) as *const SetHeader;
            let kind = f64::from_bits(js_object_get_field(iter_obj(), 2).bits()) as i32;
            if set().is_null() {
                return emit_iter_result(target, JSValue::undefined(), true);
            }
            let cursor = f64::from_bits(js_object_get_field(iter_obj(), 1).bits()) as u32;
            // Field 3: the backing Set's compaction epoch (see the Map arm).
            let epoch = f64::from_bits(js_object_get_field(iter_obj(), 3).bits());
            let epoch = if epoch > 0.0 { epoch as u32 } else { 0 };
            let used = crate::set::set_used_entries(set());
            let next = crate::set::set_cursor_next_raw(set(), cursor, epoch);
            js_object_set_field(
                iter_obj(),
                3,
                JSValue::number(crate::set::set_compaction_epoch(set()) as f64),
            );
            let Some(idx) = next else {
                js_object_set_field(iter_obj(), 1, JSValue::number(used as f64));
                js_object_set_field(iter_obj(), 0, JSValue::undefined());
                return emit_iter_result(target, JSValue::undefined(), true);
            };

            let elem = crate::set::set_value_raw(set(), idx);
            js_object_set_field(iter_obj(), 1, JSValue::number((idx + 1) as f64));

            let value = match kind {
                // For Sets, keys === values; entries yields [v, v] pairs.
                KIND_ENTRIES => JSValue::from_bits(make_pair_array(elem, elem).to_bits()),
                _ => JSValue::from_bits(elem.to_bits()),
            };
            emit_iter_result(target, value, false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj() as i64),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

unsafe fn emit_iter_result(
    target: crate::iter_result::IterResultTarget,
    value: JSValue,
    done: bool,
) -> f64 {
    crate::iter_result::emit_iter_result(
        target,
        crate::iter_result::IterResultOrder::ValueDone,
        value,
        done,
    )
}

/// Native consumer of the same Map/Set advance used by manual next calls.
pub(crate) unsafe fn dispatch_collection_iterator_step(
    obj: *mut ObjectHeader,
    out: *mut crate::iter_result::IteratorStep,
) {
    let target = crate::iter_result::IterResultTarget::Step(out);
    match (*obj).class_id {
        MAP_ITERATOR_CLASS_ID => {
            dispatch_map_iterator_method_emit(obj, "next", target, false);
        }
        SET_ITERATOR_CLASS_ID => {
            dispatch_set_iterator_method_emit(obj, "next", target, false);
        }
        _ => unreachable!(),
    }
}

/// Public result-object protocol; only native compiled steps may elide it.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_for_of_next(iter: f64) -> f64 {
    let result = crate::object::js_native_call_method(
        iter,
        b"next".as_ptr() as *const i8,
        4,
        std::ptr::null(),
        0,
    );
    crate::symbol::js_iterator_result_validate(result)
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_FOR_OF_NEXT: unsafe extern "C-unwind" fn(f64) -> f64 = js_for_of_next;
