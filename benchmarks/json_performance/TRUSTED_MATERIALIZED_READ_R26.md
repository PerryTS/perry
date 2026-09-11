# Direct reads of owned materialized JSON arrays: R26

Post-parse materialized reads improve without adding intermediate allocations. In the initial access window, 1 MB field reads use 15.84% less CPU, random reads 9.60% less and sequential reads 8.07% less; 16 KB random reads improve 8.24%. An independent 11-repetition run reproduces these gains at 15.15%, 9.51%, 7.61% and 8.23%, respectively, with separated observed ranges. These are read gains after parsing. End-to-end full consumption improves 0.82% at 1 MB and 0.68% at 8 MB, also with separated ranges.

There are measured tradeoffs. The 20 MB sequential-access median increases 3.30%, then 3.21% in the independent recheck, approximately 0.5 ns per read. Both ranges overlap, but the repeated positive trend remains a cost, not equivalence. The ordinary array dispatcher has identical symbolic instructions; this does not establish the cause. Large Unicode stringify initially rises 7.95%, then 3.10% in an independent 11-repetition recheck, with overlapping ranges. ASCII stringify rises 3.40% initially and falls 0.52% in the recheck. The Unicode positive trend remains unresolved.

All 38 repeated-input parse/stringify medians remain below Node and Bun, as already true in R25; caches and lazy construction contribute, so this does not establish general object dominance. All 15 changing-input/select controls, seven stringify option controls, five historical regression controls and 36 retained-output CPU ranges overlap. The other 48 full-matrix ranges overlap. Full-matrix peak RSS changes are 0 to +48 KiB; retained peak and after-RSS changes are 0 to +32 KiB. No retained output count or full-output check failed. The zero-spacing stringify gains remain intact.

The eight windows contain 4,196 timed trials, 624 verification records and 60 calibration trials. All pass checksum, source/artifact hash, sample-vector, median and quiet-window validation. These are controlled R25 comparisons, not a new merged-main baseline. Remaining work includes fresh small-object parse, large Unicode text parse versus Bun, pretty/callback stringify, and object-field access versus Node. The 1 MB field-access result is still about 4.88 times Node CPU and 3.68 times Bun CPU. Independent R25 profiles identify repeated UTF-16 counting as a large Unicode parse cost; the attached length-reuse model is a proposal, not a production speedup.

## Scope and reference

The outlined lazy-array read helper now reads the exact materialized array edge owned and traced by its live lazy header. A normal GC array header, non-forwarded state and the existing length/capacity/sanity bounds permit the direct read. Growth forwarding and inconsistent headers retain the general resolver; descriptors, holes and cold construction retain the rooted accessor. Both fast and resolved paths refresh the cached length. No generic array resolver, GC policy, threshold, parse-boundary or lazy-admission change is included.

Measured source `3aac4d6335da54abeeed73df842decbbe6dd5d71` versus exact frozen R25 `01f2878dad8efc92e394c49b88fe0800b871f593`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R25, not current main. Earlier R24/R25 implementation bytes are in ready PRs #10050/#10052 with separate release metadata. This experiment is not a merged-main measurement.

## Validation

295 serial release JSON runtime tests pass on measured source 3aac4d6335da54abeeed73df842decbbe6dd5d71, including actual copied-minor materialized-array movement, identity, descriptors and growth-forwarding tests. The extended mutation witness asserts an unforwarded shrink, a distinct capacity-crossing grown array, an initially stale owner edge and correct cached lengths without entering rooted construction on a dense hit. Exact all-three production build completed in 429.554 seconds; all frozen artifact hashes and post-start mtimes verified. Candidate runs all 81 behavior/options checks freshly; the exact R25 reference reuses all 81. Every scheduled subject has positive moved-object and protected-from-space counters. Six worker objects and 24 native/shadow IR files match (native ModuleID comment only normalized). New zero/retained native and all shadow checks pass; changing-record native retains four unsuppressed R25 findings, and the original corpus retains sixteen. Matching baselines are not clean native passes. Known lazy-spacer crashes/noncanonical outputs, getter and fractional-spacing differences remain explicit preserved baseline outcomes. Script lint passes 73/74 executed checks, with the existing public benchmark freshness failure; compile-tier and CI-only checks are skipped. File cap passes. No GC policy or parse-boundary changes.

All 81 reference behavior/options receipts are reused from the exact R25 build. Candidate executions are fresh. Original compiler input paths are held fixed; native IR normalizes only the ModuleID path comment, and shadow IR must match byte-for-byte. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; the candidate zero-spacing, changing-record and retained-zero subjects run fresh checker commands, while their reference verdicts are reused from R25. Existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration, whole-process peak RSS is reported separately. Every terminal window is archived first, including failures. Access loops measure post-parse reads; repeated-input parse includes existing caches/lazy construction and is complemented by consumption and rotating-input cases. Retained outputs remain live. One-operation CPU rows have limited timer resolution. Overlapping ranges do not establish equivalence.

### Post-parse array access

Window: 2026-09-11T09:17:47Z to 2026-09-11T09:18:10Z.

| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / repeat | 0.010193 | 0.010191 | 0.002756 | 0.004516 | -0.02% | overlap |
| records_array_16k / random | 0.023805 | 0.021843 | 0.006681 | 0.009295 | -8.24% | gain |
| records_array_16k / fields | 0.056949 | 0.057002 | 0.005401 | 0.009247 | +0.09% | overlap |
| records_array_16k / sequential | 0.026555 | 0.026569 | 0.003934 | 0.005743 | +0.05% | overlap |
| records_array_1m / repeat | 0.010191 | 0.010183 | 0.002775 | 0.004546 | -0.08% | overlap |
| records_array_1m / random | 0.033652 | 0.030423 | 0.010631 | 0.010739 | -9.60% | gain |
| records_array_1m / fields | 0.062667 | 0.052741 | 0.010800 | 0.014344 | -15.84% | gain |
| records_array_1m / sequential | 0.026519 | 0.024378 | 0.008210 | 0.007477 | -8.07% | gain |
| records_array_20m / repeat | 0.005649 | 0.005638 | 0.003074 | 0.004823 | -0.19% | overlap |
| records_array_20m / random | 0.025680 | 0.025583 | 0.008606 | 0.010032 | -0.38% | overlap |
| records_array_20m / fields | 0.027165 | 0.027187 | 0.011691 | 0.014624 | +0.08% | overlap |
| records_array_20m / sequential | 0.015614 | 0.016129 | 0.006889 | 0.008241 | +3.30% | overlap |

| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| records_array_16k / repeat | 13.031 | 13.047 | 57.875 | 34.781 | +0.016 |
| records_array_16k / random | 13.406 | 13.422 | 57.906 | 35.406 | +0.016 |
| records_array_16k / fields | 13.156 | 13.172 | 58.062 | 35.969 | +0.016 |
| records_array_16k / sequential | 13.156 | 13.172 | 57.828 | 35.219 | +0.016 |
| records_array_1m / repeat | 17.547 | 17.562 | 63.734 | 37.922 | +0.016 |
| records_array_1m / random | 18.469 | 18.484 | 66.688 | 38.906 | +0.016 |
| records_array_1m / fields | 18.250 | 18.266 | 66.781 | 40.281 | +0.016 |
| records_array_1m / sequential | 18.250 | 18.281 | 66.609 | 39.188 | +0.031 |
| records_array_20m / repeat | 96.891 | 96.922 | 194.750 | 93.938 | +0.031 |
| records_array_20m / random | 96.922 | 96.922 | 194.797 | 94.703 | +0.000 |
| records_array_20m / fields | 96.906 | 96.938 | 195.078 | 95.359 | +0.031 |
| records_array_20m / sequential | 96.906 | 96.938 | 194.875 | 94.594 | +0.031 |

### Independent access recheck (11 repetitions)

Window: 2026-09-11T09:21:22Z to 2026-09-11T09:21:56Z.

| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / repeat | 0.010178 | 0.010178 | 0.002751 | 0.004502 | +0.00% | overlap |
| records_array_16k / random | 0.023791 | 0.021833 | 0.006679 | 0.009285 | -8.23% | gain |
| records_array_16k / fields | 0.057060 | 0.057034 | 0.005402 | 0.009255 | -0.05% | overlap |
| records_array_16k / sequential | 0.026569 | 0.026572 | 0.003920 | 0.005763 | +0.01% | overlap |
| records_array_1m / repeat | 0.010176 | 0.010187 | 0.002785 | 0.004558 | +0.11% | overlap |
| records_array_1m / random | 0.033643 | 0.030445 | 0.010594 | 0.010781 | -9.51% | gain |
| records_array_1m / fields | 0.062428 | 0.052971 | 0.010791 | 0.014326 | -15.15% | gain |
| records_array_1m / sequential | 0.026452 | 0.024439 | 0.008166 | 0.007471 | -7.61% | gain |
| records_array_20m / repeat | 0.005649 | 0.005645 | 0.003053 | 0.004801 | -0.07% | overlap |
| records_array_20m / random | 0.025661 | 0.025579 | 0.008587 | 0.010036 | -0.32% | overlap |
| records_array_20m / fields | 0.027153 | 0.027165 | 0.011687 | 0.014585 | +0.04% | overlap |
| records_array_20m / sequential | 0.015619 | 0.016121 | 0.006863 | 0.008258 | +3.21% | overlap |

| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| records_array_16k / repeat | 13.047 | 13.047 | 57.859 | 34.781 | +0.000 |
| records_array_16k / random | 13.406 | 13.422 | 57.875 | 35.406 | +0.016 |
| records_array_16k / fields | 13.156 | 13.172 | 58.000 | 35.984 | +0.016 |
| records_array_16k / sequential | 13.156 | 13.172 | 57.828 | 35.219 | +0.016 |
| records_array_1m / repeat | 17.547 | 17.562 | 63.688 | 37.922 | +0.016 |
| records_array_1m / random | 18.469 | 18.469 | 66.641 | 38.875 | +0.000 |
| records_array_1m / fields | 18.266 | 18.281 | 66.797 | 40.297 | +0.016 |
| records_array_1m / sequential | 18.250 | 18.281 | 66.625 | 39.188 | +0.031 |
| records_array_20m / repeat | 96.906 | 96.906 | 194.766 | 93.938 | +0.000 |
| records_array_20m / random | 96.922 | 96.938 | 194.812 | 94.734 | +0.016 |
| records_array_20m / fields | 96.922 | 96.922 | 195.016 | 95.328 | +0.000 |
| records_array_20m / sequential | 96.906 | 96.938 | 194.859 | 94.562 | +0.031 |

### Historical regression screen (11 repetitions)

Window: 2026-09-11T09:20:02Z to 2026-09-11T09:20:55Z.

| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| tiny_object / parse | 0.036800 | 0.036804 | 0.081770 | 0.045172 | +0.01% | overlap |
| records_array_16k / roundtrip | 20.994908 | 20.981851 | 53.968795 | 57.672803 | -0.06% | overlap |
| records_array_1m / roundtrip | 1410.686957 | 1414.669565 | 3582.182609 | 3109.008696 | +0.28% | overlap |
| records_object_1m / parse | 2008.469136 | 2009.962963 | 2750.037037 | 2152.123457 | +0.07% | overlap |
| heterogeneous_1m / stringify | 718.422018 | 718.637615 | 897.683486 | 1055.706422 | +0.03% | overlap |

| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / parse | 32.266 | 32.281 | 59.578 | 69.078 | +0.016 |
| records_array_16k / roundtrip | 69.219 | 69.234 | 65.891 | 70.922 | +0.016 |
| records_array_1m / roundtrip | 61.469 | 61.484 | 152.109 | 93.312 | +0.016 |
| records_object_1m / parse | 66.422 | 66.469 | 92.953 | 79.188 | +0.047 |
| heterogeneous_1m / stringify | 61.703 | 61.719 | 128.984 | 104.891 | +0.016 |

### Original 38 parse/stringify plus 12 consumption cases

Window: 2026-09-11T09:23:56Z to 2026-09-11T09:30:45Z.

| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| null / parse | 0.010631 | 0.010629 | 0.027352 | 0.019164 | -0.02% | overlap |
| null / stringify | 0.006563 | 0.006569 | 0.027213 | 0.031079 | +0.09% | overlap |
| string_a / parse | 0.012821 | 0.012818 | 0.031737 | 0.022575 | -0.02% | overlap |
| string_a / stringify | 0.009689 | 0.009692 | 0.030075 | 0.031968 | +0.03% | overlap |
| empty_object / parse | 0.023386 | 0.023435 | 0.045186 | 0.024139 | +0.21% | overlap |
| empty_object / stringify | 0.017576 | 0.017576 | 0.031947 | 0.032567 | +0.00% | overlap |
| tiny_object / parse | 0.036744 | 0.036724 | 0.081602 | 0.045172 | -0.05% | overlap |
| tiny_object / stringify | 0.034004 | 0.033963 | 0.037158 | 0.043165 | -0.12% | overlap |
| small_record / parse | 0.098751 | 0.098676 | 0.340155 | 0.244102 | -0.08% | overlap |
| small_record / stringify | 0.045690 | 0.045657 | 0.108210 | 0.119312 | -0.07% | overlap |
| object_1k / parse | 0.082167 | 0.082036 | 0.533959 | 0.228081 | -0.16% | overlap |
| object_1k / stringify | 0.115320 | 0.115539 | 0.193702 | 0.210967 | +0.19% | overlap |
| records_array_16k / parse | 13.588355 | 13.601294 | 41.365739 | 33.833392 | +0.10% | overlap |
| records_array_16k / stringify | 10.987258 | 11.007723 | 13.181790 | 23.220017 | +0.19% | overlap |
| records_array_16k / sparse | 20.040544 | 20.102186 | 39.786096 | 33.936451 | +0.31% | overlap |
| records_array_16k / scan | 59.010370 | 58.976070 | 41.861739 | 35.709652 | -0.06% | overlap |
| records_array_16k / roundtrip | 20.973495 | 20.971928 | 54.191017 | 57.673717 | -0.01% | overlap |
| records_array_1m / parse | 918.805882 | 917.252941 | 2754.035294 | 2141.070588 | -0.17% | overlap |
| records_array_1m / stringify | 649.427948 | 647.017467 | 848.152838 | 966.786026 | -0.37% | overlap |
| records_array_1m / sparse | 966.888199 | 968.472050 | 2666.869565 | 2184.403727 | +0.16% | overlap |
| records_array_1m / scan | 2813.463768 | 2790.347826 | 2962.115942 | 2228.884058 | -0.82% | gain |
| records_array_1m / roundtrip | 1411.417391 | 1411.756522 | 3552.669565 | 3110.860870 | +0.02% | overlap |
| records_object_1m / parse | 2009.617284 | 2009.666667 | 2738.259259 | 2152.543210 | +0.00% | overlap |
| records_object_1m / stringify | 649.620087 | 648.362445 | 848.947598 | 966.925764 | -0.19% | overlap |
| records_array_8m / parse | 8274.625000 | 8278.312500 | 32861.125000 | 20785.062500 | +0.04% | overlap |
| records_array_8m / stringify | 4735.172414 | 4728.620690 | 7249.586207 | 8164.448276 | -0.14% | overlap |
| records_array_8m / sparse | 8861.266667 | 8845.533333 | 33636.066667 | 21556.266667 | -0.18% | overlap |
| records_array_8m / scan | 21746.125000 | 21598.375000 | 29692.375000 | 21295.750000 | -0.68% | gain |
| records_array_8m / roundtrip | 13105.181818 | 13119.181818 | 36694.909091 | 27948.363636 | +0.11% | overlap |
| records_object_8m / parse | 15908.000000 | 15889.100000 | 34471.600000 | 20849.000000 | -0.12% | overlap |
| records_object_8m / stringify | 4780.655172 | 4780.137931 | 7269.172414 | 8200.655172 | -0.01% | overlap |
| records_array_20m / parse | 39698.500000 | 39706.750000 | 94315.500000 | 53575.750000 | +0.02% | overlap |
| records_array_20m / stringify | 11790.833333 | 11783.666667 | 17308.750000 | 20117.750000 | -0.06% | overlap |
| records_array_20m / sparse | 39739.250000 | 39720.750000 | 97010.500000 | 53648.250000 | -0.05% | overlap |
| records_array_20m / scan | 40839.000000 | 40857.000000 | 90303.000000 | 54155.250000 | +0.04% | overlap |
| records_array_20m / roundtrip | 99217.500000 | 99246.000000 | 103404.000000 | 77123.000000 | +0.03% | overlap |
| records_object_20m / parse | 39720.500000 | 39680.500000 | 97426.250000 | 53752.250000 | -0.10% | overlap |
| records_object_20m / stringify | 11747.500000 | 11748.000000 | 17336.666667 | 20157.750000 | +0.00% | overlap |
| numbers_1m / parse | 1550.921569 | 1552.323529 | 3129.558824 | 3209.137255 | +0.09% | overlap |
| numbers_1m / stringify | 1015.945578 | 1014.448980 | 1975.136054 | 2943.823129 | -0.15% | overlap |
| long_string_1m / parse | 0.348895 | 0.349964 | 364.483963 | 64.627584 | +0.31% | overlap |
| long_string_1m / stringify | 35.081165 | 36.275234 | 102.079084 | 95.209157 | +3.40% | overlap |
| escaped_1m / parse | 981.050633 | 980.164557 | 1761.575949 | 2073.151899 | -0.09% | overlap |
| escaped_1m / stringify | 884.012048 | 884.867470 | 1893.680723 | 2083.855422 | +0.10% | overlap |
| unicode_1m / parse | 0.348528 | 0.348528 | 443.351400 | 58.178033 | +0.00% | overlap |
| unicode_1m / stringify | 28.085657 | 30.317172 | 409.084040 | 448.523636 | +7.95% | overlap |
| wide_1m / parse | 2906.031250 | 2906.562500 | 5290.375000 | 4133.312500 | +0.02% | overlap |
| wide_1m / stringify | 634.073276 | 634.672414 | 6337.310345 | 664.172414 | +0.09% | overlap |
| heterogeneous_1m / parse | 1116.035211 | 1117.105634 | 3846.901408 | 2937.718310 | +0.10% | overlap |
| heterogeneous_1m / stringify | 718.444954 | 718.701835 | 896.715596 | 1056.275229 | +0.04% | overlap |

| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| null / parse | 12.797 | 12.812 | 57.703 | 35.750 | +0.016 |
| null / stringify | 13.000 | 13.016 | 59.656 | 129.812 | +0.016 |
| string_a / parse | 12.797 | 12.812 | 57.719 | 36.125 | +0.016 |
| string_a / stringify | 13.000 | 13.016 | 59.625 | 129.812 | +0.016 |
| empty_object / parse | 32.125 | 32.141 | 59.547 | 69.078 | +0.016 |
| empty_object / stringify | 13.094 | 13.109 | 59.656 | 129.812 | +0.016 |
| tiny_object / parse | 32.266 | 32.281 | 59.641 | 69.078 | +0.016 |
| tiny_object / stringify | 32.328 | 32.328 | 59.688 | 129.797 | +0.000 |
| small_record / parse | 79.531 | 79.547 | 59.672 | 79.906 | +0.016 |
| small_record / stringify | 33.328 | 33.344 | 59.734 | 378.703 | +0.016 |
| object_1k / parse | 64.938 | 64.953 | 61.844 | 71.281 | +0.016 |
| object_1k / stringify | 33.359 | 33.375 | 61.797 | 70.875 | +0.016 |
| records_array_16k / parse | 63.422 | 63.438 | 65.859 | 70.641 | +0.016 |
| records_array_16k / stringify | 33.516 | 33.531 | 61.938 | 71.062 | +0.016 |
| records_array_16k / sparse | 72.219 | 72.234 | 66.016 | 70.656 | +0.016 |
| records_array_16k / scan | 204.453 | 204.453 | 61.891 | 77.422 | +0.000 |
| records_array_16k / roundtrip | 68.750 | 68.766 | 65.922 | 70.875 | +0.016 |
| records_array_1m / parse | 67.109 | 67.109 | 125.656 | 88.750 | +0.000 |
| records_array_1m / stringify | 63.000 | 63.016 | 129.391 | 103.703 | +0.016 |
| records_array_1m / sparse | 67.625 | 67.656 | 125.625 | 97.500 | +0.031 |
| records_array_1m / scan | 158.500 | 158.516 | 97.516 | 83.250 | +0.016 |
| records_array_1m / roundtrip | 61.469 | 61.484 | 152.156 | 93.297 | +0.016 |
| records_object_1m / parse | 66.422 | 66.438 | 93.000 | 79.172 | +0.016 |
| records_object_1m / stringify | 63.031 | 63.047 | 129.453 | 103.719 | +0.016 |
| records_array_8m / parse | 108.812 | 108.828 | 266.000 | 141.406 | +0.016 |
| records_array_8m / stringify | 121.125 | 121.141 | 212.547 | 176.594 | +0.016 |
| records_array_8m / sparse | 109.156 | 109.172 | 266.000 | 188.375 | +0.016 |
| records_array_8m / scan | 186.719 | 186.734 | 230.172 | 133.641 | +0.016 |
| records_array_8m / roundtrip | 129.250 | 129.266 | 267.453 | 149.359 | +0.016 |
| records_object_8m / parse | 187.312 | 187.328 | 244.266 | 131.500 | +0.016 |
| records_object_8m / stringify | 121.156 | 121.188 | 212.516 | 176.812 | +0.031 |
| records_array_20m / parse | 255.422 | 255.438 | 359.812 | 220.719 | +0.016 |
| records_array_20m / stringify | 235.203 | 235.219 | 459.469 | 304.953 | +0.016 |
| records_array_20m / sparse | 255.422 | 255.422 | 359.938 | 220.656 | +0.000 |
| records_array_20m / scan | 255.422 | 255.438 | 330.828 | 225.984 | +0.016 |
| records_array_20m / roundtrip | 295.344 | 295.359 | 342.172 | 256.172 | +0.016 |
| records_object_20m / parse | 255.406 | 255.422 | 359.734 | 220.625 | +0.016 |
| records_object_20m / stringify | 235.250 | 235.266 | 459.516 | 304.938 | +0.016 |
| numbers_1m / parse | 62.766 | 62.781 | 104.156 | 68.234 | +0.016 |
| numbers_1m / stringify | 57.828 | 57.875 | 113.094 | 74.266 | +0.047 |
| long_string_1m / parse | 16.438 | 16.438 | 209.203 | 159.453 | +0.000 |
| long_string_1m / stringify | 54.469 | 54.516 | 183.656 | 161.641 | +0.047 |
| escaped_1m / parse | 58.391 | 58.406 | 102.562 | 67.906 | +0.016 |
| escaped_1m / stringify | 54.875 | 54.891 | 124.828 | 74.797 | +0.016 |
| unicode_1m / parse | 15.766 | 15.766 | 198.312 | 154.516 | +0.000 |
| unicode_1m / stringify | 53.844 | 53.859 | 186.297 | 155.344 | +0.016 |
| wide_1m / parse | 227.688 | 227.703 | 121.141 | 95.078 | +0.016 |
| wide_1m / stringify | 68.625 | 68.672 | 117.969 | 84.500 | +0.047 |
| heterogeneous_1m / parse | 62.547 | 62.562 | 92.891 | 99.531 | +0.016 |
| heterogeneous_1m / stringify | 61.703 | 61.719 | 128.938 | 104.094 | +0.016 |

### Independent large-string stringify recheck (11 repetitions)

Window: 2026-09-11T09:33:01Z to 2026-09-11T09:33:34Z.

| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| long_string_1m / stringify | 36.109781 | 35.921956 | 102.544745 | 95.676899 | -0.52% | overlap |
| unicode_1m / stringify | 28.769697 | 29.661010 | 409.786667 | 449.090505 | +3.10% | overlap |

| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| long_string_1m / stringify | 54.469 | 54.484 | 183.641 | 155.609 | +0.016 |
| unicode_1m / stringify | 53.844 | 53.859 | 186.203 | 155.375 | +0.016 |

### Stringify option controls

Window: 2026-09-11T09:36:30Z to 2026-09-11T09:37:34Z.

| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.044920 | 0.044793 | 0.108583 | 0.119955 | -0.28% | overlap |
| small_record / dynamic-zero | 0.044672 | 0.044635 | 0.247271 | 0.394588 | -0.08% | overlap |
| small_record / zero | 0.044873 | 0.044823 | 0.247764 | 0.394338 | -0.11% | overlap |
| small_record / pretty | 0.486143 | 0.483458 | 0.304180 | 0.525227 | -0.55% | overlap |
| small_record / keys | 0.460815 | 0.460790 | 0.594732 | 0.484313 | -0.01% | overlap |
| small_record / callback | 0.946265 | 0.945875 | 0.602920 | 0.580590 | -0.04% | overlap |
| records_array_16k / pretty | 46.209000 | 46.343000 | 33.699000 | 45.851000 | +0.29% | overlap |

| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.406 | 33.422 | 59.609 | 378.578 | +0.016 |
| small_record / dynamic-zero | 33.438 | 33.406 | 59.562 | 378.641 | -0.031 |
| small_record / zero | 33.422 | 33.422 | 59.594 | 378.625 | +0.000 |
| small_record / pretty | 33.766 | 33.750 | 59.641 | 239.359 | -0.016 |
| small_record / keys | 33.734 | 33.703 | 59.703 | 207.766 | -0.031 |
| small_record / callback | 32.500 | 32.469 | 59.688 | 97.703 | -0.031 |
| records_array_16k / pretty | 35.078 | 35.062 | 57.016 | 49.734 | -0.016 |

### Eight rotating inputs per fixture

Window: 2026-09-11T09:41:36Z to 2026-09-11T09:43:19Z.

| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494463 | 0.494365 | 0.331560 | 0.255622 | -0.02% | overlap |
| small_record / same | 0.098989 | 0.099110 | 0.339871 | 0.245228 | +0.12% | overlap |
| small_record / select | 0.009427 | 0.009437 | 0.002668 | 0.003622 | +0.12% | overlap |
| records_array_1m / rotating | 943.354286 | 942.977143 | 2733.960000 | 2161.880000 | -0.04% | overlap |
| records_array_1m / same | 939.160000 | 939.228571 | 2777.411429 | 2152.428571 | +0.01% | overlap |
| records_array_1m / select | 0.011429 | 0.011319 | 0.003111 | 0.003835 | -0.96% | overlap |
| long_string_1m / rotating | 99.934109 | 100.062016 | 372.451163 | 70.704651 | +0.13% | overlap |
| long_string_1m / same | 0.360013 | 0.360651 | 376.974171 | 65.765944 | +0.18% | overlap |
| long_string_1m / select | 0.012002 | 0.011809 | 0.003093 | 0.003886 | -1.60% | overlap |
| escaped_1m / rotating | 1013.540881 | 1013.471698 | 1726.157233 | 2078.213836 | -0.01% | overlap |
| escaped_1m / same | 994.550000 | 994.350000 | 1723.906250 | 2074.818750 | -0.02% | overlap |
| escaped_1m / select | 0.011515 | 0.011229 | 0.003110 | 0.003868 | -2.49% | overlap |
| unicode_1m / rotating | 141.950178 | 141.774377 | 439.562989 | 62.933096 | -0.12% | overlap |
| unicode_1m / same | 0.358727 | 0.357296 | 437.377325 | 59.469957 | -0.40% | overlap |
| unicode_1m / select | 0.011388 | 0.011539 | 0.003139 | 0.003896 | +1.32% | overlap |

| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.969 | 31.984 | 59.891 | 80.172 | +0.016 |
| small_record / same | 77.609 | 77.609 | 59.641 | 79.859 | +0.000 |
| small_record / select | 12.797 | 12.812 | 57.859 | 35.250 | +0.016 |
| records_array_1m / rotating | 76.281 | 76.297 | 128.891 | 100.406 | +0.016 |
| records_array_1m / same | 76.281 | 76.297 | 128.828 | 99.953 | +0.016 |
| records_array_1m / select | 22.578 | 22.594 | 67.625 | 42.500 | +0.016 |
| long_string_1m / rotating | 90.219 | 90.234 | 177.609 | 152.484 | +0.016 |
| long_string_1m / same | 24.578 | 24.609 | 197.609 | 142.547 | +0.031 |
| long_string_1m / select | 24.312 | 24.312 | 68.984 | 43.766 | +0.000 |
| escaped_1m / rotating | 64.984 | 65.000 | 89.641 | 77.734 | +0.016 |
| escaped_1m / same | 64.984 | 65.000 | 89.625 | 77.734 | +0.016 |
| escaped_1m / select | 23.516 | 23.531 | 68.438 | 43.234 | +0.016 |
| unicode_1m / rotating | 62.391 | 62.391 | 159.484 | 147.203 | +0.000 |
| unicode_1m / same | 22.688 | 22.688 | 212.047 | 149.953 | +0.000 |
| unicode_1m / select | 22.422 | 22.438 | 68.578 | 44.641 | +0.016 |

### Retained outputs

Window: 2026-09-11T09:38:08Z to 2026-09-11T09:39:08Z.

| Fixture / operation | R25 µs | R26 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| tiny_object / retain-parse / 1 live | 32.000000 | 31.000000 | 379.000000 | 9.000000 | -3.12% | overlap |
| tiny_object / retain-parse / 200000 live | 0.044260 | 0.044365 | 0.131485 | 0.058280 | +0.24% | overlap |
| tiny_object / retain-stringify / 1 live | 11.000000 | 11.000000 | 361.000000 | 7.000000 | +0.00% | overlap |
| tiny_object / retain-stringify / 200000 live | 0.047110 | 0.047160 | 0.083200 | 0.049735 | +0.11% | overlap |
| small_record / retain-parse / 1 live | 32.000000 | 33.000000 | 384.000000 | 12.000000 | +3.12% | overlap |
| small_record / retain-parse / 100000 live | 0.103920 | 0.103910 | 0.483130 | 0.263430 | -0.01% | overlap |
| small_record / retain-stringify / 1 live | 13.000000 | 13.000000 | 370.000000 | 9.000000 | +0.00% | overlap |
| small_record / retain-stringify / 100000 live | 0.067160 | 0.067390 | 0.187090 | 0.135780 | +0.34% | overlap |
| records_array_1m / retain-parse / 1 live | 931.000000 | 935.000000 | 4001.000000 | 2404.000000 | +0.43% | overlap |
| records_array_1m / retain-parse / 16 live | 852.437500 | 855.000000 | 3642.375000 | 2131.875000 | +0.30% | overlap |
| records_array_1m / retain-stringify / 1 live | 800.000000 | 803.000000 | 1335.000000 | 1075.000000 | +0.37% | overlap |
| records_array_1m / retain-stringify / 16 live | 658.750000 | 658.625000 | 998.937500 | 992.437500 | -0.02% | overlap |
| records_object_1m / retain-parse / 1 live | 1871.000000 | 1864.000000 | 3986.000000 | 2413.000000 | -0.37% | overlap |
| records_object_1m / retain-parse / 16 live | 2977.375000 | 2983.375000 | 3667.062500 | 2136.125000 | +0.20% | overlap |
| records_object_1m / retain-stringify / 1 live | 803.000000 | 808.000000 | 1340.000000 | 1071.000000 | +0.62% | overlap |
| records_object_1m / retain-stringify / 16 live | 663.375000 | 661.062500 | 1000.250000 | 993.312500 | -0.35% | overlap |
| records_array_8m / retain-parse / 1 live | 8228.000000 | 8317.000000 | 27816.000000 | 18697.000000 | +1.08% | overlap |
| records_array_8m / retain-parse / 4 live | 8237.000000 | 8246.250000 | 28302.250000 | 21722.500000 | +0.11% | overlap |
| records_array_8m / retain-stringify / 1 live | 29778.000000 | 29714.000000 | 7771.000000 | 8236.000000 | -0.21% | overlap |
| records_array_8m / retain-stringify / 4 live | 16193.250000 | 16183.500000 | 8870.750000 | 9393.250000 | -0.06% | overlap |
| records_object_8m / retain-parse / 1 live | 14429.000000 | 14431.000000 | 28588.000000 | 18644.000000 | +0.01% | overlap |
| records_object_8m / retain-parse / 4 live | 22261.000000 | 22260.000000 | 27920.750000 | 21721.000000 | -0.00% | overlap |
| records_object_8m / retain-stringify / 1 live | 29742.000000 | 29812.000000 | 7738.000000 | 8239.000000 | +0.24% | overlap |
| records_object_8m / retain-stringify / 4 live | 16166.000000 | 16192.000000 | 8899.500000 | 9442.500000 | +0.16% | overlap |
| long_string_1m / retain-parse / 1 live | 164.000000 | 163.000000 | 813.000000 | 132.000000 | -0.61% | overlap |
| long_string_1m / retain-parse / 32 live | 5.593750 | 5.656250 | 465.593750 | 141.312500 | +1.12% | overlap |
| long_string_1m / retain-stringify / 1 live | 132.000000 | 138.000000 | 930.000000 | 152.000000 | +4.55% | overlap |
| long_string_1m / retain-stringify / 32 live | 171.375000 | 172.062500 | 193.187500 | 173.250000 | +0.40% | overlap |
| unicode_1m / retain-parse / 1 live | 198.000000 | 201.000000 | 1240.000000 | 122.000000 | +1.52% | overlap |
| unicode_1m / retain-parse / 32 live | 6.562500 | 6.593750 | 528.750000 | 104.187500 | +0.48% | overlap |
| unicode_1m / retain-stringify / 1 live | 120.000000 | 120.000000 | 1231.000000 | 587.000000 | +0.00% | overlap |
| unicode_1m / retain-stringify / 32 live | 67.250000 | 67.937500 | 495.906250 | 495.062500 | +1.02% | overlap |
| wide_1m / retain-parse / 1 live | 2611.000000 | 2606.000000 | 7526.000000 | 5711.000000 | -0.19% | overlap |
| wide_1m / retain-parse / 16 live | 3665.375000 | 3676.937500 | 5572.875000 | 4313.312500 | +0.32% | overlap |
| wide_1m / retain-stringify / 1 live | 2502.000000 | 2492.000000 | 7593.000000 | 771.000000 | -0.40% | overlap |
| wide_1m / retain-stringify / 16 live | 752.375000 | 752.500000 | 6471.000000 | 702.937500 | +0.02% | overlap |

| Fixture / operation | R25 MiB | R26 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / retain-parse / 1 live | 12.969 | 12.984 | 53.594 | 28.719 | +0.016 |
| tiny_object / retain-parse / 200000 live | 25.438 | 25.453 | 75.562 | 50.641 | +0.016 |
| tiny_object / retain-stringify / 1 live | 13.062 | 13.078 | 53.672 | 28.750 | +0.016 |
| tiny_object / retain-stringify / 200000 live | 25.516 | 25.547 | 72.047 | 47.484 | +0.031 |
| small_record / retain-parse / 1 live | 12.969 | 12.984 | 53.641 | 28.719 | +0.016 |
| small_record / retain-parse / 100000 live | 25.469 | 25.469 | 86.562 | 53.125 | +0.000 |
| small_record / retain-stringify / 1 live | 13.078 | 13.094 | 53.703 | 28.781 | +0.016 |
| small_record / retain-stringify / 100000 live | 28.703 | 28.719 | 81.188 | 51.500 | +0.016 |
| records_array_1m / retain-parse / 1 live | 17.594 | 17.625 | 60.359 | 32.078 | +0.031 |
| records_array_1m / retain-parse / 16 live | 41.484 | 41.500 | 89.562 | 56.344 | +0.016 |
| records_array_1m / retain-stringify / 1 live | 20.156 | 20.172 | 63.031 | 34.859 | +0.016 |
| records_array_1m / retain-stringify / 16 live | 32.828 | 32.844 | 77.125 | 47.672 | +0.016 |
| records_object_1m / retain-parse / 1 live | 16.406 | 16.422 | 60.328 | 32.062 | +0.016 |
| records_object_1m / retain-parse / 16 live | 52.984 | 53.000 | 89.641 | 56.359 | +0.016 |
| records_object_1m / retain-stringify / 1 live | 20.172 | 20.172 | 63.047 | 34.859 | +0.000 |
| records_object_1m / retain-stringify / 16 live | 32.844 | 32.859 | 77.172 | 47.672 | +0.016 |
| records_array_8m / retain-parse / 1 live | 49.562 | 49.578 | 97.859 | 52.766 | +0.016 |
| records_array_8m / retain-parse / 4 live | 85.812 | 85.828 | 159.203 | 92.312 | +0.016 |
| records_array_8m / retain-stringify / 1 live | 79.922 | 79.922 | 118.328 | 73.906 | +0.000 |
| records_array_8m / retain-stringify / 4 live | 99.125 | 99.141 | 144.750 | 94.906 | +0.016 |
| records_object_8m / retain-parse / 1 live | 45.453 | 45.469 | 97.781 | 52.750 | +0.016 |
| records_object_8m / retain-parse / 4 live | 94.172 | 94.188 | 159.297 | 92.266 | +0.016 |
| records_object_8m / retain-stringify / 1 live | 79.906 | 79.922 | 118.359 | 73.906 | +0.016 |
| records_object_8m / retain-stringify / 4 live | 99.125 | 99.141 | 144.625 | 94.906 | +0.016 |
| long_string_1m / retain-parse / 1 live | 16.344 | 16.344 | 57.781 | 30.984 | +0.000 |
| long_string_1m / retain-parse / 32 live | 16.359 | 16.375 | 90.188 | 62.094 | +0.016 |
| long_string_1m / retain-stringify / 1 live | 18.719 | 18.750 | 61.672 | 33.062 | +0.031 |
| long_string_1m / retain-stringify / 32 live | 51.734 | 51.750 | 93.344 | 64.203 | +0.016 |
| unicode_1m / retain-parse / 1 live | 15.672 | 15.688 | 58.594 | 31.672 | +0.016 |
| unicode_1m / retain-parse / 32 live | 15.688 | 15.688 | 87.078 | 59.391 | +0.000 |
| unicode_1m / retain-stringify / 1 live | 17.594 | 17.609 | 61.641 | 34.516 | +0.016 |
| unicode_1m / retain-stringify / 32 live | 42.969 | 42.984 | 90.000 | 62.219 | +0.016 |
| wide_1m / retain-parse / 1 live | 21.438 | 21.453 | 66.531 | 36.734 | +0.016 |
| wide_1m / retain-parse / 16 live | 77.641 | 77.656 | 113.281 | 69.344 | +0.016 |
| wide_1m / retain-stringify / 1 live | 23.875 | 23.891 | 69.453 | 39.688 | +0.016 |
| wide_1m / retain-stringify / 16 live | 37.031 | 37.047 | 87.266 | 53.891 | +0.016 |

| Fixture / operation | R25 after MiB | R26 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / retain-parse / 1 live | 11.906 | 11.922 | 52.828 | 28.328 | +0.016 |
| tiny_object / retain-parse / 200000 live | 24.406 | 24.422 | 74.844 | 50.250 | +0.016 |
| tiny_object / retain-stringify / 1 live | 12.000 | 12.016 | 52.922 | 28.344 | +0.016 |
| tiny_object / retain-stringify / 200000 live | 24.484 | 24.516 | 71.344 | 47.078 | +0.031 |
| small_record / retain-parse / 1 live | 11.906 | 11.922 | 52.875 | 28.328 | +0.016 |
| small_record / retain-parse / 100000 live | 24.422 | 24.422 | 85.812 | 52.734 | +0.000 |
| small_record / retain-stringify / 1 live | 12.016 | 12.031 | 52.969 | 28.375 | +0.016 |
| small_record / retain-stringify / 100000 live | 27.672 | 27.688 | 80.500 | 51.094 | +0.016 |
| records_array_1m / retain-parse / 1 live | 16.578 | 16.609 | 58.594 | 31.688 | +0.031 |
| records_array_1m / retain-parse / 16 live | 40.469 | 40.484 | 88.828 | 55.953 | +0.016 |
| records_array_1m / retain-stringify / 1 live | 19.141 | 19.156 | 60.734 | 34.469 | +0.016 |
| records_array_1m / retain-stringify / 16 live | 31.812 | 31.828 | 76.531 | 47.281 | +0.016 |
| records_object_1m / retain-parse / 1 live | 15.375 | 15.391 | 58.516 | 31.672 | +0.016 |
| records_object_1m / retain-parse / 16 live | 52.484 | 52.500 | 88.906 | 55.969 | +0.016 |
| records_object_1m / retain-stringify / 1 live | 19.156 | 19.156 | 60.688 | 34.469 | +0.000 |
| records_object_1m / retain-stringify / 16 live | 31.828 | 31.844 | 76.516 | 47.281 | +0.016 |
| records_array_8m / retain-parse / 1 live | 48.547 | 48.562 | 94.453 | 52.375 | +0.016 |
| records_array_8m / retain-parse / 4 live | 85.328 | 85.344 | 147.000 | 91.922 | +0.016 |
| records_array_8m / retain-stringify / 1 live | 77.359 | 77.359 | 114.312 | 73.516 | +0.000 |
| records_array_8m / retain-stringify / 4 live | 98.625 | 98.641 | 144.094 | 94.516 | +0.016 |
| records_object_8m / retain-parse / 1 live | 33.172 | 33.188 | 94.406 | 52.359 | +0.016 |
| records_object_8m / retain-parse / 4 live | 90.938 | 90.953 | 147.109 | 91.875 | +0.016 |
| records_object_8m / retain-stringify / 1 live | 77.344 | 77.359 | 114.359 | 73.516 | +0.016 |
| records_object_8m / retain-stringify / 4 live | 98.625 | 98.641 | 144.016 | 94.516 | +0.016 |
| long_string_1m / retain-parse / 1 live | 15.312 | 15.312 | 56.719 | 30.594 | +0.000 |
| long_string_1m / retain-parse / 32 live | 15.328 | 15.344 | 89.297 | 61.703 | +0.016 |
| long_string_1m / retain-stringify / 1 live | 17.688 | 17.719 | 60.750 | 32.672 | +0.031 |
| long_string_1m / retain-stringify / 32 live | 51.219 | 51.234 | 92.422 | 63.797 | +0.016 |
| unicode_1m / retain-parse / 1 live | 14.641 | 14.656 | 57.844 | 31.297 | +0.016 |
| unicode_1m / retain-parse / 32 live | 14.656 | 14.656 | 86.453 | 59.000 | +0.000 |
| unicode_1m / retain-stringify / 1 live | 16.562 | 16.578 | 60.969 | 34.109 | +0.016 |
| unicode_1m / retain-stringify / 32 live | 41.938 | 41.953 | 89.344 | 61.812 | +0.016 |
| wide_1m / retain-parse / 1 live | 20.656 | 20.672 | 65.734 | 36.344 | +0.016 |
| wide_1m / retain-parse / 16 live | 77.141 | 77.156 | 112.641 | 68.953 | +0.016 |
| wide_1m / retain-stringify / 1 live | 22.844 | 22.859 | 68.781 | 39.297 | +0.016 |
| wide_1m / retain-stringify / 16 live | 36.000 | 36.016 | 86.641 | 53.500 | +0.016 |

## Build fingerprints

| Artifact | R25 SHA-256 | R26 SHA-256 |
|---|---|---|
| perry | `fde1808df892dbba645ea4e66b85971c412940d515062abcc86ec07a1c70ac79` | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` |
| libperry_runtime.a | `2207fdcb4a53fa81e2a77722dfeac2650ddbeb76bd02f20ee47260b88c2427d4` | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` |
| libperry_stdlib.a | `a3d51fd8bab32984585d55149cda0d5a4212f5e03944c1241f3cafaee6bec80c` | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` |

The artifact index records archived raw windows, exact sample vectors, validation receipts, source patches and failed attempts. Independent R25 parsing profiles and a future UTF-16-length model, if included in the validation archive, are explicitly diagnostic/proposal evidence and are not R26 performance results.
