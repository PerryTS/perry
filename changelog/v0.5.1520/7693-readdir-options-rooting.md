### Fixed

- **`fs.readdir`'s options object is rooted across the `withFileTypes` key allocation (#7274).**
  `fs/dirent.rs::options_with_file_types` decoded a raw `*const ObjectHeader` out of
  the NaN-boxed `options` argument, then called
  `js_string_from_bytes(b"withFileTypes")` — a collection point — and then
  dereferenced the address it had computed *before* the collection. `options_value`
  is a plain Rust `f64` local: nothing kept the object alive and nothing rewrote the
  pointer. `{ withFileTypes: true }` is a fresh object literal at the call site, so
  it is a nursery object — precisely the generation an evacuating minor relocates.

  The allocation is now hoisted above the decode (bound together with
  `RuntimeHandle::across_nanbox`, so no pre-collection address is nameable) and the
  options value is rooted in a `RuntimeHandleScope`. The decode itself — the
  POINTER_TAG / raw-address forms plus the #7259 `is_handle_band` floor — is
  factored into a single `options_object_ptr` helper: it appeared three times in
  this file, and the drift between two of those copies is exactly what the bug was.
  `options_field_value`, 40 lines below, already had the correct shape.

  **Which configuration this bites in**, stated precisely rather than overclaimed:
  `gc_check_trigger`'s alloc-point arm engages
  `ManualGcScanGuard::force_full_scan(NurseryChurnSlackValve)` (unconditional since
  #7682), and when that guard *engages* the conservative native-stack scan both
  retains the raw local and makes the copying minor ineligible
  (`CopiedMinorFallbackReason::ConservativeStack`) — so the pre-fix code survived.
  It does not always engage: `force_full_scan` is a no-op when
  `CONSERVATIVE_STACK_SCAN_OVERRIDE` is already set, and an explicit
  `PERRY_CONSERVATIVE_STACK_SCAN` env value beats any pin, so
  `PERRY_CONSERVATIVE_STACK_SCAN=off` removes the valve and the alloc-point minor
  evacuates. The masking mechanism is the bounded valve #7148 documents *as* a
  bounded valve, not a guarantee.

  Witness: two knob-free unit tests in
  `gc/tests/runtime_roots/fs_options_object.rs` drive a real evacuating minor from
  inside the function's own key allocation, in exactly that configuration. The
  subject is asserted live — the options object is held by nothing but the function
  under test, and a separately rooted sentinel must come back at a different address,
  so a cycle that moved nothing cannot certify the file. That assertion paid for
  itself immediately: the first draft pinned `force_legacy_gc_pacing()`, which routes
  the trigger to the non-moving budgeted stepper, and the test failed rather than
  passing vacuously. Sabotage-verified: restoring the decode-then-allocate order
  fails the positive test.
