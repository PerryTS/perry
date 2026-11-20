### ci: refresh the three stale `compiler-output-regression` expectations (#10784)

`compiler-output-regression` had been red on **every** `main` sweep for 18 days
— last green `0b24670dd9` (2026-09-02T11:45Z), first red `3a7c0b0a81` the same
day. It is a `main-gate` input, so every merge in that window bypassed it.

The six failing verdicts were three unrelated events, none of them a compiler
regression. Each is refreshed with the measurement that established that, and
each refresh is accompanied by a sabotage showing the gate can still go red.

**1 — `vectorization_expectation` on `dynamic_fractional_array_index`,
`vectorized_buffer_transform` and `hir_fact_rewrite`
(`unexpected_missed_reason_kinds: ["control_flow"]`).**

`allowed_missed_reason_kinds` is a *module-wide* sweep, and the remark it was
catching is emitted by `main`'s compiler-emitted **event loop**, which every
Perry binary carries — not by any of the three fixtures. Three independent
measurements:

* `opt -passes='loop-simplify,print<loops>'` finds exactly **one** loop in
  `dynamic_fractional_array_index`'s whole module, in `main`
  (`event_loop.header / check_pending / body / body_check / body_wait / exit`).
  The fixture source contains **no loop at all** and still reports
  `control_flow: 1`.
* A bare `console.log(1)` program emits the identical five remarks.
* Reconstructing the pre-#9441 three-block event loop in the emitted IR removes
  exactly the `Control flow cannot be substituted for a select` remark and
  leaves the other four, all of which were already allowed.

Attributed to `b13a20095c` (#9441, "the event loop looks before it sleeps"),
which added the `event_loop.body_check` / `event_loop.body_wait` diamond LLVM
cannot if-convert. That commit is a real fix (1082 ms → node-comparable on a
20 ms `setTimeout`), so the expectation moves, not the codegen. `cb877d61f3`
later added a second conditional back edge from `event_loop.exit` to the
header, which emits the same remark independently — reverting #9441 alone would
no longer silence it. `control_flow` was already allowed in 22 of the 25
workloads (#8490, #8854 are the precedent); these were the last three.

*Sabotage:* dropping a different allowed kind (`call_instruction`) from
`dynamic_fractional_array_index` turns the workload red with
`unexpected_missed_reason_kinds: ["call_instruction"]`; raising
`vectorized_buffer_transform`'s `min_vectorized_loops` to 99 turns it red on the
positive half (`vectorized_count: 2`). Both halves of the check stay live.

**2 — `raw_numeric_field_get_guard` plus three
`native_reps_required_raw_class_field_get_*` on `raw_numeric_object_fields`.**

One changed string seen twice: that workload is a member of *both*
`SUITES["native-region-proof"]` and `SUITES["native-abi-proof"]` with identical
arguments, so four errors appeared as eight job-log occurrences.
`fc54e1b181` ("codegen: collapse the class-field GET tower to one exit")
replaced the miss-arm call sites with a single `js_class_field_get_ic`, updated
its own codegen unit tests, and missed `workloads.toml`.

* The three `require_records` now name `js_class_field_get_ic`. Every other
  field of those records — `access_mode`, `materialization_reason`,
  `fallback_reason`, and all three rejected-fact triples — is recorded exactly
  as before; the consumer string was the only difference.
* `raw_numeric_field_get_guard` was a bare `contains` for
  `js_typed_feedback_class_field_get_guard` over the **optimized** IR. That
  symbol is now emitted nowhere as a call — only as a leftover `declare` in the
  unoptimized IR, which is why fc54e1b181's own tests assert *call* forms. The
  property it names still holds, so the check is re-pointed rather than
  deleted: a function-scoped regex over `llvm_before` asserting that
  `class_field_inline.deref.N` loads the `@perry_class_guard_shape_*` latch and
  branches **into** `class_field_get.fast.N`. Its sibling
  `raw_numeric_field_get_scoped_raw_load` asserts the raw load on the other side
  of that branch.

*Sabotage:* reverting one `consumer` back to
`js_object_get_field_by_name_f64` → `matches=0`, red. Pointing the guard regex
at `@perry_class_guard_shape_NOT_EMITTED_` → red. Keeping the guard load but
requiring the branch into a block that is not the fast arm → red.

**3 — `no_dynamic_property_runtime` on `native_owned_typed_views`.**

This one was **mis-diagnosed upstream** as an unattributed ≤2026-09-05 onset
with a 267-commit window and `a2ffa80a4c` ("retire receiver fact tables") as the
leading suspect, possibly a minor alias-analysis regression. Reading the retained
CI artifacts instead of rebuilding settles it, and the answer is different:

* At the last-green SHA `0b24670dd9` the artifact's optimized IR contains **zero**
  `js_dyn_index*` calls, and `mutableAliasFallback`'s `TypedArraySet` already
  recorded `access_mode: dynamic_fallback`, `fallback_reason: mutable_alias` —
  lowered to a single bare `js_typed_array_set`, which is not on the watched
  helper list.
* Today the very same record is produced, byte-for-byte in every field. Only the
  *shape* of the fallback changed: `d792c0a761` ("perf(codegen): inline element
  access on declared typed arrays with unproven indexes") routes a declared
  receiver that misses the native lowering through the guarded inline arms, whose
  cold exit is `js_dyn_index_set_strict` — a helper that *is* on the list.
* Narrowed by job log: `6bab431189` (2026-09-15T17:18Z, without `d792c0a761`)
  passes; `ea42912d50` (2026-09-15T21:26Z, with it) fails.

So the alias analysis never "stopped seeing through" `const alias = view`: it did
not see through it before either, and the workload *requires* that fallback
(`native_owned_mutable_alias_fallback`, which has passed throughout). **Designed
fallback, and not even a changed one** — a naming coincidence in a raw substring
sweep.

Rather than set `allow_dynamic_property_runtime = true` (which would also stop
catching a real degradation in `nativeOwnedPositive`, the whole point of the
check), the sweep is scoped: it now walks **function bodies** and exempts a
function whose own native-rep record declares a fallback the workload already
listed in `native_rep_checks.allow_materialization_reasons`. Two side effects
worth naming: a module-scope `declare` for a helper nothing calls no longer
counts, and unlabelled entry blocks are now covered (the existing
`extract_blocks_with_functions` helper keys on label lines and silently drops
them, so a sweep built on it could not see a call in the entry block).

*Sabotage:* injecting `js_dyn_index_get` into `nativeOwnedPositive` or
`nativeOwnedKinds` → red, naming the function. The same helper in
`disposedFallback`, which declares `use_after_dispose` → still green. Removing
`mutable_alias` from `allow_materialization_reasons` → red on
`mutableAliasFallback`. Five new unit tests pin all of this, and both
implementation mutations (sweep always empty; label-keyed splitter) fail them.

**4 — incidental.** The `#8489` comment on `native_owned_typed_views`'
`control_flow` attributed it to #8457's `-O3 → -Os` switch. Measured, the remark
appears identically at `-O3`, `-O2` and `-Os`, and only `-Oz` suppresses it, by
disabling the vectorizer wholesale. The corrected note names what the three
remarks actually are: two are the GC loop safepoint poll
(`load volatile @PERRY_GC_POLL_ARMED` guarding a `js_gc_loop_safepoint()` call,
default-on since #7721) in `nativeOwnedPositive`'s two loops — flattening just
those two diamonds in the emitted IR drops `control_flow` from 3 to 1 — and the
third is `main`'s event loop.

**Validation.** Run as `test.yml`'s `compiler-output-regression` job invokes
them, against the pinned v0.5.1611 compiler: `suite --suite native-region-proof`
and `suite --suite native-abi-proof` both `failed_workloads: []`; the
`vectorized_buffer_transform` and `hir_fact_rewrite` captures exit 0;
`python3 -m unittest tests.test_compiler_output_regression` (85 tests),
`tests.test_native_abi_evidence_report` (19) and
`tests.test_typed_feedback_runtime_evidence` (1) all pass; the
`compiler_output_step_liveness.py` self-test and check pass. A 25-workload
before/after sweep shows the 20 workloads this change does not target produce
**byte-identical** structural reports (19 verbatim; `fma_contract` differs only
in the per-run temp scratch directory echoed into a `clang_args` detail string).
The two `--march=haswell` FMA steps could not be exercised on this arm64 host —
clang rejects the x86 `-march` before the harness verifies anything, and neither
file this change touches is on that path.
