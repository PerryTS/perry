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
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let gc_header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        Some((*(addr as *const ObjectHeader)).class_id)
    }
}

unsafe fn alloc_iterator(class_id: u32, coll_nanboxed: f64, kind: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let coll_h = scope.root_nanbox_f64(coll_nanboxed);
    let obj_h = scope.root_raw_mut_ptr(js_object_alloc(class_id, 6));
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
    // Field 3: collection size observed at the last `next()`. `-1` sentinel means
    // "not started" (no entry returned yet). Used to detect a mid-iteration
    // delete (which compacts the entries array, shifting live entries below the
    // cursor) so the cursor can be re-derived from the last key (#6075).
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| js_object_set_field(obj, 3, JSValue::number(-1.0)));
    // Field 4: the KEY of the last-returned entry (a Map key / Set value), used
    // to re-derive the cursor after a delete-shift. Undefined until started.
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| js_object_set_field(obj, 4, JSValue::undefined()));
    // Field 5: the recycled `{value, done}` result the FUSED for-of driver
    // mutates in place (one allocation per loop, not per element). Manual
    // `.next()` calls never touch it — they keep returning fresh objects, so
    // a caller that retains results observes spec behavior.
    obj_h.with_mut_ptr::<ObjectHeader, _>(|obj| js_object_set_field(obj, 5, JSValue::undefined()));
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
#[used]
static KEEP_MAP_ENTRIES_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_entries_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_MAP_KEYS_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_keys_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_MAP_VALUES_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_values_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_SET_VALUES_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_values_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_SET_KEYS_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_keys_iter_obj;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_SET_ENTRIES_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_entries_iter_obj;

/// Build the `{ value, done }` iterator-result object. Mirrors
/// `array/iter_object.rs::make_iter_result`.
// #7564: this was a local five-allocation copy with every intermediate in a
// bare Rust local — see `crate::iter_result` for what that cost and why it was
// a stale-from-space hazard. `use` rather than a wrapper so the call sites
// below read unchanged.
use crate::iter_result::make_iter_result;

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

/// Compute the entries-array index to read next, self-correcting for a
/// mid-iteration delete. `cursor` = index just past the last-returned entry;
/// `last_key_in_place` = the previously-read key is still at `cursor-1`;
/// `find_last` locates the last-returned key's current index (or `< 0` if it was
/// deleted).
///
/// Deleting an entry compacts the backing array (entries after the hole shift
/// down one slot, #2831), so a delete at index ≤ cursor would move an unvisited
/// entry below the cursor and skip it. If the last-returned key is still sitting
/// at `cursor-1`, no such shift happened and the plain cursor is correct — so
/// normal / append-only iteration keeps the fast path and object-keyed maps pay
/// no lookup. Otherwise re-derive from the last key: locate it (`+1` after it),
/// or, if it was itself deleted, read the entry that shifted into its slot
/// (`cursor-1`). Comparing the key (rather than the size) also catches a delete
/// balanced by an add in the same turn. (#6075 / #6165)
fn next_read_index(cursor: u32, last_key_in_place: bool, find_last: impl FnOnce() -> i32) -> u32 {
    if cursor == 0 || last_key_in_place {
        return cursor;
    }
    let j = find_last();
    // A delete only shifts entries DOWN, so a last key that merely shifted is now
    // below the cursor (`j < cursor`) → resume after it. Otherwise it was deleted
    // (`j < 0`) or deleted-then-re-added at the end (`j >= cursor`) — either way
    // the entry that shifted into its old slot sits at `cursor-1`.
    if j >= 0 && (j as u32) < cursor {
        (j as u32) + 1
    } else {
        cursor.saturating_sub(1)
    }
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on a Map iterator object.
pub unsafe fn dispatch_map_iterator_method(iter_obj: *mut ObjectHeader, method_name: &str) -> f64 {
    dispatch_map_iterator_method_emit(iter_obj, method_name, false, true)
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
    dispatch_map_iterator_method_emit(iter_obj, method_name, false, false)
}

unsafe fn dispatch_map_iterator_method_emit(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
    emit_cached: bool,
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
                return emit_iter_result(&scope, &iter_h, emit_cached, JSValue::undefined(), true);
            }
            let cursor = f64::from_bits(js_object_get_field(iter_obj(), 1).bits()) as u32;
            let last_key = js_object_get_field(iter_obj(), 4);
            let used = crate::map::map_used_entries(map());
            // Is the last-returned key still at cursor-1? (SameValueZero, so a
            // NaN key matches itself.) If so, no delete shifted an entry at/below
            // the cursor.
            let in_place = cursor > 0 && {
                let prev = crate::map::map_entry_key_raw(map(), cursor - 1);
                crate::value::js_jsvalue_same_value_zero(prev, f64::from_bits(last_key.bits())) != 0
            };
            let mut idx = next_read_index(cursor, in_place, || {
                crate::map::find_key_index(map(), f64::from_bits(last_key.bits()))
            });
            // Tombstoned deletes leave holes in the raw entry order; the
            // cursor walks raw indices, so step over them here.
            while idx < used
                && crate::map::map_entry_key_raw(map(), idx).to_bits()
                    == crate::map::MAP_HOLE_KEY_BITS
            {
                idx += 1;
            }
            if idx >= used {
                js_object_set_field(iter_obj(), 1, JSValue::number(used as f64));
                // Once a collection iterator is exhausted it stays exhausted,
                // even if entries are appended later.
                js_object_set_field(iter_obj(), 0, JSValue::undefined());
                return emit_iter_result(&scope, &iter_h, emit_cached, JSValue::undefined(), true);
            }

            let entry_key = crate::map::map_entry_key_raw(map(), idx);
            // Record state for the next re-derive BEFORE any allocation below.
            js_object_set_field(iter_obj(), 1, JSValue::number((idx + 1) as f64));
            js_object_set_field(iter_obj(), 4, JSValue::from_bits(entry_key.to_bits()));

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
            emit_iter_result(&scope, &iter_h, emit_cached, value, false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj() as i64),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on a Set iterator object.
pub unsafe fn dispatch_set_iterator_method(iter_obj: *mut ObjectHeader, method_name: &str) -> f64 {
    dispatch_set_iterator_method_emit(iter_obj, method_name, false, true)
}

/// Builtin advance only — see [`dispatch_map_iterator_method_builtin`].
pub(crate) unsafe fn dispatch_set_iterator_method_builtin(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    dispatch_set_iterator_method_emit(iter_obj, method_name, false, false)
}

unsafe fn dispatch_set_iterator_method_emit(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
    emit_cached: bool,
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
                return emit_iter_result(&scope, &iter_h, emit_cached, JSValue::undefined(), true);
            }
            let cursor = f64::from_bits(js_object_get_field(iter_obj(), 1).bits()) as u32;
            let last_val = js_object_get_field(iter_obj(), 4);
            let used = crate::set::set_used_entries(set());
            let in_place = cursor > 0 && {
                let prev = crate::set::set_value_raw(set(), cursor - 1);
                crate::value::js_jsvalue_same_value_zero(prev, f64::from_bits(last_val.bits())) != 0
            };
            let mut idx = next_read_index(cursor, in_place, || {
                crate::set::find_value_index(set(), f64::from_bits(last_val.bits()))
            });
            // Tombstoned deletes leave holes in the raw order; step over them.
            while idx < used
                && crate::set::set_value_raw(set(), idx).to_bits()
                    == crate::set::SET_HOLE_VALUE_BITS
            {
                idx += 1;
            }
            if idx >= used {
                js_object_set_field(iter_obj(), 1, JSValue::number(used as f64));
                js_object_set_field(iter_obj(), 0, JSValue::undefined());
                return emit_iter_result(&scope, &iter_h, emit_cached, JSValue::undefined(), true);
            }

            let elem = crate::set::set_value_raw(set(), idx);
            js_object_set_field(iter_obj(), 1, JSValue::number((idx + 1) as f64));
            js_object_set_field(iter_obj(), 4, JSValue::from_bits(elem.to_bits()));

            let value = match kind {
                // For Sets, keys === values; entries yields [v, v] pairs.
                KIND_ENTRIES => JSValue::from_bits(make_pair_array(elem, elem).to_bits()),
                _ => JSValue::from_bits(elem.to_bits()),
            };
            emit_iter_result(&scope, &iter_h, emit_cached, value, false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj() as i64),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Emit a `{value, done}` iterator result.
///
/// `emit_cached == false` (every manual `.next()` and both public
/// dispatchers) allocates a fresh object per call, exactly as before —
/// results a caller retains behave per spec.
///
/// `emit_cached == true` is reserved for [`js_for_of_next`], whose only
/// caller is the compiler's `for…of` desugar. There the result local is a
/// compiler temporary the loop body cannot name, read for `done`/`value`
/// before the next advance — so mutating one cached object per ITERATOR is
/// unobservable, and it deletes the per-element allocation that dominated
/// generic iteration. The cache lives in the iterator object's field 5, so
/// the GC traces and rewrites it like any other field.
unsafe fn emit_iter_result(
    scope: &crate::gc::RuntimeHandleScope,
    iter_h: &crate::gc::RuntimeHandle,
    emit_cached: bool,
    value: JSValue,
    done: bool,
) -> f64 {
    let iter_obj = || js_nanbox_get_pointer(iter_h.get_nanbox_f64()) as *mut ObjectHeader;
    if !emit_cached {
        return make_iter_result(value, done);
    }
    let cached = js_object_get_field(iter_obj(), 5);
    if JSValue::from_bits(cached.bits()).is_pointer() {
        let res = js_nanbox_get_pointer(f64::from_bits(cached.bits())) as *mut ObjectHeader;
        // Barriered field stores: the iterator (and its cached result) may be
        // tenured while `value` is young.
        js_object_set_field(res, 0, value);
        js_object_set_field(res, 1, JSValue::bool(done));
        return js_nanbox_pointer(res as i64);
    }
    // First fused advance on this iterator: build the result once and cache
    // it. `make_iter_result` allocates, so root `value` across it.
    let value_h = scope.root_nanbox_u64(value.bits());
    let res = make_iter_result(JSValue::from_bits(value_h.get_nanbox_u64()), done);
    let res_h = scope.root_nanbox_f64(res);
    js_object_set_field(
        iter_obj(),
        5,
        JSValue::from_bits(res_h.get_nanbox_f64().to_bits()),
    );
    res_h.get_nanbox_f64()
}

/// One fused `IteratorNext` for the `for…of` desugar: advance + result in a
/// single runtime call.
///
/// A builtin Map/Set iterator advances in place and reuses its cached result
/// object (see [`emit_iter_result`]); the override probe inside the
/// dispatcher still runs first, so a patched `next` wins exactly as it does
/// on the manual path. Every other receiver — array iterators, generators,
/// user iterators — takes the arm at the bottom, which is byte-for-byte the
/// two-call desugar this entry replaces: the dynamic `.next()` dispatch
/// followed by spec IteratorNext result validation.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_for_of_next(iter: f64) -> f64 {
    let jv = JSValue::from_bits(iter.to_bits());
    if jv.is_pointer() {
        let raw = js_nanbox_get_pointer(iter) as usize;
        if raw != 0 && !crate::value::addr_class::is_small_handle(raw) {
            if let Some(header) = crate::value::addr_class::try_read_gc_header(raw) {
                if header.obj_type == crate::gc::GC_TYPE_OBJECT {
                    let obj = raw as *mut ObjectHeader;
                    let class_id = (*obj).class_id;
                    // Spec IteratorNext validation applies on the fused arms
                    // too: a builtin advance always returns an object, but a
                    // patched own `next` (#9019) can return anything, and
                    // `for…of` must throw the same TypeError the generic arm
                    // throws rather than hand the desugar a primitive.
                    if class_id == MAP_ITERATOR_CLASS_ID {
                        return crate::symbol::js_iterator_result_validate(
                            dispatch_map_iterator_method_emit(obj, "next", true, true),
                        );
                    }
                    if class_id == SET_ITERATOR_CLASS_ID {
                        return crate::symbol::js_iterator_result_validate(
                            dispatch_set_iterator_method_emit(obj, "next", true, true),
                        );
                    }
                }
            }
        }
    }
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
#[used]
static KEEP_JS_FOR_OF_NEXT: unsafe extern "C-unwind" fn(f64) -> f64 = js_for_of_next;

#[cfg(test)]
mod fused_for_of_tests {
    use super::*;

    unsafe fn value_of(res: f64) -> f64 {
        f64::from_bits(
            js_object_get_field(js_nanbox_get_pointer(res) as *mut ObjectHeader, 0).bits(),
        )
    }
    unsafe fn done_of(res: f64) -> bool {
        JSValue::from_bits(
            js_object_get_field(js_nanbox_get_pointer(res) as *mut ObjectHeader, 1).bits(),
        )
        .as_bool()
    }

    /// The fused driver must walk a Set in insertion order, terminate, and
    /// keep its recycled result in the iterator's field 5 — while the manual
    /// dispatcher keeps allocating fresh results a caller may retain.
    #[test]
    fn fused_next_walks_a_set_and_recycles_its_result() {
        unsafe {
            let set = crate::set::js_set_alloc(4);
            for v in [10.0f64, 20.0, 30.0] {
                crate::set::js_set_add(set, v);
            }
            let iter = js_nanbox_pointer(js_set_values_iter_obj(set));

            let r1 = js_for_of_next(iter);
            assert_eq!(value_of(r1), 10.0);
            assert!(!done_of(r1));
            let cached = js_object_get_field(js_nanbox_get_pointer(iter) as *mut ObjectHeader, 5);
            assert!(
                JSValue::from_bits(cached.bits()).is_pointer(),
                "the first fused advance must install the recycled result"
            );
            assert_eq!(value_of(js_for_of_next(iter)), 20.0);
            assert_eq!(value_of(js_for_of_next(iter)), 30.0);
            assert!(done_of(js_for_of_next(iter)), "exhausted after three");
            assert!(done_of(js_for_of_next(iter)), "stays exhausted");

            // The manual path still returns fresh, independent results.
            let m1 = dispatch_set_iterator_method(
                js_nanbox_get_pointer(js_nanbox_pointer(js_set_values_iter_obj(set)))
                    as *mut ObjectHeader,
                "next",
            );
            assert_eq!(value_of(m1), 10.0);
        }
    }

    /// Mid-iteration delete: the cursor-repair contract (#6075) must hold on
    /// the fused path because it runs the SAME advance code as the manual one.
    #[test]
    fn fused_next_survives_a_mid_iteration_delete() {
        unsafe {
            let map = crate::map::js_map_alloc(8);
            for k in [1.0f64, 2.0, 3.0, 4.0] {
                crate::map::js_map_set(map, k, k * 10.0);
            }
            let iter = js_nanbox_pointer(js_map_keys_iter_obj(map));
            assert_eq!(value_of(js_for_of_next(iter)), 1.0);
            // Deleting an EARLIER entry shifts the survivors down; the fused
            // next must not skip or repeat.
            crate::map::js_map_delete(map, 1.0);
            assert_eq!(value_of(js_for_of_next(iter)), 2.0);
            assert_eq!(value_of(js_for_of_next(iter)), 3.0);
            assert_eq!(value_of(js_for_of_next(iter)), 4.0);
            assert!(done_of(js_for_of_next(iter)));
        }
    }

    /// A non-collection receiver takes the generic arm: dynamic `.next()`
    /// dispatch plus validation — here, an array VALUES iterator object.
    #[test]
    fn fused_next_routes_other_iterators_through_the_generic_arm() {
        unsafe {
            let arr = crate::array::js_array_alloc(2);
            crate::array::js_array_push_f64(arr, 7.0);
            crate::array::js_array_push_f64(arr, 8.0);
            let iter = crate::array::array_values_iter(js_nanbox_pointer(arr as i64));
            assert_eq!(value_of(js_for_of_next(iter)), 7.0);
            assert_eq!(value_of(js_for_of_next(iter)), 8.0);
            assert!(done_of(js_for_of_next(iter)));
        }
    }
}
