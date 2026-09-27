//! #11471 thread-exit regression tests (stdlib_misc group).
//!
//! Each test fills one process-global perry-stdlib table from a spawned
//! thread with values allocated in that thread's arena, proves the entry is
//! present while the thread lives, and asserts it is gone once the thread has
//! exited (its `Arena::drop` ran the table's thread-exit range hook).

use perry_runtime::ClosureHeader;

extern "C" fn probe_thunk(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    0.0
}

/// A closure allocated in the calling thread's arena, as a raw address.
fn closure_here() -> i64 {
    perry_runtime::closure::js_closure_alloc(probe_thunk as *const u8, 0) as i64
}

#[cfg(all(
    feature = "tls-runtime",
    not(target_os = "ios"),
    not(target_os = "android")
))]
#[test]
fn thread_exit_releases_the_threads_tls_server_listener_and_once_entries() {
    use crate::tls::thread_exit_probe as tls;
    let (id, cb, present) = std::thread::spawn(|| {
        let sni = closure_here();
        let cb = closure_here();
        let id = tls::insert_server_for_test(sni, cb);
        let present = tls::server_present(id)
            && tls::listener_present(id, cb)
            && tls::once_flag_present(id, cb);
        (id, cb, present)
    })
    .join()
    .unwrap();
    assert!(
        present,
        "the TLS entries must exist while their thread lives"
    );
    assert!(
        !tls::server_present(id),
        "a server whose SNICallback lived in a dead thread's heap outlived it"
    );
    assert!(
        !tls::listener_present(id, cb),
        "a dead thread's TLS listener closure outlived its heap"
    );
    assert!(
        !tls::once_flag_present(id, cb),
        "a dead thread's TLS once-flag closure outlived its heap"
    );
}

#[cfg(feature = "database-sqlite")]
#[test]
fn thread_exit_releases_the_threads_node_sqlite_callbacks_and_authorizer() {
    use crate::sqlite::thread_exit_probe as sq;
    let (function, aggregate, state, db, present) = std::thread::spawn(|| {
        let callbacks: Vec<f64> = (0..4)
            .map(|_| perry_runtime::value::js_nanbox_pointer(closure_here()))
            .collect();
        let function = sq::register_function_for_test(callbacks[0]);
        let aggregate = sq::register_aggregate_for_test(callbacks[1]);
        let state = sq::register_aggregate_state_for_test(callbacks[2]);
        let db = sq::register_db_with_authorizer_for_test(callbacks[3]);
        let present = sq::function_registered(function)
            && sq::aggregate_registered(aggregate)
            && sq::aggregate_state_registered(state)
            && sq::authorizer(db).is_some();
        (function, aggregate, state, db, present)
    })
    .join()
    .unwrap();
    assert!(
        present,
        "the node:sqlite entries must exist while their thread lives"
    );
    assert!(
        !sq::function_registered(function),
        "a dead thread's db.function() callback is still scanned"
    );
    assert!(
        !sq::aggregate_registered(aggregate),
        "a dead thread's db.aggregate() callbacks are still scanned"
    );
    assert!(
        !sq::aggregate_state_registered(state),
        "a dead thread's running aggregate state is still scanned"
    );
    assert!(
        sq::authorizer(db).is_none(),
        "a dead thread's authorizer closure outlived its heap in HANDLES"
    );
    sq::free_boxes_for_test(function, aggregate, state);
    crate::common::drop_handle(db);
}

#[cfg(feature = "crypto")]
#[test]
fn thread_exit_releases_the_threads_crypto_key_entries() {
    extern "C" {
        fn perry_test_11471_register_crypto_key() -> usize;
        fn perry_test_11471_crypto_key_registered(buf_addr: usize) -> bool;
    }
    let (addr, present) = std::thread::spawn(|| unsafe {
        let addr = perry_test_11471_register_crypto_key();
        (addr, perry_test_11471_crypto_key_registered(addr))
    })
    .join()
    .unwrap();
    assert!(
        present,
        "the CryptoKey entry must exist while its thread lives"
    );
    assert!(
        !unsafe { perry_test_11471_crypto_key_registered(addr) },
        "a dead thread's CryptoKey entry outlived its Buffer"
    );
}

#[test]
fn thread_exit_releases_the_threads_worker_records() {
    use crate::worker_threads::thread_exit_probe as wt;
    let (worker_id, present) = std::thread::spawn(|| {
        let object = perry_runtime::object::js_object_alloc(0, 0);
        let callback = perry_runtime::value::js_nanbox_pointer(closure_here()).to_bits();
        let object_bits = perry_runtime::JSValue::pointer(object as *const u8).bits();
        let worker_id = wt::insert_worker_for_test(object_bits, callback);
        (worker_id, wt::worker_present(worker_id))
    })
    .join()
    .unwrap();
    assert!(
        present,
        "the Worker record must exist while its thread lives"
    );
    assert!(
        !wt::worker_present(worker_id),
        "a dead thread's Worker record outlived its heap"
    );
}

#[test]
fn thread_exit_releases_the_threads_stdin_listeners() {
    const EVENTS: [&str; 4] = ["data", "keypress", "readable", "end"];
    let _serial = crate::readline::thread_exit_test_guard();
    let (callbacks, present) = std::thread::spawn(|| {
        let names: Vec<_> = EVENTS
            .iter()
            .map(|e| perry_runtime::string::js_string_from_bytes(e.as_ptr(), e.len() as u32))
            .collect();
        let callbacks: Vec<i64> = EVENTS.iter().map(|_| closure_here()).collect();
        for (name, cb) in names.iter().zip(&callbacks) {
            crate::readline::js_readline_stdin_on(*name, *cb);
        }
        let present = EVENTS
            .iter()
            .zip(&callbacks)
            .all(|(e, cb)| crate::readline::stdin_listener_registered_for_test(e, *cb));
        (callbacks, present)
    })
    .join()
    .unwrap();
    assert!(
        present,
        "the stdin listeners must exist while their thread lives"
    );
    for (event, cb) in EVENTS.iter().zip(&callbacks) {
        assert!(
            !crate::readline::stdin_listener_registered_for_test(event, *cb),
            "a dead thread's stdin '{event}' listener outlived its heap"
        );
    }
}

#[cfg(feature = "bundled-events")]
#[test]
fn thread_exit_retires_the_threads_event_emitter_payloads() {
    use perry_runtime::JSValue;
    let (handle, listeners_while_alive) = std::thread::spawn(|| unsafe {
        let name = "__perry_11471_emitter_probe";
        let event = perry_runtime::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let event_bits = JSValue::string_ptr(event).bits() as i64;
        let handle = crate::events::js_event_emitter_new();
        let listener_bits =
            perry_runtime::value::js_nanbox_pointer(closure_here()).to_bits() as i64;
        crate::events::js_event_emitter_on(handle, event_bits, listener_bits);
        let all = JSValue::undefined().bits() as i64;
        let count = crate::events::js_event_emitter_listener_count(handle, event_bits, all);
        (handle, count)
    })
    .join()
    .unwrap();
    assert_eq!(
        listeners_while_alive, 1.0,
        "the emitter must hold the listener while its thread lives"
    );
    assert!(
        !crate::common::handle_exists(handle),
        "a dead thread's EventEmitter payload (listener closure) outlived its heap"
    );
}
