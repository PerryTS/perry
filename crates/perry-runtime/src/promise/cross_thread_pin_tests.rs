//! #9552 — a promise whose address leaves the runtime as a bare `usize` (a
//! worker future, a pending-result queue, a native async token) is pinned by
//! `js_promise_new_cross_thread` and released by its settlement. These pin the
//! constructor/settlement contract and the trust boundary that classifies a
//! returning address.

use super::native_async::{
    js_native_async_completion_new, js_native_async_completion_promise,
    js_native_async_drop_promise_token, native_async_promise_has_token, test_native_async_lock,
    test_reset_native_async_registry,
};
use super::{
    classify_native_promise_addr, js_promise_new, js_promise_new_cross_thread, js_promise_reject,
    js_promise_resolve, NativePromiseAddr, Promise,
};
use crate::value::addr_class::try_read_gc_header;

fn gc_flags(promise: *mut Promise) -> u8 {
    unsafe { try_read_gc_header(promise as usize) }
        .expect("a freshly minted promise is a tracked heap object")
        .gc_flags
}

fn pinned(promise: *mut Promise) -> bool {
    gc_flags(promise) & crate::gc::GC_FLAG_PINNED != 0
}

#[test]
fn cross_thread_promise_is_pinned_at_creation_and_released_by_fulfilment() {
    let _guard = test_native_async_lock();
    let promise = js_promise_new_cross_thread();
    assert_eq!(
        gc_flags(promise) & crate::gc::GC_FLAG_ARENA,
        0,
        "cross-thread promises are malloc-resident"
    );
    assert!(pinned(promise), "#9552: the constructor takes the pin");
    assert_eq!(unsafe { (*promise).native_pinned }, 1);

    js_promise_resolve(promise, 1.0);
    assert!(!pinned(promise), "settlement releases the pin");
    assert_eq!(unsafe { (*promise).native_pinned }, 0);

    // A second settlement is a no-op on an already-settled promise and must
    // not touch the pin state.
    js_promise_reject(promise, 2.0);
    assert!(!pinned(promise));
}

#[test]
fn rejection_releases_the_pin_too() {
    let _guard = test_native_async_lock();
    let promise = js_promise_new_cross_thread();
    assert!(pinned(promise));
    js_promise_reject(promise, 2.0);
    assert!(!pinned(promise));
    assert_eq!(unsafe { (*promise).native_pinned }, 0);
}

#[test]
fn cross_thread_promise_survives_a_full_collection_while_only_native_code_holds_it() {
    let _guard = test_native_async_lock();
    // Hold the address the way a worker future does: as a bare integer no
    // root scanner visits. XOR-hide it so a conservative stack scan (if one
    // were to run) cannot keep the object alive by accident and make the
    // assertion vacuous.
    const MASK: usize = 0x5555_5555_5555_5555;
    let hidden = (js_promise_new_cross_thread() as usize) ^ MASK;
    crate::gc::js_gc_collect();
    let raw = hidden ^ MASK;
    match classify_native_promise_addr(raw) {
        NativePromiseAddr::Live(promise) => {
            assert!(pinned(promise), "still pinned while in flight");
            js_promise_resolve(promise, 3.0);
            assert!(!pinned(promise));
        }
        other => panic!("#9552: in-flight promise did not survive the collection: {other:?}"),
    }
}

#[test]
fn arena_promises_carry_no_pin() {
    let _guard = test_native_async_lock();
    let promise = js_promise_new();
    assert!(!pinned(promise));
    assert_eq!(unsafe { (*promise).native_pinned }, 0);
    js_promise_resolve(promise, 1.0);
    assert!(!pinned(promise));
}

#[test]
fn dropping_a_token_without_settling_releases_the_pin() {
    let _guard = test_native_async_lock();
    test_reset_native_async_registry();
    let token = js_native_async_completion_new(0);
    let promise = js_native_async_completion_promise(token);
    assert!(pinned(promise), "token promises are cross-thread promises");
    assert!(native_async_promise_has_token(promise));
    // The token was the promise's root; once it is gone the pin must not keep
    // a never-settling promise alive forever.
    js_native_async_drop_promise_token(promise);
    assert!(!native_async_promise_has_token(promise));
    assert!(!pinned(promise));
    assert_eq!(unsafe { (*promise).native_pinned }, 0);
}

#[test]
fn classify_native_promise_addr_names_null_live_and_reused_slots() {
    let _guard = test_native_async_lock();
    assert_eq!(classify_native_promise_addr(0), NativePromiseAddr::Null);
    let promise = js_promise_new_cross_thread();
    assert_eq!(
        classify_native_promise_addr(promise as usize),
        NativePromiseAddr::Live(promise)
    );
    // A malloc-resident object of another type where a promise used to be.
    let occupant = crate::gc::gc_malloc(64, crate::gc::GC_TYPE_STRING) as usize;
    assert_eq!(
        classify_native_promise_addr(occupant),
        NativePromiseAddr::WrongType(crate::gc::GC_TYPE_STRING)
    );
    // Not a heap object at all.
    assert_eq!(
        classify_native_promise_addr(0x10),
        NativePromiseAddr::NotAHeapObject
    );
    js_promise_resolve(promise, 0.0);
}
