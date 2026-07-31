Added a representation-selection **promotion census**: a per-workload,
per-representation count of how many values actually get each unboxed
representation, gated against a ratcheted baseline
(`python3 scripts/compiler_output_regression.py census --gate`, new
non-required `repsel-census` CI job). #7034 had to hand-instrument the compiler
to discover that `Ptr<Shape>` promotes **nothing at all** on `batch.ts` — the
object-heavy workload it exists for — and that only 3 of 17 suite benchmarks
promote a single shape local each; nothing in CI could have surfaced that.

Two counters could not previously have been incremented at all, so their zeros
were indistinguishable from a dead instrument. `Ptr<NumArray>` had an
`opt_report::Analysis` variant, a `Ptr<NumArray>` target-rep string and denial
recording, but **no `select()` call site anywhere in the tree**; int-valued
locals (`collectors/int_valued_ta_locals.rs`) had no analysis variant. Both now
record at the single site where a candidate becomes a fact. `--opt-report=json`
additionally enumerates `Analysis::ALL`, so an analysis with no entries renders
an explicit `0` rather than an absent key, and `PtrNumArray` gets an explicit
serde rename — it was serialized `ptr-num-array` in `Entry::analysis` while the
summary row said `ptr-numarray`, one analysis under two names in one document.
Every recording site stays behind `opt_report::enabled()`, so no emitted byte
changes.

The gate is falsifiable by construction, because per-workload floors alone are
not: the honest floor for `Ptr<Shape>` on real code is zero today, and a zero
floor can never go red. The corpus therefore carries hand-written **liveness
fixtures** whose minimums live in `LIVENESS_FLOORS` in the script rather than in
the regenerable baseline (`--update` refuses to write below them), plus a
corpus-wide assertion that no census key reads zero everywhere. Verified by
sabotage in both directions: each of `PERRY_PTR_SHAPE_LOCALS=0`,
`PERRY_PTR_NUMARRAY_LOCALS=0`, `PERRY_CANONICAL_I32_LOCALS=0`,
`PERRY_CANONICAL_STR_LOCALS=0` and `PERRY_INT_VALUED_LOCALS=0` turns the census
red, as does reverting the new `Ptr<NumArray>` `select()` (i.e. the pre-change
compiler), while the unmodified build is green. CI re-runs the first arm on
every job and asserts the exit code *and* the reason string, so "red for an
unrelated reason" does not count as proof that the gate's subject was live.

Current numbers across 18 real workloads: `Ptr<Shape>` promotes in 3,
`Ptr<NumArray>` in 4, canonical `I32` in 2, and canonical `U32`, canonical
`Str`, int-valued, specialized-ABI entries and `TaPtr` parameter slots in **none**
outside the fixtures. The masked-window/buffer-view `TaPtr` *region* machinery
has no `opt_report` analysis and is documented as unmeasured rather than
reported as a zero.
