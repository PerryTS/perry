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
    perry_runtime::closure::js_closure_alloc(perry_runtime::fn_info!(probe_thunk, 0), 0) as i64
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
