**Fix the `perry-codegen` lib-test build, which `-D warnings --all-targets` rejects on `main`.**

#10484's `constructor_has_synthetic_arguments` field was added to `ImportedClass`, but two test
fixtures construct that struct literally and were never updated:
`expr/instanceof_imported_rhs_tests.rs` and `lower_call/new_builtin_shadow_tests.rs`. Both now set
it to `false`, which is the pre-#10484 behaviour they were written against.

`cargo check -p perry-codegen --lib` does not compile `cfg(test)` code, so this is invisible to the
per-crate preflight and only the workspace-wide `--all-targets` step sees it — which is why it
reached `main`. Verified: `RUSTFLAGS="-D warnings" cargo check --workspace --all-targets` (with the
usual cross-host UI exclusions) now finishes clean.
