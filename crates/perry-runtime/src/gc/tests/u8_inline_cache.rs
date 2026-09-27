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

    // Unmarked: a Node `Buffer`, whose element semantics are a Uint8Array's
    // (#10515 admits it).
    crate::buffer::u8_inline_cache_try_prime(addr);
    assert!(
        crate::buffer::test_u8_inline_cache_holds(addr),
        "an inline-storage Buffer must be admitted"
    );

    crate::buffer::mark_as_uint8array(addr);
    crate::buffer::u8_inline_cache_try_prime(addr);
    assert!(
        crate::buffer::test_u8_inline_cache_holds(addr),
        "a marked inline-storage Uint8Array must be admitted"
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

/// A registered Uint8Array view has no inline payload. Runtime reads resolve
/// to shared backing bytes; admitting it would read past its header on a
/// cache hit (#9360/#7219/#10056).
#[test]
fn test_prime_rejects_registered_view() {
    let _guard = GcTestIsolationGuard::new();

    let backing = crate::buffer::js_array_buffer_new(4);
    let boxed_backing = crate::value::js_nanbox_pointer(backing as i64);
    let view = crate::buffer::js_uint8array_new(boxed_backing);
    let addr = view as usize;

    unsafe {
        *crate::buffer::buffer_data_mut(backing).add(1) = 0xAB;
    }
    assert_eq!(
        crate::buffer::js_buffer_index_get_value(view, 1),
        0xAB as f64,
        "test premise: the runtime read resolves the view to its backing"
    );
    assert_eq!(
        unsafe { *crate::buffer::buffer_data(view).add(1) },
        0xAB,
        "runtime and native accessors must expose the same shared bytes"
    );
    assert_ne!(crate::buffer::buffer_data(view), unsafe {
        (view as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>())
    });

    crate::buffer::u8_inline_cache_try_prime(addr);
    assert!(
        !crate::buffer::test_u8_inline_cache_holds(addr),
        "a registered view must not be admitted: cache-hit reads bypass the \
         authoritative backing"
    );
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

/// #10515: the non-integer-indexed brands that share `BufferHeader` storage —
/// ArrayBuffer, SharedArrayBuffer, DataView (whose payload holds its data
/// pointer) — are never admitted, and a brand mark that arrives after a prime
/// revokes the admission. Fails if `u8_inline_cache_try_prime` stops asking
/// the brand, or a `mark_as_*` loses its invalidation.
#[test]
fn test_non_byte_view_brands_are_never_admitted() {
    let _guard = GcTestIsolationGuard::new();

    type Mark = fn(usize);
    let marks: [(&str, Mark); 3] = [
        ("ArrayBuffer", crate::buffer::mark_as_array_buffer),
        (
            "SharedArrayBuffer",
            crate::buffer::mark_as_shared_array_buffer,
        ),
        ("DataView", crate::buffer::mark_as_data_view),
    ];
    for (brand, mark) in marks {
        let buf = crate::buffer::buffer_alloc(16);
        let addr = buf as usize;
        unsafe { (*buf).length = 16 };
        mark(addr);
        crate::buffer::u8_inline_cache_try_prime(addr);
        assert!(
            !crate::buffer::test_u8_inline_cache_holds(addr),
            "a {brand} is not integer-indexed and must not be admitted"
        );

        // Mark AFTER a prime: the admission must be revoked.
        let buf = crate::buffer::buffer_alloc(16);
        let addr = buf as usize;
        unsafe { (*buf).length = 16 };
        crate::buffer::u8_inline_cache_try_prime(addr);
        assert!(
            crate::buffer::test_u8_inline_cache_holds(addr),
            "test premise: a plain Buffer is admitted"
        );
        mark(addr);
        assert!(
            !crate::buffer::test_u8_inline_cache_holds(addr),
            "marking a primed buffer as a {brand} must revoke its admission"
        );
    }
}

/// #10515: the runtime byte accessors answer an admitted buffer from the cache
/// and prime a fresh one on its first access, so the SECOND access of an owning
/// buffer through any runtime route is a cache hit.
#[test]
fn test_runtime_byte_access_primes_and_hits() {
    let _guard = GcTestIsolationGuard::new();

    let buf = crate::buffer::buffer_alloc(8);
    let addr = buf as usize;
    unsafe { (*buf).length = 8 };
    assert!(!crate::buffer::test_u8_inline_cache_holds(addr));
    crate::buffer::js_buffer_set(buf, 2, 0x1FF);
    assert!(
        crate::buffer::test_u8_inline_cache_holds(addr),
        "the first byte store must prime the admission"
    );
    assert_eq!(crate::buffer::cached_u8_read(addr, 2), Some(0xFF));
    assert!(crate::buffer::cached_u8_write(addr, 3, 7));
    assert_eq!(crate::buffer::js_buffer_get(buf, 3), 7);
    assert_eq!(
        crate::buffer::cached_u8_read(addr, 8),
        None,
        "out of range leaves the cache"
    );
    assert!(!crate::buffer::cached_u8_write(addr, -1, 1));
}
