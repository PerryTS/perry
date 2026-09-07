# `PERRY_GC_VERIFY_EVACUATION` malloc-borrow fix

## Commit

- Fix commit: `499b71628d53b76d87ba68c6f4a59ed597e6d82e` (`fix(gc): release malloc borrow before verification`)
- Base: `8b7dc3342b22fe6270739c8d51585c3d2cdfa618` (`origin/main` when the task started)

## Re-entrancy path

The observed copied-minor path is diagnostic-only:

1. `crates/perry-runtime/src/gc/copying.rs:1516-1519` gates
   `verify_old_to_young_edges_covered()` on
   `gc_verify_evacuation_enabled()`.
2. Before this fix, `verify_old_to_young_edges_collect()` held a shared
   `MALLOC_STATE` borrow while iterating `s.objects` at
   `crates/perry-runtime/src/gc/verify.rs:798-805` (base commit lines).
3. Each candidate reached
   `verify_old_young_parent_slots_covered()` → `visit_gc_rewrite_slots()` →
   `verify_old_young_slot_covered()` at current
   `crates/perry-runtime/src/gc/verify.rs:731-753` and `:690-700`.
4. A non-arena child reaches the exact membership check in
   `remembered_child_needs_tracking()` at
   `crates/perry-runtime/src/gc/barrier/mod.rs:1568-1585`.
5. That calls `gc_malloc_header_is_tracked()`, whose inner mutable borrow is
   `crates/perry-runtime/src/gc/malloc.rs:526-529` (`borrow_mut()` is line 527)
   and whose `ensure_set_built()` may rebuild from `objects` at `:508-515`.

Because `MALLOC_STATE` is thread-local, the shared outer borrow and mutable
inner borrow are on the same collection thread. `RefCell` therefore panics
before the verifier can inspect the heap. The path is entered only when
`PERRY_GC_VERIFY_EVACUATION` is enabled; the later stale-forwarded-reference
walk is independently gated at `copying.rs:1596-1600`.

I audited the production exact-membership callers (`barrier/mod.rs`,
`young_log.rs`, `native_handle.rs`, `timer.rs`, `path.rs`, `symbol/get.rs`,
`value/dyn_index.rs`, and `json/stringify.rs`). None invokes the helper while
holding a `MALLOC_STATE` borrow. The exact nested path above is verifier-only;
there is no non-diagnostic production re-entrancy to prioritize. A second
diagnostic (`PERRY_GC_VERIFY_CLASSIFIER`) can cause live classification from
some GC walks, but that is also diagnostic, not a production path.

## Change

`crates/perry-runtime/src/gc/verify.rs:10-12` now snapshots the malloc header
vector and releases the `MALLOC_STATE` borrow before any verifier callback.
Every verifier-owned malloc-object walk uses that helper, including the
old-to-young check, marked-child checks, array-slot enumeration, and the final
evacuation heap walk. Exact validation semantics remain unchanged: there is no
`try_borrow` fallback and no weakened pointer check.

The named regression test is
`gc::tests::copying::verify_malloc_borrow::test_copied_minor_verify_evacuation_releases_malloc_registry_before_validation`.
It runs on a spawned worker thread, creates a malloc-backed closure parent and
malloc-backed child, makes the non-empty registry inactive, proves the exact
lookup rebuild count advances, then completes a copying minor with evacuation
verification enabled and asserts that an actual nursery object copied and both
verification phases ran. Sabotage is explicit: restore the malloc verifier loop
under `MALLOC_STATE.with(...borrow())`; the child lookup's `borrow_mut()` panics
the worker and makes `join().expect(...)` fail.

## Validation

Not run because the mandatory pre-Cargo check, `df -g /`, reported only **11
GB available**, below the 12 GB floor. Per task instructions I did not invoke
Cargo and did not wait for disk capacity. Consequently these gates were not
run:

- the named regression test;
- every test matching `verify_evacuation`;
- every test matching `malloc`;
- `cargo test -p perry-runtime --release --lib -- --test-threads=1`;
- `cargo build --release -p perry-runtime --features wasm-host`.

Non-Cargo static checks completed: `rustfmt --check` on all edited Rust files
and `git diff --check`. The largest touched Rust file is 1,994 lines, below the
2,000-line repository cap.

## Perrymaster request

Relink on main's cache, then run `cc` with
`PERRY_GC_VERIFY_EVACUATION=1` for **4 turns**. The run must complete all four
turns, emit `[gc-verify]`-style verifier output proving the diagnostic was live,
and contain no `RefCell already borrowed` or other panic.
