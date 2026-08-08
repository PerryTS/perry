### Coverage for the root lowering that actually ships (#7502)

Native roots (RS4GC statepoints) have been the default lowering on every target
whose frames the runtime can walk since #7370, and had **no assertions
anywhere**. #7493 repaired `shadow_slot_hygiene` and `scalar_replaced_slot_roots`
by pinning them to `NativeRootsPin::shadow()`, which was the right repair — both
lowerings are supported and the shadow-pinned suites stay — but it made explicit
that nine root-lowering mechanics had zero coverage against the lowering Perry
emits, and that three tests reading as coverage were measuring nothing: they
asserted `js_shadow_slot_bind` was *absent*, which under the native default is
true of every program, rooted or not (CLAUDE.md hazard 4).

`crates/perry-codegen/src/native_root_coverage/` adds **8 mechanic tests and 5
harness self-tests**, in-crate `#[cfg(test)]` so they run in the per-PR
`cargo-test` gate rather than the nightly-only `tests/*.rs` tier (#5960).

**Three vantages, because each is blind to what the next one catches.**

1. *Pre-`opt` IR* — the `ptr addrspace(1)` allocas codegen asks for. The only
   place "codegen never requested a root" and "LLVM removed one" are still
   distinguishable, so the negative claims live here.
2. *Post-RS4GC IR* — each `gc.statepoint`'s `"gc-live"` bundle, keyed by callee
   name, produced by running the production pass string
   (`inprocess::STATEPOINT_REWRITE_PASSES`). Makes "is this value a root across
   *that* call" a direct question; the shadow suites had no equivalent.
3. *The emitted stack map* — per-safepoint root lists decoded back out of the
   compact `__perry_gcmap` blob the collector reads at run time
   (`gc_map::decode_stack_map_roots`, which round-trips through `encode_stream` +
   `verify_roundtrip`, so an assertion is about what the binary ships).

Both shipped native-roots targets (`arm64-apple-macosx`,
`x86_64-unknown-linux-gnu`) are compiled and emitted for on every host, **pinned
rather than host-derived**: `cargo-test` runs on x86_64 Linux and development
happens on arm64 macOS, and a suite that silently changes subject with the host
is how a target-specific lowering bug reaches a release.

**Non-vacuity is structural, not a promise.** `Statepoints::at` panics when the
callee it is asked about produced no safepoint and `map_records_for` panics when
the function is absent from the map, so "zero roots" is only ever asserted about
a record that exists; every negative claim carries a differential control in the
same test, so a lowering that roots *nothing* fails the control half. The
harness has its own coverage, which earned itself twice during development — the
callee parser initially read `@llvm.experimental.gc.statepoint.p0` for every
safepoint, and the live-set parser truncated at the `)` inside
`ptr addrspace(1)` and reported an empty live set for every statepoint in every
program. Either bug would have made every "nothing is live here" assertion pass
for the wrong reason.

**Every test is sabotage-verified** — ten sabotages, each confirmed to compile
(`error[` count 0) and to reach the test binary (`Running unittests` present)
before its verdict was believed. Each test's doc comment names its sabotage and
the numbers it moved. Highlights: emitting root allocas as `alloca double`
reddens all eight mechanics; reintroducing the explicit bridge's conservative
CFG-union liveness reddens the dead-value test **and nothing else**; removing
`root_scalar_replaced_slot`'s `root_entry_alloca` call (#6968 reintroduced)
collapses the heap/numeric difference to `3 vs 3`; and removing the
`expr_is_known_non_pointer_shadow_value` early-out takes the numeric-only
literal's map from **0 to 2 roots**, which is the direct answer to whether that
negative assertion is vacuous under its lowering.

**Two findings.**

*#7502's table is wrong about row 9.* It marks #7184's out-of-range slot index
`n/a` under native roots because "no frame bound exists". The *frame* bound is
gone, but the defect was never about a frame — it is about an index falling
silently outside the structure that collects roots, and
`lower_precise_roots_to_native_stack` still has one: it collects with
`roots.get_mut(idx)` over a `slot_count`-sized vector, so an out-of-range index
drops the alloca from `root_ptrs` and it is never retyped and never rooted, with
no diagnostic. Sizing that vector one element short removes a root from the
emitted map while the function still compiles, verifies and emits identically.
Now tested and sabotage-verified.

*`mem2reg` promoting every root alloca is a native-only precondition with no
shadow counterpart.* RS4GC relocates `addrspace(1)` **SSA values** and does not
scan allocas, so a root slot that escapes promotion is one the collector never
rewrites — the value reads as rooted and is not, which is the #7184/#7192
presentation exactly. `no_root_alloca_survives_the_statepoint_rewrite` asserts
it directly; leaking a root alloca's address to a call reddens it with two
allocas surviving.

Production surface is confined to two `#[cfg(test)]` seams
(`gc_map::decode_stack_map_roots`, `inprocess::statepoint_rewritten_ir`) and one
named constant replacing an inline string literal of identical value, so the
suite cannot go green against a pipeline production stopped using. Emitted IR is
unchanged.

Validation: `perry-codegen --lib` 724/724, `perry-runtime --lib` 1915/1915,
`cargo check --all-targets`, all 22 lint-job commands, and
`gc_root_dominance_check.py --moving-only` over the 149-module corpus — 0
violations, 40/40 seeded violations caught, 0 unrooted allocas.
