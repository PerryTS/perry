//! Canonical `{ value, done }` iterator-result construction (#7564).
//!
//! Every `.next()` that the runtime answers itself — array, string, typed
//! array/Buffer, Map/Set, and the `Iterator.from(...)` helper chain — hands
//! back a two-field object. Five copies of the constructor had drifted apart,
//! and all five allocated **five** heap objects to deliver one value:
//!
//! | allocation | information it carries |
//! |---|---|
//! | the result object | the value and the done flag |
//! | `"value"` string | none — same six bytes every call |
//! | `"done"` string | none — same four bytes every call |
//! | the 2-element keys array | none — same two pointers every call |
//! | the shape install keyed on that array's ADDRESS | none, and worse: a *new* shape id per call |
//!
//! Four of the five are the same constant on every call, so this module
//! builds them once per thread and shares them. That is sound because
//! `keys_array` sharing is already how object literals and `JSON.parse`
//! results work: the array is stamped [`GC_FLAG_SHAPE_SHARED`], and every
//! path that would mutate an object's key list — adding a property
//! (`field_set_by_name`), deleting one (`delete_rest`), or a proxy trap
//! (`proxy::put_value`) — clones before writing. Nothing can reach through
//! a result object and disturb the shared array.
//!
//! The shape install is the reason this is more than an allocation win.
//! `shape_id_for_keys_ensure` keys the shape table on the keys array's
//! ADDRESS, so a fresh array per `.next()` minted a fresh shape id per
//! `.next()` — every read of `.value`/`.done` off the result was a
//! guaranteed inline-cache miss, and the shape table grew without bound.
//! One shared array means one shape id for every iterator result in the
//! program.
//!
//! ## Rooting
//!
//! The old constructors were also a live stale-from-space defect. They ran
//! five allocations back to back with every intermediate in a bare Rust
//! local: a copying minor inside allocation *k* moved what locals 1..k-1
//! named and rewrote only slots it could see, which a Rust local is not.
//! `array/iter_object.rs`'s copy was rooted by #7475; the other four were
//! not, and `collection_iter_object.rs`'s was observed taking a bus error on
//! a retired from-space key string under `PERRY_GC_PROTECT_FROMSPACE=1`.
//!
//! This module removes the hazard structurally rather than by adding roots
//! to a five-allocation sequence: on the steady-state path there is exactly
//! **one** allocation (the result object), and both pointers used after it
//! are re-read from storage the collector rewrites — the object from its
//! [`RuntimeHandleScope`] handle, the keys array from the thread-local
//! `ITER_RESULT_KEYS` slot that [`scan_iter_result_keys_roots_mut`] visits.
//! No pre-collection address is nameable.
//!
//! `ITER_RESULT_KEYS` is exactly the "runtime-side cache of a raw heap
//! pointer" that the static `gc_root_dominance_check.py` cannot see, so its
//! scanner is registered in `gc/mod.rs` in the same commit — see the
//! "unrooted runtime caches" note in CLAUDE.md.

use crate::array::ArrayHeader;
use crate::gc::RuntimeRootVisitor;
use crate::object::ObjectHeader;
use crate::value::JSValue;
use std::cell::UnsafeCell;

/// Field/key order of the result object.
///
/// The language always says `{ value, done }`. `node:sqlite`'s statement
/// iterator is the one exception in the runtime: it builds `{ done, value }`,
/// and its key order is observable through `Object.keys` and
/// `JSON.stringify`, so it gets its own shared array rather than being
/// quietly normalized.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IterResultOrder {
    /// `{ value, done }`.
    ValueDone = 0,
    /// `{ done, value }` — `node:sqlite` only.
    DoneValue = 1,
}

const ORDER_COUNT: usize = 2;

thread_local! {
    /// Per-thread shared keys arrays, indexed by [`IterResultOrder`].
    ///
    /// Per-thread and not process-global for the same reason the intern
    /// table is: each `perry/thread` worker has its own arena, so a pointer
    /// interned from worker A's arena is a cross-arena read from worker B.
    ///
    /// GC-visible through [`scan_iter_result_keys_roots_mut`], which both
    /// MARKS (nothing else references these arrays) and REWRITES them (an
    /// evacuating collection moves them like anything else).
    static ITER_RESULT_KEYS: UnsafeCell<[*mut ArrayHeader; ORDER_COUNT]> =
        const { UnsafeCell::new([std::ptr::null_mut(); ORDER_COUNT]) };
}

#[inline(always)]
fn cached_keys(order: IterResultOrder) -> *mut ArrayHeader {
    ITER_RESULT_KEYS.with(|c| unsafe { (*c.get())[order as usize] })
}

/// NaN-boxed bits of an interned constant property name.
#[inline]
fn interned_key_bits(bytes: &[u8]) -> u64 {
    JSValue::string_ptr(crate::string::intern_ascii_literal(bytes) as *mut _).bits()
}

/// Build this thread's shared keys array for `order`, if it does not have one.
///
/// Cold and at most twice per thread for the program's lifetime, so it is
/// written for obviousness rather than speed: every intermediate is rooted
/// across every allocation that follows it.
#[cold]
unsafe fn build_shared_keys(order: IterResultOrder) {
    let (first, second): (&[u8], &[u8]) = match order {
        IterResultOrder::ValueDone => (b"value", b"done"),
        IterResultOrder::DoneValue => (b"done", b"value"),
    };

    let scope = crate::gc::RuntimeHandleScope::new();
    let keys_h = scope.root_raw_mut_ptr(crate::array::js_array_alloc(2));

    // Interning makes these pointer-identical to the `"value"` / `"done"` the
    // READ side (`result.value`) hashes — the property-plan cache's verdicts
    // are pointer-identity based — and gives them a second, independent root
    // in the intern table.
    //
    // Both interns ALLOCATE on a first-call-per-thread miss, so the array's
    // address is taken back out of its handle ACROSS them rather than bound
    // before them. `first_h` is a handle, so it is rewritten by the same
    // collection that would move the array.
    let first_h = scope.root_nanbox_u64(interned_key_bits(first));
    let (second_bits, keys) = keys_h.across_mut::<ArrayHeader, _>(|| interned_key_bits(second));
    let second_h = scope.root_nanbox_u64(second_bits);

    (*keys).length = 2;
    crate::array::store_array_slot(keys, 0, first_h.get_nanbox_u64());
    crate::array::store_array_slot(keys, 1, second_h.get_nanbox_u64());
    crate::array::rebuild_array_layout_exact(keys);

    // Copy-on-write marker. Without it, `result.extra = 1` on ONE result
    // object would append to the array every other result shares.
    let gc_header = (keys as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    (*gc_header).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;

    ITER_RESULT_KEYS.with(|c| (*c.get())[order as usize] = keys);
    crate::gc::runtime_write_barrier_root_raw_ptr(keys);
}

/// Build one iterator-result object.
///
/// `first`/`second` are the two field VALUES in `order`'s declaration order —
/// callers should prefer [`make_iter_result`], which names them.
pub(crate) unsafe fn build_iter_result_ordered(
    first: JSValue,
    second: JSValue,
    order: IterResultOrder,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    // Both field values are caller-supplied and can be heap pointers; the
    // object allocation below can collect and move them.
    let first_h = scope.root_nanbox_u64(first.bits());
    let second_h = scope.root_nanbox_u64(second.bits());

    // Fill the keys cache BEFORE the result object exists, so its cold
    // allocations cannot invalidate a pointer we are already holding.
    if cached_keys(order).is_null() {
        build_shared_keys(order);
    }

    let obj_h = scope.root_nanbox_f64(crate::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 2) as i64,
    ));

    // Everything below re-reads through storage the collector rewrites: the
    // object from its handle, the keys array from the scanned thread-local.
    // Nothing here names an address that predates the allocation above.
    let obj = || crate::js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *mut ObjectHeader;
    crate::object::js_object_set_keys(obj(), cached_keys(order));
    crate::object::js_object_set_field(obj(), 0, JSValue::from_bits(first_h.get_nanbox_u64()));
    crate::object::js_object_set_field(obj(), 1, JSValue::from_bits(second_h.get_nanbox_u64()));
    crate::js_nanbox_pointer(obj() as i64)
}

/// The `{ value, done }` result every iterator in the language returns.
#[inline]
pub(crate) unsafe fn make_iter_result(value: JSValue, done: bool) -> f64 {
    build_iter_result_ordered(value, JSValue::bool(done), IterResultOrder::ValueDone)
}

/// `node:sqlite`'s `{ done, value }` result.
#[inline]
pub(crate) unsafe fn make_sqlite_iter_result(value: JSValue, done: bool) -> f64 {
    build_iter_result_ordered(JSValue::bool(done), value, IterResultOrder::DoneValue)
}

/// GC root scanner for the shared keys arrays.
///
/// MARKING: nothing else in the heap references these arrays — the result
/// objects that use them are short-lived and the cache outlives them — so
/// without this they would be swept and every later `.next()` would install
/// a freed keys array.
///
/// REWRITING: an evacuating collection moves them like any other array, and
/// the thread-local slot is the only place the new address can be recorded.
pub fn scan_iter_result_keys_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    ITER_RESULT_KEYS.with(|c| unsafe {
        for slot in (*c.get()).iter_mut() {
            visitor.visit_raw_mut_ptr_slot(slot);
        }
    });
}

/// Legacy mark-only shim, mirroring the other runtime root scanners.
pub fn scan_iter_result_keys_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_iter_result_keys_roots_mut(&mut visitor);
}

/// Drop the cached arrays. The unit-test harness resets arenas between tests
/// while thread-locals persist, which would leave these pointing into a
/// deallocated block.
#[cfg(test)]
pub(crate) fn reset_shared_keys_for_test() {
    ITER_RESULT_KEYS.with(|c| unsafe {
        (*c.get()) = [std::ptr::null_mut(); ORDER_COUNT];
    });
}

/// Read a cached slot without building it.
#[cfg(test)]
pub(crate) fn shared_keys_peek_for_test(order: IterResultOrder) -> *mut ArrayHeader {
    cached_keys(order)
}

/// Force both slots to be built, and return them in `IterResultOrder` order.
#[cfg(test)]
pub(crate) fn populate_shared_keys_for_test() -> [*mut ArrayHeader; ORDER_COUNT] {
    for order in [IterResultOrder::ValueDone, IterResultOrder::DoneValue] {
        if cached_keys(order).is_null() {
            unsafe { build_shared_keys(order) };
        }
    }
    ITER_RESULT_KEYS.with(|c| unsafe { *c.get() })
}
