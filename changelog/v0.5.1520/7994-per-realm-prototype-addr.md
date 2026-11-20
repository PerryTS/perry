### Fixed

- **`Array.prototype` / `Object.prototype` address caches are per-realm now, not process-global (#7988).**
  `crates/perry-runtime/src/array/prototype_addr.rs` memoized both intrinsics in
  two process-global `AtomicUsize` cells holding **raw addresses of objects in a
  thread-local arena**, while the realm they name is per-thread:
  `js_get_global_this` bootstraps `THREAD_GLOBAL_THIS` once per *thread*, but
  `resolve_prototype_addr` missed only once per *process*. The first thread to
  touch either intrinsic decided the value for every other `perry/thread` agent,
  with three consequences — all closed structurally by moving the storage into
  one `crate::perry_thread_local!` declaration:

  1. **Wrong identity.** `object_prototype_addr_matches(addr)` on agent B
     compared B's objects against **A's** `Object.prototype`, so B's own
     intrinsic was never recognised. Observable: `Object.prototype[7] = v` on a
     worker never flipped `OBJECT_PROTO_HAS_INDEX` (the write hook's "is this
     the prototype?" test compared against a foreign address), so the hole/OOB
     read fallback stayed switched off and `[1,2,3][7]` read `undefined` on
     every thread but the first. Same for `Array.prototype[8] = v`.
  2. **Unattributed dereference.** `heal_prototype_addr` read the cached
     address's `GcHeader` from *any* thread with no ownership check, and
     `note_array_index_write` calls it on every indexed array write. A's
     collector can sweep or move that object, and A's arena blocks are
     `dealloc`'d at thread exit; a stray `GC_FLAG_FORWARDED` byte there sent
     `resolve_forwarding` one word further into memory the reader did not own.
  3. **Cross-thread root rewrite.** `scan_prototype_addr_cache_roots_mut` is
     registered per thread and *writes* the cell with its **own** to-space
     address, so agent B's collector could overwrite a cell naming A's heap.

  #7974 fixed the *test* that exposed this (#7975's `dead_owner_side_tables`
  ordering flake) by driving the #6981 algebra on a cell the test owns, and
  explicitly left the shipped cells alone; the product bug survived. Same family
  as #7954 (a process-global promotion veto) and #7981 (a class-parent edge read
  from a shape stamp).

  **The recorded obstacle was stale.** #7988/#7955 record the objection as "the
  accessor is on `note_array_index_write`, the not-forwarded case must stay
  call-free, and Darwin has no local-exec TLS". True of `std::thread_local!`;
  not true of `crate::perry_thread_local!` (#7469, `tls_hot.rs`), which puts the
  value's address in this thread's `HotTls` cache — an `mrs` plus two loads that
  LLVM CSEs across the enclosing function rather than an out-of-line
  `_tlv_get_addr` call. Both intrinsics now share ONE declaration (a
  `[Cell<usize>; 2]` indexed positionally against `PROTOTYPE_ADDR_BUILTINS`), so
  a function that consults both — `array_oob_prototype_get` does — pays one
  resolution instead of two, and the `globalThis` walk moved behind a `#[cold]`
  boundary so the hot arm is a slot load, a compare and a branch. The root
  scanner iterates that array itself, so the #6981 invariant ("every cell an
  accessor reads is a cell the collector rewrites") stays true by construction.

  Coverage: `a_second_agents_prototype_addresses_are_its_own` (in
  `gc::tests::runtime_roots::prototype_addr_cache`, so `cargo-test`-visible) —
  two live agents, bootstraps serialized and both threads held live across the
  comparison by a barrier, must memoize **different** addresses; each address is
  asserted to be a real one (non-zero, not the `usize::MAX` sentinel) *before*
  the distinctness check, so a green verdict cannot be earned by two agents that
  resolved nothing. `the_shipped_cells_are_the_ones_the_scanner_visits` gained
  the assertion that every accessor's row lies inside the array the collector
  rewrites. `test-files/test_issue_7988_thread_realm_prototype.ts` is the
  behavioural multi-agent probe (main thread warms both intrinsics first, which
  is what makes it discriminating rather than lucky). The gap suite and the
  compile corpus are **vacuous gates** for this change — nothing in either uses
  `perry/thread`.
