# Structural JSON depth scanning

The ARM64 depth preflight now classifies quotes and brackets in the same 16-byte windows. It combines bracket counts when no ordering can cross a depth bound, visits individual brackets near a bound, and retains scalar behavior for malformed escapes outside strings. Long quoted spans keep the previous bulk scanner. This stage allocates no memory; GC policy, parse construction and stringify implementation are unchanged. Other architectures retain their previous depth scanner.

Runtime source: `15d2aeafde90f28984134515233de4439731576a` on `codex/json-structural-depth-1520`, based on selected depth22 `ae9283ca3276a983be0ac38c067b4a7bd56337d3`. Version remains 0.5.1520. Matched shipping-profile runtime/stdlib archives and both worker hashes are pinned in `candidate/provenance.json`.

The candidate remains experimental and has **not been promoted over depth22**. The tiny-scalar CPU regressions and peak-RSS increases need resolution before it meets the no-regression requirement.

## Qualified performance

All numbers below are from the complete launch29 repeat, not the rejected first run. See [all 38 CPU and RSS rows](launch29/results/recheck/all-38.md), [raw ranges](launch29/results/recheck/summary.json), [retained memory](launch29/results/recheck/memory-summary.json), and [lifetimes including final GC](launch29/results/lifetimes/summary.json).

| Parse fixture | Previous Perry, µs | Structural Perry, µs | Change | Node, µs | Bun, µs |
|---|---:|---:|---:|---:|---:|
| records_array_1m | 1477.547 | 1200.432 | -18.76% | 2724.389 | 2136.411 |
| records_object_1m | 2909.085 | 2636.437 | -9.37% | 2738.493 | 2132.577 |
| records_object_8m | 22265.250 | 20121.000 | -9.63% | 30077.625 | 21739.500 |
| records_object_20m | 55197.333 | 50077.333 | -9.28% | 97702.000 | 56808.000 |
| escaped_1m | 1151.037 | 1136.610 | -1.25% | 1732.854 | 2084.244 |
| wide_1m | 15158.611 | 14909.778 | -1.64% | 4958.667 | 4161.278 |

The structural build is at or below both engines on **17/38 CPU rows** and **32/38 peak-RSS rows**. These median standings do not establish no regression. The older full target inventory and known semantic gaps remain open.

Rows with separated observed CPU ranges in the slower direction: null parse (+2.636%), string_a parse (+2.155%).

Main peak-RSS median changes versus depth22 range from -32 to +832 KiB. Across 40 retained-memory groups, peak changes range from -48 to +80 KiB (26 positive), and current-RSS changes range from -48 to +80 KiB (29 positive). Memory changes are reported even though the scanner adds no heap allocations.

| Lifetime workload | Mode | Iterations | Total CPU change | Peak-RSS change, KiB |
|---|---|---:|---:|---:|
| escaped_1m parse | discard | 100 | -1.130% | -96 |
| escaped_1m parse | latest | 100 | -0.825% | -112 |
| escaped_1m parse | retain | 32 | -1.091% | -96 |
| records_object_20m roundtrip | discard | 24 | -3.730% | -96 |
| small_record stringify | discard | 100000 | +0.011% | -96 |
| small_record stringify | latest | 100000 | -0.135% | -96 |
| small_record stringify | retain | 100000 | +0.061% | -80 |

Full-lifetime CPU includes collecting after the work. Small stringify deltas must be read with their saved ranges; the source change does not directly optimize stringify. Array-root parse can defer work, while stringify starts with materialized inputs.

## Validation and measurement limits

3,288 runtime tests pass (4 ignored), 40 compiled checks match pinned Node, and 52 seeded moving-GC checks plus default/off controls pass. Every seeded case asserts scheduled collections, copying minors, moved objects and loop polls were nonzero. The inherited root-array `Object.prototype.toJSON` mismatch is compared against the previous result and is not counted as Node parity.

The new tests cover all 15,625 six-byte bracket/quote/backslash patterns across 17 block offsets, compare against an independent scalar oracle, and exercise loads/tails ending at a protected page. Existing arbitrary-byte and escape-mask tests also exercise the scanner. Root-holder and Node-version inventories pass. Existing file-size failures (`inprocess.rs`, 2,416 lines; HIR `lower/tests.rs`, 2,009) and three address-inventory findings are unchanged.

Both attempts complete 190 output verifications, 950 timed trials, 360 retained-memory trials and 63 lifetimes. launch27 passes its entry/exit load gates but catches XProtect at 13.1% CPU in one of 63 monitoring observations; it is preserved as **unqualified**, with no pooling into the accepted numbers.
launch29 passes entry and exit gates (load 1.516 to 1.959), with 63 clean observations, no competing workers and no XProtect observation above 5%. All executable files stay immutable during trials. The same fixed argv[0]/CWD/fixture paths and previously qualified launcher are retained.

`python3 benchmarks/json_performance/results/json-structural-depth/verify.py` verifies source identities, saved evidence, qualification and summary medians. [The next stringify investigation](next-investigation.md) has an ABI probe showing why a 16-byte packed key plan can avoid a return-buffer copy; it is not yet a runtime optimization.

A [separate longer-loop recheck](diag30/README.md) confirms the tiny-scalar CPU regressions and escaped-parse RSS increase. Its GC traces show equal collection/scan counts, but varying conservative retention; the memory cause remains unresolved.
