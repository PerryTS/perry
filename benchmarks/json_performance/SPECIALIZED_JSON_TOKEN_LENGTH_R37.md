# Specialized JSON token lengths and scanner continuation: R37

R37 is not promoted. Fresh Unicode parsing improves 41.34%, fresh 1 MB Korean parsing improves 18.83%, and records-array parsing improves about 1.8% against frozen R26. However, escaped 1 MB input regresses 8.34% fresh and 8.51% repeated, with separated sample ranges. Small fresh parsing is 0.49% slower at the median, with overlapping ranges and all seven paired trials slower; it needs an independent recheck if the escaped regression is fixed. This round contains two rotating-input windows only: 588 timed trials, 140 full-output checks and 84 calibrations across 21 comparisons. No current-source stringify-option, full-matrix, changing-object, retained-output or access performance window ran.

## Scope and reference

The inherited stringify work roots callback keys, reuses escape-free string provenance safely, clears that provenance on concatenation/appending, handles decoded wide keys through the normalizing builder, and avoids primitive toJSON key copies. R37 specializes the direct parser by input length: small inputs compile without the dominant-token length proof, while large borrowed tokens can derive their UTF-16 length from the exact rooted source under bounded ASCII/outside-token checks. Both managed string constructors are explicitly outlined. A valid-ED continuation scanner handles Korean UTF-8 without repeated false-positive dispatch, and the escaped decoder copies ordinary spans eight bytes at a time only after its normal escape dispatch, within existing 64-byte input/output windows. The decoder does not reserve or allocate additional scratch storage. GC policy, parse boundaries, root ordering, admission and thresholds remain unchanged.

Measured source `3d59ed5219ace6bf001df3de9108c52434dc4232` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

Final source 3d59ed5219ace6bf001df3de9108c52434dc4232 passed 308 serial release JSON tests (2.89 s) and 119 string-filter tests (1.69 s), overlapping sets. This includes guarded input, every truncation of escaped boundary cases, randomized serde output comparisons, scratch-capacity checks the complete-escape/word-copy boundary test, and four new source-length proof tests covering malformed UTF-8 seams, surrogate handling and dispatch boundaries. The normal compiler/runtime-static/stdlib-static release build completed in 366.116713 seconds; immutable artifacts were copied with source, mtime and SHA checks. All 81 original candidate executions, 20 expanded emitter executions and four primitive-key executions match complete Node output. Scheduled cases have positive moved-object and protected-retired-set witnesses. All 28 normalized IR files and six worker objects match frozen R26. Existing native findings and lazy/getter/fraction outcomes remain recorded; the primitive fixture retains its same non-moving native global-load finding in both arms, with identical reports/IR and passing shadow checks. No checker or allowlist changed. Fresh final-source lint passed 73 of 74 executed gates, with public benchmark freshness the sole failure; file cap passed. The initial unit controller exited at an explicit preproduction hold after passing; production resumed only after the source-consistent unit/lint review.

Two initial attempts remain archived: 77ea4c1 passed units/lint but was held before production for the scanner revision; 599b191 passed JSON units but failed formatting and public freshness, and was held before production. The final 3d59ed5 revision changes formatting only from 599b191 and reruns units, string checks and lint. Original bootstrap-at-599b records are preserved alongside the source-format follow-up proof.

All 81 original reference behavior/options receipts and the 24 added reference fixture receipts are reused with exact source, fixture and frozen R26 artifact hash checks. All 81 original candidate executions are fresh. The expanded emitter regression has 20 fresh candidate executions and 20 verified reference receipts: the reference has fourteen output/callback failures, while the candidate passes all 20. Native/shadow checker comparison covers the emitter and primitive-key fixtures. Four candidate primitive-key executions match complete Node output and the verified reference receipts; the native checker finding described above remains visible. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Eight rotating inputs per fixture

Window: 2026-09-11T15:02:58Z to 2026-09-11T15:04:41Z.

| Fixture / operation | R26 µs | R37 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494761 | 0.497203 | 0.346233 | 0.254587 | +0.49% | overlap |
| small_record / same | 0.099052 | 0.099327 | 0.333727 | 0.246674 | +0.28% | overlap |
| small_record / select | 0.009433 | 0.009430 | 0.002648 | 0.003615 | -0.03% | overlap |
| records_array_1m / rotating | 942.276836 | 925.259887 | 2780.581921 | 2149.073446 | -1.81% | gain |
| records_array_1m / same | 937.681818 | 921.250000 | 2716.250000 | 2152.068182 | -1.75% | gain |
| records_array_1m / select | 0.011244 | 0.011695 | 0.003136 | 0.003877 | +4.02% | overlap |
| long_string_1m / rotating | 100.907834 | 98.432412 | 373.039171 | 70.466974 | -2.45% | overlap |
| long_string_1m / same | 0.358681 | 0.357376 | 374.440927 | 65.908616 | -0.36% | overlap |
| long_string_1m / select | 0.011426 | 0.011859 | 0.003085 | 0.003867 | +3.79% | overlap |
| escaped_1m / rotating | 994.841772 | 1077.822785 | 1726.291139 | 2078.493671 | +8.34% | regression |
| escaped_1m / same | 994.143750 | 1078.712500 | 1723.887500 | 2074.143750 | +8.51% | regression |
| escaped_1m / select | 0.011468 | 0.011508 | 0.003121 | 0.003854 | +0.36% | overlap |
| unicode_1m / rotating | 141.687143 | 83.112857 | 439.489286 | 63.332143 | -41.34% | gain |
| unicode_1m / same | 0.356894 | 0.354805 | 439.657382 | 59.072075 | -0.59% | overlap |
| unicode_1m / select | 0.011520 | 0.011175 | 0.003122 | 0.003886 | -2.99% | overlap |

| Fixture / operation | R26 MiB | R37 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.938 | 60.078 | 80.188 | -0.047 |
| small_record / same | 75.969 | 75.938 | 59.859 | 79.844 | -0.031 |
| small_record / select | 12.812 | 12.859 | 58.031 | 35.266 | +0.047 |
| records_array_1m / rotating | 76.297 | 76.266 | 129.234 | 98.312 | -0.031 |
| records_array_1m / same | 76.297 | 76.266 | 129.219 | 99.984 | -0.031 |
| records_array_1m / select | 22.594 | 22.594 | 67.844 | 42.531 | +0.000 |
| long_string_1m / rotating | 90.234 | 90.172 | 177.906 | 145.516 | -0.062 |
| long_string_1m / same | 24.594 | 24.594 | 205.125 | 145.547 | +0.000 |
| long_string_1m / select | 24.328 | 24.344 | 69.266 | 43.781 | +0.016 |
| escaped_1m / rotating | 65.000 | 64.984 | 89.984 | 77.766 | -0.016 |
| escaped_1m / same | 65.000 | 64.984 | 89.953 | 77.750 | -0.016 |
| escaped_1m / select | 23.531 | 23.547 | 68.703 | 43.266 | +0.016 |
| unicode_1m / rotating | 61.531 | 61.516 | 159.906 | 153.438 | -0.016 |
| unicode_1m / same | 22.703 | 22.719 | 212.469 | 153.562 | +0.016 |
| unicode_1m / select | 22.438 | 22.453 | 68.828 | 44.656 | +0.016 |

### Valid Korean input controls

Window: 2026-09-11T15:07:31Z to 2026-09-11T15:08:13Z.

| Fixture / operation | R26 µs | R37 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| korean_small / rotating | 0.517991 | 0.508230 | 0.399581 | 0.213697 | -1.88% | gain |
| korean_small / same | 0.082805 | 0.082782 | 0.391425 | 0.204464 | -0.03% | overlap |
| korean_small / select | 0.009429 | 0.009430 | 0.002653 | 0.003626 | +0.01% | overlap |
| korean_1m / rotating | 164.745288 | 133.715741 | 243.398879 | 45.801834 | -18.83% | gain |
| korean_1m / same | 0.357971 | 0.354105 | 248.471685 | 43.564703 | -1.08% | gain |
| korean_1m / select | 0.011697 | 0.011517 | 0.003135 | 0.003839 | -1.54% | overlap |

| Fixture / operation | R26 MiB | R37 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| korean_small / rotating | 32.297 | 32.266 | 60.188 | 70.641 | -0.031 |
| korean_small / same | 64.953 | 64.922 | 62.016 | 70.391 | -0.031 |
| korean_small / select | 12.812 | 12.844 | 58.156 | 35.328 | +0.031 |
| korean_1m / rotating | 90.594 | 90.547 | 151.422 | 122.062 | -0.047 |
| korean_1m / same | 24.656 | 24.672 | 191.156 | 154.125 | +0.016 |
| korean_1m / select | 24.328 | 24.344 | 66.734 | 43.094 | +0.016 |

## Build fingerprints

| Artifact | R26 SHA-256 | R37 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `b9027253820f6b6e57ae0a526be8ed8ce61761f487a0ec26f1220f27629d8874` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `f41dcd1f762810b816ef44d36c229ce845d16eb273905e9398b54afcbd9b4ef8` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `f9bd50a5ab076dab4d2131a21c93b24c97cfc2fb9c8da1f4af3347608ed56330` |

## Diagnostic follow-up

Linked ARM64 disassembly confirms that the small parser specialization contains no dominant-token length proof. Its string-value function has 158 instructions and object parsing has 1186, compared with the earlier R26 diagnostic counts of 239 and 1405. The large specialization inlines several string/object paths into parse_value and retains the proof call. These instruction counts explain code shape, not measured causality. The escape slow path has 828 instructions; the residual escaped-input regression remains a measured failure.

The managed builder still uses a scalar ASCII predicate, emitted as four byte loads and branches per loop. An independently archived R26 escaped-input profile previously attributed 116 of 757 self samples to that builder. R38 will replace the equivalent predicate with the standard bulk is_ascii check; that change is not present in these R37 measurements.

Fresh R26 profiles of 1 MB escaped-string pretty printing and allocating-replacer serialization identify the general escaping writer as 97.94% and 97.50% of inclusive samples, respectively. Small-copy routines account for 20.85% and 19.61% inclusive samples inside that work; inclusive categories overlap and must not be added. Collection accounts for 1.10% and 0.39%. Each profile executes 2000 calls plus eight warmups and matches the full output and checksum of the pinned Node oracle running those same counts. Sampled runs are diagnostics, not CPU benchmark evidence. R38 will reuse the existing checked expansion plan and bounded writer to avoid per-span native-buffer copies, while preserving the surrogate fallback and buffer growth. Neither proposed R38 improvement is a claimed R37 gain.

The artifact index records both terminal windows, exact CPU/peak-RSS vectors, full output checks, validation receipts, source patch and linked-code diagnostics. Both remote controllers exited zero and archived their windows before subsequent remote actions. Median peak-RSS differences across the 21 comparisons range from -64 KiB to +48 KiB. These process peaks do not establish retained live-heap behavior. The candidate has no ready PR or merged-main measurement.
