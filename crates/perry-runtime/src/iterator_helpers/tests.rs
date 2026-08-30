//! #7576 acceptance: the TC39 iterator-helpers surface, exercised through the
//! same dispatch tower a compiled program reaches (`js_native_call_method`),
//! not by calling [`dispatch_iterator_helper_method`] directly.
//!
//! Going through the tower is the point. The bug was a class-id COLLISION —
//! `ITERATOR_HELPER_CLASS_ID` and `STRING_ITERATOR_CLASS_ID` were both
//! `0xFFFF_0009`, and the tower's String arm is tested first — so a test that
//! called the helper dispatcher directly passed while every real program got
//! an empty iterator. Any test here that stops short of the tower would have
//! been green throughout the outage.
//!
//! These live as `cargo test -p perry-runtime` unit tests rather than under
//! `crates/*/tests/` because the integration suites only run nightly/tag
//! (CLAUDE.md #5960). The user-visible byte-for-byte parity half is
//! `test-files/test_gap_iterator_helpers_{2874,7576}.ts` — the first was
//! already in `test-parity/known_failures.json` for the whole outage, which is
//! why the outage lasted; it is un-skipped by this change.

use super::{ITERATOR_HELPER_CLASS_ID, OP_DROP, OP_FILTER, OP_FLATMAP, OP_MAP, OP_TAKE};
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// A NaN-boxed array of the given numbers.
unsafe fn number_array(values: &[f64]) -> f64 {
    let mut a = crate::array::js_array_alloc(values.len().max(1) as u32);
    for v in values {
        a = crate::array::js_array_push_f64(a, *v);
    }
    crate::value::js_nanbox_pointer(a as i64)
}

/// Call `recv.method(args)` through the production dispatch tower.
unsafe fn tower(recv: f64, method: &str, args: &[f64]) -> f64 {
    crate::object::js_native_call_method(
        recv,
        method.as_ptr() as *const i8,
        method.len(),
        if args.is_empty() {
            std::ptr::null()
        } else {
            args.as_ptr()
        },
        args.len(),
    )
}

/// The `(value, done)` pair of an iterator-result object.
unsafe fn step(result: f64) -> (JSValue, bool) {
    let p = crate::value::js_nanbox_get_pointer(result);
    assert_ne!(
        p,
        0,
        "`.next()` must return an iterator-result OBJECT, got bits {:#018x}",
        result.to_bits()
    );
    let obj = p as *mut ObjectHeader;
    let value = crate::object::js_object_get_field(obj, 0);
    let done = crate::object::js_object_get_field(obj, 1);
    (
        value,
        crate::value::js_is_truthy(f64::from_bits(done.bits())) != 0,
    )
}

/// Drive `iter.next()` through the tower until done, collecting the numbers.
unsafe fn drain(iter: f64) -> Vec<f64> {
    let mut out = Vec::new();
    for _ in 0..64 {
        let (value, done) = step(tower(iter, "next", &[]));
        if done {
            return out;
        }
        out.push(f64::from_bits(value.bits()));
    }
    panic!("iterator did not finish in 64 steps");
}

/// The numbers in a NaN-boxed array value.
unsafe fn array_numbers(value: f64) -> Vec<f64> {
    let p = crate::value::js_nanbox_get_pointer(value);
    assert_ne!(
        p,
        0,
        "expected an ARRAY, got bits {:#018x}",
        value.to_bits()
    );
    let arr = p as *mut ArrayHeader;
    (0..crate::array::js_array_length(arr))
        .map(|i| crate::array::js_array_get_f64(arr, i))
        .collect()
}

/// `Iterator.from(iterable)` — the object every case below starts from.
unsafe fn helper_over(values: &[f64]) -> f64 {
    let h = super::js_iterator_from(number_array(values));
    let p = crate::value::js_nanbox_get_pointer(h);
    assert_ne!(p, 0, "`Iterator.from` must return an object");
    assert_eq!(
        (*(p as *const ObjectHeader)).class_id,
        ITERATOR_HELPER_CLASS_ID,
        "`Iterator.from` must return an iterator-helper object"
    );
    h
}

/// The closure the combinator tests map with. Perry closures are invoked with
/// the argument in the first `f64` slot; `js_closure_alloc` + a registered
/// arity is the runtime-side equivalent of a compiled arrow function.
extern "C" fn double_it(_c: *const crate::closure::ClosureHeader, x: f64) -> f64 {
    f64::from_bits(JSValue::number(f64::from_bits(x.to_bits()) * 2.0).bits())
}

extern "C" fn is_even(_c: *const crate::closure::ClosureHeader, x: f64) -> f64 {
    let v = f64::from_bits(x.to_bits());
    f64::from_bits(if v as i64 % 2 == 0 {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// `(x) => [x, x * 10]`, for `flatMap`.
extern "C" fn pair_with_ten_times(_c: *const crate::closure::ClosureHeader, x: f64) -> f64 {
    let v = f64::from_bits(x.to_bits());
    unsafe { number_array(&[v, v * 10.0]) }
}

/// `(acc, v) => acc + v`, for `reduce`.
extern "C" fn add(_c: *const crate::closure::ClosureHeader, a: f64, b: f64) -> f64 {
    let sum = f64::from_bits(a.to_bits()) + f64::from_bits(b.to_bits());
    f64::from_bits(JSValue::number(sum).bits())
}

unsafe fn closure1(f: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64) -> f64 {
    let p = f as *const u8;
    crate::closure::js_register_closure_arity(p, 1);
    crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(p, 0) as i64)
}

unsafe fn closure2(f: extern "C" fn(*const crate::closure::ClosureHeader, f64, f64) -> f64) -> f64 {
    let p = f as *const u8;
    crate::closure::js_register_closure_arity(p, 2);
    crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(p, 0) as i64)
}

// ---------------------------------------------------------------------------
// The collision itself.
// ---------------------------------------------------------------------------

/// #7576: every runtime-defined iterator family must own a DISTINCT class id.
///
/// This is the test that would have caught the outage at the moment it was
/// introduced. The helper id was copied from the comment above the String
/// iterator's ("sits just past the Set iterator id"), landing on the identical
/// value; because the towers test String first, the collision presented as a
/// silently empty iterator rather than as anything resembling a duplicate
/// constant.
///
/// SABOTAGE CHECK: set `ITERATOR_HELPER_CLASS_ID` back to `0xFFFF_0009` and
/// this fails on the `iterator-helper` / `string` pair.
#[test]
fn iterator_class_ids_are_pairwise_distinct() {
    let ids: [(&str, u32); 7] = [
        ("buffer", crate::buffer::BUFFER_ITERATOR_CLASS_ID),
        ("array", crate::array::ARRAY_ITERATOR_CLASS_ID),
        ("map", crate::collection_iter_object::MAP_ITERATOR_CLASS_ID),
        ("set", crate::collection_iter_object::SET_ITERATOR_CLASS_ID),
        ("string", crate::string::STRING_ITERATOR_CLASS_ID),
        (
            "regexp-string",
            crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID,
        ),
        ("iterator-helper", ITERATOR_HELPER_CLASS_ID),
    ];
    for (i, (a_name, a_id)) in ids.iter().enumerate() {
        for (b_name, b_id) in &ids[i + 1..] {
            assert_ne!(
                a_id, b_id,
                "class-id collision: `{a_name}` and `{b_name}` are both {a_id:#010x}. \
                 Every dispatch tower matches these in a fixed order, so the LATER \
                 arm becomes unreachable and its whole method surface dies silently."
            );
        }
    }
}

/// The op-kind discriminants live in field 1 of the helper object, which is
/// exactly the slot the String iterator's dispatcher read as a cursor index.
/// Pin them so a future renumbering is a deliberate act.
#[test]
fn op_kinds_are_distinct() {
    let ops = [
        super::OP_IDENTITY,
        OP_MAP,
        OP_FILTER,
        OP_TAKE,
        OP_DROP,
        OP_FLATMAP,
    ];
    for (i, a) in ops.iter().enumerate() {
        for b in &ops[i + 1..] {
            assert_ne!(a, b, "duplicate iterator-helper op kind {a}");
        }
    }
}

// ---------------------------------------------------------------------------
// The surface, all of it, through the tower.
// ---------------------------------------------------------------------------

#[test]
fn iterator_from_yields_the_source_sequence() {
    unsafe {
        assert_eq!(drain(helper_over(&[1.0, 2.0, 3.0])), vec![1.0, 2.0, 3.0]);
    }
}

#[test]
fn iterator_from_on_an_existing_helper_returns_it_unchanged() {
    unsafe {
        let h = helper_over(&[1.0, 2.0]);
        assert_eq!(
            super::js_iterator_from(h).to_bits(),
            h.to_bits(),
            "`Iterator.from` on something already inheriting Iterator.prototype \
             must return it unchanged"
        );
    }
}

#[test]
fn helper_inherits_a_callable_next_from_its_shared_prototype() {
    unsafe {
        let h = helper_over(&[1.0]);
        let obj = crate::value::js_nanbox_get_pointer(h) as *const ObjectHeader;
        let expected = crate::object::iterator_prototype_for_class_id(ITERATOR_HELPER_CLASS_ID)
            .expect("iterator-helper class id must have a prototype");
        assert_eq!(
            crate::object::prototype_chain::object_static_prototype(obj as usize),
            Some(expected.to_bits()),
            "a helper allocation must attach the helper-family prototype"
        );
        assert_eq!(
            crate::object::js_object_get_prototype_of(h).to_bits(),
            expected.to_bits(),
            "Object.getPrototypeOf(helper) must return the helper-family prototype"
        );

        let next_key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
        let next = crate::object::js_object_get_field_by_name(obj, next_key);
        let next_ptr = crate::value::js_nanbox_get_pointer(f64::from_bits(next.bits()));
        assert!(
            crate::closure::is_closure_ptr(next_ptr as usize),
            "helper.next must resolve to a callable inherited method"
        );
    }
}

#[test]
fn helper_created_from_a_raw_iterator_inherits_the_same_next() {
    unsafe {
        let array = number_array(&[1.0]);
        let array_iter = crate::array::array_values_iter(array);
        let helper = tower(array_iter, "map", &[closure1(double_it)]);
        let obj = crate::value::js_nanbox_get_pointer(helper) as *const ObjectHeader;
        assert_eq!((*obj).class_id, ITERATOR_HELPER_CLASS_ID);

        let expected = crate::object::iterator_prototype_for_class_id(ITERATOR_HELPER_CLASS_ID)
            .expect("iterator-helper class id must have a prototype");
        assert_eq!(
            crate::object::prototype_chain::object_static_prototype(obj as usize),
            Some(expected.to_bits()),
            "raw-iterator helper dispatch must preserve the helper prototype"
        );
        let next_key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
        let next = crate::object::js_object_get_field_by_name(obj, next_key);
        let next_ptr = crate::value::js_nanbox_get_pointer(f64::from_bits(next.bits()));
        assert!(crate::closure::is_closure_ptr(next_ptr as usize));
    }
}

#[test]
fn helper_next_rejects_a_non_helper_iterator_receiver() {
    unsafe {
        let array_iter = crate::array::array_values_iter(number_array(&[9.0]));
        let scope = crate::gc::RuntimeHandleScope::new();
        let array_iter_h = scope.root_nanbox_f64(array_iter);

        let helper = helper_over(&[1.0]);
        let helper_obj = crate::value::js_nanbox_get_pointer(helper) as *const ObjectHeader;
        let next_key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
        let next = crate::object::js_object_get_field_by_name(helper_obj, next_key);
        let next_h = scope.root_nanbox_f64(f64::from_bits(next.bits()));

        // This is the runtime equivalent of `helper.next.call(arrayIter)`. The
        // canonical helper method must enforce the Iterator Helper brand; it
        // cannot fall through to the array iterator's own `next` algorithm.
        let rebound = crate::closure::clone_closure_rebind_this(
            next_h.get_nanbox_u64(),
            array_iter_h.get_nanbox_f64(),
        );
        let rebound_h = scope.root_nanbox_f64(f64::from_bits(rebound));
        let result = crate::exception::js_call_catching(|| {
            crate::closure::js_native_call_value(rebound_h.get_nanbox_f64(), std::ptr::null(), 0)
        });
        assert!(
            result.is_err(),
            "%Iterator Helper Prototype%.next must throw on an array iterator receiver"
        );
    }
}

#[test]
fn map_returns_a_helper_and_transforms_lazily() {
    unsafe {
        let m = tower(helper_over(&[1.0, 2.0, 3.0]), "map", &[closure1(double_it)]);
        let p = crate::value::js_nanbox_get_pointer(m);
        assert_ne!(p, 0, "`.map()` must return a helper OBJECT, not undefined");
        assert_eq!(
            (*(p as *const ObjectHeader)).class_id,
            ITERATOR_HELPER_CLASS_ID
        );
        assert_eq!(drain(m), vec![2.0, 4.0, 6.0]);
    }
}

#[test]
fn filter_returns_a_helper_and_keeps_matching_values() {
    unsafe {
        let f = tower(
            helper_over(&[1.0, 2.0, 3.0, 4.0]),
            "filter",
            &[closure1(is_even)],
        );
        assert_ne!(crate::value::js_nanbox_get_pointer(f), 0);
        assert_eq!(drain(f), vec![2.0, 4.0]);
    }
}

#[test]
fn take_returns_a_helper_and_stops_after_n() {
    unsafe {
        let t = tower(
            helper_over(&[1.0, 2.0, 3.0, 4.0, 5.0]),
            "take",
            &[f64::from_bits(JSValue::number(2.0).bits())],
        );
        assert_ne!(crate::value::js_nanbox_get_pointer(t), 0);
        assert_eq!(drain(t), vec![1.0, 2.0]);
    }
}

#[test]
fn drop_returns_a_helper_and_skips_the_first_n() {
    unsafe {
        let d = tower(
            helper_over(&[1.0, 2.0, 3.0, 4.0]),
            "drop",
            &[f64::from_bits(JSValue::number(2.0).bits())],
        );
        assert_ne!(crate::value::js_nanbox_get_pointer(d), 0);
        assert_eq!(drain(d), vec![3.0, 4.0]);
    }
}

#[test]
fn flat_map_returns_a_helper_and_flattens_one_level() {
    unsafe {
        let f = tower(
            helper_over(&[1.0, 2.0]),
            "flatMap",
            &[closure1(pair_with_ten_times)],
        );
        assert_ne!(crate::value::js_nanbox_get_pointer(f), 0);
        assert_eq!(drain(f), vec![1.0, 10.0, 2.0, 20.0]);
    }
}

#[test]
fn map_iterator_inherits_flat_map_and_to_array_through_the_tower() {
    let _serialized = crate::array::test_serialize();
    unsafe {
        let map = crate::map::js_map_alloc(2);
        crate::map::js_map_set(map, 1.0, 1.0);
        crate::map::js_map_set(map, 2.0, 2.0);
        let raw_iter = crate::collection_iter_object::js_map_values_iter_obj(map);
        let iter = crate::value::js_nanbox_pointer(raw_iter);

        let flattened = tower(iter, "flatMap", &[closure1(pair_with_ten_times)]);
        let helper_ptr = crate::value::js_nanbox_get_pointer(flattened);
        assert_ne!(
            helper_ptr, 0,
            "MapIterator.prototype.flatMap must return a helper object"
        );
        assert_eq!(
            (*(helper_ptr as *const ObjectHeader)).class_id,
            ITERATOR_HELPER_CLASS_ID
        );
        assert_eq!(
            array_numbers(tower(flattened, "toArray", &[])),
            vec![1.0, 10.0, 2.0, 20.0]
        );
    }
}

#[test]
fn chained_helpers_compose() {
    unsafe {
        // Iterator.from([1..6]).map(x => x*2).filter(even).take(2) → [2, 4]
        let m = tower(
            helper_over(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            "map",
            &[closure1(double_it)],
        );
        let f = tower(m, "filter", &[closure1(is_even)]);
        let t = tower(f, "take", &[f64::from_bits(JSValue::number(2.0).bits())]);
        assert_eq!(drain(t), vec![2.0, 4.0]);
    }
}

#[test]
fn to_array_drains_the_chain() {
    unsafe {
        let a = tower(helper_over(&[1.0, 2.0, 3.0]), "toArray", &[]);
        assert_eq!(array_numbers(a), vec![1.0, 2.0, 3.0]);
        let mapped = tower(helper_over(&[1.0, 2.0]), "map", &[closure1(double_it)]);
        assert_eq!(array_numbers(tower(mapped, "toArray", &[])), vec![2.0, 4.0]);
    }
}

#[test]
fn reduce_with_and_without_an_initial_value() {
    unsafe {
        let with_init = tower(
            helper_over(&[1.0, 2.0, 3.0]),
            "reduce",
            &[closure2(add), f64::from_bits(JSValue::number(10.0).bits())],
        );
        assert_eq!(f64::from_bits(with_init.to_bits()), 16.0);

        let no_init = tower(
            helper_over(&[1.0, 2.0, 3.0, 4.0]),
            "reduce",
            &[closure2(add)],
        );
        assert_eq!(f64::from_bits(no_init.to_bits()), 10.0);
    }
}

#[test]
fn some_every_find_short_circuit() {
    unsafe {
        assert_eq!(
            tower(helper_over(&[1.0, 2.0, 3.0]), "some", &[closure1(is_even)]).to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(
            tower(helper_over(&[1.0, 3.0, 5.0]), "some", &[closure1(is_even)]).to_bits(),
            crate::value::TAG_FALSE
        );
        assert_eq!(
            tower(helper_over(&[2.0, 4.0]), "every", &[closure1(is_even)]).to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(
            tower(helper_over(&[2.0, 3.0]), "every", &[closure1(is_even)]).to_bits(),
            crate::value::TAG_FALSE
        );
        let found = tower(helper_over(&[1.0, 2.0, 3.0]), "find", &[closure1(is_even)]);
        assert_eq!(f64::from_bits(found.to_bits()), 2.0);
        let missing = tower(helper_over(&[1.0, 3.0]), "find", &[closure1(is_even)]);
        assert!(JSValue::from_bits(missing.to_bits()).is_undefined());
    }
}

#[test]
fn symbol_iterator_returns_the_helper_itself() {
    unsafe {
        let h = helper_over(&[1.0]);
        assert_eq!(tower(h, "@@iterator", &[]).to_bits(), h.to_bits());
    }
}

/// `next()` written as an OWN method that reads `this` must receive the
/// iterator as its receiver — `IteratorNext`'s `Call(next, iterator)`.
///
/// This pins the second half of the `iterator_step` fix independently of the
/// own-vs-inherited lookup: an own `next` takes the closure branch either way,
/// so only the `js_implicit_this_set` around the call makes this pass. It is
/// the shape a hand-written `next() { return this.#impl.next(); }` takes, and
/// pre-fix it saw `undefined`.
///
/// SABOTAGE CHECK: drop the `js_implicit_this_set` pair in `iterator_step` and
/// this reports a receiver of `undefined`.
#[test]
fn an_own_next_method_is_called_with_the_iterator_as_this() {
    use std::sync::atomic::{AtomicU64, Ordering};
    /// Bits of the `this` the last `next()` observed.
    static OBSERVED_THIS: AtomicU64 = AtomicU64::new(0);

    extern "C" fn next_recording_this(_c: *const crate::closure::ClosureHeader, _arg: f64) -> f64 {
        let this = crate::object::js_implicit_this_get();
        OBSERVED_THIS.store(this.to_bits(), Ordering::SeqCst);
        unsafe { crate::iter_result::make_iter_result(JSValue::undefined(), true) }
    }

    unsafe {
        // A bare `{ next() {...} }` object: one own field named `next`.
        let obj = crate::object::js_object_alloc(0, 1);
        let key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
        let p = next_recording_this as *const u8;
        crate::closure::js_register_closure_arity(p, 0);
        let closure = crate::closure::js_closure_alloc(p, 0);
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            crate::value::js_nanbox_pointer(closure as i64),
        );
        let iterable = crate::value::js_nanbox_pointer(obj as i64);

        let h = super::js_iterator_from(iterable);
        OBSERVED_THIS.store(0, Ordering::SeqCst);
        let (_v, done) = step(tower(h, "next", &[]));
        assert!(done, "the stub `next` reports done");

        let observed = OBSERVED_THIS.load(Ordering::SeqCst);
        assert_ne!(observed, 0, "`next` was never called");
        assert_eq!(
            observed,
            iterable.to_bits(),
            "`next()` must run with `this` === the iterator (got bits {observed:#018x}, \
             expected {:#018x}); `undefined` here is the pre-#7576 behaviour",
            iterable.to_bits()
        );
    }
}

/// A String iterator must still dispatch as a String iterator after the
/// renumbering — the other half of the collision.
#[test]
fn string_iterator_still_dispatches_to_its_own_family() {
    unsafe {
        let s = crate::string::js_string_from_bytes(b"ab".as_ptr(), 2);
        let it = crate::string::string_values_iter(s);
        assert_eq!(
            (*(crate::value::js_nanbox_get_pointer(it) as *const ObjectHeader)).class_id,
            crate::string::STRING_ITERATOR_CLASS_ID
        );
        let (v, done) = step(tower(it, "next", &[]));
        assert!(!done, "a fresh String iterator must not report done");
        assert!(
            !JSValue::from_bits(v.bits()).is_undefined(),
            "a String iterator's first step must carry a code point"
        );
    }
}
