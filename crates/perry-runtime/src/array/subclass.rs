//! `class X extends Array` support — predicates + a dense-snapshot materializer.
//!
//! An Array subclass instance is a plain `ObjectHeader` (perry has no exotic
//! array-object representation), so its inherited `Array.prototype` methods run
//! through the spec-generic array-like engine in [`super::generic`], and
//! iteration / spread materialize a dense snapshot of its indexed elements.
//! Kept out of `generic.rs` so that module stays under the file-size gate.

use std::ptr;

use super::generic::{al_get, al_length, nanbox_arr};
use crate::array::{js_array_alloc_with_length, note_array_slot, ArrayHeader};
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// True when `class_id` is a user class that extends `Array` (the reserved
/// parent id `0xFFFF0024` appears in its class chain), i.e. `class X extends
/// Array`. Such instances are plain `ObjectHeader`s, so the array-like engines
/// must run on them (`x.push(1)`, `x.map(...)`) — they are otherwise excluded by
/// the "plain objects only" guard alongside ordinary user classes. Purely
/// additive: only newly admits Array subclasses, never changes plain-object or
/// ordinary-class-instance behavior.
pub(crate) fn is_array_subclass_class_id(class_id: u32) -> bool {
    const CLASS_ID_ARRAY: u32 = 0xFFFF0024;
    if class_id == 0 {
        return false;
    }
    let mut cur = class_id;
    // Bounded walk up the parent chain; guards against a corrupt cyclic edge.
    for _ in 0..64 {
        match crate::object::get_parent_class_id(cur) {
            Some(parent) if parent == CLASS_ID_ARRAY => return true,
            Some(parent) => cur = parent,
            None => return false,
        }
    }
    false
}

/// True when `object` is a live `class X extends Array` instance: a heap
/// `GC_TYPE_OBJECT` whose class id chains to the reserved `Array` parent id.
/// Used to route inherited *read* Array methods (`map` / `filter` / `join` /
/// `at` / `indexOf` / …) and iteration/spread over the subclass instance.
/// `try_read_gc_header` magnitude-classifies the address first, so a non-heap
/// handle id is never dereferenced as a `GcHeader`.
pub fn is_array_subclass_instance(object: f64) -> bool {
    let jsv = JSValue::from_bits(object.to_bits());
    if !jsv.is_pointer() {
        return false;
    }
    let raw = jsv.as_pointer::<u8>();
    if raw.is_null() || !crate::object::is_valid_obj_ptr(raw) {
        return false;
    }
    let obj_type = match unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) } {
        Some(hdr) => hdr.obj_type,
        None => return false,
    };
    if obj_type != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let class_id = crate::object::js_object_get_class_id(raw as *const ObjectHeader);
    is_array_subclass_class_id(class_id)
}

/// Materialize a `class X extends Array` instance into a fresh dense array by
/// reading its `length` + indexed elements through the array-like accessors.
/// Iteration (`for…of`, spread, `Array.from`, destructuring, `[].concat(sub)`)
/// drives the array iterator / spread, which read a real `ArrayHeader`; an
/// object-backed subclass instance would be misread, so those paths iterate
/// this snapshot instead. Snapshot (not live) semantics — a full fix would need
/// an object-backed array iterator. Absent indices materialize as `undefined`
/// (not preserved holes): correct for iteration/spread (the array iterator
/// yields `undefined` for holes anyway); a sparse subclass fed to `concat`
/// therefore yields `undefined` rather than a preserved hole — an accepted
/// limitation for this rare case.
pub fn array_subclass_dense_snapshot(recv: f64) -> f64 {
    let len = al_length(recv).max(0);
    // ArrayCreate throws a RangeError for len ≥ 2^32 (matching `js_arraylike_map`)
    // — and, critically, this guard prevents the `as u32` truncation below from
    // under-allocating the buffer while the `0..len` loop iterates the full i64
    // count and writes out of bounds.
    if len > u32::MAX as i64 {
        crate::array::array_length_range_error();
    }
    let result = js_array_alloc_with_length(len as u32);
    let elems = unsafe { (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for k in 0..len {
        let v = al_get(recv, k);
        unsafe {
            // GC_STORE_AUDIT(BARRIERED): note_array_slot re-stores with the barrier.
            ptr::write(elems.add(k as usize), v);
            note_array_slot(result, k as usize, v.to_bits());
        }
    }
    nanbox_arr(result)
}

/// True when an Array-subclass instance carries a USER `[Symbol.iterator]`
/// override — an own `inst[Symbol.iterator] = …` / symbol accessor, or a class
/// method `*[Symbol.iterator]()` (registered under the synthetic `@@iterator`
/// name). The default array iterator is a runtime default (not a class vtable
/// method), so a hit means the user declared their own. The snapshot iteration
/// shortcuts must defer to such an override and only synthesize the default
/// array iterator when none exists. Mirrors
/// `object::map_set_subclass::subclass_has_iterator_override`.
pub fn array_subclass_has_iterator_override(value: f64) -> bool {
    let iter_wk = crate::symbol::well_known_symbol("iterator");
    if iter_wk.is_null() {
        return false;
    }
    let iter_f64 = f64::from_bits(JSValue::pointer(iter_wk as *const u8).bits());
    if unsafe { crate::symbol::own_symbol_property(value, iter_f64) }.is_some() {
        return true;
    }
    let raw = value.to_bits() & 0x0000_FFFF_FFFF_FFFF;
    let class_id = crate::object::js_object_get_class_id(raw as *const ObjectHeader);
    class_id != 0 && crate::object::method_owner_class_id(class_id, "@@iterator").is_some()
}
