### Fixed

- **`lint` was red on `main` and on every open PR (#7585).** #7579 added a bare
  `as *mut GcHeader` cast in `iter_result.rs` to stamp the shared iterator-result
  keys array copy-on-write, and `scripts/addr_class_inventory.py` refuses that
  outside `gc/` and `value/addr_class.rs`. It was found by an agent working an
  unrelated issue that happened to touch the same file — not by the gate
  blocking a merge, because merges were bypassing it.

  **Not fixed with a 127th allowlist entry, deliberately.** Every one of the 126
  grandfathered entries in `scripts/addr_class_allowlist.txt` carries the same
  sentence — *"migrate to `addr_class::try_read_gc_header` in a follow-up"* — so
  adding another borrows against a debt nobody is paying down.

  The reason none of them has migrated is **structural rather than neglect**, and
  it is worth writing down because it will otherwise be rediscovered:
  `try_read_gc_header` returns `&'static GcHeader`, a **shared** reference. That
  is the whole point of it — it is the safe probe for an address that might be in
  the handle band, so it must not be able to write through what it validates. But
  these call sites do not want to read a header; they want to **set a flag**. No
  amount of follow-up work makes a shared reference serve a `*mut` write. The
  migration target the allowlist kept promising did not exist.

  So this adds it. `gc::mark_shape_shared` lives in `gc/`, where the cast is
  permitted, and states the precondition the allowlist entries were implicitly
  relying on: the pointer must be the user pointer of an object this thread has
  just allocated, never an address decoded from a NaN-box payload — the same
  discipline the arena walkers are already allowlisted under, and the reason no
  handle band can reach it. `iter_result.rs` calls it and the cast is gone. The
  allowlist does not grow, and the remaining sites now have a real target.

  Also drops one stale addr-class ratchet entry that the tool itself reported as
  over-counted (`object/field_get_set.rs` `lone-valid-obj-ptr`: baseline says 1,
  found 0). Same family as #7582 — a suppression that outlived its subject, and
  a ratchet is only as tight as its least-current entry.

  Validation: `addr_class_inventory.py` passes (885 files scanned, 267
  allowlisted, 542 sites held by the ratchet); `cargo test -p perry-runtime
  --lib` 1818 passed / 0 failed, including
  `shared_iter_result_keys_are_marked_copy_on_write`, which is the test guarding
  this exact flag write; `raw_handle_debt` 998 (baseline 998);
  `check_file_size.sh` and `cargo fmt --check` clean.
