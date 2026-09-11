# Canonical zero-spacing stringify: R25

**Controlled R24 comparison complete; unmerged.** Numeric-zero stringify now uses the existing canonical fast emitters. Repeated small-record zero-spacing CPU falls about 90% (436 ns to 45 ns), and changing the record before every call still improves about 69% (437 ns to 137 ns). One-megabyte ASCII and Unicode zero-spacing CPU falls 63% and 78%, with peak RSS lower by about 1.16 and 1.22 MiB. Retaining 200,000 tiny results saves 10.53 MiB peak RSS; retaining 100,000 small results saves 9.83 MiB.

The full 50-case matrix has overlapping candidate/reference CPU ranges in every case. All 38 repeated-input parse/stringify medians remain below Node and Bun, already true of R24; those cache/lazy-sensitive microbenchmarks do not establish general JSON dominance. Full-matrix peak RSS changes range from -64 KiB to zero. All 36 plain retained-output CPU ranges overlap; peak RSS is 16–48 KiB lower and after-run RSS is unchanged or up to 32 KiB lower.

A small parsing tradeoff remains: rotating small-record parsing is +0.21% with separated ranges in the first run and +0.23% in the independent eleven-repetition recheck, whose ranges overlap (nine of eleven pairs slower). This is about 1.1 ns per parse and is disclosed as a persistent median trend, not claimed to be zero. Other rotating-input cases have overlapping CPU ranges. Plain ASCII-string stringify was +7.46% in the first option control but +1.28% with overlapping ranges in its independent recheck and -0.10% in the full matrix. Plain Unicode stringify is +3.16% with overlapping ranges in the full matrix (four of seven pairs slower), versus +0.72% in the large-option control. These measurements do not establish exact equivalence.

Remaining peer gaps include changing-record plain stringify (1.28x Node), small-record pretty output (1.60x Node), callback replacers (1.57x Node / 1.63x Bun), rotating small-record parsing (1.48x Node / 1.94x Bun), and rotating 1 MB Unicode-string parsing (2.23x Bun). Prior R24 field-access loops also remain substantially slower than Node. Existing lazy-array stringify correctness failures remain separate, explicit limitations.

The reference is exact measured R24, not current main. PR #10050 contains the R24 implementation and remains ready/open at the latest pre-recheck status read; this R25 experiment is not a merged-main measurement. The public-benchmark freshness lint failure and unsuppressed baseline native-root findings remain documented and unwaived. CI is not being waited on before continuing optimization.

## Scope and reference

The canonical primitive/string/record/flat emitters now accept numeric +0, -0 and tagged INT32 zero as the spacer when the replacer is null or undefined. The separate lazy raw-source-copy predicate remains unchanged. Replacers, observable spacer coercions, descriptors and toJSON keep the existing semantic fallbacks. No production GC policy, threshold or parse-boundary change is included.

Candidate source: `01f2878dad8efc92e394c49b88fe0800b871f593`. Reference: measured R24 `9495bfc95e2afcfb5a7cb535e440e61ec0722cb1`. Both use workspace version 0.5.1531 and the same production build command. The harness calls the reference `main`/`baseline`; it is R24, **not current main**. PR #10050 contains the same R24 implementation/test bytes with separate release metadata; this experiment does not claim a merged-main measurement.

The four original benchmark object files and the two added control objects match between arms. Original R24 behavior/options results are reused with exact hashes and provenance. The new zero-spacing fixture and both new controls run against both frozen builds. Native and shadow IR comparisons normalize only the native ModuleID path comment.

## Validation and known limitations

295 serial release JSON runtime tests pass on the final measured source. The initial new cache-hit witness failed after only four cold calls; the established six-field fixture and eight output-verified calls cover cache admission before requiring reuse. The failed run and source are preserved. Formatting was corrected before the final build. Script lint passes 73 of 74 executed checks, retaining the existing public benchmark freshness failure; file-size checking passes. Compile-tier and CI-only lint checks are not represented as passes.

Each arm has 46 original behavior comparisons, 14 original option comparisons, nine zero-spacing semantic/GC comparisons, four changing-record comparisons and eight retained-zero comparisons. Every scheduled subject reports positive moved-object and protected-from-space counters. The new semantic fixture checks getters, own/inherited toJSON, mutations after caching, boxed-spacer coercion, replacers and live retained outputs.

The new zero-spacing fixture and retained-zero worker pass native and shadow root checks. The changing-record worker passes shadow checks but retains four unsuppressed R24 native unrooted/global property-write findings. The original fixture set retains its 16 unsuppressed native findings. Matching IR/fingerprints establish an unchanged baseline, not a clean native safety verdict. The original 18 IR files reuse their previously executed checker verdicts only after exact emitted-IR and checker-source equivalence; the three added subjects run fresh checks in both arms.

The existing lazy-spacer crashes/noncanonical outputs, lazy-getter failures and fractional-spacing difference remain explicit baseline outcomes. They are not fixed or counted as conformance passes. The zero-spacing fast path does not widen the lazy source-copy shortcut.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1 and Bun 1.3.14. Fresh processes and interleaved engine order; fixed workloads and seven repetitions unless a recheck says otherwise. Every terminal window is archived before any subsequent remote operation, including failed attempts. CPU is user+system time per measured loop iteration; RSS is whole-process peak memory. Changing-record loops include the per-iteration mutation. Retained loops include retaining results. One-operation retained CPU measurements are cold and have limited timer resolution; use the larger-count rows for throughput comparisons. Repeated-input parse timings include existing input caches and lazy construction. Consumption loops measure additional access work separately. Overlapping sample ranges do not establish equivalence.

### Original stringify-option cases

Window: 2026-09-11T08:25:53Z to 2026-09-11T08:27:08Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.045296 | 0.044854 | 0.108453 | 0.119914 | -0.97% | gain |
| small_record / dynamic-zero | 0.435984 | 0.044567 | 0.247717 | 0.394491 | -89.78% | gain |
| small_record / zero | 0.436010 | 0.044820 | 0.247494 | 0.394276 | -89.72% | gain |
| small_record / pretty | 0.485877 | 0.484270 | 0.302888 | 0.524905 | -0.33% | overlap |
| small_record / keys | 0.459748 | 0.460551 | 0.597278 | 0.484141 | +0.17% | overlap |
| small_record / callback | 0.945910 | 0.947095 | 0.602905 | 0.582635 | +0.13% | overlap |
| records_array_16k / pretty | 46.124000 | 46.292000 | 33.389000 | 45.856000 | +0.36% | overlap |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.406 | 33.438 | 59.594 | 378.562 | +0.031 |
| small_record / dynamic-zero | 33.781 | 33.438 | 59.625 | 378.625 | -0.344 |
| small_record / zero | 33.766 | 33.422 | 59.609 | 378.625 | -0.344 |
| small_record / pretty | 33.781 | 33.766 | 59.625 | 239.359 | -0.016 |
| small_record / keys | 33.766 | 33.750 | 59.719 | 207.781 | -0.016 |
| small_record / callback | 32.484 | 32.484 | 59.656 | 97.688 | +0.000 |
| records_array_16k / pretty | 35.078 | 35.078 | 57.047 | 49.719 | +0.000 |

### Changing record before every stringify

Window: 2026-09-11T08:27:45Z to 2026-09-11T08:28:12Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / changing-plain | 0.136797 | 0.136790 | 0.107215 | 0.120530 | -0.01% | overlap |
| small_record / changing-zero | 0.437063 | 0.136780 | 0.247764 | 0.394572 | -68.70% | gain |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / changing-plain | 33.672 | 33.688 | 59.859 | 378.500 | +0.016 |
| small_record / changing-zero | 33.969 | 33.688 | 59.734 | 378.578 | -0.281 |

### One-megabyte strings and Unicode

Window: 2026-09-11T08:29:45Z to 2026-09-11T08:30:13Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| long_string_1m / plain | 36.889648 | 39.641602 | 106.894531 | 99.096680 | +7.46% | overlap |
| long_string_1m / dynamic-zero | 106.222656 | 39.350586 | 393.194336 | 532.836914 | -62.95% | gain |
| unicode_1m / plain | 33.609375 | 33.852539 | 415.519531 | 452.879883 | +0.72% | overlap |
| unicode_1m / dynamic-zero | 147.859375 | 32.056641 | 542.926758 | 440.618164 | -78.32% | gain |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| long_string_1m / plain | 54.578 | 54.578 | 154.016 | 155.578 | +0.000 |
| long_string_1m / dynamic-zero | 55.734 | 54.578 | 131.844 | 151.359 | -1.156 |
| unicode_1m / plain | 53.984 | 53.984 | 147.844 | 152.594 | +0.000 |
| unicode_1m / dynamic-zero | 55.172 | 53.953 | 126.156 | 150.797 | -1.219 |

### Independent ASCII plain recheck (11 repetitions)

Window: 2026-09-11T08:33:15Z to 2026-09-11T08:33:20Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| long_string_1m / plain | 38.120117 | 38.607422 | 107.276367 | 99.112305 | +1.28% | overlap |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| long_string_1m / plain | 54.609 | 54.578 | 154.000 | 155.516 | -0.031 |

### Original 38 plus 12 consumption cases

Window: 2026-09-11T08:38:57Z to 2026-09-11T08:45:48Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| null / parse | 0.010622 | 0.010632 | 0.027345 | 0.019169 | +0.09% | overlap |
| null / stringify | 0.006560 | 0.006567 | 0.027203 | 0.031263 | +0.10% | overlap |
| string_a / parse | 0.012828 | 0.012821 | 0.031732 | 0.022610 | -0.05% | overlap |
| string_a / stringify | 0.009692 | 0.009692 | 0.030076 | 0.032051 | +0.01% | overlap |
| empty_object / parse | 0.023440 | 0.023419 | 0.045156 | 0.024112 | -0.09% | overlap |
| empty_object / stringify | 0.017633 | 0.017607 | 0.031966 | 0.032645 | -0.15% | overlap |
| tiny_object / parse | 0.036826 | 0.036762 | 0.081479 | 0.045202 | -0.17% | overlap |
| tiny_object / stringify | 0.033950 | 0.033932 | 0.037170 | 0.043155 | -0.05% | overlap |
| small_record / parse | 0.098576 | 0.098582 | 0.342048 | 0.244202 | +0.01% | overlap |
| small_record / stringify | 0.045914 | 0.045712 | 0.108008 | 0.119419 | -0.44% | overlap |
| object_1k / parse | 0.082004 | 0.082024 | 0.532162 | 0.227457 | +0.02% | overlap |
| object_1k / stringify | 0.115039 | 0.115454 | 0.193485 | 0.211251 | +0.36% | overlap |
| records_array_16k / parse | 13.593850 | 13.588355 | 40.897643 | 33.816466 | -0.04% | overlap |
| records_array_16k / stringify | 10.989652 | 10.997683 | 13.188200 | 23.154915 | +0.07% | overlap |
| records_array_16k / sparse | 19.986909 | 19.972039 | 39.682893 | 33.938485 | -0.07% | overlap |
| records_array_16k / scan | 58.880617 | 58.887264 | 41.383143 | 35.691040 | +0.01% | overlap |
| records_array_16k / roundtrip | 20.991383 | 20.977151 | 53.202637 | 57.690952 | -0.07% | overlap |
| records_array_1m / parse | 918.935294 | 917.694118 | 2674.923529 | 2143.958824 | -0.14% | overlap |
| records_array_1m / stringify | 644.816594 | 645.262009 | 846.340611 | 965.934498 | +0.07% | overlap |
| records_array_1m / sparse | 968.279503 | 966.888199 | 2720.347826 | 2182.124224 | -0.14% | overlap |
| records_array_1m / scan | 2814.376812 | 2816.260870 | 2965.275362 | 2227.260870 | +0.07% | overlap |
| records_array_1m / roundtrip | 1409.982609 | 1411.452174 | 3614.495652 | 3109.330435 | +0.10% | overlap |
| records_object_1m / parse | 2008.469136 | 2010.074074 | 2712.111111 | 2152.135802 | +0.08% | overlap |
| records_object_1m / stringify | 647.672489 | 650.663755 | 850.021834 | 965.755459 | +0.46% | overlap |
| records_array_8m / parse | 8278.312500 | 8287.875000 | 32928.250000 | 20853.375000 | +0.12% | overlap |
| records_array_8m / stringify | 4716.931034 | 4730.793103 | 7262.344828 | 8156.482759 | +0.29% | overlap |
| records_array_8m / sparse | 8856.466667 | 8855.733333 | 32417.200000 | 21521.733333 | -0.01% | overlap |
| records_array_8m / scan | 21825.375000 | 21801.375000 | 29949.625000 | 21397.125000 | -0.11% | overlap |
| records_array_8m / roundtrip | 13136.727273 | 13112.636364 | 36706.545455 | 27358.818182 | -0.18% | overlap |
| records_object_8m / parse | 15925.500000 | 15953.200000 | 34205.400000 | 20897.400000 | +0.17% | overlap |
| records_object_8m / stringify | 4777.103448 | 4769.103448 | 7255.517241 | 8203.793103 | -0.17% | overlap |
| records_array_20m / parse | 39730.500000 | 39753.750000 | 94999.000000 | 53749.750000 | +0.06% | overlap |
| records_array_20m / stringify | 11786.083333 | 11781.583333 | 17299.000000 | 20193.833333 | -0.04% | overlap |
| records_array_20m / sparse | 39742.750000 | 39663.000000 | 94006.500000 | 53582.750000 | -0.20% | overlap |
| records_array_20m / scan | 40806.250000 | 40863.250000 | 89892.500000 | 54291.750000 | +0.14% | overlap |
| records_array_20m / roundtrip | 99246.500000 | 99293.500000 | 103711.500000 | 77098.500000 | +0.05% | overlap |
| records_object_20m / parse | 39730.500000 | 39703.500000 | 96283.500000 | 53623.250000 | -0.07% | overlap |
| records_object_20m / stringify | 11744.666667 | 11738.083333 | 17292.416667 | 20170.666667 | -0.06% | overlap |
| numbers_1m / parse | 1586.264706 | 1582.480392 | 3128.019608 | 3200.019608 | -0.24% | overlap |
| numbers_1m / stringify | 1014.061224 | 1013.727891 | 1975.680272 | 2942.755102 | -0.03% | overlap |
| long_string_1m / parse | 0.349964 | 0.349964 | 364.807199 | 64.888097 | +0.00% | overlap |
| long_string_1m / stringify | 36.137877 | 36.103538 | 102.091051 | 95.181582 | -0.10% | overlap |
| escaped_1m / parse | 980.962025 | 980.550633 | 1763.000000 | 2073.107595 | -0.04% | overlap |
| escaped_1m / stringify | 884.325301 | 884.837349 | 1899.879518 | 2087.397590 | +0.06% | overlap |
| unicode_1m / parse | 0.349605 | 0.349246 | 441.970567 | 58.221106 | -0.10% | overlap |
| unicode_1m / stringify | 27.827475 | 28.706667 | 410.091313 | 447.957172 | +3.16% | overlap |
| wide_1m / parse | 2912.781250 | 2908.265625 | 5250.812500 | 4142.984375 | -0.16% | overlap |
| wide_1m / stringify | 634.241379 | 634.133621 | 6334.362069 | 663.987069 | -0.02% | overlap |
| heterogeneous_1m / parse | 1117.140845 | 1116.746479 | 3852.429577 | 2967.126761 | -0.04% | overlap |
| heterogeneous_1m / stringify | 718.655963 | 717.766055 | 898.160550 | 1059.440367 | -0.12% | overlap |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| null / parse | 12.828 | 12.797 | 57.641 | 35.750 | -0.031 |
| null / stringify | 13.031 | 13.000 | 59.609 | 129.812 | -0.031 |
| string_a / parse | 12.828 | 12.797 | 57.734 | 36.109 | -0.031 |
| string_a / stringify | 13.031 | 13.000 | 59.672 | 129.812 | -0.031 |
| empty_object / parse | 32.156 | 32.125 | 59.547 | 69.062 | -0.031 |
| empty_object / stringify | 13.125 | 13.094 | 59.641 | 129.828 | -0.031 |
| tiny_object / parse | 32.312 | 32.297 | 59.578 | 69.094 | -0.016 |
| tiny_object / stringify | 32.344 | 32.312 | 59.672 | 129.797 | -0.031 |
| small_record / parse | 79.578 | 79.547 | 59.734 | 79.922 | -0.031 |
| small_record / stringify | 33.359 | 33.328 | 59.719 | 378.734 | -0.031 |
| object_1k / parse | 64.969 | 64.938 | 61.844 | 71.266 | -0.031 |
| object_1k / stringify | 33.375 | 33.344 | 61.859 | 70.891 | -0.031 |
| records_array_16k / parse | 63.469 | 63.422 | 65.875 | 70.641 | -0.047 |
| records_array_16k / stringify | 33.547 | 33.516 | 61.922 | 71.047 | -0.031 |
| records_array_16k / sparse | 72.250 | 72.219 | 66.031 | 70.641 | -0.031 |
| records_array_16k / scan | 204.469 | 204.438 | 61.953 | 77.406 | -0.031 |
| records_array_16k / roundtrip | 68.812 | 68.766 | 65.891 | 70.891 | -0.047 |
| records_array_1m / parse | 67.125 | 67.094 | 125.719 | 88.719 | -0.031 |
| records_array_1m / stringify | 63.031 | 63.000 | 129.391 | 103.688 | -0.031 |
| records_array_1m / sparse | 67.656 | 67.625 | 125.656 | 97.453 | -0.031 |
| records_array_1m / scan | 158.531 | 158.500 | 97.578 | 83.188 | -0.031 |
| records_array_1m / roundtrip | 61.500 | 61.469 | 152.125 | 93.328 | -0.031 |
| records_object_1m / parse | 66.453 | 66.422 | 93.031 | 79.188 | -0.031 |
| records_object_1m / stringify | 63.094 | 63.031 | 129.422 | 103.703 | -0.062 |
| records_array_8m / parse | 108.844 | 108.812 | 266.031 | 141.062 | -0.031 |
| records_array_8m / stringify | 121.156 | 121.125 | 212.688 | 196.641 | -0.031 |
| records_array_8m / sparse | 109.188 | 109.156 | 265.969 | 188.109 | -0.031 |
| records_array_8m / scan | 186.750 | 186.719 | 230.094 | 133.641 | -0.031 |
| records_array_8m / roundtrip | 129.281 | 129.250 | 267.438 | 147.781 | -0.031 |
| records_object_8m / parse | 187.344 | 187.312 | 244.281 | 131.625 | -0.031 |
| records_object_8m / stringify | 121.188 | 121.172 | 212.609 | 196.781 | -0.016 |
| records_array_20m / parse | 255.453 | 255.406 | 359.750 | 220.594 | -0.047 |
| records_array_20m / stringify | 235.234 | 235.203 | 459.516 | 304.906 | -0.031 |
| records_array_20m / sparse | 255.438 | 255.422 | 359.969 | 220.797 | -0.016 |
| records_array_20m / scan | 255.469 | 255.438 | 330.828 | 225.891 | -0.031 |
| records_array_20m / roundtrip | 295.375 | 295.344 | 342.125 | 255.969 | -0.031 |
| records_object_20m / parse | 255.453 | 255.422 | 359.641 | 220.828 | -0.031 |
| records_object_20m / stringify | 235.250 | 235.219 | 459.516 | 304.828 | -0.031 |
| numbers_1m / parse | 62.797 | 62.766 | 104.172 | 68.234 | -0.031 |
| numbers_1m / stringify | 57.859 | 57.859 | 113.062 | 74.297 | +0.000 |
| long_string_1m / parse | 16.469 | 16.438 | 209.188 | 164.422 | -0.031 |
| long_string_1m / stringify | 54.500 | 54.500 | 183.641 | 153.656 | +0.000 |
| escaped_1m / parse | 58.422 | 58.391 | 102.625 | 67.891 | -0.031 |
| escaped_1m / stringify | 54.906 | 54.875 | 124.875 | 74.797 | -0.031 |
| unicode_1m / parse | 15.781 | 15.750 | 199.969 | 163.391 | -0.031 |
| unicode_1m / stringify | 53.875 | 53.844 | 186.422 | 152.672 | -0.031 |
| wide_1m / parse | 227.719 | 227.688 | 121.172 | 95.094 | -0.031 |
| wide_1m / stringify | 68.688 | 68.625 | 117.984 | 84.625 | -0.062 |
| heterogeneous_1m / parse | 62.578 | 62.547 | 92.781 | 99.672 | -0.031 |
| heterogeneous_1m / stringify | 61.734 | 61.703 | 128.969 | 104.062 | -0.031 |

### Eight rotating inputs per fixture

Window: 2026-09-11T08:47:33Z to 2026-09-11T08:49:16Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.493817 | 0.494836 | 0.335190 | 0.255234 | +0.21% | regression |
| small_record / same | 0.099047 | 0.099088 | 0.343890 | 0.245732 | +0.04% | overlap |
| small_record / select | 0.009430 | 0.009437 | 0.002658 | 0.003611 | +0.07% | overlap |
| records_array_1m / rotating | 942.293103 | 942.178161 | 2676.511494 | 2158.028736 | -0.01% | overlap |
| records_array_1m / same | 940.017143 | 937.862857 | 2625.685714 | 2150.805714 | -0.23% | overlap |
| records_array_1m / select | 0.011454 | 0.011632 | 0.003145 | 0.003842 | +1.56% | overlap |
| long_string_1m / rotating | 100.083794 | 99.967589 | 373.133597 | 70.525692 | -0.12% | overlap |
| long_string_1m / same | 0.361580 | 0.360943 | 377.057025 | 66.031220 | -0.18% | overlap |
| long_string_1m / select | 0.011910 | 0.011288 | 0.003089 | 0.003863 | -5.22% | overlap |
| escaped_1m / rotating | 1011.415094 | 1011.861635 | 1725.993711 | 2078.144654 | +0.04% | overlap |
| escaped_1m / same | 993.893750 | 993.006250 | 1723.193750 | 2073.712500 | -0.09% | overlap |
| escaped_1m / select | 0.011746 | 0.011022 | 0.003126 | 0.003867 | -6.17% | overlap |
| unicode_1m / rotating | 141.788476 | 141.954048 | 439.762217 | 63.787746 | +0.12% | overlap |
| unicode_1m / same | 0.356497 | 0.357582 | 440.522620 | 59.723489 | +0.30% | overlap |
| unicode_1m / select | 0.011917 | 0.011314 | 0.003141 | 0.003887 | -5.07% | overlap |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.969 | 59.891 | 80.172 | -0.016 |
| small_record / same | 75.984 | 75.953 | 59.562 | 79.875 | -0.031 |
| small_record / select | 12.812 | 12.797 | 57.859 | 35.281 | -0.016 |
| records_array_1m / rotating | 76.297 | 76.281 | 128.828 | 100.453 | -0.016 |
| records_array_1m / same | 76.297 | 76.281 | 128.844 | 100.141 | -0.016 |
| records_array_1m / select | 22.594 | 22.578 | 67.625 | 42.531 | -0.016 |
| long_string_1m / rotating | 90.234 | 90.219 | 176.562 | 148.453 | -0.016 |
| long_string_1m / same | 24.609 | 24.578 | 204.641 | 148.578 | -0.031 |
| long_string_1m / select | 24.328 | 24.312 | 69.031 | 43.781 | -0.016 |
| escaped_1m / rotating | 65.000 | 64.984 | 89.656 | 77.750 | -0.016 |
| escaped_1m / same | 65.000 | 64.984 | 89.641 | 77.766 | -0.016 |
| escaped_1m / select | 23.531 | 23.516 | 68.453 | 43.266 | -0.016 |
| unicode_1m / rotating | 61.156 | 61.141 | 158.531 | 155.266 | -0.016 |
| unicode_1m / same | 22.703 | 22.688 | 208.750 | 153.531 | -0.016 |
| unicode_1m / select | 22.422 | 22.422 | 68.562 | 44.641 | +0.000 |

### Independent small-record rotating recheck (11 repetitions)

Window: 2026-09-11T08:50:13Z to 2026-09-11T08:50:41Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.493474 | 0.494615 | 0.343240 | 0.254888 | +0.23% | overlap |
| small_record / same | 0.098940 | 0.099067 | 0.341858 | 0.245664 | +0.13% | overlap |
| small_record / select | 0.009430 | 0.009433 | 0.002658 | 0.003641 | +0.04% | overlap |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.969 | 59.922 | 80.188 | -0.016 |
| small_record / same | 78.438 | 78.422 | 59.656 | 79.859 | -0.016 |
| small_record / select | 12.812 | 12.797 | 57.844 | 35.281 | -0.016 |

### Retained zero-spacing outputs

Window: 2026-09-11T08:32:27Z to 2026-09-11T08:32:37Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| tiny_object / retain-stringify / 1 live | 16.000000 | 10.000000 | 357.000000 | 11.000000 | -37.50% | overlap |
| tiny_object / retain-stringify / 200000 live | 0.253885 | 0.046980 | 0.112850 | 0.135745 | -81.50% | gain |
| small_record / retain-stringify / 1 live | 18.000000 | 12.000000 | 366.000000 | 13.000000 | -33.33% | gain |
| small_record / retain-stringify / 100000 live | 0.461890 | 0.067160 | 0.324940 | 0.408080 | -85.46% | gain |
| long_string_1m / retain-stringify / 1 live | 272.000000 | 135.000000 | 1236.000000 | 649.000000 | -50.37% | gain |
| long_string_1m / retain-stringify / 32 live | 246.812500 | 169.937500 | 485.687500 | 605.312500 | -31.15% | gain |
| unicode_1m / retain-stringify / 1 live | 284.000000 | 117.000000 | 1391.000000 | 558.000000 | -58.80% | gain |
| unicode_1m / retain-stringify / 32 live | 189.562500 | 69.375000 | 626.062500 | 483.500000 | -63.40% | gain |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / retain-stringify / 1 live | 13.094 | 13.047 | 53.609 | 28.719 | -0.047 |
| tiny_object / retain-stringify / 200000 live | 36.047 | 25.516 | 72.156 | 47.484 | -10.531 |
| small_record / retain-stringify / 1 live | 13.109 | 13.062 | 53.641 | 28.734 | -0.047 |
| small_record / retain-stringify / 100000 live | 38.531 | 28.703 | 81.219 | 51.516 | -9.828 |
| long_string_1m / retain-stringify / 1 live | 19.891 | 18.734 | 61.656 | 34.062 | -1.156 |
| long_string_1m / retain-stringify / 32 live | 52.906 | 51.781 | 93.359 | 65.172 | -1.125 |
| unicode_1m / retain-stringify / 1 live | 18.547 | 17.594 | 61.594 | 34.469 | -0.953 |
| unicode_1m / retain-stringify / 32 live | 43.922 | 42.969 | 89.844 | 62.172 | -0.953 |

| Fixture / operation | R24 after MiB | R25 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / retain-stringify / 1 live | 12.000 | 11.969 | 52.844 | 28.312 | -0.031 |
| tiny_object / retain-stringify / 200000 live | 35.547 | 24.453 | 71.453 | 47.078 | -11.094 |
| small_record / retain-stringify / 1 live | 12.016 | 11.984 | 52.906 | 28.328 | -0.031 |
| small_record / retain-stringify / 100000 live | 30.781 | 27.641 | 80.484 | 51.109 | -3.141 |
| long_string_1m / retain-stringify / 1 live | 18.812 | 17.672 | 60.766 | 33.656 | -1.141 |
| long_string_1m / retain-stringify / 32 live | 52.406 | 51.266 | 92.469 | 64.766 | -1.141 |
| unicode_1m / retain-stringify / 1 live | 17.469 | 16.531 | 60.938 | 34.062 | -0.938 |
| unicode_1m / retain-stringify / 32 live | 42.844 | 41.906 | 89.172 | 61.766 | -0.938 |

### Retained plain outputs

Window: 2026-09-11T08:46:18Z to 2026-09-11T08:47:18Z.

| Fixture / operation | R24 µs | R25 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| tiny_object / retain-parse / 1 live | 31.000000 | 32.000000 | 381.000000 | 9.000000 | +3.23% | overlap |
| tiny_object / retain-parse / 200000 live | 0.044305 | 0.044485 | 0.132875 | 0.058755 | +0.41% | overlap |
| tiny_object / retain-stringify / 1 live | 11.000000 | 11.000000 | 365.000000 | 7.000000 | +0.00% | overlap |
| tiny_object / retain-stringify / 200000 live | 0.047155 | 0.047100 | 0.083655 | 0.049555 | -0.12% | overlap |
| small_record / retain-parse / 1 live | 33.000000 | 33.000000 | 385.000000 | 13.000000 | +0.00% | overlap |
| small_record / retain-parse / 100000 live | 0.104260 | 0.104090 | 0.481750 | 0.263780 | -0.16% | overlap |
| small_record / retain-stringify / 1 live | 12.000000 | 13.000000 | 365.000000 | 9.000000 | +8.33% | overlap |
| small_record / retain-stringify / 100000 live | 0.067730 | 0.067160 | 0.186570 | 0.136190 | -0.84% | overlap |
| records_array_1m / retain-parse / 1 live | 944.000000 | 949.000000 | 3845.000000 | 2412.000000 | +0.53% | overlap |
| records_array_1m / retain-parse / 16 live | 855.062500 | 857.000000 | 3617.750000 | 2135.250000 | +0.23% | overlap |
| records_array_1m / retain-stringify / 1 live | 804.000000 | 805.000000 | 1340.000000 | 1077.000000 | +0.12% | overlap |
| records_array_1m / retain-stringify / 16 live | 656.875000 | 661.937500 | 1000.500000 | 992.875000 | +0.77% | overlap |
| records_object_1m / retain-parse / 1 live | 1867.000000 | 1868.000000 | 3982.000000 | 2412.000000 | +0.05% | overlap |
| records_object_1m / retain-parse / 16 live | 2976.750000 | 2987.062500 | 3603.375000 | 2133.562500 | +0.35% | overlap |
| records_object_1m / retain-stringify / 1 live | 806.000000 | 806.000000 | 1339.000000 | 1072.000000 | +0.00% | overlap |
| records_object_1m / retain-stringify / 16 live | 664.750000 | 661.187500 | 1000.937500 | 992.625000 | -0.54% | overlap |
| records_array_8m / retain-parse / 1 live | 8286.000000 | 8270.000000 | 28265.000000 | 18730.000000 | -0.19% | overlap |
| records_array_8m / retain-parse / 4 live | 8269.250000 | 8277.250000 | 27079.250000 | 21805.000000 | +0.10% | overlap |
| records_array_8m / retain-stringify / 1 live | 29807.000000 | 29710.000000 | 7737.000000 | 8221.000000 | -0.33% | overlap |
| records_array_8m / retain-stringify / 4 live | 16214.750000 | 16205.000000 | 8903.500000 | 9397.250000 | -0.06% | overlap |
| records_object_8m / retain-parse / 1 live | 14484.000000 | 14440.000000 | 28698.000000 | 18557.000000 | -0.30% | overlap |
| records_object_8m / retain-parse / 4 live | 22370.250000 | 22363.250000 | 27837.750000 | 21774.500000 | -0.03% | overlap |
| records_object_8m / retain-stringify / 1 live | 30019.000000 | 30023.000000 | 7884.000000 | 8269.000000 | +0.01% | overlap |
| records_object_8m / retain-stringify / 4 live | 16335.000000 | 16374.000000 | 9046.000000 | 9482.250000 | +0.24% | overlap |
| long_string_1m / retain-parse / 1 live | 166.000000 | 163.000000 | 817.000000 | 129.000000 | -1.81% | overlap |
| long_string_1m / retain-parse / 32 live | 5.625000 | 5.593750 | 466.156250 | 140.031250 | -0.56% | overlap |
| long_string_1m / retain-stringify / 1 live | 133.000000 | 135.000000 | 936.000000 | 152.000000 | +1.50% | overlap |
| long_string_1m / retain-stringify / 32 live | 171.906250 | 169.906250 | 193.750000 | 171.906250 | -1.16% | overlap |
| unicode_1m / retain-parse / 1 live | 197.000000 | 199.000000 | 1235.000000 | 117.000000 | +1.02% | overlap |
| unicode_1m / retain-parse / 32 live | 6.781250 | 6.687500 | 528.437500 | 103.375000 | -1.38% | overlap |
| unicode_1m / retain-stringify / 1 live | 117.000000 | 118.000000 | 1229.000000 | 583.000000 | +0.85% | overlap |
| unicode_1m / retain-stringify / 32 live | 67.250000 | 65.968750 | 498.625000 | 494.875000 | -1.91% | overlap |
| wide_1m / retain-parse / 1 live | 2608.000000 | 2626.000000 | 7525.000000 | 5724.000000 | +0.69% | overlap |
| wide_1m / retain-parse / 16 live | 3691.625000 | 3691.687500 | 5576.937500 | 4304.500000 | +0.00% | overlap |
| wide_1m / retain-stringify / 1 live | 2505.000000 | 2502.000000 | 7627.000000 | 782.000000 | -0.12% | overlap |
| wide_1m / retain-stringify / 16 live | 751.625000 | 752.062500 | 6483.125000 | 704.812500 | +0.06% | overlap |

| Fixture / operation | R24 MiB | R25 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / retain-parse / 1 live | 13.000 | 12.969 | 53.609 | 28.719 | -0.031 |
| tiny_object / retain-parse / 200000 live | 25.469 | 25.422 | 75.609 | 50.641 | -0.047 |
| tiny_object / retain-stringify / 1 live | 13.094 | 13.062 | 53.688 | 28.750 | -0.031 |
| tiny_object / retain-stringify / 200000 live | 25.562 | 25.531 | 72.094 | 47.516 | -0.031 |
| small_record / retain-parse / 1 live | 12.984 | 12.953 | 53.625 | 28.703 | -0.031 |
| small_record / retain-parse / 100000 live | 25.484 | 25.469 | 86.312 | 53.125 | -0.016 |
| small_record / retain-stringify / 1 live | 13.109 | 13.078 | 53.703 | 28.766 | -0.031 |
| small_record / retain-stringify / 100000 live | 28.734 | 28.703 | 81.172 | 51.531 | -0.031 |
| records_array_1m / retain-parse / 1 live | 17.625 | 17.594 | 60.359 | 32.062 | -0.031 |
| records_array_1m / retain-parse / 16 live | 41.516 | 41.484 | 89.531 | 56.375 | -0.031 |
| records_array_1m / retain-stringify / 1 live | 20.188 | 20.156 | 63.000 | 34.859 | -0.031 |
| records_array_1m / retain-stringify / 16 live | 32.859 | 32.812 | 77.094 | 47.672 | -0.047 |
| records_object_1m / retain-parse / 1 live | 16.422 | 16.406 | 60.328 | 32.078 | -0.016 |
| records_object_1m / retain-parse / 16 live | 53.016 | 52.984 | 89.750 | 56.344 | -0.031 |
| records_object_1m / retain-stringify / 1 live | 20.203 | 20.156 | 63.031 | 34.875 | -0.047 |
| records_object_1m / retain-stringify / 16 live | 32.875 | 32.844 | 77.141 | 47.672 | -0.031 |
| records_array_8m / retain-parse / 1 live | 49.594 | 49.562 | 97.766 | 52.750 | -0.031 |
| records_array_8m / retain-parse / 4 live | 85.844 | 85.812 | 159.375 | 92.266 | -0.031 |
| records_array_8m / retain-stringify / 1 live | 79.922 | 79.891 | 118.406 | 73.891 | -0.031 |
| records_array_8m / retain-stringify / 4 live | 99.172 | 99.141 | 144.797 | 94.906 | -0.031 |
| records_object_8m / retain-parse / 1 live | 45.484 | 45.453 | 97.797 | 52.750 | -0.031 |
| records_object_8m / retain-parse / 4 live | 94.203 | 94.172 | 159.344 | 92.281 | -0.031 |
| records_object_8m / retain-stringify / 1 live | 79.938 | 79.906 | 118.375 | 73.906 | -0.031 |
| records_object_8m / retain-stringify / 4 live | 99.156 | 99.141 | 144.672 | 94.906 | -0.016 |
| long_string_1m / retain-parse / 1 live | 16.359 | 16.328 | 57.719 | 30.984 | -0.031 |
| long_string_1m / retain-parse / 32 live | 16.391 | 16.344 | 90.219 | 62.047 | -0.047 |
| long_string_1m / retain-stringify / 1 live | 18.766 | 18.734 | 61.703 | 33.047 | -0.031 |
| long_string_1m / retain-stringify / 32 live | 51.766 | 51.719 | 93.359 | 64.203 | -0.047 |
| unicode_1m / retain-parse / 1 live | 15.688 | 15.672 | 58.594 | 31.688 | -0.016 |
| unicode_1m / retain-parse / 32 live | 15.703 | 15.672 | 87.125 | 59.406 | -0.031 |
| unicode_1m / retain-stringify / 1 live | 17.625 | 17.594 | 61.672 | 34.500 | -0.031 |
| unicode_1m / retain-stringify / 32 live | 43.000 | 42.969 | 89.969 | 62.312 | -0.031 |
| wide_1m / retain-parse / 1 live | 21.469 | 21.438 | 66.531 | 36.734 | -0.031 |
| wide_1m / retain-parse / 16 live | 77.672 | 77.641 | 113.281 | 69.312 | -0.031 |
| wide_1m / retain-stringify / 1 live | 23.906 | 23.859 | 69.469 | 39.672 | -0.047 |
| wide_1m / retain-stringify / 16 live | 37.062 | 37.031 | 87.328 | 53.906 | -0.031 |

| Fixture / operation | R24 after MiB | R25 after MiB | Node after MiB | Bun after MiB | After-RSS change MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / retain-parse / 1 live | 11.922 | 11.906 | 52.859 | 28.328 | -0.016 |
| tiny_object / retain-parse / 200000 live | 24.422 | 24.391 | 74.906 | 50.250 | -0.031 |
| tiny_object / retain-stringify / 1 live | 12.016 | 12.000 | 52.938 | 28.344 | -0.016 |
| tiny_object / retain-stringify / 200000 live | 24.516 | 24.500 | 71.375 | 47.109 | -0.016 |
| small_record / retain-parse / 1 live | 11.906 | 11.891 | 52.875 | 28.328 | -0.016 |
| small_record / retain-parse / 100000 live | 24.422 | 24.422 | 85.562 | 52.734 | +0.000 |
| small_record / retain-stringify / 1 live | 12.031 | 12.016 | 52.984 | 28.359 | -0.016 |
| small_record / retain-stringify / 100000 live | 27.688 | 27.672 | 80.453 | 51.125 | -0.016 |
| records_array_1m / retain-parse / 1 live | 16.594 | 16.578 | 58.547 | 31.672 | -0.016 |
| records_array_1m / retain-parse / 16 live | 40.484 | 40.469 | 88.766 | 55.984 | -0.016 |
| records_array_1m / retain-stringify / 1 live | 19.156 | 19.141 | 60.688 | 34.469 | -0.016 |
| records_array_1m / retain-stringify / 16 live | 31.828 | 31.797 | 76.453 | 47.281 | -0.031 |
| records_object_1m / retain-parse / 1 live | 15.375 | 15.375 | 58.516 | 31.688 | +0.000 |
| records_object_1m / retain-parse / 16 live | 52.500 | 52.484 | 89.016 | 55.953 | -0.016 |
| records_object_1m / retain-stringify / 1 live | 19.172 | 19.141 | 60.703 | 34.484 | -0.031 |
| records_object_1m / retain-stringify / 16 live | 31.844 | 31.828 | 76.484 | 47.281 | -0.016 |
| records_array_8m / retain-parse / 1 live | 48.562 | 48.547 | 94.438 | 52.359 | -0.016 |
| records_array_8m / retain-parse / 4 live | 85.344 | 85.328 | 147.125 | 91.875 | -0.016 |
| records_array_8m / retain-stringify / 1 live | 77.344 | 77.328 | 114.328 | 73.500 | -0.016 |
| records_array_8m / retain-stringify / 4 live | 98.656 | 98.641 | 144.109 | 94.516 | -0.016 |
| records_object_8m / retain-parse / 1 live | 33.188 | 33.172 | 94.438 | 52.359 | -0.016 |
| records_object_8m / retain-parse / 4 live | 90.953 | 90.938 | 147.031 | 91.891 | -0.016 |
| records_object_8m / retain-stringify / 1 live | 77.359 | 77.344 | 114.312 | 73.516 | -0.016 |
| records_object_8m / retain-stringify / 4 live | 98.641 | 98.641 | 144.062 | 94.516 | +0.000 |
| long_string_1m / retain-parse / 1 live | 15.312 | 15.297 | 56.656 | 30.594 | -0.016 |
| long_string_1m / retain-parse / 32 live | 15.344 | 15.312 | 89.297 | 61.656 | -0.031 |
| long_string_1m / retain-stringify / 1 live | 17.719 | 17.703 | 60.750 | 32.656 | -0.016 |
| long_string_1m / retain-stringify / 32 live | 51.234 | 51.203 | 92.422 | 63.797 | -0.031 |
| unicode_1m / retain-parse / 1 live | 14.641 | 14.641 | 57.844 | 31.312 | +0.000 |
| unicode_1m / retain-parse / 32 live | 14.656 | 14.641 | 86.469 | 59.016 | -0.016 |
| unicode_1m / retain-stringify / 1 live | 16.578 | 16.562 | 60.984 | 34.094 | -0.016 |
| unicode_1m / retain-stringify / 32 live | 41.953 | 41.938 | 89.328 | 61.906 | -0.016 |
| wide_1m / retain-parse / 1 live | 20.672 | 20.656 | 65.797 | 36.344 | -0.016 |
| wide_1m / retain-parse / 16 live | 77.156 | 77.141 | 112.656 | 68.922 | -0.016 |
| wide_1m / retain-stringify / 1 live | 22.859 | 22.828 | 68.766 | 39.281 | -0.031 |
| wide_1m / retain-stringify / 16 live | 36.016 | 36.000 | 86.656 | 53.516 | -0.016 |

## Build fingerprints

| Artifact | R24 SHA-256 | R25 SHA-256 |
|---|---|---|
| perry | `c8e83e7dc6680c54044b09c6493522bb0375a93d61730c3c68ebd9cc851b7b79` | `fde1808df892dbba645ea4e66b85971c412940d515062abcc86ec07a1c70ac79` |
| libperry_runtime.a | `3e925cb1a91bf474203c69b33ae2189481e46e0bccebd4b2cdcf35c1a963bbbc` | `2207fdcb4a53fa81e2a77722dfeac2650ddbeb76bd02f20ee47260b88c2427d4` |
| libperry_stdlib.a | `143309e7959802db73b86be87702d83fcf3e917b16f377f8c256c7c72d1398e8` | `a3d51fd8bab32984585d55149cda0d5a4212f5e03944c1241f3cafaee6bec80c` |

The artifact index records the exact archived measurements, validation receipts, source patches and failed attempts. Frozen binaries are identified above; benchmark source paths and build/link commands are preserved in their provenance records.
