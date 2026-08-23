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

// ---------------------------------------------------------------------------
// #7574 — raw `js_array_*` receiver resolution for an array-like OBJECT.
//
// Codegen decides "this receiver is an Array" from the DECLARED TypeScript type
// of the binding (`is_array_expr` / `Type::Array(_)` / `Generic { base: "Array" }`),
// then emits a raw `js_array_*` call whose first act is to dereference the
// receiver as an `ArrayHeader`. A declared type is a hint, never a layout fact
// (CLAUDE.md, *Known Limitations*: annotations are erased and nothing validates
// them at runtime), so any binding annotated with the BASE type — `const a:
// number[] = new MyArr()`, a parameter, a class field, a return type, an
// `as number[]` cast — can be holding a `class X extends Array` instance, which
// perry models as a plain `ObjectHeader`. The two headers overlay field for
// field:
//
//     ArrayHeader.length   (u32 @0)  <- ObjectHeader.class_id        (#8113)
//     ArrayHeader.capacity (u32 @4)  <- ObjectHeader.parent_class_id  (ShapeId)
//     elements[0]          (@8)      <- keys_array   (a *mut ArrayHeader)
//     elements[1]          (@16)     <- meta         (a *mut ObjectMeta)
//     elements[2]          (@24)     <- inline field slot 0
//
// so element WRITES overwrite two live GC child edges with arbitrary doubles —
// the collector then traces whatever the mutator stored. `a.push(1); a.push(2)`
// SIGSEGVs (exit 139) on the second push.
//
// `clean_arr_ptr` now refuses a `GC_TYPE_OBJECT` allocation outright, which
// makes every one of its ~190 call sites fail-CLOSED. That is the memory-safety
// half. The correctness half is these helpers: the entry points reachable from
// the declared-type codegen tiers re-enter through their EXISTING null branch
// and run the operation on the spec-generic array-like engine
// (`super::generic` / `super::generic_object`), which already models an Array
// subclass correctly — it is the same engine the UNANNOTATED path has always
// used via `js_native_call_method`.
//
// Unlike #7573's Map/Set fix there is nothing to *redirect* to: an Array
// subclass instance has no hidden backing collection (`js_array_subclass_init`
// installs a `length` own property and the elements are ordinary indexed object
// properties — see `node_stream_constructors/builders.rs`). Minting one would
// split element storage in two, since `Object.keys` / `for…in` /
// `JSON.stringify` / the generic engine all read the object's own properties;
// the answer here is therefore "run the generic engine", not "redirect".
// ---------------------------------------------------------------------------

/// The array-like OBJECT receiver a raw `js_array_*` entry point must actually
/// run on, or `None` when the pointer is not one.
///
/// Admits exactly what [`super::generic::plain_object_value`] admits — an
/// object literal, an anonymous shape, or a `class X extends Array` instance —
/// so ordinary user-class instances, real arrays, typed arrays, buffers, and
/// proxies all answer `None` and keep their existing behaviour.
///
/// Marked `#[cold]`/`#[inline(never)]`: every caller reaches it only from a
/// branch `clean_arr_ptr` already refused, so a genuine `ArrayHeader` never
/// executes a byte of this.
/// One-load brand pre-filter: true only when `arr` has a readable `GcHeader`
/// saying `GC_TYPE_OBJECT`. A genuine `ArrayHeader` answers false without
/// touching a side table, so callers can gate the (registry-probing)
/// [`array_object_receiver`] behind it on a hot path. Uses
/// `addr_class::try_read_gc_header`, which magnitude-classifies the address
/// before any dereference.
#[inline]
pub(crate) fn raw_receiver_is_heap_object(arr: *const ArrayHeader) -> bool {
    let raw = ((arr as u64) & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw == 0 {
        return false;
    }
    match unsafe { crate::value::addr_class::try_read_gc_header(raw) } {
        Some(header) => header.obj_type == crate::gc::GC_TYPE_OBJECT,
        None => false,
    }
}

#[cold]
#[inline(never)]
pub(crate) fn array_object_receiver(arr: *const ArrayHeader) -> Option<f64> {
    let raw = (arr as u64) & 0x0000_FFFF_FFFF_FFFF;
    if raw == 0 {
        return None;
    }
    super::generic::plain_object_value(raw as *const ArrayHeader)
}

/// True when `value` is a live `class X extends Array` INSTANCE — the
/// annotation-independent brand test the generic `[[Set]]` funnels use to
/// decide whether the Array-exotic `length` steps apply.
pub(crate) fn is_array_subclass_value(value: f64) -> bool {
    if !JSValue::from_bits(value.to_bits()).is_pointer() {
        return false;
    }
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
    // `raw_receiver_is_heap_object` magnitude-classifies through
    // `addr_class::try_read_gc_header` and proves `GC_TYPE_OBJECT` before the
    // class-id read below dereferences the header.
    if !raw_receiver_is_heap_object(raw as *const ArrayHeader) {
        return false;
    }
    let class_id = crate::object::js_object_get_class_id(raw);
    class_id != 0 && is_array_subclass_class_id(class_id)
}

/// Run an `Array.prototype` method generically on the array-like object
/// `recv`, covering both the mutating family (`push` / `pop` / `shift` /
/// `unshift` / `reverse` / `splice` / `sort` / `concat`) and the read family
/// (`map` / `filter` / `forEach` / `join` / `slice` / `indexOf` / …).
///
/// Returns `None` only for a method name neither engine implements.
#[cold]
#[inline(never)]
pub(crate) fn array_object_method(recv: f64, method: &str, args: &[f64]) -> Option<f64> {
    let (ptr, len) = (args.as_ptr(), args.len());
    if let Some(result) = super::generic::run_object_mutator(recv, method, ptr, len) {
        return Some(result);
    }
    super::generic::dispatch_arraylike_read_method(recv, method, ptr, len)
}

/// `Get(recv, ToString(index))` for an array-like object receiver.
#[cold]
#[inline(never)]
pub(crate) fn array_object_index_get(recv: f64, index: u32) -> f64 {
    al_get(recv, index as i64)
}

/// `Set(recv, ToString(index), value, …)` PLUS the Array-exotic `length`
/// maintenance the receiver's class inherits from `Array`.
///
/// A `class X extends Array` instance is a real Array in JavaScript, so
/// `sub[3] = v` sets `length` to 4 (ECMA-262 §10.4.2.1
/// `ArraySetLength`/`ArrayDefineOwnProperty`). Perry models the instance as a
/// plain object, whose `[[DefineOwnProperty]]` has no such step — pre-fix
/// `sub[0] = 10; sub.length` read back `0`, on the ANNOTATED and unannotated
/// paths alike. Emulate the exotic step here so both agree with node.
#[cold]
#[inline(never)]
pub(crate) fn array_object_index_set(recv: f64, index: u32, value: f64) {
    // The store interns a key string and can allocate, so root the receiver
    // across it — it is a movable `ObjectHeader` and is read again below.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    crate::object::js_object_set_index_polymorphic(
        (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64,
        index as f64,
        value,
    );
    maintain_array_exotic_length(handle.get_nanbox_f64(), index);
}

/// The Array-exotic `length` step for an indexed own-property write, applied by
/// the two generic OBJECT store funnels (`js_put_value_set` and
/// `js_object_set_index_polymorphic`) AFTER the store has landed.
///
/// `sub[3] = v` on a `class X extends Array` instance must leave `length == 4`.
/// Perry models the instance as a plain object, so nothing in its
/// `[[DefineOwnProperty]]` does that — pre-fix `sub[0] = 10; sub.length` read
/// back `0` on the annotated AND unannotated paths alike, which then made the
/// next `sub.push(v)` append at index 0 and overwrite the element.
///
/// Gated on the receiver's class chain reaching `Array`, so an object literal
/// (`class_id == 0`) short-circuits on one load and an ordinary class instance
/// on a bounded parent walk. `key` is a property-key VALUE; a non-canonical
/// array index (`"length"`, `"foo"`, `"01"`, a symbol) is a no-op.
pub(crate) fn note_array_subclass_index_write(recv: f64, key: f64) {
    if !is_array_subclass_value(recv) {
        return;
    }
    let key_ptr = crate::value::js_jsvalue_to_string(key) as *const crate::string::StringHeader;
    // The `&str` borrows the heap `StringHeader`'s bytes. `canonical_array_index`
    // only parses digits — it allocates nothing, so the borrow cannot straddle a
    // collection point (the `&[u8]`-into-a-StringHeader hazard in CLAUDE.md).
    let index = unsafe {
        match crate::object::has_own_helpers::str_from_string_header(key_ptr)
            .and_then(crate::object::canonical_array_index)
        {
            Some(i) => i,
            None => return,
        }
    };
    maintain_array_exotic_length(recv, index);
}

/// The `length`-bumping half of `array_object_index_set`, split out so the
/// generic OBJECT index-store funnels can apply it without re-entering the
/// store.
pub(crate) fn maintain_array_exotic_length(recv: f64, index: u32) {
    let current = al_length(recv);
    if (index as i64) < current {
        return;
    }
    // `js_string_from_bytes` ALLOCATES, so it is a collection point: root the
    // receiver and re-read it afterwards rather than deriving the raw pointer
    // first (the #7192 store-after-an-allocating-call shape — a movable
    // `ObjectHeader` written through a pre-allocation address lands on a
    // forwarding stub).
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    crate::object::js_object_set_field_by_name(raw, key, (index as f64) + 1.0);
}

/// `Set(recv, "length", new_length, true)` for an array-like object receiver:
/// truncating deletes the indices at or above the new length, exactly as the
/// Array-exotic `[[DefineOwnProperty]]` would.
#[cold]
#[inline(never)]
pub(crate) fn array_object_set_length(recv: f64, new_length: f64) {
    if !new_length.is_finite() || new_length < 0.0 || new_length.trunc() != new_length {
        crate::array::array_length_range_error();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_nanbox_f64(recv);
    let target = new_length as i64;
    let current = al_length(handle.get_nanbox_f64());
    for k in target..current {
        let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
        crate::object::js_object_delete_dynamic(raw, k as f64);
    }
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let raw = (handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
    crate::object::set_field_by_name_object_tail(raw, key, new_length);
}
