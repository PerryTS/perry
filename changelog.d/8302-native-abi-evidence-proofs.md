### Fixed

- Restored native ABI evidence for compiler-owned Buffer, typed-array, arena,
  POD-layout, and packed numeric-array values. Buffer numeric reads now retain
  stable pointer facts (including `native_u32`), while optimized native paths
  stay distinct from semantics-preserving erased-annotation fallbacks.

  The failure came from runtime-derived identities being dropped after
  TypeScript annotation trust was tightened: constructor facts were not carried
  through `crates/perry-codegen/src/codegen/module_globals_emit.rs` and
  `crates/perry-codegen/src/type_analysis/refine.rs`, Buffer numeric reads in
  `crates/perry-codegen/src/lower_call/buffer_intrinsic.rs` did not attach their
  view facts, and `crates/perry-codegen/src/collectors/ptr_shape_numeric.rs` did
  not recognize native-view or POD-layout values as Number-producing. The repair
  preserves those facts while explicitly excluding BigInt-backed typed arrays
  and retaining guarded array stores. Regression coverage in
  `crates/perry-codegen/tests/native_proof_buffer_views.rs`,
  `crates/perry-codegen/src/collectors/ptr_shape_group_numeric_tests.rs`, and
  `tests/test_native_abi_contract.sh` verifies the restored `u32`/`f32` records,
  native numeric additions, BigInt mixed-add dispatch, and the full native-ABI
  contract; the native ABI compiler-output suite validates the optimized and
  fallback IR gates.
