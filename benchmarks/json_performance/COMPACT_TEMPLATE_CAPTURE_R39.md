# Compact parse-template capture: R39

R39 is not promoted. Fresh small-record parsing improves 2.91% in the first quiet window and 2.78% in an independent 11-pair recheck, with every pair improving. The broader 50-row screen finds seven separated regressions; all seven recur in the independent recheck. Confirmed deltas are +0.61% for 16 KiB record-array sparse consumption, +0.70% and +1.42% for 1 MiB and 8 MiB record-object parsing, +0.66% for 20 MiB record-array stringify, +4.51% for wide-object parsing, +2.32% for wide-object stringify and +0.65% for heterogeneous parsing. Long ASCII/Unicode stringify rechecks overlap with negative median deltas.

Four qualified windows contain 2,348 timed trials, 415 complete-output verification records and 72 calibration trials, producing 77 row comparisons including rechecks. The original 38 repeated-input parse/stringify medians remain below Node and Bun, but repeated-input caches are not representative of fresh input or general object workloads. Rotating input retains the earlier escaped-string parsing improvement (about 6%) and fresh Unicode improvement (about 41%). Peak RSS median deltas across the 77 comparisons range from -176.0 to 128.0 KiB. No R39 options, large-options, Korean, changing-object, retained-output or access performance window ran. These are frozen R26 comparisons, not merged-main measurements.

## Scope and reference

The inherited stringify work roots callback keys, reuses escape-free string provenance safely, clears that provenance on concatenation/appending, handles decoded wide keys through the normalizing builder, and avoids primitive toJSON key copies. The inherited parser separates small inputs from the dominant-token length proof, handles valid-ED continuation bytes and copies ordinary decoded spans within bounded scratch windows. The inherited native-buffer writer retains UTF-8 validation, exact output bounds, geometric growth and WTF-8 fallback; the builder uses a bulk ASCII check. R39 changes only small-object template capture: validated local slots are initialized on demand, and an existing cached plan is updated in place. Failed partial capture leaves the previous plan intact. No managed intermediate allocation or GC entry is added. The decoder does not reserve or allocate additional scratch storage. GC policy, parse boundaries, root ordering, admission and thresholds remain unchanged.

Measured source `e69d1292110cf9abee24f8690c584652805a4167` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

Measured source e69d1292110cf9abee24f8690c584652805a4167: 311 serial release JSON tests and 119 string-filter tests pass (overlapping filters). All 125 fresh candidate full-output checks pass, including moving/protected stress with positive witnesses. All 30 normalized native/shadow IR comparisons and six worker object comparisons match frozen R26. Ten original-suite checker commands ran freshly across both arms after an initial reuse-guard refusal; its logs and the original candidate stop are preserved under initial-root-reuse-refusal. Known unsuppressed native findings, reference lazy/getter/fraction failures and emitter failures remain explicit. The normal three-package release build completed in 389.64131689071655 seconds, with clean committed source and hashed frozen artifacts. Final-source lint: 72/74. Public baseline freshness and the updated-main raw-handle ceiling issue remain; implementation audits, formatting and file cap pass. The raw-handle failure is independently identical on unchanged R38 against pinned main f6c6879613c0b00b5e3ae873b8d102710d2d0585. No GC policy, thresholds, admission, root ordering or parse boundary changes.

All 125 reference executions are reused with exact source, fixture and frozen R26 artifact hashes; all 125 candidate executions are fresh. The reference has 14 failures in the 20 original emitter checks and eight moving-stress crashes in the 20 large-emitter checks. The candidate passes both fixtures. The primitive-key and large-emitter native findings remain explicit, with identical complete reports and IR in both arms; shadow checks pass. Ten original-suite checker commands executed freshly for both arms over 18 IR files. The full set of comparisons covers 30 IR files and preserves every known finding. The original reuse refusal was due to the generated shape-census metadata change; fresh checks supersede it without changing checker code, budgets or allowlists.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Independent recheck of full-screen regressions (11 repetitions)

Window: 2026-09-11T16:43:19Z to 2026-09-11T16:45:34Z.

| Fixture / operation | R26 µs | R39 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / sparse | 20.059100 | 20.181749 | 41.116802 | 33.951576 | +0.61% | regression |
| records_object_1m / parse | 2010.975309 | 2025.074074 | 2781.814815 | 2152.197531 | +0.70% | regression |
| records_object_8m / parse | 15927.200000 | 16154.000000 | 34586.000000 | 20972.800000 | +1.42% | regression |
| records_array_20m / stringify | 11748.833333 | 11826.333333 | 17321.250000 | 20046.416667 | +0.66% | regression |
| long_string_1m / stringify | 35.786681 | 34.377732 | 101.989594 | 94.725806 | -3.94% | overlap |
| unicode_1m / stringify | 29.202424 | 28.660606 | 409.037172 | 447.877576 | -1.86% | overlap |
| wide_1m / parse | 2885.156250 | 3015.359375 | 5254.734375 | 4134.578125 | +4.51% | regression |
| wide_1m / stringify | 633.482759 | 648.189655 | 6354.219828 | 664.000000 | +2.32% | regression |
| heterogeneous_1m / parse | 1117.422535 | 1124.704225 | 3894.070423 | 2967.549296 | +0.65% | regression |

| Fixture / operation | R26 MiB | R39 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| records_array_16k / sparse | 72.234 | 72.281 | 66.031 | 70.672 | +0.047 |
| records_object_1m / parse | 66.469 | 66.484 | 92.922 | 79.188 | +0.016 |
| records_object_8m / parse | 187.328 | 187.375 | 244.250 | 131.266 | +0.047 |
| records_array_20m / stringify | 235.219 | 235.281 | 459.500 | 304.844 | +0.062 |
| long_string_1m / stringify | 54.484 | 54.531 | 183.656 | 151.656 | +0.047 |
| unicode_1m / stringify | 53.859 | 53.906 | 186.297 | 157.125 | +0.047 |
| wide_1m / parse | 227.719 | 227.750 | 121.156 | 95.125 | +0.031 |
| wide_1m / stringify | 68.641 | 68.766 | 117.984 | 84.656 | +0.125 |
| heterogeneous_1m / parse | 62.562 | 62.625 | 92.906 | 99.516 | +0.062 |

### Original 38 parse/stringify plus 12 consumption cases

Window: 2026-09-11T16:35:11Z to 2026-09-11T16:42:01Z.

| Fixture / operation | R26 µs | R39 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| null / parse | 0.010626 | 0.010629 | 0.027359 | 0.019232 | +0.02% | overlap |
| null / stringify | 0.006565 | 0.006570 | 0.027225 | 0.031130 | +0.08% | overlap |
| string_a / parse | 0.012825 | 0.012823 | 0.031733 | 0.022599 | -0.02% | overlap |
| string_a / stringify | 0.009692 | 0.009691 | 0.030072 | 0.032035 | -0.01% | overlap |
| empty_object / parse | 0.023434 | 0.023433 | 0.045154 | 0.024189 | -0.00% | overlap |
| empty_object / stringify | 0.017581 | 0.017587 | 0.031989 | 0.032647 | +0.03% | overlap |
| tiny_object / parse | 0.036763 | 0.037065 | 0.081360 | 0.045244 | +0.82% | overlap |
| tiny_object / stringify | 0.033985 | 0.033978 | 0.037179 | 0.043216 | -0.02% | overlap |
| small_record / parse | 0.098690 | 0.098510 | 0.329418 | 0.245514 | -0.18% | overlap |
| small_record / stringify | 0.045744 | 0.045791 | 0.108099 | 0.119304 | +0.10% | overlap |
| object_1k / parse | 0.082121 | 0.082037 | 0.532706 | 0.227765 | -0.10% | overlap |
| object_1k / stringify | 0.115324 | 0.115288 | 0.193733 | 0.211002 | -0.03% | overlap |
| records_array_16k / parse | 13.594470 | 13.641794 | 40.417937 | 33.838621 | +0.35% | overlap |
| records_array_16k / stringify | 11.012511 | 11.074600 | 13.174454 | 23.193065 | +0.56% | overlap |
| records_array_16k / sparse | 20.056558 | 20.174504 | 39.096467 | 33.955008 | +0.59% | regression |
| records_array_16k / scan | 58.734379 | 58.915980 | 41.051316 | 35.672162 | +0.31% | overlap |
| records_array_16k / roundtrip | 20.978457 | 20.966053 | 54.459721 | 57.507377 | -0.06% | overlap |
| records_array_1m / parse | 920.341176 | 922.341176 | 2711.152941 | 2139.635294 | +0.22% | overlap |
| records_array_1m / stringify | 648.344978 | 651.737991 | 847.209607 | 966.427948 | +0.52% | overlap |
| records_array_1m / sparse | 967.111801 | 970.347826 | 2792.664596 | 2180.285714 | +0.33% | overlap |
| records_array_1m / scan | 2790.507246 | 2790.130435 | 2838.130435 | 2230.043478 | -0.01% | overlap |
| records_array_1m / roundtrip | 1409.460870 | 1414.026087 | 3633.460870 | 3107.539130 | +0.32% | overlap |
| records_object_1m / parse | 2007.975309 | 2024.308642 | 2754.074074 | 2152.000000 | +0.81% | regression |
| records_object_1m / stringify | 648.768559 | 648.576419 | 847.414847 | 966.388646 | -0.03% | overlap |
| records_array_8m / parse | 8301.250000 | 8305.250000 | 32737.375000 | 20824.375000 | +0.05% | overlap |
| records_array_8m / stringify | 4729.758621 | 4757.034483 | 7250.034483 | 8155.206897 | +0.58% | overlap |
| records_array_8m / sparse | 8831.133333 | 8860.266667 | 33037.800000 | 21567.733333 | +0.33% | overlap |
| records_array_8m / scan | 21581.875000 | 21312.375000 | 29981.875000 | 21438.625000 | -1.25% | gain |
| records_array_8m / roundtrip | 13097.363636 | 13124.545455 | 36231.272727 | 27462.181818 | +0.21% | overlap |
| records_object_8m / parse | 15923.400000 | 16153.600000 | 33600.700000 | 20957.100000 | +1.45% | regression |
| records_object_8m / stringify | 4788.344828 | 4805.689655 | 7246.137931 | 8219.758621 | +0.36% | overlap |
| records_array_20m / parse | 39388.750000 | 39272.250000 | 94842.500000 | 53036.750000 | -0.30% | overlap |
| records_array_20m / stringify | 11747.833333 | 11825.083333 | 17287.416667 | 20041.166667 | +0.66% | regression |
| records_array_20m / sparse | 39379.500000 | 39296.750000 | 96179.750000 | 53237.500000 | -0.21% | overlap |
| records_array_20m / scan | 40353.750000 | 40290.250000 | 90546.000000 | 53937.750000 | -0.16% | gain |
| records_array_20m / roundtrip | 99059.500000 | 99124.500000 | 102109.000000 | 76720.500000 | +0.07% | overlap |
| records_object_20m / parse | 39375.250000 | 39332.250000 | 95395.500000 | 53034.250000 | -0.11% | overlap |
| records_object_20m / stringify | 11704.416667 | 11758.666667 | 17314.666667 | 20107.250000 | +0.46% | overlap |
| numbers_1m / parse | 1549.656863 | 1550.480392 | 3127.382353 | 3201.303922 | +0.05% | overlap |
| numbers_1m / stringify | 1015.340136 | 1019.795918 | 1975.380952 | 2944.020408 | +0.44% | overlap |
| long_string_1m / parse | 0.350677 | 0.347113 | 364.438703 | 64.418033 | -1.02% | overlap |
| long_string_1m / stringify | 34.724246 | 35.724246 | 101.858481 | 95.020812 | +2.88% | overlap |
| escaped_1m / parse | 980.898734 | 922.867089 | 1763.487342 | 2072.936709 | -5.92% | gain |
| escaped_1m / stringify | 884.698795 | 884.397590 | 1894.518072 | 2089.759036 | -0.03% | overlap |
| unicode_1m / parse | 0.349246 | 0.347452 | 441.105887 | 57.904164 | -0.51% | overlap |
| unicode_1m / stringify | 28.870707 | 29.785051 | 410.065859 | 447.664646 | +3.17% | overlap |
| wide_1m / parse | 2890.109375 | 3011.890625 | 5266.843750 | 4135.515625 | +4.21% | regression |
| wide_1m / stringify | 633.543103 | 647.887931 | 6343.952586 | 663.681034 | +2.26% | regression |
| heterogeneous_1m / parse | 1116.401408 | 1123.992958 | 3927.619718 | 2941.929577 | +0.68% | regression |
| heterogeneous_1m / stringify | 717.348624 | 716.839450 | 897.619266 | 1052.972477 | -0.07% | overlap |

| Fixture / operation | R26 MiB | R39 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| null / parse | 12.812 | 12.844 | 57.703 | 35.781 | +0.031 |
| null / stringify | 13.016 | 13.031 | 59.641 | 129.828 | +0.016 |
| string_a / parse | 12.812 | 12.844 | 57.719 | 36.125 | +0.031 |
| string_a / stringify | 13.016 | 13.031 | 59.656 | 129.844 | +0.016 |
| empty_object / parse | 32.141 | 32.203 | 59.625 | 69.094 | +0.062 |
| empty_object / stringify | 13.109 | 13.109 | 59.641 | 129.812 | +0.000 |
| tiny_object / parse | 32.281 | 32.328 | 59.609 | 69.094 | +0.047 |
| tiny_object / stringify | 32.328 | 32.344 | 59.672 | 129.828 | +0.016 |
| small_record / parse | 79.562 | 79.594 | 59.625 | 79.953 | +0.031 |
| small_record / stringify | 33.344 | 33.359 | 59.766 | 378.719 | +0.016 |
| object_1k / parse | 64.953 | 64.969 | 61.828 | 71.266 | +0.016 |
| object_1k / stringify | 33.359 | 33.406 | 61.797 | 70.891 | +0.047 |
| records_array_16k / parse | 63.438 | 63.500 | 65.844 | 70.641 | +0.062 |
| records_array_16k / stringify | 33.531 | 33.594 | 61.891 | 71.078 | +0.062 |
| records_array_16k / sparse | 72.234 | 72.281 | 66.062 | 70.672 | +0.047 |
| records_array_16k / scan | 204.469 | 204.547 | 61.953 | 77.406 | +0.078 |
| records_array_16k / roundtrip | 68.781 | 68.875 | 65.891 | 70.906 | +0.094 |
| records_array_1m / parse | 67.125 | 67.172 | 125.672 | 88.734 | +0.047 |
| records_array_1m / stringify | 63.016 | 63.094 | 129.391 | 103.703 | +0.078 |
| records_array_1m / sparse | 67.641 | 67.703 | 125.562 | 97.469 | +0.062 |
| records_array_1m / scan | 158.516 | 158.594 | 97.625 | 83.297 | +0.078 |
| records_array_1m / roundtrip | 61.484 | 61.562 | 152.156 | 93.359 | +0.078 |
| records_object_1m / parse | 66.438 | 66.484 | 93.031 | 79.203 | +0.047 |
| records_object_1m / stringify | 63.047 | 63.156 | 129.391 | 103.703 | +0.109 |
| records_array_8m / parse | 108.828 | 108.891 | 265.906 | 140.875 | +0.062 |
| records_array_8m / stringify | 121.141 | 121.219 | 212.562 | 176.609 | +0.078 |
| records_array_8m / sparse | 109.172 | 109.219 | 266.094 | 188.328 | +0.047 |
| records_array_8m / scan | 186.734 | 186.812 | 230.125 | 133.547 | +0.078 |
| records_array_8m / roundtrip | 129.266 | 129.344 | 267.438 | 148.031 | +0.078 |
| records_object_8m / parse | 187.328 | 187.375 | 244.281 | 131.281 | +0.047 |
| records_object_8m / stringify | 121.172 | 121.266 | 212.484 | 196.938 | +0.094 |
| records_array_20m / parse | 255.438 | 255.469 | 359.703 | 220.859 | +0.031 |
| records_array_20m / stringify | 235.219 | 235.266 | 459.516 | 304.922 | +0.047 |
| records_array_20m / sparse | 255.438 | 255.484 | 359.812 | 220.812 | +0.047 |
| records_array_20m / scan | 255.453 | 255.500 | 330.750 | 225.750 | +0.047 |
| records_array_20m / roundtrip | 295.359 | 295.422 | 342.078 | 255.688 | +0.062 |
| records_object_20m / parse | 255.438 | 255.484 | 359.719 | 220.656 | +0.047 |
| records_object_20m / stringify | 235.234 | 235.297 | 459.484 | 304.891 | +0.062 |
| numbers_1m / parse | 62.781 | 62.844 | 104.125 | 68.250 | +0.062 |
| numbers_1m / stringify | 57.844 | 57.922 | 113.094 | 74.297 | +0.078 |
| long_string_1m / parse | 16.438 | 16.484 | 209.297 | 149.453 | +0.047 |
| long_string_1m / stringify | 54.516 | 54.531 | 183.656 | 157.656 | +0.016 |
| escaped_1m / parse | 58.406 | 58.438 | 102.609 | 67.922 | +0.031 |
| escaped_1m / stringify | 54.922 | 54.938 | 124.859 | 74.844 | +0.016 |
| unicode_1m / parse | 15.766 | 15.812 | 203.906 | 158.016 | +0.047 |
| unicode_1m / stringify | 53.859 | 53.938 | 186.344 | 160.672 | +0.078 |
| wide_1m / parse | 227.703 | 227.750 | 121.172 | 95.125 | +0.047 |
| wide_1m / stringify | 68.641 | 68.734 | 118.016 | 84.734 | +0.094 |
| heterogeneous_1m / parse | 62.562 | 62.625 | 92.891 | 99.609 | +0.062 |
| heterogeneous_1m / stringify | 61.719 | 61.797 | 128.875 | 104.938 | +0.078 |

### Eight rotating inputs per fixture

Window: 2026-09-11T16:31:51Z to 2026-09-11T16:33:34Z.

| Fixture / operation | R26 µs | R39 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.493983 | 0.479591 | 0.349583 | 0.255565 | -2.91% | gain |
| small_record / same | 0.098948 | 0.098982 | 0.329544 | 0.246268 | +0.03% | overlap |
| small_record / select | 0.009433 | 0.009432 | 0.002657 | 0.003621 | -0.01% | overlap |
| records_array_1m / rotating | 943.937853 | 927.158192 | 2661.344633 | 2154.045198 | -1.78% | gain |
| records_array_1m / same | 937.870056 | 922.378531 | 2705.180791 | 2155.768362 | -1.65% | gain |
| records_array_1m / select | 0.011382 | 0.011465 | 0.003118 | 0.003872 | +0.73% | overlap |
| long_string_1m / rotating | 99.701901 | 97.878327 | 372.457795 | 69.366540 | -1.83% | gain |
| long_string_1m / same | 0.359612 | 0.357674 | 371.000646 | 65.912439 | -0.54% | overlap |
| long_string_1m / select | 0.011572 | 0.011277 | 0.003095 | 0.003851 | -2.55% | overlap |
| escaped_1m / rotating | 1007.195266 | 950.248521 | 1724.207101 | 2075.680473 | -5.65% | gain |
| escaped_1m / same | 988.834320 | 933.816568 | 1724.325444 | 2073.887574 | -5.56% | gain |
| escaped_1m / select | 0.011301 | 0.011349 | 0.003116 | 0.003851 | +0.42% | overlap |
| unicode_1m / rotating | 141.524912 | 83.170526 | 439.594386 | 63.330526 | -41.23% | gain |
| unicode_1m / same | 0.357864 | 0.354978 | 437.453824 | 59.152958 | -0.81% | overlap |
| unicode_1m / select | 0.011328 | 0.011489 | 0.003119 | 0.003883 | +1.43% | overlap |

| Fixture / operation | R26 MiB | R39 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.875 | 59.891 | 80.156 | -0.109 |
| small_record / same | 79.297 | 79.172 | 59.656 | 79.859 | -0.125 |
| small_record / select | 12.812 | 12.781 | 57.844 | 35.297 | -0.031 |
| records_array_1m / rotating | 76.297 | 76.203 | 128.828 | 100.172 | -0.094 |
| records_array_1m / same | 76.297 | 76.203 | 128.812 | 100.484 | -0.094 |
| records_array_1m / select | 22.594 | 22.547 | 67.609 | 42.500 | -0.047 |
| long_string_1m / rotating | 90.219 | 90.094 | 177.594 | 123.484 | -0.125 |
| long_string_1m / same | 24.594 | 24.531 | 197.891 | 148.641 | -0.062 |
| long_string_1m / select | 24.328 | 24.281 | 69.000 | 43.766 | -0.047 |
| escaped_1m / rotating | 65.000 | 64.906 | 90.344 | 77.766 | -0.094 |
| escaped_1m / same | 65.000 | 64.906 | 90.391 | 77.766 | -0.094 |
| escaped_1m / select | 23.516 | 23.484 | 68.438 | 43.266 | -0.031 |
| unicode_1m / rotating | 62.406 | 62.234 | 160.406 | 149.859 | -0.172 |
| unicode_1m / same | 22.703 | 22.641 | 199.969 | 135.703 | -0.062 |
| unicode_1m / select | 22.438 | 22.391 | 68.609 | 44.656 | -0.047 |

### Independent small-record recheck (11 repetitions)

Window: 2026-09-11T16:34:09Z to 2026-09-11T16:34:36Z.

| Fixture / operation | R26 µs | R39 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494104 | 0.480374 | 0.342022 | 0.255517 | -2.78% | gain |
| small_record / same | 0.099028 | 0.099000 | 0.354679 | 0.245511 | -0.03% | overlap |
| small_record / select | 0.009430 | 0.009441 | 0.002657 | 0.003615 | +0.12% | overlap |

| Fixture / operation | R26 MiB | R39 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.844 | 59.938 | 80.188 | -0.141 |
| small_record / same | 76.781 | 76.656 | 59.625 | 79.875 | -0.125 |
| small_record / select | 12.812 | 12.781 | 57.844 | 35.281 | -0.031 |

## Build fingerprints

| Artifact | R26 SHA-256 | R39 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `284121540807cecb846fb664ab79b1a3e7af7ffac4a3854074741b890961fd72` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `31feb0087ff8e5b134ffa77f7fcc4db82489c7841f9eeb94ec9901662793fbdd` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `b6c88a3060e5ccdaa1fef4443e1c16c69e69ee2e7ab57cdcd9a5f5aa2900683a` |

## Diagnostic follow-up

The linked template capture function has 402 instructions versus 388 in R26 and a 0x4e0 local frame versus 0x500. These totals include cold initialization and do not prove a speedup; actual paired windows supply performance evidence. R39 avoids initializing unused local template slots and updates an existing cached plan in place, while preserving failed-capture behavior and all root barriers. The separate large-emitter stale-pointer fault diagnostic belongs to the R38 reference investigation and was not rerun here. The next independently committed experiment targets ARM native-buffer escaping; no R40 result is attributed to R39.

The attempted wide-object profiles are unqualified. The first long R26 parse loop exceeded the 2 GiB diagnostic RSS cap and was stopped. A retry-preparation syntax error invoked the unchanged controller again; the existing result directory rejected it. Original profile payloads remain intact, but the first window/controller envelopes were overwritten and are available only as prior tool observations. The corrected retry completed all six shorter workers and samplers within the same limits. Its analyzer then refused the first profile because it contained 293 workload samples, below the required 500. No profile attribution is accepted. Raw data, failed results and the archive limitation are preserved. These sampled-process RSS observations are not added to the benchmark RSS comparison.

The artifact index records qualification for every benchmark window and preserves every benchmark terminal receipt, exact CPU/RSS vector, complete output check, validation log, source patch and linked-code diagnostic. The diagnostic envelope loss is described above. Every benchmark window was archived before subsequent remote operations. Process peak RSS does not by itself establish retained live-heap behavior. This is experimental-source evidence; no merged-main claim is made.
