Work through this checklist by dependency readiness. Fully implement one ready task before moving to the next. Keep this branch focused on native-memory user value: safe high-throughput views, generic POD ergonomics, and proof that the public API works. Do not duplicate the separate GC cleanup pass. Validate each task against the cited issue before replying.
-----
- [x] typedarray-byte-helper-live-validation - Reuse live typed-array validation in byte helper callers
  Problem: Byte helpers such as `typed_array_bytes` and `typed_array_bytes_mut` can still trust stale `TYPED_ARRAY_REGISTRY` entries outside the new `NativeMemory` path.
  Context: `crates/perry-runtime/src/typedarray.rs`, `strict_typed_array_from_raw`, `typed_array_bytes(_mut)`, and callers such as `crypto.randomFillSync` in `object/native_module_dispatch.rs`; keep GC side-table cleanup in the separate GC pass.
  Reference: Review finding on stale/wrong registry entries outside `NativeMemory`.
  Acceptance: Forged or finalized old-arena typed-array registry entries are rejected through a non-`NativeMemory` byte-helper caller, and existing native-memory safety tests still pass.

- [x] stable-hash-native-memory-tags - Give native-memory HIR nodes unique stable hash tags
  Problem: New native-memory hash tags collide with existing `GetIterator` and `ForOfToArray` discriminants.
  Context: `crates/perry-hir/src/stable_hash/expr.rs` tags for `NativeArenaView`, `NativeMemoryFillU32`, `GetIterator`, and `ForOfToArray`.
  Reference: Review finding on duplicate tags `11238` and `11243`.
  Acceptance: Stable hash tags are unique, with a regression guard that detects duplicate `tag(h, N)` discriminants.

- [x] generic-podview-typevar-monomorph - Preserve bare type parameters in `NativeArena.podView<T>()`
  Problem: `podView<T>()` can resolve bare constrained type params through `extract_ts_type_with_ctx`, embedding the constraint instead of `TypeVar("T")`.
  Context: `try_native_arena_public_api`, `bare_type_param_type_arg`, `Expr::NativePodView.view_type`, and monomorph substitution.
  Reference: Review finding on `crates/perry-hir/src/lower/expr_call/intrinsics.rs:655`.
  Acceptance: A generic `T extends PerryPod<any>` function using `arena.podView<T>()` specializes to concrete POD layouts in HIR/codegen tests.

- [x] native-memory-generic-operand-regression - Prove generic calls inside NativeMemory operands before broad traversal changes
  Problem: Review suspected generic calls nested inside `NativeMemory.fillU32` and `NativeMemory.copy` operands may not be discovered or rewritten.
  Context: `crates/perry-hir/src/monomorph/driver.rs`, `update_call_sites.rs`, `NativeMemoryFillU32`, and `NativeMemoryCopy`.
  Reference: Review finding on wildcard fallthrough in monomorph traversal.
  Acceptance: Add focused tests for `NativeMemory.copy(makeView<T>(), other<T>())` and `NativeMemory.fillU32(makeView<T>(), value<T>())`; only change traversal code if the regression fails.

- [x] final-native-memory-evidence - Run final native-memory proof on the current branch
  After: typedarray-byte-helper-live-validation, stable-hash-native-memory-tags, generic-podview-typevar-monomorph, native-memory-generic-operand-regression
  Problem: The MVP needs one end-to-end signal that the public native-memory surface is safe and usable after the focused fixes land.
  Context: Current `origin/main`, the separate GC pass if it has landed, runtime/HIR/codegen tests, and `native-abi-proof` workloads; use Python 3.11+ for the proof runner if local `python3` is too old.
  Reference: Latest xhigh review findings across native-memory safety, compiler stability, and integration.
  Acceptance: After a fresh fetch/rebase if needed, `git rev-list --left-right --count origin/main...HEAD` reports `0 N`, `git status --short` has no unintended files, and runtime/HIR/codegen tests plus `native-abi-proof --gate` pass.
