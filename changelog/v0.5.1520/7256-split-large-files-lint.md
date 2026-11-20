**Split the 16 files that had drifted past the 2,000-line `lint` gate.**
`scripts/check_file_size.sh` is a required step of the `lint` job and 16 tracked
Rust files were over the cap. Each was split along an existing topical seam into
sibling modules, re-exported so every existing path resolves unchanged — a pure
refactor with no renames, no signature changes, and no new allowlist entries.

Largest cuts: `native_module/callable_exports.rs` 2596→1851 (the
`CALLABLE_EXPORT_ARITY_TABLE` static and its tests peeled into
`callable_export_arity_table.rs`), `native_module/constants.rs` 2427→1410
(zlib/crypto/errno/signal const groups → `constants_tables.rs`),
`native_module/callable_export_check.rs` 2218→1106 (`CALLABLE_EXPORT_TABLE` →
`callable_export_table.rs`), `lower/expr_call/native_module.rs` 2046→716 (five
receiver-dispatch blocks → `native_module/`), and the two GC test files
`gc/tests/layout_trace.rs` 2152→425 and `gc/tests/runtime_roots.rs` 2035→604.

Two hazards specific to these files shaped the seams:

- Three `native_module` files carry oracle tests that harvest *every string
  literal in their own source* via `include_str!("<self>.rs")`, one of them
  asserting a `checked > 100_000` floor. A naive split silently shrinks that
  literal universe — the test keeps passing while covering less. Those sites now
  `concat!(include_str!(trunk), include_str!(sibling))`, preserving the set
  exactly.
- `CALLABLE_EXPORT_ARITY_TABLE` and `CALLABLE_EXPORT_TABLE` are `binary_search`
  targets (order is load-bearing) and the `BUFFER_*`/`SQLITE_*`/`ASSERT_*`
  slices are `for`-iterated into observable `for…in` order. Every table moved as
  one intact item; none was partitioned.

The `addr_class_inventory.py` ratchet baseline and its whole-file allowlist
entries are **path-keyed**, so a ratcheted or allowlisted site moving to a new
sibling is a hard gate failure. Both traps fired during the work; the seams were
re-picked so every such site stays in its original file
(`proto_chain_contains_real_array`, `non_array_object_receiver` and
`plain_object_value` are deliberately kept in `array/generic.rs`). The
`addr_class_inventory.py` and `gc_store_site_inventory.py` finding sets are
byte-identical to before, and no gate script or allowlist was edited.

Verified by: `cargo check --tests` clean on all six crates with an unchanged
warning set; crate-wide Rust-token multisets conserved across 5.39M tokens with
zero non-plumbing losses; the GC test files reassembling byte-identically from
parent + children with `cargo test` reporting the same 47 and 77 passing tests;
and unchanged `pub extern "C"` symbol lists for the FFI-heavy `perry-stdlib` and
`perry-ext-net` files.

Note that this does not by itself make `lint` green: the job stops at the first
failing step, and three earlier/later steps on `main` fail independently —
`ci_public_baseline_check.py` (both artifact fingerprints stale for 40+
commits), `gc_store_site_inventory.py` (19 unaudited store sites) and
`addr_class_inventory.py` (2 ratchet regressions plus a stale allowlist
substring). Those are tracked separately.
