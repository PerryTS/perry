# Bulk ASCII checks and native-buffer escaping: R38

R38 is not promoted yet because fresh small-record parsing regresses 0.63% with separated sample ranges. The large escaped-input regression is removed: fresh and repeated parsing improve 5.81% and 5.86%. Fresh Unicode and 1 MB Korean parsing improve 41.24% and 18.83%. Large escaped-string pretty printing and replacer serialization improve 49.91% and 49.00%, respectively; their process peak RSS drops about 3.5 MiB. Both now beat Node on that fixture but remain about 1.60–1.62 times Bun CPU. The other measured stringify options show gains or overlapping ranges. All changes are relative to the frozen R26 reference.

Four qualified quiet windows contain 952 timed trials, 205 full-output checks and 84 calibrations across 34 comparisons. One initial rotating window is excluded because its ending load exceeded the existing quietness ceiling; its 420 timed trials, 100 output checks and 60 calibrations are retained separately. The successful retry required a local archive-name correction after its measurement completed; the original controller failure and exact recovery proof are preserved. No quietness, output or source check was relaxed. No R38 full-matrix, independent small recheck, changing-object, retained-output or post-parse-access performance window ran. R39 is investigating the remaining small-template overhead independently.

## Scope and reference

The inherited stringify work roots callback keys, reuses escape-free string provenance safely, clears that provenance on concatenation/appending, handles decoded wide keys through the normalizing builder, and avoids primitive toJSON key copies. The inherited parser separates small inputs from the dominant-token length proof, handles valid-ED continuation bytes and copies ordinary decoded spans within bounded scratch windows. R38 reuses the existing UTF-8-validated expansion plan and exact bounded writer for large strings in general stringify buffers, retaining geometric growth and the WTF-8 fallback. The builder uses the standard bulk ASCII predicate. Neither change adds managed intermediate allocation or a GC entry. The decoder does not reserve or allocate additional scratch storage. GC policy, parse boundaries, root ordering, admission and thresholds remain unchanged.

Measured source `5f6448ce7b38754481ab020f2f6d059c40dac0d2` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

Final source 5f6448ce7b38754481ab020f2f6d059c40dac0d2 passes 309 serial release JSON tests (2.94 s) and 119 string-filter tests (1.63 s), overlapping sets. The normal compiler/runtime-static/stdlib-static release build completed in 337.325434 s, and 52 build/unit/lint payloads were copied with clean matching source, artifact mtime and SHA checks. All 125 candidate output checks pass: 81 original, 20 expanded emitter, four primitive-key and 20 new large-emitter executions. The new fixture tests large escaped keys and values, allocating callbacks, retained complete outputs, 255/256/257-byte thresholds and lone-surrogate fallbacks. Its scheduled runs exercise 1512 protected retired sets and 167766 moved objects. The frozen R26 reference passes 12 normal/full-GC executions but crashes in all eight scheduled executions; a separate symbolized diagnostic locates the stale large-key use inside stringify_object_with_replacer_pretty.

All 30 normalized IR files and six worker objects match R26. Native/shadow checks are freshly executed for the large-emitter fixture: native retains one non-moving strhandle/overflow-store finding with identical complete reports and IR in both arms; both shadow variants pass. The original native, primitive-key and lazy/getter/fraction findings remain unsuppressed and equivalent to the reference. No checker or allowlist changes. Final-source lint passes 73 of 74 executed gates; public benchmark freshness is the sole failure and formatting/file cap pass. Compile/CI-only tiers remain explicitly recorded. The initial a23 unit compilation failure and b880 interruption are preserved; neither produced a production build or performance result.

After the recorded full lint run, shared origin/main advanced to f6c6879. A pinned read-only ratchet comparison now rejects the same inherited array/sort ceiling on both unchanged R38 and the R39 follow-up, with identical checker, baseline and sort-source hashes. This remains an integration issue against newer main; it is not counted as a passing current-main gate.

All 81 original reference behavior/options receipts and the 24 added reference fixture receipts are reused with exact source, fixture and frozen R26 artifact hash checks. All 81 original candidate executions are fresh. The expanded emitter regression has 20 fresh candidate executions and 20 verified reference receipts: the reference has fourteen output/callback failures, while the candidate passes all 20. Native/shadow checker comparison covers the emitter, primitive-key and new large-emitter fixtures. The new fixture has 20 fresh executions in each arm: R26 passes 12 and crashes in eight moving-stress cases; R38 passes all 20 with full Node output and positive moving/protected witnesses. Its separate native static finding remains explicit, with identical complete reports and IR in both arms. Four candidate primitive-key executions match complete Node output and the verified reference receipts; the native checker finding described above remains visible. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Stringify option controls

Window: 2026-09-11T15:41:58Z to 2026-09-11T15:43:01Z.

| Fixture / operation | R26 µs | R38 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.044866 | 0.044818 | 0.107807 | 0.119477 | -0.11% | overlap |
| small_record / dynamic-zero | 0.044582 | 0.044595 | 0.247524 | 0.394202 | +0.03% | overlap |
| small_record / zero | 0.044904 | 0.044820 | 0.247589 | 0.394234 | -0.19% | overlap |
| small_record / pretty | 0.486280 | 0.451657 | 0.304145 | 0.524904 | -7.12% | gain |
| small_record / keys | 0.461010 | 0.449846 | 0.598825 | 0.484222 | -2.42% | gain |
| small_record / callback | 0.947505 | 0.889890 | 0.603660 | 0.581670 | -6.08% | gain |
| records_array_16k / pretty | 46.204000 | 42.351000 | 33.764000 | 45.714000 | -8.34% | gain |

| Fixture / operation | R26 MiB | R38 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.422 | 33.375 | 59.578 | 378.578 | -0.047 |
| small_record / dynamic-zero | 33.406 | 33.375 | 59.656 | 378.641 | -0.031 |
| small_record / zero | 33.406 | 33.406 | 59.594 | 378.641 | +0.000 |
| small_record / pretty | 33.750 | 33.766 | 59.641 | 239.391 | +0.016 |
| small_record / keys | 33.719 | 33.734 | 59.703 | 207.766 | +0.016 |
| small_record / callback | 32.500 | 32.516 | 59.625 | 97.734 | +0.016 |
| records_array_16k / pretty | 35.062 | 35.047 | 57.047 | 49.734 | -0.016 |

### Megabyte pretty printing and replacers

Window: 2026-09-11T15:46:39Z to 2026-09-11T15:47:04Z.

| Fixture / operation | R26 µs | R38 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| long_string_1m / pretty | 127.835938 | 86.785156 | 415.867188 | 536.097656 | -32.11% | gain |
| long_string_1m / callback | 156.453125 | 114.664062 | 442.000000 | 546.367188 | -26.71% | gain |
| unicode_1m / pretty | 168.601562 | 132.707031 | 560.882812 | 446.703125 | -21.29% | gain |
| unicode_1m / callback | 196.898438 | 160.382812 | 586.914062 | 455.085938 | -18.55% | gain |
| escaped_1m / pretty | 1793.406250 | 898.332031 | 1207.343750 | 561.957031 | -49.91% | gain |
| escaped_1m / callback | 1811.101562 | 923.585938 | 1234.468750 | 571.773438 | -49.00% | gain |

| Fixture / operation | R26 MiB | R38 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| long_string_1m / pretty | 55.703 | 55.672 | 89.781 | 87.219 | -0.031 |
| long_string_1m / callback | 55.812 | 55.781 | 89.016 | 75.844 | -0.031 |
| unicode_1m / pretty | 55.172 | 55.141 | 86.688 | 82.781 | -0.031 |
| unicode_1m / callback | 55.219 | 55.219 | 80.328 | 72.812 | +0.000 |
| escaped_1m / pretty | 59.047 | 55.562 | 87.422 | 83.609 | -3.484 |
| escaped_1m / callback | 59.188 | 55.641 | 81.875 | 74.875 | -3.547 |

### Eight rotating inputs per fixture

Window: 2026-09-11T15:36:09Z to 2026-09-11T15:37:51Z.

| Fixture / operation | R26 µs | R38 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494588 | 0.497725 | 0.354322 | 0.254781 | +0.63% | regression |
| small_record / same | 0.099201 | 0.099108 | 0.337235 | 0.245480 | -0.09% | gain |
| small_record / select | 0.009429 | 0.009434 | 0.002657 | 0.003623 | +0.05% | overlap |
| records_array_1m / rotating | 942.170455 | 925.886364 | 2743.869318 | 2156.505682 | -1.73% | gain |
| records_array_1m / same | 938.380682 | 922.198864 | 2714.653409 | 2161.755682 | -1.72% | gain |
| records_array_1m / select | 0.011526 | 0.011394 | 0.003144 | 0.003833 | -1.15% | overlap |
| long_string_1m / rotating | 99.816436 | 97.684332 | 371.649002 | 69.825653 | -2.14% | gain |
| long_string_1m / same | 0.360115 | 0.356914 | 376.635403 | 65.771767 | -0.89% | overlap |
| long_string_1m / select | 0.011567 | 0.011414 | 0.003100 | 0.003884 | -1.32% | overlap |
| escaped_1m / rotating | 1008.153846 | 949.568047 | 1725.047337 | 2076.526627 | -5.81% | gain |
| escaped_1m / same | 989.776471 | 931.811765 | 1723.300000 | 2072.652941 | -5.86% | gain |
| escaped_1m / select | 0.011376 | 0.011512 | 0.003127 | 0.003861 | +1.20% | overlap |
| unicode_1m / rotating | 141.633844 | 83.219548 | 439.691466 | 63.236324 | -41.24% | gain |
| unicode_1m / same | 0.356863 | 0.354367 | 438.398217 | 59.126916 | -0.70% | gain |
| unicode_1m / select | 0.011265 | 0.011389 | 0.003123 | 0.003857 | +1.10% | overlap |

| Fixture / operation | R26 MiB | R38 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.812 | 59.875 | 80.172 | -0.172 |
| small_record / same | 74.375 | 74.219 | 59.688 | 79.844 | -0.156 |
| small_record / select | 12.812 | 12.766 | 57.875 | 35.250 | -0.047 |
| records_array_1m / rotating | 76.297 | 76.172 | 128.844 | 100.281 | -0.125 |
| records_array_1m / same | 76.297 | 76.172 | 128.953 | 100.344 | -0.125 |
| records_array_1m / select | 22.594 | 22.531 | 67.625 | 42.500 | -0.062 |
| long_string_1m / rotating | 90.234 | 90.031 | 177.688 | 145.453 | -0.203 |
| long_string_1m / same | 24.609 | 24.516 | 197.578 | 142.562 | -0.094 |
| long_string_1m / select | 24.328 | 24.266 | 69.047 | 43.766 | -0.062 |
| escaped_1m / rotating | 65.000 | 64.875 | 90.406 | 77.766 | -0.125 |
| escaped_1m / same | 65.000 | 64.875 | 90.391 | 77.750 | -0.125 |
| escaped_1m / select | 23.531 | 23.469 | 68.484 | 43.266 | -0.062 |
| unicode_1m / rotating | 61.125 | 60.984 | 158.562 | 151.641 | -0.141 |
| unicode_1m / same | 22.703 | 22.609 | 223.672 | 147.250 | -0.094 |
| unicode_1m / select | 22.438 | 22.375 | 68.609 | 44.656 | -0.062 |

### Valid Korean input controls

Window: 2026-09-11T15:39:59Z to 2026-09-11T15:40:41Z.

| Fixture / operation | R26 µs | R38 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| korean_small / rotating | 0.518470 | 0.506920 | 0.402201 | 0.214288 | -2.23% | gain |
| korean_small / same | 0.082994 | 0.083253 | 0.391996 | 0.204611 | +0.31% | overlap |
| korean_small / select | 0.009520 | 0.009505 | 0.002659 | 0.003628 | -0.17% | overlap |
| korean_1m / rotating | 164.881101 | 133.836968 | 243.511423 | 45.884216 | -18.83% | gain |
| korean_1m / same | 0.356682 | 0.353791 | 248.350233 | 43.595953 | -0.81% | overlap |
| korean_1m / select | 0.011643 | 0.011740 | 0.003133 | 0.003842 | +0.84% | overlap |

| Fixture / operation | R26 MiB | R38 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| korean_small / rotating | 32.312 | 32.156 | 59.969 | 70.625 | -0.156 |
| korean_small / same | 64.922 | 64.828 | 61.703 | 70.375 | -0.094 |
| korean_small / select | 12.812 | 12.781 | 57.891 | 35.328 | -0.031 |
| korean_1m / rotating | 90.594 | 90.422 | 150.438 | 120.734 | -0.172 |
| korean_1m / same | 24.656 | 24.562 | 192.203 | 155.453 | -0.094 |
| korean_1m / select | 24.328 | 24.266 | 66.516 | 43.109 | -0.062 |

## Build fingerprints

| Artifact | R26 SHA-256 | R38 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `a57b0b9c33f2a337be58f26a8f860a6ee700ef2262d67f15486b45404d57f81a` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `432c48a1ee523b6d9e424d6a901b018acecf5323702d15e69947244ab7450a8f` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `77348ba7de47a7c45db50f476defa7da912cb8c0b1a9667a4ca288db1cbe66f5` |

## Diagnostic follow-up

The final linked builder uses four 128-bit loads per 64-byte ASCII iteration with vector OR/reduction, replacing R37's scalar byte loop. This is a code-shape observation; timing remains a separate measurement.

The new large-emitter fixture reproduces a reference stale-pointer fault after 62 copying/protected cycles. A separate native object linked with symbols places the stale use in stringify_object_with_replacer_pretty. R38 completes all 20 fresh runs and retains exact Node output after allocating callbacks and 167766 moved objects. The native static checker separately reports one nonmoving string-handle/overflow-store finding; it has identical IR and full reports in both arms and remains unsuppressed. Both shadow variants pass. This static finding is not the moving runtime failure just repaired.

The initial rotating timing window ended above the existing 2.5 load ceiling (load 3.25), despite no named competing workload. Its full raw data and terminal receipt are retained and excluded from performance conclusions. A distinct retry uses the same source, immutable binaries, fixture bytes, counts and seven repetitions. No quietness threshold was changed.

The artifact index distinguishes qualified quiet windows from the excluded initial rotating run and preserves every terminal receipt, exact CPU/RSS vector, complete output check, validation log, source patch and linked-code diagnostic. Every remote window is archived before subsequent remote operations. Process peak RSS does not by itself establish retained live-heap behavior. This is experimental-source evidence; no merged-main claim is made.
