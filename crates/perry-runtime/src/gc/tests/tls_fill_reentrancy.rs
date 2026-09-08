//! A thread whose FIRST act is resolving the hot-TLS cache must survive it.
//!
//! `tls_hot::fill`'s first provider is `arena_hot_addr()`, which initializes
//! `ARENA` — and `Arena::new` reads `ARENA_TOTAL_BYTES` and `BLOCK_POOL`. If
//! either of those were declared with `crate::perry_thread_local!`, the read
//! would go `HotKey::get -> hot() -> hot_uncached -> hot_via_tls`, find
//! `temp_roots` still null (`fill` writes it LAST, deliberately, so a
//! re-entrant reader cannot mistake a half-filled cache for a ready one), call
//! `fill` again, and re-run `ARENA`'s initializer — without bound.
//!
//! That failure is a stack overflow at thread start, not a slow path, and it
//! only appears on a thread where the first `hot()` precedes the first arena
//! touch. This test is that thread. It is the standing guard on the rule
//! recorded at those three declarations in `arena/block.rs`: a thread-local
//! read from inside the dynamic extent of a `fill` provider cannot use the
//! macro.
//!
//! Sabotage-proved: moving `ARENA_TOTAL_BYTES` alone into the
//! `crate::perry_thread_local!` block below it makes this test abort with
//! `thread '<unknown>' has overflowed its stack / fatal runtime error: stack
//! overflow` (release, 2026-09-05). It is not a hypothetical.
#[test]
fn a_fresh_thread_may_resolve_the_hot_cache_before_touching_the_arena() {
    // A small stack so an unbounded `fill` recursion aborts here rather than
    // running long enough to look like a hang.
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            // `published_slots` reaches `hot()` and nothing else, so this is
            // the first arena-touching call on the thread.
            let _ = crate::tls_hot::published_slots();
            // And the arena still works afterwards: `fill` ran to completion
            // rather than being abandoned mid-way by a recursion guard.
            assert!(
                crate::arena::arena_total_bytes() > 0,
                "the arena reported no reserved bytes after `fill`, so \
                 `ARENA`'s initializer did not complete on this thread"
            );
        })
        .unwrap()
        .join()
        .expect("resolving the hot TLS cache first on a fresh thread panicked");
}
