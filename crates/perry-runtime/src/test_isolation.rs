//! Tests that assert on process-wide state no lock can quiesce.
//!
//! `NOTIFIED`, the pending-microtask probes, the has-active registries and the
//! stdin listener are single process-wide cells that any of the other ~6000
//! tests may touch (every promise settlement notifies). A test that needs them
//! idle cannot get that from a mutex only its own module takes, so it runs in a
//! child process holding nothing but itself.

/// Run `body` in a process running only the calling test.
///
/// libtest already gives every test a fresh process of its own (the workspace
/// runs tests with `panic-abort-tests`), so `body` runs inline once that is
/// checked.
pub(crate) fn in_own_process(body: impl FnOnce()) {
    let _ = crate::test_support::current_test_name();
    body();
}
