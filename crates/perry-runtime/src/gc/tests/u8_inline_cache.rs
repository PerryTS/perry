//! Lifecycle proof for the #9342 `PERRY_U8_INLINE_CACHE` admission cache.
//!
//! The cache contract ("an entry names a live, u8-marked, inline-storage
//! `BufferHeader`") is held up by two invalidation sites — buffer death
//! (`finalize_collected_dead_buffer`) and address re-issue
//! (`register_buffer`) — riding the same chokepoints as every other buffer
//! identity table. A stale hit is SILENT (the emitted reader would interpret
//! the new tenant's memory as `(length, bytes)`), so each site is proved here
//! by a test that fails when that specific call is removed: delete the
//! finalize call and `test_dead_u8_entry_pruned_on_full_gc` fails; delete the
//! register call and `test_reissued_address_does_not_inherit_admission`
//! fails.

use super::super::*;
use super::support::*;

fn full_gc() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

/// Prime admits a `mark_as_uint8array`-marked inline-storage buffer, and the
/// entry means what the emitted guard thinks it means: header `length` at
/// offset 0, live bytes at `header + 8`.
#[test]
fn test_prime_admits_inline_u8_and_contract_holds() {
    let _guard = GcTestIsolationGuard::new();

    let buf = crate::buffer::buffer_alloc(16);
    let addr = buf as usize;
    unsafe {
        (*buf).length = 16;
        *crate::buffer::buffer_data_mut(buf).add(3) = 0xAB;
    }

    // Unmarked: not a Uint8Array, must not be admitted.
    crate::buffer::u8_inline_cache_try_prime(addr);
    assert!(
        !crate::buffer::test_u8_inline_cache_holds(addr),
        "an unmarked buffer must not be admitted"
    );

    crate::buffer::mark_as_uint8array(addr);
    crate::buffer::u8_inline_cache_try_prime(addr);
    assert!(
        crate::buffer::test_u8_inline_cache_holds(addr),
        "a marked inline-storage buffer must be admitted"
    );

    // The emitted reader's view of an admitted entry: length then byte.
    let len = unsafe { *(addr as *const u32) };
    let byte = unsafe { *((addr + 8 + 3) as *const u8) };
    assert_eq!(len, 16, "length must be readable at header offset 0");
    assert_eq!(byte, 0xAB, "bytes must be inline at header + 8");
}

/// A foreign-backed wrapper (header-only allocation, bytes owned elsewhere)
/// must never be admitted — `header + 8` is past its allocation.
#[test]
fn test_prime_rejects_foreign_backed_wrapper() {
    let _guard = GcTestIsolationGuard::new();

    let mut bytes = [7u8; 8];
    let buf = crate::buffer::buffer_alloc_foreign(bytes.as_mut_ptr(), bytes.len() as u32);
    let addr = buf as usize;
    crate::buffer::mark_as_uint8array(addr);
    crate::buffer::u8_inline_cache_try_prime(addr);
    assert!(
        !crate::buffer::test_u8_inline_cache_holds(addr),
        "a foreign-backed wrapper must not be admitted: its bytes are not \
         inline and the emitted load would read past the allocation"
    );
    crate::buffer::finalize_collected_dead_buffer(addr);
}

/// Death pruning: a dead buffer's admission must not survive the full trace
/// that collects it — the recycled address's next tenant is arbitrary memory
/// to the emitted reader. Fails if `finalize_collected_dead_buffer` loses its
/// `u8_inline_cache_invalidate` call.
#[test]
fn test_dead_u8_entry_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let addr = crate::buffer::buffer_alloc(16) as usize;
    crate::buffer::mark_as_uint8array(addr);
    crate::buffer::u8_inline_cache_try_prime(addr);
    assert!(
        crate::buffer::test_u8_inline_cache_holds(addr),
        "test premise: the buffer is admitted while live"
    );

    // No roots: dead at the full trace (buffers are TENURED old-gen residents).
    full_gc();

    assert!(
        !crate::buffer::test_u8_inline_cache_holds(addr),
        "a dead buffer's inline-read admission must be pruned on the trace \
         that collects it — a stale hit reads the next tenant's memory as \
         (length, bytes)"
    );
}

/// Re-issue pruning: registering a fresh buffer at an address must clear any
/// admission the previous tenant held (belt and suspenders over death
/// pruning, mirroring `register_buffer`'s own-props clear). Fails if
/// `register_buffer` loses its `u8_inline_cache_invalidate` call.
#[test]
fn test_reissued_address_does_not_inherit_admission() {
    let _guard = GcTestIsolationGuard::new();

    let buf = crate::buffer::buffer_alloc(16);
    let addr = buf as usize;
    crate::buffer::mark_as_uint8array(addr);
    crate::buffer::u8_inline_cache_try_prime(addr);
    assert!(crate::buffer::test_u8_inline_cache_holds(addr));

    // Simulate the re-issue path directly: a new tenant registering at the
    // same address (the death finalizer is deliberately NOT run first, so
    // this passes only on register_buffer's own clear).
    crate::buffer::register_buffer(buf);
    assert!(
        !crate::buffer::test_u8_inline_cache_holds(addr),
        "a re-registered address must not inherit the dead tenant's \
         inline-read admission"
    );
}
