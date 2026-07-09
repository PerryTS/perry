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
    let obj = js_object_alloc(class_id, 5);
    // Field 0: backing collection (NaN-boxed pointer so the GC scanner keeps it).
    js_object_set_field(obj, 0, JSValue::from_bits(coll_nanboxed.to_bits()));
    // Field 1: cursor index (index just past the last-returned entry), starts at 0.
    js_object_set_field(obj, 1, JSValue::number(0.0));
    // Field 2: iterator kind.
    js_object_set_field(obj, 2, JSValue::number(kind as f64));
    // Field 3: collection size observed at the last `next()`. `-1` sentinel means
    // "not started" (no entry returned yet). Used to detect a mid-iteration
    // delete (which compacts the entries array, shifting live entries below the
    // cursor) so the cursor can be re-derived from the last key (#6075).
    js_object_set_field(obj, 3, JSValue::number(-1.0));
    // Field 4: the KEY of the last-returned entry (a Map key / Set value), used
    // to re-derive the cursor after a delete-shift. Undefined until started.
    js_object_set_field(obj, 4, JSValue::undefined());
    // Link `[[Prototype]]` to the shared `%MapIteratorPrototype%` /
    // `%SetIteratorPrototype%` singleton so `Object.getPrototypeOf(it)` and the
    // inherited `.next` read resolve.
    crate::object::attach_iterator_prototype(obj, class_id);
    js_nanbox_pointer(obj as i64)
}

/// Build a fresh Map iterator object for `map` (raw pointer) of the given
/// kind. Returns the RAW iterator-object pointer as i64 (caller NaN-boxes).
unsafe fn map_iter_obj_raw(map: *const MapHeader, kind: i32) -> i64 {
    if map.is_null() {
        return 0;
    }
    let nanboxed = alloc_iterator(MAP_ITERATOR_CLASS_ID, js_nanbox_pointer(map as i64), kind);
    js_nanbox_get_pointer(nanboxed)
}

unsafe fn set_iter_obj_raw(set: *const SetHeader, kind: i32) -> i64 {
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
#[used]
static KEEP_MAP_ENTRIES_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_entries_iter_obj;
#[used]
static KEEP_MAP_KEYS_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_keys_iter_obj;
#[used]
static KEEP_MAP_VALUES_ITER: extern "C" fn(*const MapHeader) -> i64 = js_map_values_iter_obj;
#[used]
static KEEP_SET_VALUES_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_values_iter_obj;
#[used]
static KEEP_SET_KEYS_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_keys_iter_obj;
#[used]
static KEEP_SET_ENTRIES_ITER: extern "C" fn(*const SetHeader) -> i64 = js_set_entries_iter_obj;

/// Build the `{ value, done }` iterator-result object. Mirrors
/// `array/iter_object.rs::make_iter_result`.
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

/// `[key, value]` pair array for Map entries / Set entries (`[v, v]`).
unsafe fn make_pair_array(a: f64, b: f64) -> f64 {
    let pair = crate::array::js_array_alloc(2);
    crate::array::store_array_slot(pair, 0, a.to_bits());
    crate::array::store_array_slot(pair, 1, b.to_bits());
    (*pair).length = 2;
    crate::array::rebuild_array_layout_exact(pair);
    js_nanbox_pointer(pair as i64)
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
    match method_name {
        "next" => {
            let backing = f64::from_bits(js_object_get_field(iter_obj, 0).bits());
            let map = js_nanbox_get_pointer(backing) as *const MapHeader;
            let kind = f64::from_bits(js_object_get_field(iter_obj, 2).bits()) as i32;
            if map.is_null() {
                return make_iter_result(JSValue::undefined(), true);
            }
            let cursor = f64::from_bits(js_object_get_field(iter_obj, 1).bits()) as u32;
            let last_key = js_object_get_field(iter_obj, 4);
            let size = crate::map::js_map_size(map);
            // Is the last-returned key still at cursor-1? (SameValueZero, so a
            // NaN key matches itself.) If so, no delete shifted an entry at/below
            // the cursor.
            let in_place = cursor > 0 && {
                let prev = crate::map::js_map_entry_key_at(map, cursor - 1);
                crate::value::js_jsvalue_same_value_zero(prev, f64::from_bits(last_key.bits())) != 0
            };
            let idx = next_read_index(cursor, in_place, || {
                crate::map::find_key_index(map, f64::from_bits(last_key.bits()))
            });
            if idx >= size {
                js_object_set_field(iter_obj, 1, JSValue::number(size as f64));
                return make_iter_result(JSValue::undefined(), true);
            }

            let entry_key = crate::map::js_map_entry_key_at(map, idx);
            // Record state for the next re-derive BEFORE any allocation below.
            js_object_set_field(iter_obj, 1, JSValue::number((idx + 1) as f64));
            js_object_set_field(iter_obj, 4, JSValue::from_bits(entry_key.to_bits()));

            let value = match kind {
                KIND_KEYS => JSValue::from_bits(entry_key.to_bits()),
                KIND_VALUES => {
                    JSValue::from_bits(crate::map::js_map_entry_value_at(map, idx).to_bits())
                }
                _ => {
                    let val = crate::map::js_map_entry_value_at(map, idx);
                    JSValue::from_bits(make_pair_array(entry_key, val).to_bits())
                }
            };
            make_iter_result(value, false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj as i64),
        "return" | "throw" => make_iter_result(JSValue::undefined(), true),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on a Set iterator object.
pub unsafe fn dispatch_set_iterator_method(iter_obj: *mut ObjectHeader, method_name: &str) -> f64 {
    match method_name {
        "next" => {
            let backing = f64::from_bits(js_object_get_field(iter_obj, 0).bits());
            let set = js_nanbox_get_pointer(backing) as *const SetHeader;
            let kind = f64::from_bits(js_object_get_field(iter_obj, 2).bits()) as i32;
            if set.is_null() {
                return make_iter_result(JSValue::undefined(), true);
            }
            let cursor = f64::from_bits(js_object_get_field(iter_obj, 1).bits()) as u32;
            let last_val = js_object_get_field(iter_obj, 4);
            let size = crate::set::js_set_size(set);
            let in_place = cursor > 0 && {
                let prev = crate::set::js_set_value_at(set, cursor - 1);
                crate::value::js_jsvalue_same_value_zero(prev, f64::from_bits(last_val.bits())) != 0
            };
            let idx = next_read_index(cursor, in_place, || {
                crate::set::find_value_index(set, f64::from_bits(last_val.bits()))
            });
            if idx >= size {
                js_object_set_field(iter_obj, 1, JSValue::number(size as f64));
                return make_iter_result(JSValue::undefined(), true);
            }

            let elem = crate::set::js_set_value_at(set, idx);
            js_object_set_field(iter_obj, 1, JSValue::number((idx + 1) as f64));
            js_object_set_field(iter_obj, 4, JSValue::from_bits(elem.to_bits()));

            let value = match kind {
                // For Sets, keys === values; entries yields [v, v] pairs.
                KIND_ENTRIES => JSValue::from_bits(make_pair_array(elem, elem).to_bits()),
                _ => JSValue::from_bits(elem.to_bits()),
            };
            make_iter_result(value, false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj as i64),
        "return" | "throw" => make_iter_result(JSValue::undefined(), true),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
