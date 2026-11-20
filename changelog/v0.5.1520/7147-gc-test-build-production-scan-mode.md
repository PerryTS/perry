**GC / tests:** the perry-runtime unit-test build ran a *different GC root
configuration from production*. `gc::roots::conservative_stack_scan_mode()`
defaulted the test build to `ConservativeStackScanMode::Full` while production
resolves `Auto -> SkipDisabled`, and every consequence pointed in the direction
that hides defects: a missing precise root (#7055's shape) was rescued by the
native scan under test while being a live wrong answer in production, so the
suite filtered out precisely the failure class it exists to catch; the copying
minor was ineligible under test (`CopiedMinorFallbackReason::ConservativeStack`)
so relocation went largely unexercised; and a conservative scan keeps arbitrary
native-stack garbage alive, which is the wrong direction for *"this object
should have been collected"* — degrading exactly the tests most dependent on
precision. The comment justifying the split described how some tests were
written, not a requirement of testing. The correct mechanism already existed and
the tests already used it (`RuntimeHandleScope` / `gc_temp_root_*`, plus the
isolation guards, which pin `Auto` themselves): **351 of the 531 gc tests were
already running in production's mode**, so flipping the default for the rest cost
exactly one test conversion.

That conversion is `test_minor_preserves_old_to_young_edge_across_minors`, which
asserted the parent's slot still held `ptr_bits(child)` — the address captured
*before* the first minor. It only held because `Full` made the minor non-moving;
with precise roots the copying minor relocates the child and correctly rewrites
the slot (`left: 9222531965582835720` vs `right: 9222531965580476424`). Reading a
raw local across a relocating collection is the defect class this suite exists to
catch, so the test now does what generated code does and re-reads the child out
of the parent's slot after every minor. Measured: cycles 0–2 run `copied=1
promoted=0` with the remembered-set edge intact; cycle 3 reports `promoted=1` and
the child leaves the nursery, at which point the edge is old→old and the
remembered set correctly retires it — handled explicitly now, with an
`rs_covered_cycles >= 2` floor so the loop cannot go vacuous. Evidence (Mac mini,
darwin-arm64, release, `--test-threads=1`): `PERRY_CONSERVATIVE_STACK_SCAN=0` went
1573/1 → **1574/0**, default unchanged at 1574/0. Sabotage-verified both ways —
injecting `remembered_set_clear()` after the minor (the #6181 dropped edge) turns
the converted test red in *both* scan modes.

`ConservativeScanAutoGuard` is deleted: with the default already `Auto` it set the
value it was going to get anyway and could no longer fail, so per CLAUDE.md's
kill-policy it goes rather than lingering as an untested no-op. Its single user
moves to `ConservativeScanDisabledGuard`, which asserts the stronger property that
test needs (objects held only as native-stack locals must be *collected*).

**GC / runtime (#7145):** `js_shadow_frame_pop`'s corrupted-handle guard used
`debug_assert!(false, …)` **inside an `extern "C"` fn**. In a debug build the
assert fires, the panic cannot unwind across the `extern "C"` boundary, and the
process aborts — so `cargo test -p perry-runtime --lib` in the dev profile
SIGABRTed the entire test binary at
`gc::tests::shadow_stack_ops::out_of_range_frame_pop_is_ignored`, which drives
that path on purpose. CI runs the suite `--release`, where `debug_assert!`
compiles out, so the gate was structurally blind to it: a whole profile of the
runtime test suite was un-runnable and no gate could go red. Both that site and
`js_gc_temp_root_push`'s identical overflow guard (unreachable in practice, hence
never caught) now use the `report_growth_stub_skipped_below_heap_min` pattern —
`#[cold]`, one line on stderr, once per process, so it cannot perturb a stdout
parity comparison. Skip behaviour is unchanged in both profiles.

**Build:** `crates/perry-codegen/src/linker.rs:96` landed with a rustfmt violation
in #7135, leaving `cargo fmt --all -- --check` red on `main` as of `a3b31c0d8` and
`lint` failing on every open PR. Reformatted here; no behaviour change.
