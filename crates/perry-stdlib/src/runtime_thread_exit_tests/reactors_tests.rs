//! #11471 thread-exit regression tests (reactors group).
//!
//! Each test populates one process-global reactor table from a spawned
//! thread, proves the entry exists while that thread lives (so the absence
//! asserted afterwards is not vacuous), joins, and asserts the entry is gone.
//!
//! Not covered here, and why:
//! * `dgram_reactor::LIVE` and `dgram::DGRAM_REGISTRY` live behind
//!   perry-runtime's `mod-dgram` feature, which perry-stdlib's test build does
//!   not enable (and a test here cannot `cfg` on a dependency's feature).
//! * `timer::mock::MOCK_TIMERS` is reachable only through `node:test` mock
//!   timers, whose `enable` is process-global: it would capture every
//!   concurrently running test's `setTimeout`/`setInterval`.

use perry_runtime::JSValue;

/// A fresh arena object on the calling thread, NaN-boxed.
fn arena_object() -> f64 {
    let arr = perry_runtime::js_array_alloc(0);
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

#[test]
fn thread_exit_releases_the_threads_live_child_process_entries() {
    use perry_runtime::child_process::reactor as cp;
    let (handle, registered_while_alive) = std::thread::spawn(|| {
        let handle = cp::cp_register_idle_live_child_for_test(arena_object());
        (handle, cp::cp_live_child_registered_for_test(handle))
    })
    .join()
    .unwrap();
    assert!(
        registered_while_alive,
        "the child entry must exist while its thread lives"
    );
    assert!(
        !cp::cp_live_child_registered_for_test(handle),
        "a dead thread's ChildProcess entry outlived its heap"
    );
}

#[cfg(unix)]
#[test]
fn thread_exit_releases_the_threads_live_pty_entries() {
    use perry_runtime::child_process::reactor::thread_exit_test_probes as probes;
    let (handle, registered_while_alive) = std::thread::spawn(|| {
        let handle = probes::pty_register_idle_live_for_test(arena_object());
        (handle, probes::pty_live_registered_for_test(handle))
    })
    .join()
    .unwrap();
    assert!(
        registered_while_alive,
        "the pty entry must exist while its thread lives"
    );
    assert!(
        !probes::pty_live_registered_for_test(handle),
        "a dead thread's IPty entry outlived its heap"
    );
}

/// Range half: a token holding a value from the dead thread's arena (here an
/// attached handle) is released by the arena-drop hook, even on a thread with
/// no agent of its own.
#[test]
fn thread_exit_releases_native_async_tokens_holding_the_threads_values() {
    use perry_runtime::promise::native_async as na;
    let (promise, registered_while_alive) = std::thread::spawn(|| {
        let token = na::js_native_async_completion_new(0);
        let promise = na::js_native_async_completion_promise(token);
        let status = na::js_native_async_completion_attach_handle(
            token,
            arena_object().to_bits(),
            // Cleanup on success only: the token is never settled here.
            na::PERRY_NATIVE_ASYNC_CLEANUP_ON_SUCCESS,
        );
        assert_eq!(status, na::PERRY_NATIVE_ASYNC_OK);
        (
            promise as usize,
            na::native_async_promise_has_token(promise),
        )
    })
    .join()
    .unwrap();
    assert!(
        registered_while_alive,
        "the token must be registered while its thread lives"
    );
    assert!(
        !na::native_async_promise_has_token(promise as *mut _),
        "a dead thread's native-async token outlived its heap"
    );
}

/// Agent half: an idle token (its Promise is malloc-resident, so no arena
/// range names it) is released when its worker agent retires — otherwise it
/// would count in the keep-alive mirror forever.
#[test]
fn agent_retirement_releases_the_agents_idle_native_async_tokens() {
    use perry_runtime::promise::native_async as na;
    let (promise, registered_while_alive) = std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        let token = na::js_native_async_completion_new(0);
        let promise = na::js_native_async_completion_promise(token);
        let registered = na::native_async_promise_has_token(promise);
        perry_runtime::agent::retire_agent(agent);
        (promise as usize, registered)
    })
    .join()
    .unwrap();
    assert!(
        registered_while_alive,
        "the token must be registered while its agent lives"
    );
    assert!(
        !na::native_async_promise_has_token(promise as *mut _),
        "a retired agent's idle native-async token kept the event loop alive"
    );
}

/// A worker agent's `process.on(signal)` subscribes ITS loop; at retirement
/// the process-global slot must return to "not installed", or the next
/// listener on any other thread never subscribes.
#[cfg(unix)]
#[test]
fn agent_retirement_releases_the_agents_turnloop_signal_subscription() {
    use perry_runtime::child_process::reactor::thread_exit_test_probes as probes;
    // SIGUSR1: carried by turnloop, and not used by any other stdlib test.
    const SIGNAL: &str = "SIGUSR1";
    let (agent, while_alive) = std::thread::spawn(|| {
        let agent = perry_runtime::agent::enter_worker_agent();
        probes::set_process_signal_listener_count_for_test(SIGNAL, 1);
        let while_alive = probes::process_signal_subscription_for_test(SIGNAL);
        perry_runtime::agent::retire_agent(agent);
        (agent, while_alive)
    })
    .join()
    .unwrap();
    let (installed, on_turnloop, id, owner) = while_alive.expect("SIGUSR1 is a process signal");
    assert!(
        installed && on_turnloop && id != 0 && owner == agent,
        "the subscription must be on the worker's loop while it lives \
         (installed={installed} on_turnloop={on_turnloop} id={id} owner={owner} agent={agent})"
    );
    let after = probes::process_signal_subscription_for_test(SIGNAL).unwrap();
    // Leave the slot clean for the rest of the process either way.
    probes::set_process_signal_listener_count_for_test(SIGNAL, 0);
    assert_eq!(
        (after.0, after.1, after.2),
        (false, false, 0),
        "a retired agent's signal subscription was left installed"
    );
}

#[test]
fn thread_exit_releases_the_threads_typed_feedback_observations() {
    use perry_runtime::typed_feedback as tf;
    const SITE: u64 = 0x1147_1000_0000_0001;
    let observed_while_alive = std::thread::spawn(|| {
        let arr = perry_runtime::js_array_alloc(0);
        tf::observe_array_address_for_test(SITE, arr as usize);
        tf::site_observation_count_for_test(SITE)
    })
    .join()
    .unwrap();
    assert_eq!(
        observed_while_alive, 1,
        "the observation must exist while its thread lives"
    );
    assert_eq!(
        tf::site_observation_count_for_test(SITE),
        0,
        "a dead thread's typed-feedback observation outlived its heap"
    );
}
