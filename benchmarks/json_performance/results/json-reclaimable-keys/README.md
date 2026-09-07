# Reclaimable JSON keys and safe typed hints

Typed48 combines reclaimable direct-parser key storage with corrected typed-hint ordering. Keys45 is retained only as a comparison: its typed-hint pointer crossed entry GC and failed a protected-from-space test. Typed48 captures the hint after that collection, inside construction suppression, and passes the same test plus valid/invalid metadata fallback cases.

Direct-parser property names and their ordered arrays previously used an arena that is never swept. Clearing the parser cache dropped its references but did not release storage. One 50,000-field parse added exactly **2,400,016 permanent bytes**. The regression test fails on branch44 and passes after ordinary collectible allocation replaces that storage.

Typed48 preserves collection-free key misses, including reviver source annotation, with the existing nesting-safe tiny suppression scope. Array construction uses the existing batch helper, which publishes layout facts and old-to-young edges before return. Cache scanners, shape tracing, collection scheduling and generic GC policy are unchanged. Typed class-key bootstrap and tape-materializer cache misses retain their existing allocation policy and remain outside this change.

[Launch46](launch46/results/qualification.json) and [launch49](launch49/results/qualification.json) are retained as unqualified: their continuous monitors observed XProtect at 5.6% and 35.2%, respectively, above the unchanged 5% threshold. No measurements from either window are pooled into this comparison. Branch44 first moves the wide-key decision into the existing heap-field branch. It changes no allocation policy and provides a separate control for that dispatch adjustment.

Source commits: keys45 `916afe4784b6df1f17ce9eeeab8a98b3317e3bd4`, typed48 `9b26ef107c58fa8a5b99b9d74f56a7e24650205c`. Both retain version 0.5.1520.

[All 38 CPU/RSS rows](launch52/results/recheck/all-38.md), [ranges](launch52/results/recheck/summary.json), [retained memory](launch52/results/recheck/memory-summary.json), [complete lifetimes](launch52/results/lifetimes/summary.json).

| Workload | Copy41 µs | Keys45 µs | Typed48 µs | Typed48 vs Copy41 | Node µs | Bun µs |
|---|---:|---:|---:|---:|---:|---:|
| wide_1m parse | 11436.750000 | 7101.944444 | 7100.222222 | -37.917% | 4932.138889 | 4162.138889 |
| wide_1m stringify | 1668.516279 | 1692.860465 | 1693.000000 | +1.467% | 6341.069767 | 668.530233 |
| small_record parse | 0.533748 | 0.537469 | 0.537114 | +0.631% | 0.346202 | 0.253682 |
| small_record stringify | 0.235635 | 0.234969 | 0.234694 | -0.399% | 0.109109 | 0.121111 |
| records_object_1m parse | 2661.901408 | 2677.183099 | 2675.394366 | +0.507% | 2854.971831 | 2137.563380 |

Typed48 is at or below both engines on 17/38 CPU rows and 32/38 peak-RSS rows. The all-row no-regression objective remains open; this candidate is not promoted over depth22.

## Regressions and RSS

branch versus copy: empty_object stringify +0.326%; escaped_1m stringify +30.610%; object_1k parse +0.157%; small_record parse +1.323%.

keys versus branch: numbers_1m stringify +2.181%; object_1k parse +1.043%; records_array_20m parse +0.969%; records_object_1m parse +1.317%; records_object_20m parse +0.621%; records_object_8m parse +0.944%; wide_1m stringify +1.270%.

candidate versus keys: no separated slower CPU ranges.

candidate versus branch: numbers_1m stringify +2.190%; object_1k parse +1.089%; records_array_20m parse +0.926%; records_object_1m parse +1.249%; records_object_20m parse +0.829%; records_object_8m parse +0.682%; wide_1m stringify +1.279%.

candidate versus copy: empty_object stringify +0.792%; numbers_1m stringify +2.072%; object_1k parse +1.248%; wide_1m stringify +1.467%.

candidate versus fused: empty_object stringify +0.798%; numbers_1m stringify +2.421%; records_array_20m parse +1.195%; records_object_1m parse +1.821%; records_object_20m parse +1.019%; records_object_8m parse +0.839%; small_record parse +0.992%; wide_1m stringify +1.357%.

candidate versus previous: small_record parse +1.809%; wide_1m stringify +0.614%.

Typed48 vs copy: main peak RSS -18384 to +5856 KiB; 7 increases.
Retained peak_rss: -1120 to +12352 KiB; 7 increases.
Retained rss_after: -1120 to +12656 KiB; 8 increases.

Typed48 vs branch: main peak RSS -18320 to +5920 KiB; 19 increases.
Retained peak_rss: -1120 to +12416 KiB; 26 increases.
Retained rss_after: -1136 to +12736 KiB; 33 increases.

Typed48 vs previous: main peak RSS -18352 to +5968 KiB; 20 increases.
Retained peak_rss: -1072 to +12384 KiB; 21 increases.
Retained rss_after: -1088 to +12672 KiB; 13 increases.

## Complete wide-parse lifetimes

| Mode | Iterations | Copy41 total CPU ms | Typed48 total CPU ms | Copy41 peak MiB | Typed48 peak MiB | Copy41 drained MiB | Typed48 drained MiB |
|---|---:|---:|---:|---:|---:|---:|---:|
| discard | 100 | 2108.155 | 1151.888 | 485.781 | 544.672 | 485.125 | 544.062 |
| latest | 100 | 2135.576 | 1144.388 | 488.125 | 544.719 | 487.297 | 544.062 |
| retain | 16 | 171.879 | 196.745 | 110.969 | 120.625 | 103.938 | 118.219 |

## Validation and measurement

Qualified launch52 completed 304 output comparisons, 1,520 timing trials, 720 retained-memory trials and 180 complete lifetimes, with 118 clean observations; load 1.387 to 1.974. The existing seven lifetime cases remain; three wide-object discard/latest/retain cases are added. No traced measurements enter CPU/RSS summaries.

Branch44 passes 3,292 runtime tests, keys45 passes 3,295, and typed48 passes 3,296 (four ignored each). Each passes 40 compiled Node comparisons and 52 seeded moving-GC checks plus default/off controls. Seeded checks require actual collections, copying minors, moved objects and loop polls. The inherited root-array prototype toJSON gap remains reference-compared and excluded from Node parity. The four final lifetime tests prove reclamation, real movement of cached strings/arrays, retained wide-object survival through minor/full GC, and typed-hint correctness after entry collection. The original 40/52 checks missed the keys45 typed-hint bug; its failure and the fixed test are retained.

Node-version and root-holder inventories pass. File-size and address inventory failures are byte-for-byte identical to copy41; their logs are retained. Matched release/dist runtime and stdlib archives, source stamps and immutable executable hashes are recorded.

## Separate GC diagnostics

copy41: 6 cycles (1 full), 4,212,357 pointer-slot reads, 88,800,904 permanent bytes at the final recorded collection. Same 36 parses plus two warmups; diagnostic counts only.
typed48: 8 cycles (0 full), 720,779 pointer-slot reads, 312 permanent bytes at the final recorded collection. Same 36 parses plus two warmups; diagnostic counts only.
keys45: 8 cycles (0 full), 720,779 pointer-slot reads, 312 permanent bytes at the final recorded collection. Same 36 parses plus two warmups; diagnostic counts only.

The historical inventory remains 147 comparisons against 11 older builds over these same 38 workloads. Its no-regression requirements and the existing semantic gaps remain open.

Run `python3 benchmarks/json_performance/results/json-reclaimable-keys/verify.py` to verify this evidence.
