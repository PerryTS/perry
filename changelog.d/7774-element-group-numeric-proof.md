**repsel: element-group members now claim numeric fields (~10-12% on the target read loop) — proven reads drop `js_number_coerce` (#7770, PR #7774).**

A `Ptr<Shape>`-proven `const r = a[i]` (or a producer pushed into an
element-shape-proven array) stood down to zero numeric fields, so every
declared-`number` field read paid the Phase-5a checked load with a cold
`js_number_coerce` arm. The stand-down existed because one member's stores
cannot witness a sibling's — but the E1–E5 containment that licenses the
SHAPE proof also closes the group's store universe, so the exhaustive
reachable-store proof is now discharged once per array root
(`collectors/ptr_shape_numeric.rs::prove_group_numeric_fields`): constructor
parameters resolve as the meet over every push's `new` argument list, member
field stores are unioned, and method parameters resolve through group-merged
call sites. Every member carries the group verdict; group integrity drops
claim and fact together when any member fails rule 2.

Constructor arguments like `new P(i, i + 1)` additionally needed the loop
counter: `collect_numeric_by_construction_locals` proves locals whose every
write is number-producing by construction (optimistic greatest fixpoint like
`collect_not_bigint_locals`, with no declared-type leaf — annotations stay
untrusted; a no-init `let` poisons). The expression proof consults it in
function scope, and `i++` as a value resolves through the not-BigInt fact.

On the issue's reproducer the read loop's `js_number_coerce` sites go 4 → 0
while `--opt-report` still shows the `Ptr<Shape>` promotion; output verified
byte-identical vs Node 26.5.1 across sibling/push-site/method poison
channels, NaN / Infinity / −0 payloads, and `null`/`{}`/`1n`/`true` stores
(`test-files/test_gap_repsel_element_group_numeric.ts`), and a
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` run relocated 4014 objects
under the bare loads with a clean verdict. On the pinned quiet mini
(interleaved, best-of-15, two runs) the issue's read loop goes 101/102 ms ->
91/90 ms while `batch.ts`, `suite/04_array_read` and `suite/09_method_calls`
are unchanged; the A/B's subject is verified live (base arm emits 4
`js_number_coerce` sites and 12 checked-load diamonds on the benchmarked
source, branch arm zero) -- note that benchmarking this needs the array and
its read loop in ONE function, since an array crossing a function boundary is
the #7766 shape this change does not address. Pass 4 moved wholesale into the
`ptr_shape_numeric.rs` child module for the 2000-line gate. A neighbouring
PRE-EXISTING divergence found during validation (`o.x + 1` coercing where
Node concatenates, plus an evaporating any-laundered add) is filed as #7773.
