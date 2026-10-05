//! The unhandled-rejection bookkeeping must follow a promise that a
//! collection moves.
use super::combinators::gc_tests::moving_gc;
use super::*;
use crate::gc::RuntimeHandleScope;

/// The internally-handled mark (Node's `markPromiseAsHandled`, which the
/// stream machinery puts on its `closed` / `ready` promises at creation) must
/// move with the promise: a marked promise that a collection moves and that
/// is rejected afterwards is still handled, so nothing is tracked to report.
#[test]
fn internally_handled_mark_moves_with_the_promise() {
    let _gc = moving_gc();
    let scope = RuntimeHandleScope::new();
    let promise = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_promise_new() as i64));
    let live = || crate::value::js_nanbox_get_pointer(promise.get_nanbox_f64()) as *mut Promise;
    js_promise_mark_internally_handled(live());
    let before = promise.get_nanbox_f64().to_bits();

    crate::gc::gc_collect_minor();

    assert_ne!(
        before,
        promise.get_nanbox_f64().to_bits(),
        "the promise must actually move in the collection"
    );
    js_promise_reject(live(), 7.0);
    assert!(
        !rejection::checkpoint_work_pending(),
        "a promise marked handled before it moved must not be tracked as an unhandled rejection"
    );
}
