Follow-up to #7117, which made the census count *consumed* promotions rather
than selected ones. Two gaps in that instrument, one found by re-running the
sabotage matrix and one raised in review.

## Two consumption recorders had never fired

`Ptr<Shape>` consumption is recorded at six codegen sites. The consumed count
is per **value**, so one working recorder marks a value consumed and the other
five can rot silently — the limitation #7117 shipped as known-and-ungated.

Measured, it was worse than "ungated": **two of the six had never fired on any
workload in the corpus.**

| site | file | fired before |
|---|---|---|
| `ptr_shape_get_number` | `expr/property_get/helpers.rs` | yes |
| `ptr_shape_set` | `expr/property_set.rs` | yes |
| `class_field_get.shape_proven_load` | `expr/property_get.rs` | yes |
| `ptr_shape_method` | `lower_call/property_get/dynamic_dispatch.rs` | yes |
| `class_field_get_number.shape_proven_load` | `expr/property_get/helpers.rs` | **no** |
| `ptr_shape_update` | `expr/instance_misc1.rs` | **no** |

Both are reachable; nothing reached them. `fixture_ptr_shape_sites.ts` does,
and it took a specific shape to get there — an in-loop **plain field store** is
what defeats scalar replacement (#7115). With only the `++` update the object is
deleted and nothing is consumed at all, which is the same trap #7117 documented
one level up.

- `site` is now a first-class `Entry` field and is what `Consumed` entries dedup
  on, so one value consumed at three sites stays three facts.
- `CONSUMPTION_SITES` registry in the script, not the baseline. Every registered
  site must fire somewhere in the corpus; a consumption recorded at an
  unregistered site (or with no site) raises rather than being absorbed.
- Corpus: **7 selected, 3 consumed**, all six sites exercised.

## Review of #7117

**Accepted.**

- The census self-test's "missing analyses" fixture still declared
  `schema_version: 1` after `SUPPORTED_REPORT_SCHEMA` moved to 2, so it raised
  on schema drift and the branch it names went unexercised. It now tracks the
  constant.
- The README's IR-comparison recipe wrote both arms to the same path — and
  `--no-link` does not honour `-o` at all, so neither file existed. The
  documented procedure could not have reproduced anything. Replaced with one
  that captures the printed object path, verified end-to-end.
- `render_text` printed the consumption line only when a count was nonzero, so
  an analysis whose promotions were **all** wasted (`selected > 0`,
  `consumed == 0`, no mechanism — what a `PERRY_CANONICAL_I32_LOCALS=0` build
  produces) read identically to an uninstrumented one. Instrumentation is now a
  property of `Analysis` (`records_consumption()`), serialized, and the census
  cross-checks it against `CONSUMPTION_INSTRUMENTED` so the two tables cannot
  drift.
- `check_unconsumed_is_explained` summed the gap across analyses; a mechanism
  recorded for one representation could excuse a silent gap in another. Now per
  analysis.
- The NOT-INSTRUMENTED list keyed off a `-consumed` name suffix rather than the
  table; `seen_consumed`'s annotation described a 2-tuple while a 3-tuple is
  inserted.

**Rejected, with evidence.** "Unconsumed entries are not deduplicated per value,
so the mechanism totals count access sites." They are already per value: an
unconsumed entry carries no `site`, so `Entry::dedup_key` collapses them in the
compiler. On `batch`, `totals` has two access sites and reports one
`module_init_context`. Documented rather than duplicated.

## A previously-passing sabotage arm had gone green

Re-running the matrix after the per-site change (rather than assuming it still
held) caught this: once `site` entered `dedup_key`, stripping `outcome` from the
key no longer turned anything red, and the unit test asserting that property
built a consumed entry with **no site** — a shape the recorders never emit, so
it passed vacuously.

`outcome` is still correct to keep, but for a different pair (`Unconsumed` vs
`Denied`) and **it cannot currently be observed to matter**: a denied value was
never selected while an unconsumed one always was, so no binding emits both. The
docs and tests now say which element defends which pair, and label `outcome` as
defensive with a hand-constructed test as its only exercise rather than implying
gate coverage that does not exist.

## Verification

- Byte-neutral: **24/24** workloads identical with the report off vs on, and
  **24/24** between the pre-change and post-change compilers with the report off.
- Nine sabotage arms red, control green: dropping `site` from `dedup_key`,
  removing all six consumption recorders, removing **either single** uncovered
  recorder (new — this is the gap that is now closed), removing either mechanism
  recorder, counting per access site, and folding proven-`this` consumption into
  the local column.
- No floor lowered against `origin/main`; `batch` `ptr-shape` still 2.
- CI's sabotage step additionally asserts a consumption site goes dark.
