//! Every path that inserts into the process-global external-Uint8Array
//! registry must arm `EXTERNAL_UINT8ARRAYS_NONEMPTY` first.
//!
//! `is_uint8array_buffer_slow` consults that map ONLY when the latch is armed,
//! so an inserter that forgets to arm makes its own entry invisible: the
//! address is in the registry and the probe answers "no". The registry is
//! process-global precisely so an address registered on one thread is visible
//! from another, and the thread-local set that covers the registering thread
//! does not cover any other — which is why this test asks from a fresh thread.

use super::header::*;

/// Synthetic addresses. Every path under test is address-keyed and never
/// dereferences the pointer, so these need not be real allocations — but they
/// must not collide with a live buffer, hence the deliberately absurd base.
const CRYPTO_KEY_ADDR: usize = 0x9176_0000_1000;
const PLAIN_MARK_ADDR: usize = 0x9176_0000_2000;

/// Ask from a thread that has never registered anything, so the thread-local
/// set and its address range cannot answer and the global registry must.
fn visible_from_a_fresh_thread(addr: usize) -> bool {
    std::thread::spawn(move || is_uint8array_buffer(addr))
        .join()
        .expect("probe thread must not panic")
}

#[test]
fn crypto_key_external_registration_arms_the_uint8array_latch() {
    js_buffer_mark_as_crypto_key_external(CRYPTO_KEY_ADDR, 1, 1, 1, 1, 0, 256);
    let seen = visible_from_a_fresh_thread(CRYPTO_KEY_ADDR);
    // Drop the synthetic entry from every address-keyed table before
    // asserting, so a failure cannot poison the rest of this binary.
    finalize_collected_dead_buffer(CRYPTO_KEY_ADDR);
    assert!(
        seen,
        "js_buffer_mark_as_crypto_key_external inserts into external_uint8arrays() \
         but did not arm EXTERNAL_UINT8ARRAYS_NONEMPTY, so is_uint8array_buffer \
         skipped the mutex and denied a registered address"
    );
}

/// The sibling path, which established the invariant. Included so the pair is
/// checked together: if this one ever regresses the same way, the failure
/// names the same cause instead of looking like a crypto-key-only quirk.
#[test]
fn plain_external_uint8array_registration_arms_the_latch() {
    js_buffer_mark_as_uint8array_external(PLAIN_MARK_ADDR);
    let seen = visible_from_a_fresh_thread(PLAIN_MARK_ADDR);
    finalize_collected_dead_buffer(PLAIN_MARK_ADDR);
    assert!(
        seen,
        "js_buffer_mark_as_uint8array_external must arm EXTERNAL_UINT8ARRAYS_NONEMPTY \
         before inserting"
    );
}
