# Combined object-key checks

Combining key ordering checks, quoted-output planning and own-key `toJSON` exclusion improves small-record stringify by 6.28% versus scalar31. Tiny-object stringify improves 3.58%, and the 1 KiB object improves 3.90%. Small-record workloads including final collection improve 2.99–3.83%. The candidate stays experimental because small-parse and RSS regressions remain.

Source `18d8e529f151ca614dbaca158627beeec669942c`, branch `codex/json-fused-key-checks-1520`, based on scalar31 (`8d8c4cac0`). Version remains 0.5.1520. The rejected compact-key layout is absent from this branch. The plan keeps the previous representation, and only the validated input graph is rooted. There are no allocations or callbacks during planning; rooting still precedes prototype initialization and final output allocation. Class/prototype checks remain intact, including native forwarding markers. GC policy and parse source are unchanged from scalar31.

[All 38 CPU/RSS rows](launch36/results/recheck/all-38.md), [ranges and both baseline comparisons](launch36/results/recheck/summary.json), [retained memory](launch36/results/recheck/memory-summary.json), and [complete lifetimes](launch36/results/lifetimes/summary.json).

| Stringify workload | Scalar31, µs | Fused35, µs | Change | Node, µs | Bun, µs |
|---|---:|---:|---:|---:|---:|
| empty_object | 0.048315 | 0.046321 | -4.128% | 0.032021 | 0.029928 |
| tiny_object | 0.093870 | 0.090511 | -3.579% | 0.037218 | 0.039426 |
| small_record | 0.283068 | 0.265279 | -6.284% | 0.108455 | 0.120645 |
| object_1k | 0.237503 | 0.228245 | -3.898% | 0.199592 | 0.214992 |
| records_object_1m | 1452.689655 | 1454.896552 | +0.152% | 847.804598 | 970.856322 |

Median standings remain 17/38 CPU rows and 32/38 peak-RSS rows at or below both engines. Small-record stringify is still about 2.45× Node and 2.20× Bun. The older full147 inventory and known semantic gaps remain open.

## Regressions and limits

Separated slower CPU ranges versus depth22: small_record parse +0.676%.
Main peak-RSS changes versus depth22: -1664 to +832 KiB.
Retained peak_rss versus depth22, 40 groups: -128 to +80 KiB; 24 increases.
Retained rss_after versus depth22, 40 groups: -112 to +80 KiB; 28 increases.

Separated slower CPU ranges versus scalar31: empty_object parse +0.671%, tiny_object parse +1.483%.
Main peak-RSS changes versus scalar31: +0 to +96 KiB.
Retained peak_rss versus scalar31, 40 groups: -80 to +96 KiB; 31 increases.
Retained rss_after versus scalar31, 40 groups: -64 to +112 KiB; 33 increases.

Escaped-parse peak RSS is +832 KiB versus depth22. Unchanged parse source does not justify dismissing the measured deltas as noise or treating the key pass as a complete no-regression improvement.

## Validation and profiles

3290 runtime tests pass (4 ignored), all 40 compiled checks match pinned Node, and 52 seeded moving-GC cases plus default/off controls pass. Every seeded case requires nonzero scheduled collections, copying minors, moved objects and loop polls. The inherited root-array prototype `toJSON` discrepancy is reference-compared and excluded from Node parity claims. New tests exercise numeric-order boundaries, forwarding-marker fallbacks, escaped marker names and valid neighbouring names. The shared marker oracle also checks every single-byte mutation.

Matched shipping-profile runtime and stdlib archives, source hashes and worker hashes are pinned in provenance.json. Node-version and root-holder inventories pass. File-size and address-ratchet failures are byte-for-byte unchanged from key33: oversized `inprocess.rs` and HIR `lower/tests.rs`, and the three inherited JSON address sites.

The qualified window contains 228 output checks, 1140 timed trials, 480 retained-memory trials and 84 lifetimes. It has 77 clean observations, load 1.671→2.028, no competing workers and no XProtect observation above 5%.

Separate diagnostic samples in profile37 are not performance measurements and are never pooled with the benchmark. The fused35 wide-parse sample has 1192/2133 main-call samples under the loop GC safepoint (~56%). This is after returning to the generated loop. Inside parsing, repeated key-cache comparisons and hashing remain visible. Small-record stringify shows value planning and short-copy emission as substantial remaining work. These single samples identify investigation targets, not precise CPU percentages.

Run `python3 benchmarks/json_performance/results/json-fused-key-checks/verify.py` to verify source, build, correctness and measured medians.


## Longer-loop regression confirmation

[diag38](diag38/results/summary.json) repeats three parse cases seven times across depth22, scalar31 and fused35 (63 trials): empty objects at 20 million calls, tiny objects at 10 million, and small records at 2 million. It is a separate qualified window with 15 clean observations and load 1.615→2.215; its results are not pooled into the full matrix.

| Parse workload | Change vs depth22 | Change vs scalar31 |
|---|---:|---:|
| Empty object | +0.729%, separated ranges | +0.755%, separated ranges |
| Tiny object | −0.633% | +1.208%, separated ranges |
| Small record | +0.720%, separated ranges | +0.371%, overlapping ranges |

The small-parse regressions persist in longer loops. The candidate is preserved for its stringify improvement but is not promoted over depth22. Next work should preserve that improvement while resolving parse/code-generation interactions and the escaped-parse RSS increase. Separately, the profiles support investigating short-piece emission and the parse key-cache comparisons; they do not justify changing GC scheduling or suppressing conservative scans.
