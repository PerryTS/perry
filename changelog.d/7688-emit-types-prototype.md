### `--emit-types` — write the proven types back out as TypeScript (#7685, EXPERIMENTAL)

`--opt-report` (#6952) surfaces the *negative* half of representation
selection: which values Perry could not statically type, and why. The positive
half — the types it had to prove in order to pick an unboxed representation —
was computed and discarded. `--emit-types <PATH>` writes it out (`.json` →
records, anything else → TypeScript).

A **prototype**, behind a flag, marked experimental in `docs/src/cli/flags.md`,
not wired into `perry check`, gating nothing. No new analysis: a second consumer
of the existing `opt_report::Entry` stream.

**The result: a representation is not a type.** Perry has five representation
analyses. After auditing each against "never emit a wrong type", exactly **one**
licenses a TypeScript type:

| analysis | emitted | why |
|---|---|---|
| `Ptr<Shape>` | **yes** | a real value proof — exact dynamic class by provenance + containment |
| canonical `I32`/`U32` | no | a *storage* proof; the JS value can still be `undefined` (non-dominating writes to a `var` seed; `int_valued_ta` members merge into `integer_locals`) or `bigint` (the bitwise arm ignores operand types, and `not_bigint_locals` is not a term in the admission conjunction) |
| canonical `Str` | no | annotation-derived, and *designed* to tolerate the annotation being false — "a type-annotation lie degrades to today's behavior" (`expr/slot_rep.rs`) |
| `IntValuedTa` | no | self-refuting: its own module doc says an OOB read yields `undefined`, and the rep is sound *only* because every context that could observe that is forbidden. An annotation is such an observation |
| `Ptr<NumArray>` | no | `HolesOk` slots read back `undefined`; `test-files/test_gap_repsel_p4a3_ptr_numarray.ts` already pins `undefined 2 undefined` on a promoted local. True type is `(number \| undefined)[]` |
| spec-ABI | no | the most frequent argument tuple across call sites, disagreeing callers demoted behind a guarded entry — a majority, not a proof |

These representations were chosen to be *observationally equivalent* to the
boxed form, which is strictly weaker than "the value has this type" — and in
three cases the equivalence holds precisely **because** the value is never
observed in a context that could tell the difference.

**Measured, not demonstrated** (`scripts/emit_types_accuracy.py`). Round-trip
mode **erases** local annotations first, which is load-bearing rather than
hygiene: `stmt/let_stmt.rs` computes `refined_ty` as the declared type whenever
it is not `Any`, so measured un-erased, every "recovered" type is the annotation
handed straight back and the score is 100% and means nothing.

| corpus | files | recovered |
|---|---|---|
| `benchmarks/{repsel_census,suite,app-patterns}` (erased `.ts`) | 25 | 6 bindings; 22 of 25 files → zero |
| `test-files/*.ts` (erased), wide pre-audit mapping | 301 | 2 bindings; 300 files → zero |
| **real dependency JS** (lodash, semver, debug, chalk, …) | **150** | **0 bindings / 400 local declarations = 0.00 %; 0 structural shapes** |

The 17 bindings the pre-audit mapping found on dependency JS came entirely from
the four withdrawn arms. Benchmarks still recover 6, which is the positive
control that emission is alive rather than broken. The differentiating output —
a structural interface recovered from untyped JS — fires **zero** times on real
dependency JS, consistent with the repsel census recording `Ptr<Shape>` at ~7
promotions across an 18-workload corpus.

**Two claims corrected by measuring rather than reading.** Spec-ABI is *not* an
echo of the source annotation: `spec_abi::select_dominant_tuple` counts
call-site argument tuples, and an A/B with the annotations removed still reports
`i32,i32`. The stronger reason to drop it is that a function called four times
with numbers and once with a string also reports `i32,i32`. And the
synthetic-class filter was prefix-only, leaking `__anon_class_<id>`, `Box$num`
(generic monomorphization) and `Name$2` (scope-collision rename) — each a real
class name reachable at a `Ptr<Shape>` provenance site that would have emitted
TypeScript naming a nonexistent type. It now also rejects any name containing
`$`. The original list carried an `__EmptySite_` prefix that matches nothing in
the tree.

**#7234 does not block this**, and not merely because `--profile perry-dev`
inherits `release` and disables `debug_assert`s: the panicking assertion is in
`opt_report/render.rs::rule_buckets`, which only the opt-report JSON renderer
calls. `--emit-types` renders through `emit_types.rs` and never reaches it.
150/150 dependency-JS files compiled clean.

**Known limitation, stated in code rather than papered over.** `recover`'s
disagreement guard cannot fire on a real compile: `take_entries` de-duplicates
before any consumer sees the stream, and `Entry::dedup_key` omits `local_id`,
`rep` and `shape_class`, so two `Selected` rows for one name with different
classes collapse upstream. Closing that means widening `dedup_key`, which is
`--opt-report`'s contract.

**Producer-side change**, report-only and allocation-free when the report is
off: `Entry` gains `shape_class` and `shape_fields`, both
`skip_serializing_if = "Option::is_none"` so an entry with neither serializes
byte-identically to the pre-#7685 schema. They carry the `Ptr<Shape>` class name
and declared field set as *data* — `detail` already rendered them as prose, and
recovering a type by parsing an English sentence is a wrong-type bug waiting for
someone to reword the sentence.
