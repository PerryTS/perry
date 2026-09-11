# Isolated JSON scanning and bounded escape spans: R36

Rejected for promotion. Two quiet windows contain 588 timed trials, 140 complete-output oracle checks and 84 calibration trials. The historical rotating screen no longer separates small-record (+0.08%) or record-array fresh (-0.13%) timing from frozen R26. Separated regressions remain for fresh ASCII +10.19%, fresh escaped +10.83%, repeated escaped +10.72%, and fresh Unicode +5.98%. Repeated Unicode improves 0.91% with separated ranges. The new valid-Korean controls regress 81.89% at 978 bytes and 280.61% at about 1 MB for fresh parsing. Other comparisons overlap. No stringify-option, full 50-row, changing-object, large-option, retained-output or access performance window was run for R36. Earlier stringify gains are not measurements of this source.

## Scope and reference

The inherited stringify work roots callback keys, reuses escape-free string provenance safely, clears that provenance on concatenation/appending, handles decoded wide keys through the normalizing builder, and avoids primitive toJSON key copies. R36 restores the original generic tape delimiter scanner, uses the surrogate-aware scanner only for borrowed value tokens, explicitly outlines the scanned string constructor, and copies ordinary escaped-string spans eight bytes at a time within existing 64-byte input/output windows. The decoder does not reserve or allocate additional scratch storage. GC policy, parse boundaries, root ordering, admission and thresholds remain unchanged.

Measured source `2f7c36be28acc9e090d4aef1c2337335d83ee6b0` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

Final source 2f7c36be28acc9e090d4aef1c2337335d83ee6b0 passed 304 serial release JSON tests (2.91 s) and 119 string-filter tests (1.65 s), overlapping sets. This includes guarded input, every truncation of escaped boundary cases, randomized serde output comparisons, scratch-capacity checks and the new complete-escape/word-copy boundary test. The normal compiler/runtime-static/stdlib-static release build completed in 350.211699 seconds; immutable artifacts were copied with source, mtime and SHA checks. All 81 original candidate executions, 20 expanded emitter executions and four primitive-key executions match complete Node output. Scheduled cases have positive moved-object and protected-retired-set witnesses. All 28 normalized IR files and six worker objects match frozen R26. Existing native findings and lazy/getter/fraction outcomes remain recorded; the primitive fixture retains its same non-moving native global-load finding in both arms, with identical reports/IR and passing shadow checks. No checker or allowlist changed. Fresh final-source lint passed 73 of 74 executed gates, with public benchmark freshness the sole failure; file cap passed. The initial unit controller exited at an explicit preproduction hold after passing; production resumed only after the source-consistent unit/lint review.

All 81 original reference behavior/options receipts and the 24 added reference fixture receipts are reused with exact source, fixture and frozen R26 artifact hash checks. All 81 original candidate executions are fresh. The expanded emitter regression has 20 fresh candidate executions and 20 verified reference receipts: the reference has fourteen output/callback failures, while the candidate passes all 20. Native/shadow checker comparison covers the emitter and primitive-key fixtures. Four candidate primitive-key executions match complete Node output and the verified reference receipts; the native checker finding described above remains visible. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Eight rotating inputs per fixture

Window: 2026-09-11T14:25:31Z to 2026-09-11T14:27:13Z.

| Fixture / operation | R26 µs | R36 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494014 | 0.494387 | 0.341097 | 0.255375 | +0.08% | overlap |
| small_record / same | 0.099064 | 0.099076 | 0.352931 | 0.246272 | +0.01% | overlap |
| small_record / select | 0.009434 | 0.009437 | 0.002655 | 0.003612 | +0.03% | overlap |
| records_array_1m / rotating | 944.751445 | 943.560694 | 2622.017341 | 2156.144509 | -0.13% | overlap |
| records_array_1m / same | 940.626437 | 940.477011 | 2776.293103 | 2156.643678 | -0.02% | overlap |
| records_array_1m / select | 0.011335 | 0.011306 | 0.003124 | 0.003843 | -0.25% | overlap |
| long_string_1m / rotating | 99.836641 | 110.013740 | 372.720611 | 69.998473 | +10.19% | regression |
| long_string_1m / same | 0.359601 | 0.357671 | 375.969122 | 66.003538 | -0.54% | overlap |
| long_string_1m / select | 0.011449 | 0.011550 | 0.003112 | 0.003845 | +0.88% | overlap |
| escaped_1m / rotating | 994.164557 | 1101.816456 | 1725.500000 | 2078.575949 | +10.83% | regression |
| escaped_1m / same | 994.245283 | 1100.817610 | 1724.465409 | 2075.119497 | +10.72% | regression |
| escaped_1m / select | 0.011395 | 0.011470 | 0.003116 | 0.003873 | +0.66% | overlap |
| unicode_1m / rotating | 142.335901 | 150.843606 | 440.080894 | 63.788906 | +5.98% | regression |
| unicode_1m / same | 0.357864 | 0.354618 | 437.016955 | 59.472944 | -0.91% | gain |
| unicode_1m / select | 0.011518 | 0.011442 | 0.003152 | 0.003897 | -0.66% | overlap |

| Fixture / operation | R26 MiB | R36 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.969 | 32.000 | 59.891 | 80.188 | +0.031 |
| small_record / same | 77.609 | 77.641 | 59.719 | 79.875 | +0.031 |
| small_record / select | 12.828 | 12.922 | 57.812 | 35.266 | +0.094 |
| records_array_1m / rotating | 76.297 | 76.250 | 128.828 | 100.078 | -0.047 |
| records_array_1m / same | 76.297 | 76.250 | 128.797 | 100.078 | -0.047 |
| records_array_1m / select | 22.594 | 22.688 | 67.594 | 42.516 | +0.094 |
| long_string_1m / rotating | 90.234 | 90.250 | 177.594 | 148.438 | +0.016 |
| long_string_1m / same | 24.594 | 24.688 | 203.438 | 156.547 | +0.094 |
| long_string_1m / select | 24.328 | 24.422 | 68.969 | 43.766 | +0.094 |
| escaped_1m / rotating | 65.000 | 64.984 | 89.625 | 77.781 | -0.016 |
| escaped_1m / same | 65.000 | 64.984 | 89.641 | 77.781 | -0.016 |
| escaped_1m / select | 23.531 | 23.609 | 68.469 | 43.281 | +0.078 |
| unicode_1m / rotating | 61.141 | 61.141 | 156.797 | 155.297 | +0.000 |
| unicode_1m / same | 22.688 | 22.812 | 200.203 | 155.281 | +0.125 |
| unicode_1m / select | 22.438 | 22.531 | 68.578 | 44.672 | +0.094 |

### Valid Korean input controls

Window: 2026-09-11T14:28:47Z to 2026-09-11T14:29:38Z.

| Fixture / operation | R26 µs | R36 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| korean_small / rotating | 0.517856 | 0.941943 | 0.399437 | 0.213718 | +81.89% | regression |
| korean_small / same | 0.082773 | 0.082753 | 0.391850 | 0.204063 | -0.02% | overlap |
| korean_small / select | 0.009488 | 0.009425 | 0.002659 | 0.003624 | -0.66% | overlap |
| korean_1m / rotating | 165.073762 | 628.288198 | 243.652266 | 45.852476 | +280.61% | regression |
| korean_1m / same | 0.357497 | 0.355696 | 248.748987 | 43.495047 | -0.50% | overlap |
| korean_1m / select | 0.011635 | 0.011623 | 0.003096 | 0.003883 | -0.10% | overlap |

| Fixture / operation | R26 MiB | R36 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| korean_small / rotating | 32.281 | 32.312 | 59.953 | 70.656 | +0.031 |
| korean_small / same | 64.922 | 65.000 | 61.781 | 70.375 | +0.078 |
| korean_small / select | 12.812 | 12.922 | 57.859 | 35.344 | +0.109 |
| korean_1m / rotating | 90.562 | 90.609 | 150.453 | 120.734 | +0.047 |
| korean_1m / same | 24.656 | 24.766 | 191.516 | 152.781 | +0.109 |
| korean_1m / select | 24.328 | 24.422 | 66.500 | 43.125 | +0.094 |

## Build fingerprints

| Artifact | R26 SHA-256 | R36 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `a6c90cd38c1f9e391c4efe72e3c79af3a4d97610144d7bd7d828085f1030ffbb` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `12b0e21f9daa9b5ff8952bfb95e37cc4ee9ae224c46f5d75ff5b2b7bcbcac7ff` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `b97f5552212c26fd8acb3eca02395410c1a2a9c4f7850e2c6e6007e4ad68a845` |

## Diagnostic follow-up

R36 restores the generic delimiter scanner used by json_tape::skip_string and keeps surrogate-aware scanning specific to borrowed value tokens. The 15% record-array regression observed in R35 no longer separates from the reference in this controlled screen. This is consistent with the tape profile diagnosis; the original timing vectors remain available.

Linked production disassembly confirms parse_string_value returns from R35's 702 instructions to 239, matching R26, after explicitly outlining its scanned constructor. parse_object_untyped remains 2,089 versus R26's 1,405 instructions: the original JSON constructor also became a single caller and was inlined into the wide-key path. R35/R36 save additional FP/SIMD registers even on small-object calls. The next source explicitly outlines that constructor too. Instruction counts and prologues are diagnostic evidence, not performance claims.

The escaped decoder is 908 instructions versus R26's 846. R36 copies ordinary spans up to eight bytes into existing spare storage, but performs its word mask before both ordinary text and escape dispatch. The escaped control is 990,018 source bytes containing 270,000 backslashes. Moving word scanning exclusively into the ordinary-byte branch is the next hypothesis for this dense-escape workload; it preserves the 64-byte input/output proof and maximal 12-byte escape boundary.

Korean controls use eight equal-length inputs per size with independent ids, preserving the historical manifest. The surrogate-aware scanner currently filters valid ED80..9F hits individually inside each vector. The new 3.8x fresh-megabyte slowdown motivates a separate vector-refinement continuation that handles every ED lane together while keeping the common short-token path compact. Bounded byte-16 lookahead must remain covered by guard-page and seam tests. These proposed follow-ups were not present in the measured source.

An additional frozen-R32 template-capture disassembly is included with its own binary hash. Sampled offsets come from the previously archived R32 profile; no new sampled run was performed. It shows a 0x500-byte local stack frame and 576/600-byte memcpy calls; Mach-O indirect-symbol resolution identifies memcpy and memset_pattern16 explicitly. This records a future small-object investigation, not R36 CPU/RSS evidence. Inclusive profile categories cannot be summed.

The artifact index records both terminal windows, exact CPU/peak-RSS vectors, full output checks, validation receipts, source patch and linked-code diagnostics. Both remote controllers exited zero and archived their windows before subsequent remote actions. Median peak-RSS differences across the 21 comparisons range from -48 KiB to +128 KiB. These process peaks do not establish retained live-heap behavior. The candidate has no ready PR or merged-main measurement.
