//! `thisArg` of the %TypedArray%.prototype callback methods reaches the
//! callback as its `this`; the 2-argument entry points bind `undefined`.
use super::*;
use crate::closure::ClosureHeader;
use std::cell::RefCell;

thread_local! {
    static SEEN_THIS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn record_this(
    _callback: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
    _index: f64,
    _receiver: f64,
) -> f64 {
    SEEN_THIS.with(|seen| seen.borrow_mut().push(this.bits()));
    value
}

fn take_seen() -> Vec<u64> {
    SEEN_THIS.with(|seen| std::mem::take(&mut *seen.borrow_mut()))
}

fn source_and_callback() -> (*mut TypedArrayHeader, *const ClosureHeader) {
    let source = typed_array_alloc(KIND_FLOAT64, 2);
    js_typed_array_set(source, 0, 1.0);
    js_typed_array_set(source, 1, 2.0);
    let callback = crate::closure::js_closure_alloc(crate::fn_info!(record_this, 3), 0);
    (source, callback)
}

#[test]
fn callback_methods_bind_this_arg() {
    let (source, callback) = source_and_callback();
    let this_arg = 42.5_f64;
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    // Each method calls `record_this` for every element it visits; a callback
    // that returns the element makes `every`/`some`/`find*` visit them all or
    // stop early, so only the bits seen are asserted, not the count.
    type Run = fn(*const TypedArrayHeader, *const ClosureHeader, f64);
    let runs: [(&str, Run); 9] = [
        ("forEach", |t, c, a| {
            typed_array_for_each_this(t, c, a);
        }),
        ("map", |t, c, a| {
            typed_array_map_this(t, c, a);
        }),
        ("filter", |t, c, a| {
            typed_array_filter_this(t, c, a);
        }),
        ("every", |t, c, a| {
            typed_array_every_this(t, c, a);
        }),
        ("some", |t, c, a| {
            typed_array_some_this(t, c, a);
        }),
        ("find", |t, c, a| {
            typed_array_find_this(t, c, a);
        }),
        ("findIndex", |t, c, a| {
            typed_array_find_index_this(t, c, a);
        }),
        ("findLast", |t, c, a| {
            typed_array_find_last_this(t, c, a);
        }),
        ("findLastIndex", |t, c, a| {
            typed_array_find_last_index_this(t, c, a);
        }),
    ];
    for (name, run) in runs {
        take_seen();
        run(source, callback, this_arg);
        let seen = take_seen();
        assert!(!seen.is_empty(), "{name} never called back");
        assert!(
            seen.iter().all(|bits| *bits == this_arg.to_bits()),
            "{name} must bind thisArg, saw {seen:?}"
        );
        run(source, callback, undefined);
        let seen = take_seen();
        assert!(
            seen.iter().all(|bits| *bits == crate::value::TAG_UNDEFINED),
            "{name} with an undefined thisArg must bind undefined, saw {seen:?}"
        );
    }
}

#[test]
fn two_argument_entry_points_bind_undefined() {
    let (source, callback) = source_and_callback();
    take_seen();
    js_typed_array_for_each(source, callback);
    js_typed_array_find_last(source, callback);
    let seen = take_seen();
    assert_eq!(
        seen.len(),
        3,
        "forEach visits 2, findLast stops at the last"
    );
    assert!(seen.iter().all(|bits| *bits == crate::value::TAG_UNDEFINED));
}
