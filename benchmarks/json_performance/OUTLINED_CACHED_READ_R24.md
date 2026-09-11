# Outlined cached-array reads: R24

**Unmerged; performance comparison complete against pinned main.** R24 retains roughly 45% gains for repeated cached reads and 31–42% for the 1 MB random/fields/sequential loops. Full 1 MB/8 MB scans improve about 4%. It removes the eleven separated regressions seen in R23’s full matrix and the five independently reproduced regressions, by outlining the cached accessor and restoring the shared array dispatcher’s 648-instruction/mnemonic sequence.

There is a small remaining stringify tradeoff: small-record stringify is +0.31% in the full matrix and +0.35% in an eleven-repetition recheck (the latter ranges overlap, eight of eleven pairs slower). In the separate options harness, plain small-record stringify is +1.28% in both runs, with separated ranges and all eleven recheck pairs slower: about 0.57 ns per call. These are disclosed costs, not claims of zero regressions. Long-string/Unicode median slowdowns in the full run did not repeat independently.

All 38 repeated-input parse/stringify medians still beat Node and Bun, already true of main. Changing-input controls have no separated regression. Retained-output peak RSS differs by at most 48 KiB and after-run RSS by at most 32 KiB; no retained-output CPU case has a separated regression. The early 16 KB roundtrip screen had a +0.484 MiB peak-RSS difference; that did not repeat in the full matrix (+0.031 MiB). Read-loop gains do not establish general JSON dominance: 1 MB field access remains about 5.8× Node, and zero-spacing/pretty/callback stringify paths retain larger gaps.

At the time of measurement there is no R24 PR or merge. Qualification against the latest main and the required CI gate remain before ordinary protected landing. Local lint retains the existing public benchmark freshness failure; the native fixture set retains sixteen unsuppressed main findings. Neither gate is waived. Previously accepted work is merged through PR #10037; R22/R23 remain separate unmerged experiments.

Earlier accepted JSON changes are merged through PR #10037, the merge train for closed #10036. This experiment is on `codex/json-outlined-cached-read-r24`, source `9495bfc95e2afcfb5a7cb535e440e61ec0722cb1`. The controlled reference is independently built main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). It is a pinned reference, not a claim to have measured the latest main.

## Change and controls

R24 changes only R23’s `cached_read::lazy_get` annotation from `inline` to `inline(never)`. The sparse-cache and materialized dense-slot hits still bypass root construction; misses, holes and descriptors retain the original rooted fallback. There is no production GC core, policy, threshold, parse-boundary or cache-admission change.

Main’s exact frozen build, linked workers, behavior/options checks, moving-GC receipts and native/shadow IR are reused unchanged from R23. Candidate compilation reads the same immutable R23 TypeScript paths. `main-reuse-provenance.json` records the copied file hashes and original evidence commit; copied main checks are not presented as newly executed checks.

## Validation

- 294 serial release JSON runtime tests passed. The exact production build emitted all three artifacts after the recorded start, and frozen copies were SHA-256 verified.
- Candidate behavior, options, actual moving/protected GC, object-code and native/shadow comparisons are recorded below. Existing failures remain unsuppressed.
- Local script lint passed 73 of 74 executed checks; public benchmark freshness is the existing failure. The separate file-size gate passed. The compile tier and two CI-only checks were skipped; this is not a full CI pass.

main: 46 behavior and 14 option comparisons pass. Reused R23 reference results.

- Cached reads / auto: 2,608 protected retired sets, 103,251 moved objects.
- Cached reads / tape: 2,608 protected retired sets, 103,251 moved objects.
- Cached reads / direct: 2,588 protected retired sets, 103,263 moved objects.

candidate: 46 behavior and 14 option comparisons pass. New R24 executions.

- Cached reads / auto: 2,608 protected retired sets, 103,251 moved objects.
- Cached reads / tape: 2,608 protected retired sets, 103,251 moved objects.
- Cached reads / direct: 2,588 protected retired sets, 103,263 moved objects.

All four benchmark object files match main byte-for-byte. All 18 IR files match after removing only the first native ModuleID path comment; shadow IR is byte-identical. Shadow and ordinary-worker/callback native checks pass. The full native check retains the same 16 unsuppressed main findings (15 unrooted, one stale); this is not a clean full-native verdict.

The existing 24-case lazy-spacer matrix retains six SIGSEGV outcomes and two noncanonical outputs. The lazy-getter baseline still fails in auto/tape and passes in direct mode; the recorded fractional-spacing difference also remains. Preserving these outcomes is not a conformance pass.

## Measurements

Quiet M1/8 GiB host, Node 26.5.1 and Bun 1.3.14. Timed trials use fresh processes with interleaved engine order. Each terminal window was archived before any subsequent remote operation. Analyzers verify checksums/output hashes, every CPU/RSS sample and median, inputs, frozen worker hashes and the exact source patch.

CPU is user + system time per loop iteration. Access excludes parsing; the fields loop contains three indexed reads. Parse/stringify and consumption rows include different work and must not be substituted for access latency. RSS is whole-process peak RSS, not live heap size. Separated sample ranges indicate a gain/regression in that window; overlapping ranges do not prove equivalence.

### Five prior regression cases, eleven repetitions

Window: 2026-09-11T06:47:30Z to 2026-09-11T06:48:24Z; 220 timed trials.

| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| tiny_object / parse | 0.036821 | 0.036765 | 0.081364 | 0.045271 | -0.15% | overlap |
| records_array_16k / roundtrip | 20.973234 | 20.990991 | 53.182661 | 57.726596 | +0.08% | overlap |
| records_array_1m / roundtrip | 1412.130435 | 1412.469565 | 3586.895652 | 3110.165217 | +0.02% | overlap |
| records_object_1m / parse | 2009.604938 | 2008.617284 | 2769.629630 | 2153.135802 | -0.05% | overlap |
| heterogeneous_1m / stringify | 718.059633 | 718.403670 | 897.211009 | 1056.233945 | +0.05% | overlap |

| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / parse | 32.281 | 32.297 | 59.578 | 69.078 | +0.016 |
| records_array_16k / roundtrip | 68.766 | 69.250 | 65.906 | 70.906 | +0.484 |
| records_array_1m / roundtrip | 61.484 | 61.500 | 152.125 | 93.328 | +0.016 |
| records_object_1m / parse | 66.438 | 66.453 | 92.922 | 79.172 | +0.016 |
| heterogeneous_1m / stringify | 61.719 | 61.734 | 128.922 | 104.984 | +0.016 |

### Access after parsing, seven repetitions

Window: 2026-09-11T06:49:07Z to 2026-09-11T06:49:31Z; 336 timed trials.

| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / repeat | 0.018587 | 0.010190 | 0.002746 | 0.004512 | -45.18% | overlap |
| records_array_16k / random | 0.040288 | 0.023790 | 0.006671 | 0.009283 | -40.95% | gain |
| records_array_16k / fields | 0.084485 | 0.056987 | 0.005386 | 0.009239 | -32.55% | gain |
| records_array_16k / sequential | 0.035094 | 0.026567 | 0.003949 | 0.005775 | -24.30% | gain |
| records_array_1m / repeat | 0.018553 | 0.010178 | 0.002756 | 0.004570 | -45.14% | gain |
| records_array_1m / random | 0.048810 | 0.033647 | 0.010568 | 0.010757 | -31.07% | gain |
| records_array_1m / fields | 0.107937 | 0.062694 | 0.010899 | 0.014278 | -41.92% | gain |
| records_array_1m / sequential | 0.040443 | 0.026506 | 0.008256 | 0.007480 | -34.46% | gain |
| records_array_20m / repeat | 0.005636 | 0.005638 | 0.003051 | 0.004766 | +0.04% | overlap |
| records_array_20m / random | 0.025856 | 0.025679 | 0.008600 | 0.010042 | -0.68% | overlap |
| records_array_20m / fields | 0.027184 | 0.027152 | 0.011672 | 0.014512 | -0.12% | overlap |
| records_array_20m / sequential | 0.015640 | 0.015585 | 0.006848 | 0.008241 | -0.35% | overlap |

| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |
|---|---:|---:|---:|---:|---:|
| records_array_16k / repeat | 13.031 | 13.047 | 57.797 | 34.797 | +0.016 |
| records_array_16k / random | 13.406 | 13.422 | 57.875 | 35.406 | +0.016 |
| records_array_16k / fields | 13.156 | 13.172 | 58.047 | 35.969 | +0.016 |
| records_array_16k / sequential | 13.172 | 13.172 | 57.891 | 35.234 | +0.000 |
| records_array_1m / repeat | 17.562 | 17.578 | 63.734 | 37.922 | +0.016 |
| records_array_1m / random | 18.469 | 18.469 | 66.594 | 38.906 | +0.000 |
| records_array_1m / fields | 18.250 | 18.266 | 66.781 | 40.297 | +0.016 |
| records_array_1m / sequential | 18.266 | 18.266 | 66.641 | 39.188 | +0.000 |
| records_array_20m / repeat | 96.906 | 96.906 | 194.750 | 93.922 | +0.000 |
| records_array_20m / random | 96.906 | 96.922 | 194.797 | 94.719 | +0.016 |
| records_array_20m / fields | 96.906 | 96.922 | 195.047 | 95.312 | +0.016 |
| records_array_20m / sequential | 96.906 | 96.922 | 194.891 | 94.609 | +0.016 |

### Original 38 plus 12 consumption rows, seven repetitions

Window: 2026-09-11T06:50:41Z to 2026-09-11T06:57:31Z; 1,400 timed trials.

| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| null / parse | 0.010636 | 0.010631 | 0.027345 | 0.019197 | -0.05% | overlap |
| null / stringify | 0.006561 | 0.006567 | 0.027204 | 0.031177 | +0.08% | overlap |
| string_a / parse | 0.012825 | 0.012824 | 0.031725 | 0.022585 | -0.00% | overlap |
| string_a / stringify | 0.009688 | 0.009692 | 0.030052 | 0.032045 | +0.04% | overlap |
| empty_object / parse | 0.023430 | 0.023390 | 0.045201 | 0.024158 | -0.17% | overlap |
| empty_object / stringify | 0.017592 | 0.017587 | 0.031967 | 0.032578 | -0.03% | overlap |
| tiny_object / parse | 0.036756 | 0.036770 | 0.081780 | 0.045246 | +0.04% | overlap |
| tiny_object / stringify | 0.033980 | 0.033993 | 0.037152 | 0.043187 | +0.04% | overlap |
| small_record / parse | 0.098532 | 0.098595 | 0.356610 | 0.244421 | +0.06% | overlap |
| small_record / stringify | 0.045684 | 0.045828 | 0.108089 | 0.119368 | +0.31% | regression |
| object_1k / parse | 0.081949 | 0.082009 | 0.531802 | 0.227438 | +0.07% | overlap |
| object_1k / stringify | 0.115219 | 0.115229 | 0.193583 | 0.211108 | +0.01% | overlap |
| records_array_16k / parse | 13.603066 | 13.590925 | 40.454183 | 33.849787 | -0.09% | overlap |
| records_array_16k / stringify | 11.056993 | 11.041393 | 13.181945 | 23.179319 | -0.14% | overlap |
| records_array_16k / sparse | 20.043340 | 20.052618 | 40.387392 | 33.940900 | +0.05% | overlap |
| records_array_16k / scan | 58.725605 | 58.972880 | 41.232385 | 35.761765 | +0.42% | overlap |
| records_array_16k / roundtrip | 20.964356 | 20.993602 | 53.963311 | 57.643818 | +0.14% | overlap |
| records_array_1m / parse | 917.864706 | 918.005882 | 2733.111765 | 2140.658824 | +0.02% | overlap |
| records_array_1m / stringify | 648.301310 | 649.240175 | 846.205240 | 966.537118 | +0.14% | overlap |
| records_array_1m / sparse | 967.757764 | 967.850932 | 2796.062112 | 2182.279503 | +0.01% | overlap |
| records_array_1m / scan | 2934.420290 | 2811.956522 | 2811.666667 | 2225.884058 | -4.17% | gain |
| records_array_1m / roundtrip | 1411.782609 | 1412.756522 | 3538.200000 | 3112.113043 | +0.07% | overlap |
| records_object_1m / parse | 2010.691358 | 2009.296296 | 2886.135802 | 2151.049383 | -0.07% | overlap |
| records_object_1m / stringify | 648.292576 | 647.524017 | 847.903930 | 967.087336 | -0.12% | overlap |
| records_array_8m / parse | 8282.062500 | 8275.875000 | 33573.250000 | 20858.687500 | -0.07% | overlap |
| records_array_8m / stringify | 4734.793103 | 4731.241379 | 7270.310345 | 8165.551724 | -0.08% | overlap |
| records_array_8m / sparse | 8860.066667 | 8859.400000 | 33261.866667 | 21489.466667 | -0.01% | overlap |
| records_array_8m / scan | 22676.875000 | 21750.500000 | 30769.375000 | 21441.375000 | -4.09% | gain |
| records_array_8m / roundtrip | 13087.090909 | 13109.727273 | 36467.454545 | 27332.909091 | +0.17% | overlap |
| records_object_8m / parse | 15949.900000 | 15936.600000 | 33411.100000 | 20850.600000 | -0.08% | overlap |
| records_object_8m / stringify | 4773.724138 | 4770.965517 | 7261.793103 | 8183.068966 | -0.06% | overlap |
| records_array_20m / parse | 39735.500000 | 39719.250000 | 94895.250000 | 53570.500000 | -0.04% | overlap |
| records_array_20m / stringify | 11785.000000 | 11782.916667 | 17289.250000 | 20156.666667 | -0.02% | overlap |
| records_array_20m / sparse | 39777.750000 | 39716.500000 | 94845.500000 | 53648.750000 | -0.15% | overlap |
| records_array_20m / scan | 40860.500000 | 40812.750000 | 91309.750000 | 53803.750000 | -0.12% | overlap |
| records_array_20m / roundtrip | 99171.000000 | 99239.500000 | 104738.500000 | 77161.500000 | +0.07% | overlap |
| records_object_20m / parse | 39845.750000 | 39679.750000 | 96058.000000 | 53752.250000 | -0.42% | overlap |
| records_object_20m / stringify | 11743.333333 | 11730.166667 | 17317.750000 | 20182.250000 | -0.11% | overlap |
| numbers_1m / parse | 1552.794118 | 1552.382353 | 3128.960784 | 3195.745098 | -0.03% | overlap |
| numbers_1m / stringify | 1012.891156 | 1014.605442 | 1975.326531 | 2944.544218 | +0.17% | overlap |
| long_string_1m / parse | 0.349608 | 0.349608 | 364.439416 | 64.414825 | +0.00% | overlap |
| long_string_1m / stringify | 34.444329 | 35.284079 | 102.040062 | 94.974506 | +2.44% | overlap |
| escaped_1m / parse | 980.443038 | 980.120253 | 1763.917722 | 2072.873418 | -0.03% | overlap |
| escaped_1m / stringify | 886.265060 | 886.204819 | 1896.789157 | 2084.759036 | -0.01% | overlap |
| unicode_1m / parse | 0.349246 | 0.349964 | 441.707107 | 57.916726 | +0.21% | overlap |
| unicode_1m / stringify | 27.576162 | 28.980202 | 409.116768 | 447.833535 | +5.09% | overlap |
| wide_1m / parse | 2899.812500 | 2902.234375 | 5259.203125 | 4131.656250 | +0.08% | overlap |
| wide_1m / stringify | 633.659483 | 634.254310 | 6300.806034 | 663.857759 | +0.09% | overlap |
| heterogeneous_1m / parse | 1118.852113 | 1118.577465 | 3851.014085 | 2951.964789 | -0.02% | overlap |
| heterogeneous_1m / stringify | 718.045872 | 718.839450 | 898.545872 | 1055.766055 | +0.11% | overlap |

| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |
|---|---:|---:|---:|---:|---:|
| null / parse | 12.812 | 12.828 | 57.719 | 35.766 | +0.016 |
| null / stringify | 13.016 | 13.031 | 59.656 | 129.797 | +0.016 |
| string_a / parse | 12.812 | 12.828 | 57.688 | 36.125 | +0.016 |
| string_a / stringify | 13.016 | 13.031 | 59.625 | 129.797 | +0.016 |
| empty_object / parse | 32.141 | 32.156 | 59.578 | 69.094 | +0.016 |
| empty_object / stringify | 13.109 | 13.125 | 59.625 | 129.812 | +0.016 |
| tiny_object / parse | 32.281 | 32.297 | 59.609 | 69.078 | +0.016 |
| tiny_object / stringify | 32.328 | 32.344 | 59.719 | 129.828 | +0.016 |
| small_record / parse | 79.578 | 79.594 | 59.703 | 79.906 | +0.016 |
| small_record / stringify | 33.344 | 33.359 | 59.781 | 378.703 | +0.016 |
| object_1k / parse | 64.953 | 64.969 | 61.844 | 71.281 | +0.016 |
| object_1k / stringify | 33.359 | 33.375 | 61.828 | 70.875 | +0.016 |
| records_array_16k / parse | 63.438 | 63.453 | 65.875 | 70.641 | +0.016 |
| records_array_16k / stringify | 33.531 | 33.562 | 61.891 | 71.062 | +0.031 |
| records_array_16k / sparse | 72.219 | 72.250 | 66.031 | 70.641 | +0.031 |
| records_array_16k / scan | 204.453 | 204.484 | 61.953 | 77.391 | +0.031 |
| records_array_16k / roundtrip | 68.766 | 68.797 | 65.859 | 70.891 | +0.031 |
| records_array_1m / parse | 67.109 | 67.125 | 125.641 | 88.719 | +0.016 |
| records_array_1m / stringify | 63.016 | 63.031 | 129.406 | 103.703 | +0.016 |
| records_array_1m / sparse | 67.641 | 67.656 | 125.578 | 97.312 | +0.016 |
| records_array_1m / scan | 158.500 | 158.531 | 97.609 | 83.188 | +0.031 |
| records_array_1m / roundtrip | 61.484 | 61.500 | 152.141 | 93.328 | +0.016 |
| records_object_1m / parse | 66.438 | 66.453 | 93.062 | 79.172 | +0.016 |
| records_object_1m / stringify | 63.078 | 63.062 | 129.484 | 103.672 | -0.016 |
| records_array_8m / parse | 108.828 | 108.844 | 266.109 | 141.375 | +0.016 |
| records_array_8m / stringify | 121.172 | 121.156 | 212.609 | 176.656 | -0.016 |
| records_array_8m / sparse | 109.156 | 109.188 | 265.953 | 187.188 | +0.031 |
| records_array_8m / scan | 186.719 | 186.750 | 230.062 | 133.641 | +0.031 |
| records_array_8m / roundtrip | 129.266 | 129.281 | 267.422 | 148.531 | +0.016 |
| records_object_8m / parse | 187.328 | 187.344 | 244.406 | 131.562 | +0.016 |
| records_object_8m / stringify | 121.188 | 121.188 | 212.578 | 184.641 | +0.000 |
| records_array_20m / parse | 255.422 | 255.453 | 359.922 | 220.812 | +0.031 |
| records_array_20m / stringify | 235.219 | 235.234 | 459.531 | 304.938 | +0.016 |
| records_array_20m / sparse | 255.438 | 255.453 | 359.984 | 220.516 | +0.016 |
| records_array_20m / scan | 255.438 | 255.469 | 329.891 | 223.062 | +0.031 |
| records_array_20m / roundtrip | 295.391 | 295.375 | 342.078 | 255.875 | -0.016 |
| records_object_20m / parse | 255.438 | 255.453 | 359.672 | 220.547 | +0.016 |
| records_object_20m / stringify | 235.266 | 235.250 | 459.516 | 304.906 | -0.016 |
| numbers_1m / parse | 62.781 | 62.797 | 104.109 | 68.234 | +0.016 |
| numbers_1m / stringify | 57.844 | 57.859 | 113.109 | 74.281 | +0.016 |
| long_string_1m / parse | 16.453 | 16.469 | 209.141 | 156.469 | +0.016 |
| long_string_1m / stringify | 54.484 | 54.500 | 183.688 | 153.672 | +0.016 |
| escaped_1m / parse | 58.406 | 58.422 | 102.609 | 67.891 | +0.016 |
| escaped_1m / stringify | 54.891 | 54.906 | 124.797 | 74.812 | +0.016 |
| unicode_1m / parse | 15.781 | 15.781 | 200.078 | 154.422 | +0.000 |
| unicode_1m / stringify | 53.859 | 53.875 | 186.312 | 155.406 | +0.016 |
| wide_1m / parse | 227.703 | 227.719 | 121.219 | 95.531 | +0.016 |
| wide_1m / stringify | 68.641 | 68.656 | 117.953 | 84.703 | +0.016 |
| heterogeneous_1m / parse | 62.562 | 62.578 | 92.734 | 99.703 | +0.016 |
| heterogeneous_1m / stringify | 61.719 | 61.734 | 128.969 | 104.969 | +0.016 |

### Three stringify rechecks, eleven repetitions

Window: 2026-09-11T06:59:02Z to 2026-09-11T06:59:43Z; 132 timed trials.

| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / stringify | 0.045940 | 0.046101 | 0.107947 | 0.119642 | +0.35% | overlap |
| long_string_1m / stringify | 35.555671 | 35.350676 | 102.616545 | 95.361602 | -0.58% | overlap |
| unicode_1m / stringify | 29.786263 | 29.273535 | 409.867879 | 448.500606 | -1.72% | overlap |

| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |
|---|---:|---:|---:|---:|---:|
| small_record / stringify | 33.344 | 33.359 | 59.750 | 378.688 | +0.016 |
| long_string_1m / stringify | 54.516 | 54.531 | 183.688 | 149.625 | +0.016 |
| unicode_1m / stringify | 53.859 | 53.906 | 186.328 | 155.359 | +0.047 |

### Stringify options, seven repetitions

Window: 2026-09-11T07:01:05Z to 2026-09-11T07:02:31Z; 196 timed trials.

| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.044751 | 0.045325 | 0.108258 | 0.119995 | +1.28% | regression |
| small_record / dynamic-zero | 0.437008 | 0.435921 | 0.247484 | 0.394535 | -0.25% | gain |
| small_record / zero | 0.436333 | 0.435492 | 0.247593 | 0.394516 | -0.19% | gain |
| small_record / pretty | 0.483536 | 0.484747 | 0.303246 | 0.525423 | +0.25% | overlap |
| small_record / keys | 0.459334 | 0.459757 | 0.602040 | 0.484607 | +0.09% | overlap |
| small_record / callback | 0.946145 | 0.947145 | 0.607265 | 0.580780 | +0.11% | overlap |
| records_array_16k / pretty | 46.079000 | 46.156000 | 33.463000 | 45.878000 | +0.17% | overlap |

| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.406 | 33.406 | 59.641 | 378.562 | +0.000 |
| small_record / dynamic-zero | 33.781 | 33.781 | 59.641 | 378.641 | +0.000 |
| small_record / zero | 33.797 | 33.812 | 59.594 | 378.625 | +0.016 |
| small_record / pretty | 33.781 | 33.766 | 59.625 | 239.359 | -0.016 |
| small_record / keys | 33.766 | 33.734 | 59.703 | 207.781 | -0.031 |
| small_record / callback | 32.484 | 32.484 | 59.625 | 97.688 | +0.000 |
| records_array_16k / pretty | 35.078 | 35.078 | 57.016 | 49.734 | +0.000 |

### Plain stringify in the options harness, eleven-repetition recheck

Window: 2026-09-11T07:10:57Z to 2026-09-11T07:11:05Z; 44 timed trials.

| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.044727 | 0.045298 | 0.108296 | 0.119927 | +1.28% | regression |

| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.406 | 33.406 | 59.578 | 378.578 | +0.000 |

### Eight-input rotation, same-input and selection controls

Window: 2026-09-11T07:04:40Z to 2026-09-11T07:06:22Z; 420 timed trials.

| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.493849 | 0.493668 | 0.339775 | 0.255965 | -0.04% | overlap |
| small_record / same | 0.099091 | 0.099141 | 0.343579 | 0.245228 | +0.05% | overlap |
| small_record / select | 0.009430 | 0.009439 | 0.002661 | 0.003641 | +0.11% | overlap |
| records_array_1m / rotating | 942.195402 | 943.172414 | 2657.459770 | 2152.258621 | +0.10% | overlap |
| records_array_1m / same | 938.793103 | 938.913793 | 2699.396552 | 2151.632184 | +0.01% | overlap |
| records_array_1m / select | 0.011326 | 0.011381 | 0.003108 | 0.003858 | +0.49% | overlap |
| long_string_1m / rotating | 99.848954 | 99.611929 | 372.292796 | 70.239349 | -0.24% | overlap |
| long_string_1m / same | 0.360536 | 0.359260 | 376.361811 | 65.379981 | -0.35% | overlap |
| long_string_1m / select | 0.012025 | 0.011583 | 0.003099 | 0.003879 | -3.68% | overlap |
| escaped_1m / rotating | 1012.207547 | 1012.125786 | 1726.515723 | 2077.905660 | -0.01% | overlap |
| escaped_1m / same | 992.874214 | 994.377358 | 1723.805031 | 2074.188679 | +0.15% | overlap |
| escaped_1m / select | 0.011258 | 0.011431 | 0.003118 | 0.003868 | +1.53% | overlap |
| unicode_1m / rotating | 141.592620 | 142.064207 | 439.923247 | 62.931365 | +0.33% | overlap |
| unicode_1m / same | 0.357554 | 0.357914 | 437.068705 | 59.702158 | +0.10% | overlap |
| unicode_1m / select | 0.011404 | 0.011420 | 0.003109 | 0.003877 | +0.14% | overlap |

| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 32.000 | 31.984 | 59.938 | 80.156 | -0.016 |
| small_record / same | 75.984 | 75.969 | 59.703 | 79.875 | -0.016 |
| small_record / select | 12.828 | 12.812 | 57.891 | 35.266 | -0.016 |
| records_array_1m / rotating | 76.312 | 76.297 | 128.812 | 99.938 | -0.016 |
| records_array_1m / same | 76.312 | 76.297 | 128.875 | 98.297 | -0.016 |
| records_array_1m / select | 22.609 | 22.594 | 67.531 | 42.500 | -0.016 |
| long_string_1m / rotating | 90.234 | 90.250 | 177.656 | 145.438 | +0.016 |
| long_string_1m / same | 24.625 | 24.609 | 208.094 | 130.469 | -0.016 |
| long_string_1m / select | 24.344 | 24.328 | 69.000 | 43.766 | -0.016 |
| escaped_1m / rotating | 65.016 | 65.000 | 89.672 | 77.750 | -0.016 |
| escaped_1m / same | 65.016 | 65.000 | 89.656 | 77.734 | -0.016 |
| escaped_1m / select | 23.547 | 23.531 | 68.453 | 43.250 | -0.016 |
| unicode_1m / rotating | 61.141 | 61.125 | 158.547 | 147.234 | -0.016 |
| unicode_1m / same | 22.719 | 22.703 | 202.719 | 153.500 | -0.016 |
| unicode_1m / select | 22.453 | 22.422 | 68.578 | 44.656 | -0.031 |

### Live retained-output comparisons

Window: 2026-09-11T07:09:07Z to 2026-09-11T07:10:07Z; 1,008 timed trials.

| Fixture / operation | Main µs | R24 µs | Node µs | Bun µs | R24 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| tiny_object / retain-parse / 1 retained | 32.000000 | 32.000000 | 381.000000 | 9.000000 | +0.00% | overlap |
| tiny_object / retain-parse / 200000 retained | 0.044320 | 0.044360 | 0.132465 | 0.058435 | +0.09% | overlap |
| tiny_object / retain-stringify / 1 retained | 11.000000 | 11.000000 | 363.000000 | 8.000000 | +0.00% | overlap |
| tiny_object / retain-stringify / 200000 retained | 0.047030 | 0.047365 | 0.083575 | 0.049520 | +0.71% | overlap |
| small_record / retain-parse / 1 retained | 32.000000 | 33.000000 | 400.000000 | 13.000000 | +3.12% | overlap |
| small_record / retain-parse / 100000 retained | 0.104080 | 0.104100 | 0.467070 | 0.263370 | +0.02% | overlap |
| small_record / retain-stringify / 1 retained | 13.000000 | 13.000000 | 369.000000 | 9.000000 | +0.00% | overlap |
| small_record / retain-stringify / 100000 retained | 0.067390 | 0.067640 | 0.183640 | 0.136130 | +0.37% | overlap |
| records_array_1m / retain-parse / 1 retained | 953.000000 | 933.000000 | 3885.000000 | 2417.000000 | -2.10% | overlap |
| records_array_1m / retain-parse / 16 retained | 852.625000 | 857.312500 | 3678.312500 | 2138.187500 | +0.55% | overlap |
| records_array_1m / retain-stringify / 1 retained | 801.000000 | 808.000000 | 1337.000000 | 1070.000000 | +0.87% | overlap |
| records_array_1m / retain-stringify / 16 retained | 663.937500 | 659.000000 | 996.812500 | 991.937500 | -0.74% | overlap |
| records_object_1m / retain-parse / 1 retained | 1861.000000 | 1867.000000 | 3956.000000 | 2413.000000 | +0.32% | overlap |
| records_object_1m / retain-parse / 16 retained | 2989.187500 | 2983.500000 | 3662.875000 | 2131.875000 | -0.19% | overlap |
| records_object_1m / retain-stringify / 1 retained | 803.000000 | 811.000000 | 1347.000000 | 1077.000000 | +1.00% | overlap |
| records_object_1m / retain-stringify / 16 retained | 663.500000 | 664.375000 | 1002.562500 | 993.125000 | +0.13% | overlap |
| records_array_8m / retain-parse / 1 retained | 8256.000000 | 8330.000000 | 27782.000000 | 18719.000000 | +0.90% | overlap |
| records_array_8m / retain-parse / 4 retained | 8244.500000 | 8231.750000 | 27927.500000 | 21659.750000 | -0.15% | overlap |
| records_array_8m / retain-stringify / 1 retained | 29760.000000 | 29840.000000 | 7807.000000 | 8250.000000 | +0.27% | overlap |
| records_array_8m / retain-stringify / 4 retained | 16176.500000 | 16222.000000 | 8795.500000 | 9391.250000 | +0.28% | overlap |
| records_object_8m / retain-parse / 1 retained | 14462.000000 | 14419.000000 | 27939.000000 | 18590.000000 | -0.30% | overlap |
| records_object_8m / retain-parse / 4 retained | 22268.250000 | 22259.500000 | 26976.750000 | 21756.250000 | -0.04% | overlap |
| records_object_8m / retain-stringify / 1 retained | 29792.000000 | 29791.000000 | 7712.000000 | 8271.000000 | -0.00% | overlap |
| records_object_8m / retain-stringify / 4 retained | 16187.250000 | 16182.250000 | 8895.750000 | 9402.500000 | -0.03% | overlap |
| long_string_1m / retain-parse / 1 retained | 172.000000 | 165.000000 | 815.000000 | 132.000000 | -4.07% | overlap |
| long_string_1m / retain-parse / 32 retained | 5.562500 | 5.593750 | 465.593750 | 140.156250 | +0.56% | overlap |
| long_string_1m / retain-stringify / 1 retained | 133.000000 | 133.000000 | 933.000000 | 154.000000 | +0.00% | overlap |
| long_string_1m / retain-stringify / 32 retained | 170.687500 | 168.812500 | 195.375000 | 168.218750 | -1.10% | overlap |
| unicode_1m / retain-parse / 1 retained | 196.000000 | 199.000000 | 1235.000000 | 118.000000 | +1.53% | overlap |
| unicode_1m / retain-parse / 32 retained | 6.656250 | 6.656250 | 528.093750 | 103.843750 | +0.00% | overlap |
| unicode_1m / retain-stringify / 1 retained | 119.000000 | 116.000000 | 1221.000000 | 577.000000 | -2.52% | overlap |
| unicode_1m / retain-stringify / 32 retained | 66.968750 | 69.031250 | 498.093750 | 497.406250 | +3.08% | overlap |
| wide_1m / retain-parse / 1 retained | 2614.000000 | 2596.000000 | 7525.000000 | 5710.000000 | -0.69% | overlap |
| wide_1m / retain-parse / 16 retained | 3662.812500 | 3662.437500 | 5557.250000 | 4296.625000 | -0.01% | overlap |
| wide_1m / retain-stringify / 1 retained | 2503.000000 | 2497.000000 | 7604.000000 | 775.000000 | -0.24% | overlap |
| wide_1m / retain-stringify / 16 retained | 754.437500 | 750.125000 | 6465.437500 | 701.875000 | -0.57% | overlap |

| Fixture / operation | Main MiB | R24 MiB | Node MiB | Bun MiB | R24 − main MiB |
|---|---:|---:|---:|---:|---:|
| tiny_object / retain-parse / 1 retained | 12.984 | 13.000 | 53.641 | 28.703 | +0.016 |
| tiny_object / retain-parse / 200000 retained | 25.453 | 25.453 | 75.734 | 50.641 | +0.000 |
| tiny_object / retain-stringify / 1 retained | 13.078 | 13.094 | 53.734 | 28.750 | +0.016 |
| tiny_object / retain-stringify / 200000 retained | 25.547 | 25.547 | 72.109 | 47.500 | +0.000 |
| small_record / retain-parse / 1 retained | 12.969 | 12.984 | 53.625 | 28.703 | +0.016 |
| small_record / retain-parse / 100000 retained | 25.484 | 25.500 | 86.359 | 53.141 | +0.016 |
| small_record / retain-stringify / 1 retained | 13.094 | 13.109 | 53.703 | 28.781 | +0.016 |
| small_record / retain-stringify / 100000 retained | 28.719 | 28.734 | 81.172 | 51.516 | +0.016 |
| records_array_1m / retain-parse / 1 retained | 17.609 | 17.641 | 60.328 | 32.078 | +0.031 |
| records_array_1m / retain-parse / 16 retained | 41.500 | 41.516 | 89.609 | 56.344 | +0.016 |
| records_array_1m / retain-stringify / 1 retained | 20.156 | 20.188 | 63.000 | 34.859 | +0.031 |
| records_array_1m / retain-stringify / 16 retained | 32.844 | 32.859 | 77.188 | 47.688 | +0.016 |
| records_object_1m / retain-parse / 1 retained | 16.422 | 16.422 | 60.359 | 32.047 | +0.000 |
| records_object_1m / retain-parse / 16 retained | 53.000 | 53.016 | 89.547 | 56.359 | +0.016 |
| records_object_1m / retain-stringify / 1 retained | 20.188 | 20.203 | 63.000 | 34.859 | +0.016 |
| records_object_1m / retain-stringify / 16 retained | 32.859 | 32.875 | 77.141 | 47.672 | +0.016 |
| records_array_8m / retain-parse / 1 retained | 49.578 | 49.594 | 97.781 | 52.766 | +0.016 |
| records_array_8m / retain-parse / 4 retained | 85.828 | 85.844 | 159.281 | 92.266 | +0.016 |
| records_array_8m / retain-stringify / 1 retained | 79.906 | 79.953 | 118.359 | 73.906 | +0.047 |
| records_array_8m / retain-stringify / 4 retained | 99.156 | 99.141 | 144.406 | 94.922 | -0.016 |
| records_object_8m / retain-parse / 1 retained | 45.469 | 45.484 | 97.812 | 52.750 | +0.016 |
| records_object_8m / retain-parse / 4 retained | 94.188 | 94.203 | 159.312 | 92.250 | +0.016 |
| records_object_8m / retain-stringify / 1 retained | 79.938 | 79.938 | 118.375 | 73.906 | +0.000 |
| records_object_8m / retain-stringify / 4 retained | 99.141 | 99.156 | 144.656 | 94.922 | +0.016 |
| long_string_1m / retain-parse / 1 retained | 16.344 | 16.359 | 57.766 | 30.969 | +0.016 |
| long_string_1m / retain-parse / 32 retained | 16.375 | 16.375 | 90.203 | 62.078 | +0.000 |
| long_string_1m / retain-stringify / 1 retained | 18.750 | 18.750 | 61.688 | 33.062 | +0.000 |
| long_string_1m / retain-stringify / 32 retained | 51.719 | 51.734 | 93.359 | 64.172 | +0.016 |
| unicode_1m / retain-parse / 1 retained | 15.688 | 15.703 | 58.578 | 31.672 | +0.016 |
| unicode_1m / retain-parse / 32 retained | 15.688 | 15.703 | 87.078 | 59.391 | +0.016 |
| unicode_1m / retain-stringify / 1 retained | 17.594 | 17.609 | 61.672 | 34.500 | +0.016 |
| unicode_1m / retain-stringify / 32 retained | 42.984 | 43.000 | 89.922 | 62.234 | +0.016 |
| wide_1m / retain-parse / 1 retained | 21.453 | 21.469 | 66.594 | 36.734 | +0.016 |
| wide_1m / retain-parse / 16 retained | 77.656 | 77.672 | 113.250 | 69.312 | +0.016 |
| wide_1m / retain-stringify / 1 retained | 23.875 | 23.906 | 69.484 | 39.688 | +0.031 |
| wide_1m / retain-stringify / 16 retained | 37.047 | 37.062 | 87.297 | 53.906 | +0.016 |

## Sampled remaining cost

Four bounded one-second profiles follow a 0.5-second settling delay, with at least 500 workload samples each. These are instrumented diagnostics, separate from uninstrumented timings. Inclusive phases overlap and must not be added; zero named property-helper samples does not mean zero property-read cost, because those reads are inlined in the generated workload.

| Fixture / arm | Workload samples | Root helpers | Materialized resolver | Numeric index dispatch | fmod |
|---|---:|---:|---:|---:|---:|
| records_array_16k / main | 731 | 27.91% | 0.82% | 9.85% | 15.05% |
| records_array_16k / candidate | 737 | 0.00% | 0.00% | 19.40% | 17.77% |
| records_array_1m / main | 755 | 20.26% | 11.66% | 6.23% | 7.42% |
| records_array_1m / candidate | 733 | 0.00% | 21.96% | 13.51% | 11.87% |

The current 1 MB materialized resolver is a concrete next target; it still performs generic ownership/classification work for an explicitly live ordinary-array edge. Numeric-index validation is another measured target. The fmod share belongs to the generated access loop. A separate stringify opportunity is a fast-emitter-only path for inert zero spacing, without widening lazy raw-source copy admission or masking lazy dispatch failures. None of these follow-ups is implemented in R24.

Across the eight benchmark windows there are 3,756 timed trials, 622 verification records and 60 separate calibration trials. The four sampled workers and their four Node checksum oracles are additional diagnostics.

## Build provenance

`cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static` on clean committed source. Started 2026-09-11T06:30:44.576706+00:00; elapsed 548.38 seconds.

| Artifact | SHA-256 |
|---|---|
| perry | `c8e83e7dc6680c54044b09c6493522bb0375a93d61730c3c68ebd9cc851b7b79` |
| libperry_runtime.a | `3e925cb1a91bf474203c69b33ae2189481e46e0bccebd4b2cdcf35c1a963bbbc` |
| libperry_stdlib.a | `143309e7959802db73b86be87702d83fcf3e917b16f377f8c256c7c72d1398e8` |

Validation, linked disassembly, build commands, raw windows, scripts and input hashes are preserved in the R24 artifact index. Performance conclusions are limited to the measured rows above.
