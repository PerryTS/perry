//! Production-path witness for timers queued by threads that use the primary
//! agent while allocating promises in their own thread-local arenas.

use std::sync::mpsc;
use std::time::Duration;

use perry_runtime::closure::{js_closure_alloc, ClosureHeader, JsThis};
use perry_runtime::timer::{
    js_clear_timeout_value, js_set_timeout_callback, js_set_timeout_value, js_timer_has_pending,
    js_timer_next_deadline,
};

#[test]
fn primary_timers_are_released_when_helper_thread_arenas_exit() {
    perry_runtime::gc::initialize_runtime_fixture();
    assert_eq!(js_timer_has_pending(), 0);

    // This main-thread timer is the live control: helper teardown must leave
    // it queued while removing only the promise allocated in the helper arena.
    let control = js_closure_alloc(perry_runtime::fn_info!(control_callback, 0), 0);
    let control_handle = js_set_timeout_callback(control as usize as i64, 120_000.0);
    assert_eq!(js_timer_has_pending(), 1);

    let (ready_tx, ready_rx) = mpsc::channel();
    let (exit_tx, exit_rx) = mpsc::channel();
    let helper = std::thread::spawn(move || {
        queue_long_timer();
        ready_tx.send(()).unwrap();
        exit_rx.recv().unwrap();
    });

    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(js_timer_has_pending(), 1, "the helper timer is queued too");
    assert!(
        js_timer_next_deadline() < 75_000.0,
        "the helper's earlier deadline is visible before its arena exits"
    );

    exit_tx.send(()).unwrap();
    helper.join().unwrap();
    assert_eq!(
        js_timer_has_pending(),
        1,
        "the helper's timer is gone and the main-thread control remains"
    );
    assert!(
        js_timer_next_deadline() > 75_000.0,
        "the next deadline is now the surviving main-thread control"
    );

    js_clear_timeout_value(perry_runtime::value::js_nanbox_pointer(control_handle));
    assert_eq!(js_timer_has_pending(), 0, "all timers are removed");
}

fn queue_long_timer() {
    perry_runtime::gc::initialize_runtime_fixture();
    let promise = js_set_timeout_value(30_000.0, 42.0);
    assert!(!promise.is_null());
}

extern "C" fn control_callback(_closure: *const ClosureHeader, _this: JsThis) -> f64 {
    0.0
}
