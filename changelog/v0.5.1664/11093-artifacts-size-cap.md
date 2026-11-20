`crates/perry-codegen/src/codegen/artifacts.rs` sat at 1999 lines — one line
under the hard 2000-line cap enforced by `scripts/check_file_size.sh`, a
required `lint` step. With that much headroom, any PR that added even two lines
to it was blocked outright; #11087 hit exactly that.

Split it along the phase boundaries the single 1958-line `emit_module_artifacts`
function already marked with `progress.checkpoint(...)`:

- `artifacts.rs` (1999 → 1044 lines): the closure-body loop, the method /
  fallback / imported-function wrappers, namespace globals and dynamic-import
  externs, the module entry call, the runtime registration metadata, and the
  string-pool emission.
- `class_artifacts.rs` (new, 572 lines): the per-class walk — instance methods
  and their typed-f64/i32/i1/string, indexed, and proven-`this` clones;
  computed members; instance and static accessors; the standalone cross-module
  constructor, including the synthesized `super(...args)` forwarding ctor; and
  static methods.
- `export_value_wrappers.rs` (new, 530 lines): the exported function-value
  surface — live ESM getters for re-exported native named imports, the
  `__perry_wrap_*` closure-ABI wrapper per top-level user function, the
  exported-alias wrappers, the renamed-export wrappers (#837), and the
  sanitize-mismatch raw aliases (#836).

This is a pure move: both extracted blocks are byte-identical to the originals
(verified by diffing the extracted line ranges against `git show HEAD:`), with
no renames and no behavior change. Both new modules follow the convention
already used by `ordinary_method_artifacts.rs` and
`indexed_method_artifacts.rs` — a `pub(super) XxxCtx<'a>` struct of borrowed
inputs plus a `pub(super) fn` that destructures it, declared as a plain `mod`
in `codegen/mod.rs` and imported with explicit named `use`. The new functions
take their context as `c` so that the class walk's `c.cross_module`
partial-move read carries over unchanged.

Two edits are not moves:

- `#[derive(Clone, Copy)]` on `OptsView` in `artifact_context.rs`, so the class
  phase can take the same by-value view without the artifact tail losing its
  own; every field was already a shared borrow or a scalar.
- `scripts/shape_descriptor_census_baseline.json` repoints its
  `object_header_size_bytes(target_triple)` callsite from `artifacts.rs` to
  `class_artifacts.rs`. That census pins callsites by file path, so a pure move
  reads to it as one site removed and one added; the multiset is otherwise
  unchanged (43 `codegen_object_header_size_sites`, as before).

Verified with `bash scripts/check_file_size.sh` (`OK: no Rust source files
exceed 2000 lines.`), `cargo fmt --all -- --check`, `cargo check -p perry
--bins`, `cargo test -p perry-codegen --lib` (1674 passed, 0 failed — this
covers the crate's source-scanning ratchets, `pshape_symbol_reachability` and
`spec_abi_symbol_reachability` among them, which hold hardcoded file-path
allowlists), and all 35 static `lint` gates including
`shape_descriptor_census.py`, `addr_class_inventory.py`,
`raw_handle_debt.py` (both the plain and the `--no-raise-vs` merge-base
invocation), `gc_runtime_root_holders.py`, `check_gc_header_constants.py`,
`workspace_architecture.py` and `check_test_registration.py`.
