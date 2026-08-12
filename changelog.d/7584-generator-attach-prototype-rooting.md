### Fixed

- **`js_generator_attach_prototype` derefed retired from-space memory (#7577).**
  It bound the generator instance's address at function entry and used it at the
  tail, with four allocating calls in between — and **this frame owns the only
  reference**: codegen drops the caller's statepoint root immediately before the
  call (`store ptr addrspace(1) null, ptr %rN` ahead of the `call`), which is the
  ordinary contract for a runtime helper except that this one never rooted its
  argument.

  Everything it calls allocates: `wrap_async_generator_instance` (three closures
  plus three keys-array transitions), `generator_prototype_ptr` (lazily builds
  the whole generator intrinsic tower on first call), `js_object_alloc`, and
  `object_set_static_prototype` itself, whose `object_meta_ensure` mints the
  object's meta record out of the arena. A copying minor in any of those windows
  gave **two** wrong answers with no diagnostic: the `[[Prototype]]` link was
  recorded against the **pre-move** address, so `Object.getPrototypeOf(gen())` on
  the live object found nothing; and the function **returned** that pre-move
  address, so the caller's generator was a dangling pointer into retired
  from-space. Under the #7154 instruments that is a SIGBUS; without them it is a
  wrong answer and exit 0.

  Same shape fixed in `js_generator_attach_closure_prototype`,
  `generator_function_prototype_of`, and `wrap_async_generator_instance` /
  `set_method` below them — fixing only the named function would have handed a
  freshly re-read receiver to a callee that lets it go stale again. Every pointer
  now comes back out of a `RuntimeHandleScope` handle after the call that could
  have moved it, and `generator_prototype_ptr` is called a second time rather
  than cached (it reads a GC-rooted atomic the collector rewrites, so a fresh
  call *is* the re-read). `make_method_wrapper` is **deleted** rather than fixed:
  taking `original` as a parameter forced every caller to bind that pointer
  before the `js_closure_alloc` inside, so the capture store wrote a
  pre-collection address — per CLAUDE.md's kill-policy the losing shape stops
  compiling.

  **Reproduced deterministically, not under zeal.** A moving collection needs a
  safepoint and there is none inside the function (allocation-triggered minors
  force a conservative scan, which makes the copying minor ineligible), so the
  issue's end-to-end reproducer does not fault reliably even with the instrument
  proven live and `copied_objects` in the thousands. The new tests inject the
  collection into the window instead: `force_next_general_arena_alloc_slow()` +
  `GcTriggerThresholdTestGuard::make_arena_trigger_due()` arm the next arena
  block allocation to collect, and the next one is the callee's own.

  Coverage: `crates/perry-runtime/src/gc/tests/runtime_roots/generator_attach_prototype.rs`
  (two `cargo test -p perry-runtime` tests, one per entry point, each asserting
  its subject was live before asserting anything else, and each calling
  `register_runtime_handle_root_scanner_for_tests()` — without which the
  `RuntimeHandleScope` under test is decorative and the test passes for the
  wrong reason). Plus `test-files/test_gap_7577_generator_prototype_rooting.ts`,
  byte-identical to `node --experimental-strip-types` and clean under
  `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1
  PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`.
