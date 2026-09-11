# Exact integer remainder for JSON access: R32

The integer remainder path improves array access while the full JSON controls show no separated regression. Independent11-repetition rechecks confirm 1 MB random reads −33.56%, field walks −12.86%, sequential reads −23.90%; 16 KB field walks −26.17% and sequential reads −41.92%; 20 MB field walks −8.23%. Other access rows overlap. Full 50 rows, seven stringify-option rows and fifteen rotating-input rows preserve their control behavior. The initial fresh escaped-input +0.20% and selective +1.55% trends overlap; an independent11-repetition recheck gives −0.01% and−1.23%, also overlapping. Across six quiet windows: 3,012 timed trials, 429 complete-output/checksum verification records and 72 calibration trials. Peak-RSS median changes across the first 96 comparisons range −32 KiB to+48 KiB; full vectors and the extra escaped controls are below. These small process-RSS changes do not establish a retained-heap allocation change.

All 38 repeat-input parse/stringify medians remain below both Node and Bun, already true of the R26 reference. This does not mean general JSON parity: fresh small parsing, large fresh Unicode parsing, and several post-parse access rows still trail the peers. The access gains remove index-arithmetic cost; parser and GC code are unchanged.

## Scope and reference

When both dynamic-remainder operands are already numeric doubles, exact nonnegative-u32 dividends and positive-u32 divisors use integer remainder. Cast round trips and a dividend sign check preserve fractions, NaN/infinity and negative zero on the existing floating-point path. Tagged numbers, objects/coercion and BigInt retain their existing paths. No allocation, GC policy, threshold, parse-boundary, parser, serializer or codegen changes. This improves workload index arithmetic such as i % records.length; it is not a new JSON parsing algorithm.

Measured source `c695f6509ed85999894a2f34418f057f32bcb363` against frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71` at workspace 0.5.1531. The harness names R26 main/baseline; it is not current main. Release metadata is prepared separately on top of PR #10064; no metadata or merged-main production measurement is claimed.

## Validation

295 serial release JSON unit tests and 5 actual runtime arithmetic tests pass. The arithmetic test covers u32 boundaries, fractions, negative values/zero, NaN/infinity and int32-tagged permutations; an earlier 8.15M standalone model is separate evidence, not a production benchmark. Both frozen builds pass six fresh remainder fixture executions (native/shadow, normal/scheduled/full GC) against complete Node output, with 72 protected retired sets and 25 moved objects in each scheduled arm. All 81 original candidate JSON/option/GC controls pass; reference receipts are reused only from exact frozen R26. Twenty-six IR files and six worker objects match. Native and unrooted-alloca checks pass for the remainder fixture; its shadow checker retains one identical unsuppressed numeric-array initialization root-store-order finding (moving reachable 0), with exact IR/fingerprint matching. Original native root findings and known lazy/getter/fraction limitations remain unsuppressed. This is not a clean all-checker verdict. Normal all-three-package production build completed in 373.882 seconds from clean c695f650 source. Local lint 73/74 executed gates pass; public benchmark freshness fails; filecap passes; compile-tier and CI-only gates skipped.

## Method and limits

Quiet M1 / 8 GiB host, Node 26.5.1 and Bun 1.3.14, interleaved fresh processes. CPU is user+system per loop iteration. Peak RSS is process-wide, including startup and fixture setup, and is not retained-live-heap size. Each terminal remote window was archived before any subsequent remote operation. Full output/checksum, worker/input hashes, every sample vector and median were verified. Overlapping observed ranges do not establish equality. Repeat-input rows include existing caches/lazy construction; rotating and consuming rows are separate controls.

### Array access

Window: 2026-09-11T12:47:56Z to 2026-09-11T12:48:19Z.

| Fixture / operation | R26 µs | R32 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / repeat | 0.010186 | 0.010174 | 0.002737 | 0.004509 | -0.12% | overlap |
| records_array_16k / random | 0.021848 | 0.021506 | 0.006660 | 0.009295 | -1.57% | gain |
| records_array_16k / fields | 0.056943 | 0.042076 | 0.005322 | 0.009255 | -26.11% | gain |
| records_array_16k / sequential | 0.026543 | 0.015423 | 0.003919 | 0.005731 | -41.89% | gain |
| records_array_1m / repeat | 0.010184 | 0.010173 | 0.002778 | 0.004549 | -0.11% | overlap |
| records_array_1m / random | 0.030415 | 0.020181 | 0.010583 | 0.010738 | -33.65% | gain |
| records_array_1m / fields | 0.053032 | 0.045860 | 0.010822 | 0.014347 | -13.52% | gain |
| records_array_1m / sequential | 0.024476 | 0.018683 | 0.008126 | 0.007475 | -23.67% | gain |
| records_array_20m / repeat | 0.005646 | 0.005639 | 0.003041 | 0.004720 | -0.12% | overlap |
| records_array_20m / random | 0.025836 | 0.025699 | 0.008578 | 0.010024 | -0.53% | overlap |
| records_array_20m / fields | 0.027216 | 0.024936 | 0.011619 | 0.014588 | -8.38% | gain |
| records_array_20m / sequential | 0.015625 | 0.015455 | 0.006811 | 0.008184 | -1.09% | overlap |

| Fixture / operation | R26 MiB | R32 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| records_array_16k / repeat | 13.047 | 13.062 | 57.891 | 34.844 | +0.016 |
| records_array_16k / random | 13.422 | 13.422 | 57.891 | 35.453 | +0.000 |
| records_array_16k / fields | 13.172 | 13.172 | 58.031 | 36.016 | +0.000 |
| records_array_16k / sequential | 13.172 | 13.172 | 57.844 | 35.250 | +0.000 |
| records_array_1m / repeat | 17.562 | 17.562 | 63.750 | 37.953 | +0.000 |
| records_array_1m / random | 18.484 | 18.484 | 66.641 | 38.922 | +0.000 |
| records_array_1m / fields | 18.281 | 18.266 | 66.734 | 40.328 | -0.016 |
| records_array_1m / sequential | 18.281 | 18.281 | 66.641 | 39.219 | +0.000 |
| records_array_20m / repeat | 96.922 | 96.922 | 194.750 | 93.922 | +0.000 |
| records_array_20m / random | 96.922 | 96.938 | 194.844 | 94.781 | +0.016 |
| records_array_20m / fields | 96.938 | 96.938 | 195.047 | 95.359 | +0.000 |
| records_array_20m / sequential | 96.922 | 96.938 | 194.844 | 94.641 | +0.016 |

### Independent access recheck (11 repetitions)

Window: 2026-09-11T12:49:47Z to 2026-09-11T12:50:20Z.

| Fixture / operation | R26 µs | R32 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / repeat | 0.010174 | 0.010187 | 0.002730 | 0.004508 | +0.13% | overlap |
| records_array_16k / random | 0.021837 | 0.021520 | 0.006660 | 0.009272 | -1.45% | overlap |
| records_array_16k / fields | 0.057045 | 0.042116 | 0.005347 | 0.009236 | -26.17% | gain |
| records_array_16k / sequential | 0.026565 | 0.015428 | 0.003908 | 0.005733 | -41.92% | gain |
| records_array_1m / repeat | 0.010178 | 0.010174 | 0.002782 | 0.004554 | -0.04% | overlap |
| records_array_1m / random | 0.030445 | 0.020227 | 0.010576 | 0.010755 | -33.56% | gain |
| records_array_1m / fields | 0.053061 | 0.046239 | 0.010786 | 0.014309 | -12.86% | gain |
| records_array_1m / sequential | 0.024480 | 0.018630 | 0.008193 | 0.007479 | -23.90% | gain |
| records_array_20m / repeat | 0.005639 | 0.005646 | 0.003057 | 0.004769 | +0.12% | overlap |
| records_array_20m / random | 0.025876 | 0.025685 | 0.008622 | 0.010031 | -0.74% | overlap |
| records_array_20m / fields | 0.027154 | 0.024920 | 0.011682 | 0.014581 | -8.23% | gain |
| records_array_20m / sequential | 0.015609 | 0.015456 | 0.006743 | 0.008244 | -0.98% | overlap |

| Fixture / operation | R26 MiB | R32 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| records_array_16k / repeat | 13.047 | 13.047 | 57.875 | 34.828 | +0.000 |
| records_array_16k / random | 13.422 | 13.422 | 57.875 | 35.453 | +0.000 |
| records_array_16k / fields | 13.172 | 13.172 | 58.031 | 36.000 | +0.000 |
| records_array_16k / sequential | 13.172 | 13.172 | 57.812 | 35.266 | +0.000 |
| records_array_1m / repeat | 17.578 | 17.562 | 63.688 | 37.953 | -0.016 |
| records_array_1m / random | 18.469 | 18.484 | 66.641 | 38.938 | +0.016 |
| records_array_1m / fields | 18.281 | 18.281 | 66.781 | 40.328 | +0.000 |
| records_array_1m / sequential | 18.281 | 18.281 | 66.625 | 39.219 | +0.000 |
| records_array_20m / repeat | 96.922 | 96.906 | 194.781 | 93.969 | -0.016 |
| records_array_20m / random | 96.938 | 96.938 | 194.812 | 94.781 | +0.000 |
| records_array_20m / fields | 96.922 | 96.922 | 195.016 | 95.344 | +0.000 |
| records_array_20m / sequential | 96.938 | 96.938 | 194.859 | 94.625 | +0.000 |

### Original 38 parse/stringify plus 12 consumption rows

Window: 2026-09-11T12:50:57Z to 2026-09-11T12:57:47Z.

| Fixture / operation | R26 µs | R32 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| null / parse | 0.010634 | 0.010627 | 0.027348 | 0.019211 | -0.07% | overlap |
| null / stringify | 0.006566 | 0.006567 | 0.027214 | 0.031157 | +0.02% | overlap |
| string_a / parse | 0.012818 | 0.012822 | 0.031748 | 0.022607 | +0.03% | overlap |
| string_a / stringify | 0.009697 | 0.009698 | 0.030052 | 0.032028 | +0.01% | overlap |
| empty_object / parse | 0.023391 | 0.023407 | 0.045187 | 0.024139 | +0.07% | overlap |
| empty_object / stringify | 0.017603 | 0.017607 | 0.031969 | 0.032528 | +0.02% | overlap |
| tiny_object / parse | 0.036782 | 0.036770 | 0.080999 | 0.045181 | -0.03% | overlap |
| tiny_object / stringify | 0.034016 | 0.033967 | 0.037122 | 0.043144 | -0.15% | overlap |
| small_record / parse | 0.098651 | 0.098555 | 0.356590 | 0.246450 | -0.10% | overlap |
| small_record / stringify | 0.045760 | 0.045713 | 0.108102 | 0.119370 | -0.10% | overlap |
| object_1k / parse | 0.082107 | 0.082071 | 0.532303 | 0.227684 | -0.04% | overlap |
| object_1k / stringify | 0.115564 | 0.115416 | 0.193658 | 0.211126 | -0.13% | overlap |
| records_array_16k / parse | 13.597306 | 13.596597 | 40.256026 | 33.819302 | -0.01% | overlap |
| records_array_16k / stringify | 10.971349 | 10.980925 | 13.134373 | 23.211599 | +0.09% | overlap |
| records_array_16k / sparse | 19.974453 | 20.016777 | 41.452211 | 33.942425 | +0.21% | overlap |
| records_array_16k / scan | 59.168838 | 58.783036 | 40.734379 | 35.722148 | -0.65% | overlap |
| records_array_16k / roundtrip | 20.966184 | 20.990338 | 53.807155 | 57.686513 | +0.12% | overlap |
| records_array_1m / parse | 919.935294 | 917.741176 | 2645.405882 | 2139.476471 | -0.24% | overlap |
| records_array_1m / stringify | 648.362445 | 648.393013 | 850.951965 | 966.248908 | +0.00% | overlap |
| records_array_1m / sparse | 967.515528 | 967.248447 | 2730.118012 | 2178.975155 | -0.03% | overlap |
| records_array_1m / scan | 2787.507246 | 2784.768116 | 2822.782609 | 2227.695652 | -0.10% | overlap |
| records_array_1m / roundtrip | 1411.991304 | 1411.730435 | 3531.582609 | 3108.921739 | -0.02% | overlap |
| records_object_1m / parse | 2010.555556 | 2009.604938 | 2808.567901 | 2151.419753 | -0.05% | overlap |
| records_object_1m / stringify | 648.995633 | 647.602620 | 849.432314 | 966.480349 | -0.21% | overlap |
| records_array_8m / parse | 8273.750000 | 8278.687500 | 33379.812500 | 20823.750000 | +0.06% | overlap |
| records_array_8m / stringify | 4724.586207 | 4728.896552 | 7250.586207 | 8160.517241 | +0.09% | overlap |
| records_array_8m / sparse | 8849.466667 | 8841.266667 | 33060.400000 | 21561.000000 | -0.09% | overlap |
| records_array_8m / scan | 21555.625000 | 21572.125000 | 30718.625000 | 21436.375000 | +0.08% | overlap |
| records_array_8m / roundtrip | 13105.000000 | 13111.454545 | 37406.000000 | 27392.454545 | +0.05% | overlap |
| records_object_8m / parse | 15946.700000 | 15937.200000 | 34467.200000 | 20996.200000 | -0.06% | overlap |
| records_object_8m / stringify | 4766.448276 | 4771.379310 | 7241.862069 | 8198.344828 | +0.10% | overlap |
| records_array_20m / parse | 39352.250000 | 39371.250000 | 96473.000000 | 53048.000000 | +0.05% | overlap |
| records_array_20m / stringify | 11744.916667 | 11753.333333 | 17302.750000 | 20078.916667 | +0.07% | overlap |
| records_array_20m / sparse | 39447.000000 | 39374.250000 | 96542.000000 | 53014.750000 | -0.18% | overlap |
| records_array_20m / scan | 40369.750000 | 40397.250000 | 91717.500000 | 53846.000000 | +0.07% | overlap |
| records_array_20m / roundtrip | 98675.500000 | 98569.000000 | 101540.000000 | 76098.500000 | -0.11% | overlap |
| records_object_20m / parse | 39440.000000 | 39480.000000 | 94901.500000 | 53110.250000 | +0.10% | overlap |
| records_object_20m / stringify | 11719.000000 | 11712.833333 | 17300.416667 | 20079.000000 | -0.05% | overlap |
| numbers_1m / parse | 1585.313725 | 1549.872549 | 3126.656863 | 3204.176471 | -2.24% | gain |
| numbers_1m / stringify | 1012.904762 | 1013.394558 | 1971.816327 | 2936.836735 | +0.05% | overlap |
| long_string_1m / parse | 0.349252 | 0.349964 | 364.193158 | 64.550606 | +0.20% | overlap |
| long_string_1m / stringify | 36.389698 | 33.332466 | 103.134235 | 94.819979 | -8.40% | overlap |
| escaped_1m / parse | 980.101266 | 980.265823 | 1761.639241 | 2073.031646 | +0.02% | overlap |
| escaped_1m / stringify | 885.710843 | 885.385542 | 1891.825301 | 2092.301205 | -0.04% | overlap |
| unicode_1m / parse | 0.349605 | 0.348169 | 441.391601 | 58.131730 | -0.41% | overlap |
| unicode_1m / stringify | 28.005657 | 27.871111 | 408.930909 | 447.303030 | -0.48% | overlap |
| wide_1m / parse | 2885.750000 | 2886.031250 | 5252.718750 | 4140.812500 | +0.01% | overlap |
| wide_1m / stringify | 634.409483 | 633.310345 | 6356.732759 | 663.887931 | -0.17% | overlap |
| heterogeneous_1m / parse | 1117.457746 | 1118.802817 | 3862.359155 | 2950.718310 | +0.12% | overlap |
| heterogeneous_1m / stringify | 718.073394 | 717.830275 | 898.105505 | 1054.876147 | -0.03% | overlap |

| Fixture / operation | R26 MiB | R32 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| null / parse | 12.812 | 12.812 | 57.719 | 35.750 | +0.000 |
| null / stringify | 13.016 | 13.016 | 59.656 | 129.828 | +0.000 |
| string_a / parse | 12.812 | 12.812 | 57.719 | 36.141 | +0.000 |
| string_a / stringify | 13.016 | 13.016 | 59.688 | 129.828 | +0.000 |
| empty_object / parse | 32.125 | 32.141 | 59.594 | 69.078 | +0.016 |
| empty_object / stringify | 13.109 | 13.109 | 59.641 | 129.828 | +0.000 |
| tiny_object / parse | 32.297 | 32.281 | 59.641 | 69.109 | -0.016 |
| tiny_object / stringify | 32.328 | 32.328 | 59.688 | 129.797 | +0.000 |
| small_record / parse | 79.547 | 79.578 | 59.688 | 79.891 | +0.031 |
| small_record / stringify | 33.344 | 33.344 | 59.750 | 378.719 | +0.000 |
| object_1k / parse | 64.922 | 64.922 | 61.844 | 71.297 | +0.000 |
| object_1k / stringify | 33.359 | 33.375 | 61.859 | 70.906 | +0.016 |
| records_array_16k / parse | 63.438 | 63.438 | 65.828 | 70.641 | +0.000 |
| records_array_16k / stringify | 33.531 | 33.531 | 61.938 | 71.078 | +0.000 |
| records_array_16k / sparse | 72.234 | 72.234 | 66.047 | 70.641 | +0.000 |
| records_array_16k / scan | 204.469 | 204.453 | 61.984 | 77.422 | -0.016 |
| records_array_16k / roundtrip | 68.766 | 68.766 | 65.953 | 70.922 | +0.000 |
| records_array_1m / parse | 67.109 | 67.109 | 125.719 | 88.719 | +0.000 |
| records_array_1m / stringify | 63.016 | 63.016 | 129.453 | 103.719 | +0.000 |
| records_array_1m / sparse | 67.641 | 67.641 | 125.625 | 97.328 | +0.000 |
| records_array_1m / scan | 158.516 | 158.516 | 97.531 | 83.219 | +0.000 |
| records_array_1m / roundtrip | 61.484 | 61.484 | 152.125 | 93.328 | +0.000 |
| records_object_1m / parse | 66.469 | 66.438 | 92.953 | 79.172 | -0.031 |
| records_object_1m / stringify | 63.047 | 63.047 | 129.422 | 103.703 | +0.000 |
| records_array_8m / parse | 108.828 | 108.828 | 266.031 | 140.719 | +0.000 |
| records_array_8m / stringify | 121.156 | 121.156 | 212.609 | 176.531 | +0.000 |
| records_array_8m / sparse | 109.172 | 109.172 | 266.031 | 187.688 | +0.000 |
| records_array_8m / scan | 186.734 | 186.734 | 230.062 | 133.578 | +0.000 |
| records_array_8m / roundtrip | 129.266 | 129.266 | 267.453 | 148.969 | +0.000 |
| records_object_8m / parse | 187.328 | 187.328 | 244.328 | 131.281 | +0.000 |
| records_object_8m / stringify | 121.188 | 121.172 | 212.719 | 196.547 | -0.016 |
| records_array_20m / parse | 255.422 | 255.438 | 359.844 | 220.703 | +0.016 |
| records_array_20m / stringify | 235.219 | 235.219 | 459.500 | 304.984 | +0.000 |
| records_array_20m / sparse | 255.438 | 255.422 | 360.047 | 220.672 | -0.016 |
| records_array_20m / scan | 255.453 | 255.453 | 330.688 | 225.828 | +0.000 |
| records_array_20m / roundtrip | 295.359 | 295.391 | 343.594 | 255.188 | +0.031 |
| records_object_20m / parse | 255.438 | 255.438 | 359.828 | 220.641 | +0.000 |
| records_object_20m / stringify | 235.234 | 235.250 | 459.547 | 304.938 | +0.016 |
| numbers_1m / parse | 62.766 | 62.781 | 104.203 | 68.234 | +0.016 |
| numbers_1m / stringify | 57.844 | 57.844 | 113.141 | 74.281 | +0.000 |
| long_string_1m / parse | 16.438 | 16.453 | 209.188 | 158.516 | +0.016 |
| long_string_1m / stringify | 54.516 | 54.484 | 183.672 | 155.672 | -0.031 |
| escaped_1m / parse | 58.406 | 58.406 | 102.594 | 67.906 | +0.000 |
| escaped_1m / stringify | 54.891 | 54.922 | 124.797 | 74.797 | +0.031 |
| unicode_1m / parse | 15.766 | 15.766 | 200.156 | 158.906 | +0.000 |
| unicode_1m / stringify | 53.859 | 53.891 | 186.297 | 155.312 | +0.031 |
| wide_1m / parse | 227.703 | 227.719 | 121.188 | 95.078 | +0.016 |
| wide_1m / stringify | 68.641 | 68.672 | 117.984 | 84.656 | +0.031 |
| heterogeneous_1m / parse | 62.562 | 62.562 | 92.844 | 99.719 | +0.000 |
| heterogeneous_1m / stringify | 61.719 | 61.719 | 129.062 | 104.109 | +0.000 |

### Stringify options

Window: 2026-09-11T12:59:16Z to 2026-09-11T13:00:19Z.

| Fixture / operation | R26 µs | R32 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.044887 | 0.044803 | 0.108377 | 0.119642 | -0.19% | overlap |
| small_record / dynamic-zero | 0.044554 | 0.044583 | 0.247932 | 0.394012 | +0.07% | overlap |
| small_record / zero | 0.044840 | 0.044896 | 0.247823 | 0.393695 | +0.12% | overlap |
| small_record / pretty | 0.484978 | 0.483828 | 0.303262 | 0.524884 | -0.24% | overlap |
| small_record / keys | 0.460638 | 0.462699 | 0.592634 | 0.484197 | +0.45% | overlap |
| small_record / callback | 0.946385 | 0.945845 | 0.603835 | 0.582855 | -0.06% | overlap |
| records_array_16k / pretty | 46.255000 | 46.301000 | 33.714000 | 45.891000 | +0.10% | overlap |

| Fixture / operation | R26 MiB | R32 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.391 | 33.438 | 59.641 | 378.562 | +0.047 |
| small_record / dynamic-zero | 33.391 | 33.406 | 59.641 | 378.625 | +0.016 |
| small_record / zero | 33.422 | 33.422 | 59.594 | 378.625 | +0.000 |
| small_record / pretty | 33.750 | 33.766 | 59.672 | 239.359 | +0.016 |
| small_record / keys | 33.719 | 33.734 | 59.750 | 207.781 | +0.016 |
| small_record / callback | 32.500 | 32.500 | 59.625 | 97.656 | +0.000 |
| records_array_16k / pretty | 35.062 | 35.078 | 56.969 | 49.719 | +0.016 |

### Eight rotating inputs per fixture

Window: 2026-09-11T13:00:21Z to 2026-09-11T13:02:03Z.

| Fixture / operation | R26 µs | R32 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494161 | 0.493722 | 0.348758 | 0.255107 | -0.09% | overlap |
| small_record / same | 0.099128 | 0.099104 | 0.344261 | 0.247189 | -0.02% | overlap |
| small_record / select | 0.009434 | 0.009442 | 0.002657 | 0.003610 | +0.08% | overlap |
| records_array_1m / rotating | 943.459770 | 942.442529 | 2798.833333 | 2157.206897 | -0.11% | overlap |
| records_array_1m / same | 939.028736 | 939.091954 | 2669.626437 | 2160.626437 | +0.01% | overlap |
| records_array_1m / select | 0.011385 | 0.011256 | 0.003136 | 0.003883 | -1.13% | overlap |
| long_string_1m / rotating | 100.210906 | 99.982358 | 372.662390 | 70.517241 | -0.23% | overlap |
| long_string_1m / same | 0.362394 | 0.359792 | 374.155172 | 65.922902 | -0.72% | overlap |
| long_string_1m / select | 0.011921 | 0.011751 | 0.003113 | 0.003847 | -1.42% | overlap |
| escaped_1m / rotating | 1011.201258 | 1013.232704 | 1725.188679 | 2077.534591 | +0.20% | overlap |
| escaped_1m / same | 994.568750 | 993.531250 | 1723.500000 | 2074.568750 | -0.10% | overlap |
| escaped_1m / select | 0.011463 | 0.011642 | 0.003124 | 0.003870 | +1.55% | overlap |
| unicode_1m / rotating | 141.891729 | 141.954887 | 440.157143 | 62.319549 | +0.04% | overlap |
| unicode_1m / same | 0.357838 | 0.356757 | 436.837477 | 59.432793 | -0.30% | overlap |
| unicode_1m / select | 0.011466 | 0.011201 | 0.003129 | 0.003868 | -2.32% | overlap |

| Fixture / operation | R26 MiB | R32 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.984 | 59.906 | 80.172 | +0.000 |
| small_record / same | 75.984 | 75.969 | 59.672 | 79.859 | -0.016 |
| small_record / select | 12.812 | 12.812 | 57.859 | 35.312 | +0.000 |
| records_array_1m / rotating | 76.297 | 76.297 | 128.875 | 100.016 | +0.000 |
| records_array_1m / same | 76.297 | 76.297 | 128.922 | 100.453 | +0.000 |
| records_array_1m / select | 22.594 | 22.594 | 67.594 | 42.516 | +0.000 |
| long_string_1m / rotating | 90.234 | 90.203 | 175.609 | 152.484 | -0.031 |
| long_string_1m / same | 24.594 | 24.594 | 201.312 | 154.547 | +0.000 |
| long_string_1m / select | 24.328 | 24.328 | 69.000 | 43.750 | +0.000 |
| escaped_1m / rotating | 65.000 | 65.000 | 89.609 | 77.797 | +0.000 |
| escaped_1m / same | 65.000 | 65.000 | 89.625 | 77.750 | +0.000 |
| escaped_1m / select | 23.531 | 23.531 | 68.453 | 43.266 | +0.000 |
| unicode_1m / rotating | 61.156 | 61.156 | 157.734 | 124.906 | +0.000 |
| unicode_1m / same | 22.703 | 22.703 | 200.297 | 151.719 | +0.000 |
| unicode_1m / select | 22.422 | 22.438 | 68.625 | 44.672 | +0.016 |

### Independent escaped-input recheck (11 repetitions)

Window: 2026-09-11T13:05:16Z to 2026-09-11T13:05:44Z.

| Fixture / operation | R26 µs | R32 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| escaped_1m / rotating | 995.506329 | 995.411392 | 1726.246835 | 2077.797468 | -0.01% | overlap |
| escaped_1m / same | 993.937107 | 993.748428 | 1723.823899 | 2075.761006 | -0.02% | overlap |
| escaped_1m / select | 0.011322 | 0.011183 | 0.003123 | 0.003866 | -1.23% | overlap |

| Fixture / operation | R26 MiB | R32 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| escaped_1m / rotating | 65.000 | 65.000 | 89.656 | 77.766 | +0.000 |
| escaped_1m / same | 65.000 | 65.000 | 89.672 | 77.766 | +0.000 |
| escaped_1m / select | 23.531 | 23.531 | 68.469 | 43.266 | +0.000 |

## Build fingerprints

| Artifact | R26 SHA-256 | R32 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `311b39a2b03507f57902f287353844df99f4a4215a522d9d46d4c63076ade460` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `33bd0b7a70d391c023061bdcbf0a0aaa399f3c944ee6773cbc35549e88fca220` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `f1ffef2c6ab18bb6e78ecf58c3df6b2bd31fd590dbc3627b9f10a2b382d7f4ca` |

The unit controller initially stopped at its explicit hold before any production build began. After local lint review, the clean frozen source completed the normal three-package production build. The reference/candidate remainder fixture exposes one identical unsuppressed shadow root-store-order finding; the initial comparator failure and its exact-fingerprint classification remain in the archive. No global allowlist was changed. Local debugger snapshots used synthetic inputs and are diagnostic only, not measurement results.
