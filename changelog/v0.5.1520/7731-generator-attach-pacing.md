Fixed `cargo test -p perry-runtime --lib` being red on `main` — all three
`gc::tests::runtime_roots::generator_attach_prototype` tests
(`attach_prototype_survives_an_alloc_point_copying_minor_inside_the_call`,
`attach_closure_prototype_survives_an_alloc_point_copying_minor_inside_the_call`,
`the_shipped_default_defers_the_trigger_out_of_the_callees_window`) were
failing, which blocks every open PR via the per-PR `--lib --bins` gate.

Bisected to #7723's `per_test_global!` conversion of the six
generator-intrinsic-tower `AtomicI64` statics
(`GENERATOR_FUNCTION_INTRINSIC_PTR` and siblings). That conversion is
correct — it gives each test a guaranteed first-touch "tower not yet built"
state instead of leaking cross-test priming through the process-global
statics — but it exposed a pre-existing bug in this test file's own setup
helper: `warm_generator_intrinsics()` called
`js_generator_attach_prototype(TAG_UNDEFINED, 0)` to pre-build the tower
before the timed call under test, and that call returns at its very first
line for any non-pointer `obj`, so it never reached
`generator_prototype_ptr` / `ensure_generator_intrinsics()` — it was always a
no-op. Pre-#7723 this went unnoticed because some earlier test in the same
binary had almost always already built the process-global tower, so the real
call under test found it cached regardless.

With the towers per-test, the real call now pays the tower's own
dozens-of-allocations build inside `build_generator_tower`'s `GcSuppressScope`
(#7251's no-move window for that build), which swallows the test's injected
arena trigger for the rest of the call — no copying minor ever runs, and by
the time the window closes the following allocations no longer need a new
arena block, so the trigger is never serviced.

Fixed `warm_generator_intrinsics()` to call
`crate::object::ensure_generator_intrinsics()` directly (the same builder
`gc::tests::lazy_intrinsic_towers` uses), so it does what its name and doc
comment always claimed. The tests' own liveness/deferral assertions are
unchanged.
