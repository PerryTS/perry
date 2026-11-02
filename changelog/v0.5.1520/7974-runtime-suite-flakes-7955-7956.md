### test(runtime): the last two `perry-runtime` suite flakes — a shared prototype-address cache, and a wall-clock pause assertion

Closes #7955 and #7956, the two members of the #7946 flaky set that #7954 named
rather than papered over. Both are test-side; no shipped behaviour changes.

**#7955 — `gc::tests::runtime_roots::prototype_addr_cache`.** All five cases in
that file drove the SHIPPED `ARRAY_PROTO_ADDR` / `OBJECT_PROTO_ADDR` statics, so
their real racing partner turned out to be *each other*: libtest schedules a
module's tests concurrently, and
`prototype_addr_cache_scanner_leaves_the_unset_sentinel_alone` stores
`usize::MAX` into both cells while a sibling has a synthetic forwarded stub
planted there. Co-scheduling them deliberately reproduces it at **28 failures in
400 runs** (`--test-threads=8 prototype_addr`), every one of them
`prototype_addr_cache_is_rewritten_by_the_collector` reading back `usize::MAX`.
The 1-in-100 full-suite failure #7955 reports is the same mechanism with the
partner arriving by luck. #7954's save/restore guard made it worse rather than
better — restoring the value read at test entry stamps a stale address over
whatever another thread resolved meanwhile.

Both #6981 defences are algebra over an `&AtomicUsize` (`heal_prototype_addr`
already took one), so `memoized_prototype_addr` and the new
`rewrite_prototype_addr_slot` take the cell as an argument and every mutating
case now owns its cell. Nothing hands out a writable reference to the realm's
real intrinsic cells any more.

What that decomposition would otherwise lose — *the collector rewrites every
cell an accessor reads*, the #6981 invariant the old test proved by mutation —
is not recovered by another test but by construction: the accessors, the
`globalThis` builtin each resolves, and the root scanner's visit list are now
one table (`PROTOTYPE_ADDR_CACHES`), so a cell some accessor reads and the
collector never rewrites is unrepresentable. One new read-only case
(`the_shipped_cells_are_the_ones_the_scanner_visits`) pins the table itself —
two DISTINCT cells, each paired with its builtin — and writes nothing, so it
cannot be raced either.

`per_test_global!` was deliberately NOT retried. The obstacle #7955 records is
real (a fresh `PerThread` cell starts unresolved, so the first read on every
libtest thread runs the allocating `globalThis` bootstrap, from paths like
`note_array_index_write` that sit on the array element-write path), but the
decisive objection is different: the accessors are hot-path code whose design
note requires "a single relaxed atomic load", Darwin has no local-exec TLS, and
per-thread storage in a test build only would give the test build a
representation the product does not have. The cache being a process-global
holding a raw address into a THREAD-LOCAL arena is still a real cross-agent
hazard under `perry/thread`; that is a shipped-representation change and wants
its own issue.

The cache moved to `crates/perry-runtime/src/array/prototype_addr.rs` —
`indexing.rs` reached 2012 lines and `scripts/check_file_size.sh` caps at 2000.

**#7956 — `gc::tests::telemetry_verifier`.** `verify_ordinary_pause_budget`
asserted `elapsed_pause_us <= soft_pause_target_us` per step. That is not a
mis-tuned bound, it is the wrong instrument, on three independent readings of
the source: `GcPauseBudget` documents itself as "hard work-unit limit plus a
**soft pause target for telemetry**"; `GcCycle::step` runs a phase for
`budget.work_units` and measures elapsed *afterwards*, so no code path can make
a step honour `pause_us`; and these fixtures drive with
`js_gc_step_work_units(1, …)`, the smallest step that exists, so a 4.9 ms work
unit leaves the pacer no smaller choice. The second arm was a tautology of the
first — `within_soft_pause_target` is computed as `elapsed_us <= target` — two
assertions carrying one bit.

The verifier now checks what the pacer actually controls, and checks more than
before: at least one step counted in ordinary pause stats (the old verifier had
no subject-was-live check at all), every included step bounded in work units
with `applied_work_units <= configured_work_budget` and never labelled
unbounded, `within_soft_pause_target` coherent with the numbers printed beside
it, and `pause_budget.max_observed_step_pause_us` equal to the max over the
steps in its own event. Elapsed microseconds stay in the trace and in every
error message as a diagnostic. Not moved behind an env opt-in: a timing arm no
CI job runs is a gate that cannot fail.

Six sabotage cases replace the single `verifier_rejects_over_budget_ordinary_step`,
one per rejection path, plus `verifier_accepts_a_slow_but_coherent_ordinary_step`
which encodes #7956's own failing numbers (4936 us against a 2000 us target) as
a case that must now PASS, so the decision cannot be reverted silently.

**Verification.** Pristine baseline preserved as a binary so each A/B swaps the
artifact, not the tree. M1 mini, load 70–98 throughout.

| arm | runs | failed |
|---|---|---|
| #7955 repro, pristine, `--test-threads=8 prototype_addr` | 400 | **28** |
| #7955, fixed | 400 | **0** |
| #7956 repro, pristine, `telemetry_verifier` under 16 spinners | 150 | **3** |
| #7956, fixed | 150 | **0** |
| full suite, pristine, default parallelism | 300 | 0 |
| full suite, fixed, default parallelism | 300 | **0** |

The two full-suite arms are a regression guard, not evidence: at a 1–2 % per-run
rate, 300 runs of that configuration is consistent with either state of the
code, which is why both targeted reproductions exist.
