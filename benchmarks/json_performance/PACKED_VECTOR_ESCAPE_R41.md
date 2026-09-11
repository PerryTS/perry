# Packed vector escaping: R41

The packed writer closes the measured dense-escaping stringify gap. For the 1 MiB escaped fixture, pretty printing takes 336.781 µs versus Bun's 561.605 µs, and the replacer callback takes 363.656 µs versus Bun's 572.148 µs. Perry uses 40.03% and 36.44% less CPU than Bun on these two rows. Peak RSS falls by 3.500 and 3.563 MiB versus frozen R26.

This experimental source is not promoted: the full lineage still has 10 rows with separated CPU regressions against R26 in this screen. The next investigation outlines object parsing from the recursive value dispatcher. No claim of merged-main performance or general parity follows from the escaping result.

Three qualified quiet windows contain 1,988 timed trials, 380 complete-output verification records and 60 calibration trials. Each timed case has seven interleaved R26/Perry/Node/Bun repetitions. Node is pinned to 26.5.1 and Bun to 1.3.14. CPU is user plus system time per loop iteration; peak RSS includes the whole process and startup.

Across all 71 comparisons, median peak-RSS changes range from -3648 to +96 KiB versus R26. Of the 38 repeated-source parse/stringify rows, 38 have CPU medians below both peers and 38 have all measured CPU samples below both peers. Fresh-input measurements remain separate below.

One earlier full-screen window failed the unchanged quiet-load gate at its end (2.5546875 one-minute load). Its 1,400 timed trials and 250 verification records are archived separately and excluded from every performance conclusion and qualified total above. The retry uses a distinct result directory and the same cases, repetitions and quiet criteria.

## Implementation and validation

For escaped blocks on ARM64, a 4 KiB byte-shuffle table packs eight input bytes and their short escapes in one operation. Plain blocks retain the existing sixteen-byte copy. Rare control bytes use the scalar six-byte encoder. Every speculative sixteen-byte output store requires at least sixteen input bytes remaining; the output length is published only after the exact bytes have been written. UTF-8 validation, expansion planning, native buffer growth, the 256-byte dispatch threshold and the fallback remain unchanged. The writer makes no managed allocation or callback, and this round changes no GC policy, roots, parse boundary, cache admission or cap.

Source 3952e832058a5e1efcd4cdf9cf28f26e31d60ea3 passes 312 serial release JSON tests (2.92 seconds) and 385 string-filter tests (2.56 seconds; overlapping filters). The guard-page test covers 8,587 source/output cases, and 509 standalone complete outputs match the prior pinned-Node corpus. All 125 canonical candidate full-output checks pass with positive moving/protected witnesses where required; 30 normalized native/shadow IR files and six worker objects match frozen R26. Known native findings and reference behavioral failures remain explicit. Original-suite checker verdicts are reused from the ten fresh R39 commands only after exact IR, full scripts-tree, source and log hashes match. Normal three-package production build completed in 338.9895827770233 seconds. Final lint is 72/74: public baseline freshness and the inherited raw-handle ceiling versus pinned updated main remain unresolved; implementation audits, formatting and file cap pass. No checker, allowlist or baseline-ceiling edits.

Known native-root findings, lazy descriptor and fractional-spacing findings, and frozen-reference emitter output/moving-GC failures remain explicit. Their receipts are retained; this is not a blanket conformance pass. R41 has no alternate-worktree validation refusal. The earlier R40 refusal is documented in its own evidence.

## Regressions in this screen

| Fixture / operation | CPU change vs R26 | Slower pairs |
|---|---:|---:|
| tiny_object / parse | +1.00% | 7/7 |
| records_array_16k / sparse | +0.46% | 7/7 |
| records_object_1m / parse | +0.82% | 7/7 |
| records_object_8m / parse | +1.22% | 7/7 |
| records_array_20m / stringify | +0.55% | 7/7 |
| records_object_20m / stringify | +0.60% | 7/7 |
| numbers_1m / stringify | +0.69% | 7/7 |
| wide_1m / parse | +4.29% | 7/7 |
| wide_1m / stringify | +2.33% | 7/7 |
| heterogeneous_1m / parse | +0.71% | 7/7 |

“Separated” means the smallest candidate sample exceeds the largest reference sample. It is a descriptive screen, not a significance test. Overlapping samples do not prove equivalence. These R41 rows have not received a second independent R41 recheck.

## Large stringify options

Window: 2026-09-11T17:26:36Z to 2026-09-11T17:27:00Z.

| Fixture / operation | R26 µs | Perry µs | Node µs | Bun µs | Change vs R26 |
|---|---:|---:|---:|---:|---:|
| long_string_1m / pretty | 127.656250 | 86.683594 | 415.386719 | 536.207031 | -32.10% |
| long_string_1m / callback | 155.953125 | 115.562500 | 441.851562 | 546.695312 | -25.90% |
| unicode_1m / pretty | 168.718750 | 133.027344 | 559.800781 | 446.199219 | -21.15% |
| unicode_1m / callback | 196.312500 | 159.843750 | 586.750000 | 456.203125 | -18.58% |
| escaped_1m / pretty | 1783.234375 | 336.781250 | 1207.000000 | 561.605469 | -81.11% |
| escaped_1m / callback | 1812.046875 | 363.656250 | 1234.953125 | 572.148438 | -79.93% |

| Fixture / operation | R26 peak MiB | Perry peak MiB | Node peak MiB | Bun peak MiB |
|---|---:|---:|---:|---:|
| long_string_1m / pretty | 55.703 | 55.656 | 89.828 | 87.219 |
| long_string_1m / callback | 55.812 | 55.766 | 88.984 | 75.875 |
| unicode_1m / pretty | 55.141 | 55.094 | 86.734 | 82.781 |
| unicode_1m / callback | 55.250 | 55.203 | 80.375 | 72.828 |
| escaped_1m / pretty | 59.047 | 55.547 | 87.281 | 83.641 |
| escaped_1m / callback | 59.188 | 55.625 | 81.797 | 74.891 |
## Fresh and repeated inputs

Window: 2026-09-11T17:32:56Z to 2026-09-11T17:34:39Z.

| Fixture / operation | R26 µs | Perry µs | Node µs | Bun µs | Change vs R26 |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 0.494253 | 0.480651 | 0.351089 | 0.255440 | -2.75% |
| small_record / same | 0.099030 | 0.099203 | 0.341231 | 0.245230 | +0.17% |
| small_record / select | 0.009435 | 0.009437 | 0.002663 | 0.003642 | +0.02% |
| records_array_1m / rotating | 951.707865 | 935.792135 | 2667.044944 | 2149.258427 | -1.67% |
| records_array_1m / same | 947.567416 | 931.578652 | 2735.359551 | 2154.376404 | -1.69% |
| records_array_1m / select | 0.011023 | 0.011461 | 0.003131 | 0.003849 | +3.97% |
| long_string_1m / rotating | 100.014774 | 97.975894 | 372.405910 | 69.936236 | -2.04% |
| long_string_1m / same | 0.360518 | 0.357605 | 376.221683 | 65.769256 | -0.81% |
| long_string_1m / select | 0.011531 | 0.011983 | 0.003096 | 0.003856 | +3.93% |
| escaped_1m / rotating | 1006.609467 | 952.284024 | 1726.301775 | 2076.568047 | -5.40% |
| escaped_1m / same | 992.398810 | 933.785714 | 1722.720238 | 2072.797619 | -5.91% |
| escaped_1m / select | 0.011461 | 0.011454 | 0.003135 | 0.003884 | -0.05% |
| unicode_1m / rotating | 141.617857 | 83.298571 | 439.545714 | 63.413571 | -41.18% |
| unicode_1m / same | 0.357193 | 0.355422 | 438.541814 | 58.918852 | -0.50% |
| unicode_1m / select | 0.011352 | 0.011107 | 0.003125 | 0.003858 | -2.16% |

| Fixture / operation | R26 peak MiB | Perry peak MiB | Node peak MiB | Bun peak MiB |
|---|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.891 | 59.922 | 80.172 |
| small_record / same | 76.781 | 76.734 | 59.672 | 79.844 |
| small_record / select | 12.812 | 12.828 | 57.875 | 35.266 |
| records_array_1m / rotating | 76.297 | 76.250 | 128.891 | 98.297 |
| records_array_1m / same | 76.297 | 76.250 | 129.109 | 99.938 |
| records_array_1m / select | 22.594 | 22.594 | 67.609 | 42.500 |
| long_string_1m / rotating | 90.234 | 90.109 | 177.672 | 123.516 |
| long_string_1m / same | 24.594 | 24.578 | 197.406 | 145.516 |
| long_string_1m / select | 24.328 | 24.328 | 68.953 | 43.750 |
| escaped_1m / rotating | 65.000 | 64.953 | 90.438 | 77.797 |
| escaped_1m / same | 65.000 | 64.953 | 89.672 | 77.750 |
| escaped_1m / select | 23.531 | 23.531 | 68.453 | 43.266 |
| unicode_1m / rotating | 61.531 | 61.438 | 159.516 | 153.453 |
| unicode_1m / same | 22.688 | 22.688 | 223.484 | 132.078 |
| unicode_1m / select | 22.438 | 22.422 | 68.641 | 44.672 |
## Complete 50-row screen

Window: 2026-09-11T17:43:22Z to 2026-09-11T17:50:10Z.

| Fixture / operation | R26 µs | Perry µs | Node µs | Bun µs | Change vs R26 |
|---|---:|---:|---:|---:|---:|
| null / parse | 0.010622 | 0.010632 | 0.027348 | 0.019207 | +0.09% |
| null / stringify | 0.006572 | 0.006565 | 0.027217 | 0.031123 | -0.11% |
| string_a / parse | 0.012823 | 0.012821 | 0.031728 | 0.022603 | -0.01% |
| string_a / stringify | 0.009692 | 0.009690 | 0.030061 | 0.032001 | -0.02% |
| empty_object / parse | 0.023420 | 0.023402 | 0.045151 | 0.024134 | -0.07% |
| empty_object / stringify | 0.017605 | 0.017574 | 0.031962 | 0.032557 | -0.18% |
| tiny_object / parse | 0.036759 | 0.037128 | 0.081706 | 0.045215 | +1.00% |
| tiny_object / stringify | 0.033933 | 0.034036 | 0.037121 | 0.043168 | +0.30% |
| small_record / parse | 0.098448 | 0.098490 | 0.343466 | 0.244138 | +0.04% |
| small_record / stringify | 0.045711 | 0.045808 | 0.107796 | 0.119367 | +0.21% |
| object_1k / parse | 0.081997 | 0.081971 | 0.533380 | 0.227682 | -0.03% |
| object_1k / stringify | 0.115393 | 0.115369 | 0.193415 | 0.210988 | -0.02% |
| records_array_16k / parse | 13.586051 | 13.632577 | 39.406859 | 33.821340 | +0.34% |
| records_array_16k / stringify | 10.976060 | 11.051896 | 13.203413 | 23.218704 | +0.69% |
| records_array_16k / sparse | 20.022877 | 20.115658 | 39.434672 | 33.932003 | +0.46% |
| records_array_16k / scan | 58.874767 | 59.185057 | 39.815475 | 35.739697 | +0.53% |
| records_array_16k / roundtrip | 20.978718 | 20.950385 | 54.501371 | 57.689124 | -0.14% |
| records_array_1m / parse | 918.888235 | 919.941176 | 2710.517647 | 2138.452941 | +0.11% |
| records_array_1m / stringify | 646.462882 | 652.694323 | 848.078603 | 965.371179 | +0.96% |
| records_array_1m / sparse | 968.434783 | 972.310559 | 2737.819876 | 2185.962733 | +0.40% |
| records_array_1m / scan | 2795.637681 | 2790.985507 | 2864.057971 | 2228.014493 | -0.17% |
| records_array_1m / roundtrip | 1411.339130 | 1414.886957 | 3587.860870 | 3108.521739 | +0.25% |
| records_object_1m / parse | 2008.913580 | 2025.308642 | 2855.740741 | 2154.234568 | +0.82% |
| records_object_1m / stringify | 646.746725 | 653.004367 | 846.986900 | 966.655022 | +0.97% |
| records_array_8m / parse | 8288.187500 | 8311.812500 | 32419.750000 | 20876.125000 | +0.29% |
| records_array_8m / stringify | 4737.793103 | 4765.241379 | 7271.896552 | 8165.379310 | +0.58% |
| records_array_8m / sparse | 8851.333333 | 8867.733333 | 32601.666667 | 21565.400000 | +0.19% |
| records_array_8m / scan | 21574.375000 | 21338.625000 | 30258.000000 | 21375.500000 | -1.09% |
| records_array_8m / roundtrip | 13109.181818 | 13172.181818 | 37026.363636 | 27454.363636 | +0.48% |
| records_object_8m / parse | 15953.800000 | 16148.100000 | 34419.700000 | 20821.500000 | +1.22% |
| records_object_8m / stringify | 4753.172414 | 4781.103448 | 7155.517241 | 8178.827586 | +0.59% |
| records_array_20m / parse | 39420.750000 | 39331.500000 | 95880.750000 | 53078.000000 | -0.23% |
| records_array_20m / stringify | 11753.833333 | 11818.833333 | 17305.416667 | 20039.000000 | +0.55% |
| records_array_20m / sparse | 39393.750000 | 39352.000000 | 92737.250000 | 53118.250000 | -0.11% |
| records_array_20m / scan | 40402.250000 | 40315.250000 | 90067.500000 | 53858.250000 | -0.22% |
| records_array_20m / roundtrip | 98587.000000 | 98537.500000 | 100427.000000 | 76474.500000 | -0.05% |
| records_object_20m / parse | 39450.250000 | 39320.000000 | 95806.000000 | 53147.000000 | -0.33% |
| records_object_20m / stringify | 11706.416667 | 11776.666667 | 17275.666667 | 20147.916667 | +0.60% |
| numbers_1m / parse | 1553.284314 | 1550.666667 | 3125.794118 | 3206.754902 | -0.17% |
| numbers_1m / stringify | 1014.006803 | 1021.000000 | 1974.523810 | 2942.741497 | +0.69% |
| long_string_1m / parse | 0.349608 | 0.347826 | 364.223450 | 64.362438 | -0.51% |
| long_string_1m / stringify | 36.181061 | 33.967742 | 102.221124 | 94.851717 | -6.12% |
| escaped_1m / parse | 980.341772 | 923.721519 | 1763.329114 | 2072.658228 | -5.78% |
| escaped_1m / stringify | 884.228916 | 886.108434 | 1894.451807 | 2087.156627 | +0.21% |
| unicode_1m / parse | 0.348887 | 0.347093 | 442.161522 | 57.942570 | -0.51% |
| unicode_1m / stringify | 28.865859 | 30.244040 | 409.896970 | 448.426263 | +4.77% |
| wide_1m / parse | 2889.843750 | 3013.828125 | 5229.531250 | 4138.078125 | +4.29% |
| wide_1m / stringify | 633.607759 | 648.366379 | 6341.974138 | 664.383621 | +2.33% |
| heterogeneous_1m / parse | 1117.612676 | 1125.549296 | 3933.859155 | 2947.084507 | +0.71% |
| heterogeneous_1m / stringify | 717.949541 | 718.022936 | 898.399083 | 1057.022936 | +0.01% |

| Fixture / operation | R26 peak MiB | Perry peak MiB | Node peak MiB | Bun peak MiB |
|---|---:|---:|---:|---:|
| null / parse | 12.812 | 12.812 | 57.688 | 35.766 |
| null / stringify | 13.016 | 13.000 | 59.609 | 129.828 |
| string_a / parse | 12.812 | 12.812 | 57.672 | 36.109 |
| string_a / stringify | 13.016 | 13.000 | 59.672 | 129.828 |
| empty_object / parse | 32.141 | 32.172 | 59.547 | 69.078 |
| empty_object / stringify | 13.109 | 13.078 | 59.672 | 129.828 |
| tiny_object / parse | 32.281 | 32.297 | 59.578 | 69.078 |
| tiny_object / stringify | 32.328 | 32.312 | 59.688 | 129.828 |
| small_record / parse | 79.578 | 79.578 | 59.703 | 79.922 |
| small_record / stringify | 33.344 | 33.328 | 59.750 | 378.719 |
| object_1k / parse | 64.953 | 64.969 | 61.844 | 71.281 |
| object_1k / stringify | 33.359 | 33.359 | 61.812 | 70.906 |
| records_array_16k / parse | 63.438 | 63.469 | 65.812 | 70.625 |
| records_array_16k / stringify | 33.547 | 33.562 | 61.875 | 71.062 |
| records_array_16k / sparse | 72.234 | 72.250 | 66.062 | 70.656 |
| records_array_16k / scan | 204.469 | 204.500 | 61.953 | 77.438 |
| records_array_16k / roundtrip | 69.234 | 68.828 | 65.891 | 70.906 |
| records_array_1m / parse | 67.109 | 67.141 | 125.672 | 88.750 |
| records_array_1m / stringify | 63.016 | 63.109 | 129.516 | 103.703 |
| records_array_1m / sparse | 67.641 | 67.656 | 125.656 | 97.219 |
| records_array_1m / scan | 158.500 | 158.562 | 97.562 | 83.188 |
| records_array_1m / roundtrip | 61.484 | 61.531 | 152.156 | 93.328 |
| records_object_1m / parse | 66.469 | 66.453 | 92.984 | 79.172 |
| records_object_1m / stringify | 63.047 | 63.109 | 129.453 | 103.734 |
| records_array_8m / parse | 108.828 | 108.859 | 266.047 | 141.141 |
| records_array_8m / stringify | 121.141 | 121.219 | 212.562 | 176.641 |
| records_array_8m / sparse | 109.172 | 109.188 | 266.000 | 187.797 |
| records_array_8m / scan | 186.734 | 186.781 | 230.078 | 133.562 |
| records_array_8m / roundtrip | 129.266 | 129.312 | 267.422 | 148.656 |
| records_object_8m / parse | 187.328 | 187.344 | 244.359 | 131.781 |
| records_object_8m / stringify | 121.188 | 121.250 | 212.594 | 176.641 |
| records_array_20m / parse | 255.422 | 255.453 | 359.656 | 220.703 |
| records_array_20m / stringify | 235.219 | 235.266 | 459.531 | 304.922 |
| records_array_20m / sparse | 255.422 | 255.453 | 359.844 | 220.719 |
| records_array_20m / scan | 255.438 | 255.453 | 330.031 | 225.797 |
| records_array_20m / roundtrip | 295.375 | 295.422 | 342.062 | 255.703 |
| records_object_20m / parse | 255.438 | 255.438 | 359.781 | 220.562 |
| records_object_20m / stringify | 235.234 | 235.297 | 459.531 | 304.906 |
| numbers_1m / parse | 62.781 | 62.812 | 104.125 | 68.250 |
| numbers_1m / stringify | 57.844 | 57.891 | 113.141 | 74.297 |
| long_string_1m / parse | 16.438 | 16.469 | 209.250 | 156.469 |
| long_string_1m / stringify | 54.500 | 54.500 | 183.656 | 155.703 |
| escaped_1m / parse | 58.406 | 58.406 | 102.609 | 67.938 |
| escaped_1m / stringify | 54.891 | 54.938 | 124.891 | 74.828 |
| unicode_1m / parse | 15.781 | 15.781 | 194.891 | 157.984 |
| unicode_1m / stringify | 53.859 | 53.875 | 186.234 | 153.547 |
| wide_1m / parse | 227.719 | 227.719 | 121.172 | 95.109 |
| wide_1m / stringify | 68.672 | 68.750 | 117.984 | 84.703 |
| heterogeneous_1m / parse | 62.562 | 62.594 | 92.891 | 99.578 |
| heterogeneous_1m / stringify | 61.719 | 61.781 | 129.000 | 104.922 |

## Earlier scalar writer and profile follow-up

The prior scalar writer in R39 measured 897.777 µs for escaped pretty printing and 924.234 µs for the replacer case. R41 reduces those medians by 62.49% and 60.65%. These are separately qualified windows, not paired R39/R41 trials. R39 is the earlier scalar implementation; the immediate source parent is the rejected R40 vector writer. The R39 control window and raw samples are in [R40 evidence](https://github.com/PerryTS/perry/blob/df1c4e18edc15ea37ec294a2b7c25af070001276/benchmarks/json_performance/NATIVE_VECTOR_ESCAPE_R40.md). They are not counted as new R41 trials.

A follow-up analyzes four earlier R26/R39 profiles that independently exceed the unchanged 500-workload-sample floor: both wide-object stringify profiles and both 8 MiB record-object parse profiles. Every complete output matches a fresh pinned-Node execution of the same loop. The two wide-parse profiles remain excluded at 293 and 448 samples. The original six-case analysis refusal and failed profiling attempts are preserved. Sampled-process CPU/RSS is diagnostic, not benchmark evidence. The historical first-attempt envelope loss remains explicit in its recovery note; no byte-preservation claim is made for those lost envelopes.

These profiles identify a large inlined object/value parser and time spent scanning keys for stream flags during wide-object stringify. They motivate new experiments but do not establish the cause of the small measured regressions.

The successful full retry initially hit a local archive filename assertion. Its first subsequent remote operation recovered the complete quiet window; a local wrapper-argument comparison then needed correction before the verification receipt was written. Both controller failures, the successful archive transfer and final local verification are recorded explicitly. The hash-matched remote controller completed its checked benchmark command, and every raw output and sample passed the ordinary analyzer. No remote performance rerun or gate relaxation was used for that archive recovery.

## Artifact provenance and limits

Measured candidate source: `3952e832058a5e1efcd4cdf9cf28f26e31d60ea3`. Frozen R26 reference source: `3aac4d6335da54abeeed73df842decbbe6dd5d71`. Normal release flags and all three production packages were used. Compiler, runtime, stdlib, workers, fixture bytes, commands, complete verification outputs and CPU/RSS sample vectors are hash-checked and indexed. Every remote window was archived before further remote operations.

No R41 Korean, small-options, changing-object, retained-output or access-specific performance window ran. The full screen includes its normal scan/roundtrip rows. Experimental harness files copied from earlier rounds are not evidence that their uninvoked phases ran. R32 integer-remainder access improvements are outside this source lineage and must be integrated and remeasured before landing.
