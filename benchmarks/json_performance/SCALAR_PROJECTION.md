# Scalar projection R1: scan gains, reuse regressions

R1 is **not accepted for landing**. Reading one numeric field directly from
lazy JSON records reduces parse-and-scan CPU by 50–63% and sharply lowers peak
RSS. Reusing an already parsed array regresses across all twelve access controls.
The next revision needs scalar caching and cheaper dispatch before broader testing.

Implementation: `3c6176c7edff20ab990c8ad2fd80c51238a9459a`, version 0.5.1530,
branch `codex/json-scalar-projection-r1`. Reference: merged main
`53df2c671fff33b1f8f624432372c2401db8b1d0`, still the remote main when checked
after these runs. The implementation is experimental; no PR has been opened.

Both windows ran on the same M1 / 8 GiB Mac, using Node 26.5.1 and Bun 1.3.14,
with exclusive benchmark locking, no competing workloads at either boundary,
and a passing final load gate. Engines ran in interleaved order with identical
work counts and seven fresh-process repetitions per case. CPU is process user
plus system time inside the timed region. RSS is whole-process peak resident
memory from `/usr/bin/time -l`, including setup and warmup.

- [Parse/scan/stringify window](results/quiet-scalar-projection-r1-focus/window.json):
  September 10, 2026, 10:40:01–10:42:41 UTC; 50 output comparisons and 280
  timed trials, all complete-count checksums verified.
- [Array reuse window](results/quiet-scalar-projection-r1-access/window.json):
  10:45:01–10:45:28 UTC; 12 complete-workload Node oracles and 336 timed
  trials, every checksum matching. Parsing happens once before timing here;
  warmup is zero. These rows measure subsequent reads, including the first read.

This is a targeted measurement. The [38-row merged-main report](MERGED_MAIN_53DF.md)
remains the complete parse/stringify baseline; this candidate has not undergone
another full matrix or retained-memory sweep.

## Parse, scan and stringify

CPU in **µs per operation**, median of seven trials. `scan` includes parsing
and summing every record's `id`; `roundtrip` includes parsing and stringifying.
Stringify-only builds an eager object through an envelope before timing, as in
the established benchmark. Fixture names are nominal sizes; the `16k` array is
approximately 13 KiB.

| Fixture | Operation | Main | R1 | Node | Bun | R1 CPU vs main |
|---|---|---:|---:|---:|---:|---:|
| records_array_16k | scan | 58.307500 | 21.572250 | 40.314250 | 35.534500 | -63.0% |
| records_array_1m | scan | 2894.950000 | 1341.430000 | 2735.980000 | 2201.940000 | -53.7% |
| records_array_8m | scan | 23704.281250 | 11814.750000 | 31080.687500 | 21555.125000 | -50.2% |
| records_array_20m | scan | 54162.000000 | 54580.250000 | 84468.687500 | 51909.687500 | +0.8% |
| records_array_20m | roundtrip | 104792.500000 | 104768.250000 | 99912.375000 | 68494.375000 | -0.0% |
| records_array_1m | parse | 919.140000 | 916.995000 | 2788.490000 | 2159.150000 | -0.2% |
| records_array_1m | stringify | 640.238281 | 639.652344 | 840.191406 | 956.796875 | -0.1% |
| small_record | parse | 0.096742 | 0.096639 | 0.353874 | 0.243376 | -0.1% |
| small_record | stringify | 0.042147 | 0.042084 | 0.107861 | 0.118103 | -0.1% |
| long_string_1m | stringify | 33.806641 | 32.239990 | 99.187500 | 92.865723 | -4.6% |

For the three smaller scans, R1 beats both other engines in every recorded CPU
sample: their ranges do not overlap R1's. The 20 MiB scan regresses by 0.8%
against main, also with separated ranges. The 20 MiB roundtrip remains about
1.53× Bun's CPU time. Small positive/negative deltas elsewhere need caution;
this window does not establish a general stringify improvement.

Peak scan RSS in **MiB**:

| Fixture | Main | R1 | Node | Bun |
|---|---:|---:|---:|---:|
| records_array_16k | 216.14 | 63.42 | 61.91 | 77.38 |
| records_array_1m | 413.72 | 67.09 | 130.20 | 99.58 |
| records_array_8m | 530.25 | 108.80 | 313.55 | 135.56 |
| records_array_20m | 691.45 | 691.41 | 529.38 | 323.61 |

The smallest scan still uses slightly more peak RSS than Node. The 20 MiB
workload retains a substantial RAM gap. No complete CPU/RSS parity is claimed.
[All CPU and RSS samples](results/quiet-scalar-projection-r1-focus/summary.json)
and [raw trials](results/quiet-scalar-projection-r1-focus/timing.jsonl) include
current RSS before/after timing as well as peaks.

## Reusing parsed arrays

CPU in **µs per loop iteration**. `repeat` reads index 7; `random` uses the same
deterministic bounded index sequence in every engine; `fields` reads `id`,
`name.length` and `active`; `sequential` repeatedly traverses the array. Each
trial performs one million iterations after a single parse. These controls
exercise reuse that a fresh-parse scan alone does not show.

| Fixture | Access | Main | R1 | Node | Bun | R1 CPU vs main |
|---|---|---:|---:|---:|---:|---:|
| records_array_16k | repeat | 0.018622 | 0.027447 | 0.002711 | 0.004533 | +47.4% |
| records_array_16k | random | 0.040380 | 0.049218 | 0.006654 | 0.009311 | +21.9% |
| records_array_16k | fields | 0.084549 | 0.117481 | 0.005348 | 0.009252 | +39.0% |
| records_array_16k | sequential | 0.035065 | 0.074588 | 0.003914 | 0.005737 | +112.7% |
| records_array_1m | repeat | 0.018561 | 0.027460 | 0.002791 | 0.004550 | +47.9% |
| records_array_1m | random | 0.048770 | 0.055773 | 0.010586 | 0.010743 | +14.4% |
| records_array_1m | fields | 0.107932 | 0.141726 | 0.010811 | 0.014347 | +31.3% |
| records_array_1m | sequential | 0.040580 | 0.065313 | 0.008177 | 0.007456 | +60.9% |
| records_array_20m | repeat | 0.005639 | 0.008696 | 0.003047 | 0.004779 | +54.2% |
| records_array_20m | random | 0.026284 | 0.027133 | 0.008574 | 0.010037 | +3.2% |
| records_array_20m | fields | 0.027194 | 0.038111 | 0.011774 | 0.014628 | +40.1% |
| records_array_20m | sequential | 0.016067 | 0.018884 | 0.006757 | 0.008243 | +17.5% |

Eleven of the twelve regression rows have separated R1/main sample ranges.
The remaining row, the smallest repeated-index case, has a 47% median slowdown
with an overlapping outlier. This is sufficient to reject R1. Peak RSS is nearly
unchanged in these controls; the CPU cost buys no material reuse-memory benefit.
[Samples and peak RSS](results/quiet-scalar-projection-r1-access/summary.json),
[full trials](results/quiet-scalar-projection-r1-access/timing.jsonl).

## What the experiment establishes

The runtime can return an own number, boolean or null from the JSON tape without
constructing its record. The probe allocates no managed objects, cannot collect
or invoke user code, and updates only scalar cursor state. Unit coverage checks
that managed allocation bytes and reparse counts stay unchanged during the scan.
This removes object construction and later tracing for these scalar reads.

R1 bounds traversal to 32 intervening elements and 32 fields. Repeated indices
fall back to normal object caching. Cached/mutated records, descriptors, ambiguous
escaped keys, strings, containers and missing properties retain ordinary access.
The shared cursor preserves the existing batch-construction signal for mixed
scalar/object reads; a test witnesses one full batch reparse in both the ordinary
and mixed-read arms. No collector policy or lazy-header layout changed.

The mutation prerequisites also correct indexed `Object.defineProperty` and
deletion on lazy arrays by materializing before the operation. Property keys
are converted once, before descriptor inspection, with the inputs rooted.

Those safeguards preserve semantics but do not make repeated reads cheap.
A scalar-only scan leaves no cached record, so another scan parses the fields
again. Ordinary and cached paths also pay the newly inserted probe call. The
reuse results show why the first traversal's gain cannot justify shipping this
implementation unchanged.

The next revision should audit reuse of the lazy array's existing value slots
for pointer-free scalar results, with an explicit property identifier and cache
validity rules. Compiler guards should route ordinary/materialized/cached records
through their existing dispatch without the extra call. Mutation must close or
invalidate scalar caching before a stale field can be returned. These are next
steps to implement and measure, not accepted performance claims.

## Correctness and limits

- 298 runtime JSON tests and 10 compiler JSON tests pass. Additional filters pass
  nine property-definition tests and fifteen GC-call-effect tests.
- The matching release compiler and both static archives were rebuilt together.
  Both benchmark objects were freshly compiled; the rotating worker object is
  byte-identical to main's. The access workers use the same source path and link
  flags on both sides. [Build provenance](results/scalar-projection-r1-validation/provenance.json).
- All 23 observable fixture results match Node under automatic tape admission,
  forced tape and direct parsing, each with normal, scheduled moving and full
  GC. Each scheduled arm protects 644 retired page sets; auto/tape moves 29,155
  objects, direct moves 81,733. Nine mutation cases and twelve access-output
  controls also pass. [GC witnesses](results/scalar-projection-r1-validation/gc-witness.json),
  [validation commands and hashes](results/scalar-projection-r1-validation/validation.json).
- The generated probe is live: 30 calls in the fixture, 12 in the access worker,
  six in the original worker. Its calls remain outside statepoints. Both native
  benchmark workers have zero reported root hazards across 582 statepoints and
  1,239 relocations. Shadow bind-dominance and unrooted-alloca checks pass for all
  three sources.
- The full native check reports **one unresolved hazard** in accessor-literal
  construction: an object temporary crosses `js_closure_unbox_callee_checked`
  before `js_closure_call1_receiverless`. A separate three-line program with no
  JSON operations or probe calls reproduces it. Those call-lowering and runtime
  unbox files are unchanged from main; an actual main-compiler reproduction was
  not performed. Checked unboxing only allocates on its throwing path, which may
  explain the report, but no exemption or checker-budget change was made. The
  full native check is recorded as failed. [Diagnostic results](results/scalar-projection-r1-validation/codegen-comparison.json),
  [no-JSON control](results/scalar-projection-r1-validation/no-json-control/accessor-control.ts),
  [checker output](results/scalar-projection-r1-validation/no-json-control/checker.log.gz).
- 73 of 74 script lint checks pass; public benchmark evidence freshness still
  fails. The compile tier was explicitly skipped during experimentation.
  Public-baseline refresh and platform/ABI validation remain prerequisites for
  any accepted follow-up. Existing main JSON canonicalization limitations remain
  outside this candidate's claimed coverage.

[Validation artifact manifest](results/scalar-projection-r1-validation/manifest.json)
records original hashes and lossless compression for large logs and LLVM IR.
Each measurement archive preserves its own source/harness/provenance snapshot;
`source.patch.gz` expands to the exact original patch, with hashes alongside it.
