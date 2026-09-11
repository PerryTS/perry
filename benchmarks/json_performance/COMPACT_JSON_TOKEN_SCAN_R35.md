# Compact JSON token scan: R35

Rejected for promotion. One quiet rotating-input window contains 420 timed trials, 100 complete-output oracle checks and 60 calibration trials. Relative to frozen R26, separated CPU regressions are small-record fresh parse +0.54%, record-array fresh +15.08% and repeated +14.97%, ASCII fresh +10.68%, escaped-string fresh +23.19% and repeated +23.42%, and Unicode fresh +6.29%. The other eight comparisons have overlapping observed ranges. No R35 stringify-option, Korean, full 50-row, changing-object, large-option, retained-output or access performance window was run. R34 option improvements are not measurements of R35.

## Scope and reference

The object replacer walk keeps one rewritable key handle across getters, toJSON and replacer callbacks. It reloads key bytes only after callbacks and scopes replacer-pointer use at each call. The same slot is reused for each property. Pretty printing and replacer scalar output now use the existing heap-string provenance and inline-short-string emitters. Concatenation and in-place appends clear escape-free provenance when strings gain arbitrary bytes, while preserving lone-surrogate metadata. Escaped strings retain their existing writer. R35 replaces the private 64-byte fused scanner with a compact inline 16-byte NEON scanner. It combines delimiter/control/surrogate-prefix masks and filters valid 0xED sequences before returning a hit; bounded word/scalar tails retain the same contract. Repeated Korean prefix tests cover false-positive filtering. The shared tape scanner is also affected in this measured source. Borrowed value tokens use the existing bounded UTF-16 counter without the separate R31 UTF-8 validation pass; surrogate tokens route to the normalizing builder. Decoded wide object keys also use the builder and cannot receive an escape-free proof. Primitive values skip the owned toJSON-key copy while BigInt and pointer-capable values preserve their hooks. GC policy, parse boundaries, cache admission and thresholds are unchanged.

Measured source `891753f2cff604c87133ac2a4435af9a2286aa22` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

Final source 891753f2cff604c87133ac2a4435af9a2286aa22 passed 303 serial release JSON tests and 119 tests selected by the string filter (overlapping sets). The normal compiler/runtime-static/stdlib-static release build completed in 356.411833 seconds; terminal controller exit 0 and immutable artifacts were copied with source, mtime and SHA checks. All 81 original candidate executions, 20 expanded emitter executions and four primitive-key executions match complete Node output. Scheduled emitter cases include 480 protected retired sets and 29,097 moved objects; the depth-64 case includes 455 and 28,541 respectively. All 28 normalized IR files and six worker objects match R26. Existing native findings and lazy/getter/fraction outcomes remain recorded. The primitive fixture retains the same non-moving native global-load finding in both arms, with identical reports and emitted IR; its shadow checks pass. No checker or allowlist changed. Fresh final-source lint passed 73 of 74 executed gates: public benchmark freshness remains the sole failure; the file cap passed. The initial unit controller exited at an explicit preproduction hold after passing tests; production started only after the source-consistent unit/lint review.

All 81 original reference behavior/options receipts and the 24 added reference fixture receipts are reused with exact source, fixture and frozen R26 artifact hash checks. All 81 original candidate executions are fresh. The expanded emitter regression has 20 fresh candidate executions and 20 verified reference receipts: the reference has fourteen output/callback failures, while the candidate passes all 20. Native/shadow checker comparison covers the emitter and primitive-key fixtures. Four candidate primitive-key executions match complete Node output and the verified reference receipts; the native checker finding described above remains visible. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Eight rotating inputs per fixture

Window: 2026-09-11T13:51:34Z to 2026-09-11T13:53:18Z.

| Fixture / operation | R26 µs | R35 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.495091 | 0.497748 | 0.347807 | 0.254761 | +0.54% | regression |
| small_record / same | 0.099141 | 0.099100 | 0.342805 | 0.246469 | -0.04% | overlap |
| small_record / select | 0.009427 | 0.009438 | 0.002662 | 0.003615 | +0.12% | overlap |
| records_array_1m / rotating | 943.354286 | 1085.605714 | 2671.017143 | 2149.605714 | +15.08% | regression |
| records_array_1m / same | 939.291429 | 1079.908571 | 2657.245714 | 2154.177143 | +14.97% | regression |
| records_array_1m / select | 0.011404 | 0.011487 | 0.003119 | 0.003888 | +0.72% | overlap |
| long_string_1m / rotating | 99.968624 | 110.649236 | 373.227675 | 70.815768 | +10.68% | regression |
| long_string_1m / same | 0.359935 | 0.357027 | 367.990630 | 66.178352 | -0.81% | overlap |
| long_string_1m / select | 0.011652 | 0.011799 | 0.003090 | 0.003858 | +1.26% | overlap |
| escaped_1m / rotating | 996.278481 | 1227.316456 | 1726.221519 | 2078.455696 | +23.19% | regression |
| escaped_1m / same | 993.425000 | 1226.068750 | 1723.618750 | 2075.187500 | +23.42% | regression |
| escaped_1m / select | 0.011517 | 0.011366 | 0.003123 | 0.003870 | -1.30% | overlap |
| unicode_1m / rotating | 141.634448 | 150.545058 | 439.812500 | 62.272529 | +6.29% | regression |
| unicode_1m / same | 0.358096 | 0.357014 | 439.418680 | 59.415795 | -0.30% | overlap |
| unicode_1m / select | 0.011456 | 0.011309 | 0.003148 | 0.003866 | -1.29% | overlap |

| Fixture / operation | R26 MiB | R35 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.922 | 59.953 | 80.172 | -0.062 |
| small_record / same | 78.453 | 78.391 | 59.656 | 79.844 | -0.062 |
| small_record / select | 12.812 | 12.828 | 57.844 | 35.266 | +0.016 |
| records_array_1m / rotating | 76.297 | 76.156 | 128.812 | 98.281 | -0.141 |
| records_array_1m / same | 76.297 | 76.156 | 128.891 | 100.328 | -0.141 |
| records_array_1m / select | 22.594 | 22.594 | 67.578 | 42.531 | +0.000 |
| long_string_1m / rotating | 90.219 | 90.156 | 175.562 | 150.516 | -0.062 |
| long_string_1m / same | 24.594 | 24.609 | 200.906 | 152.578 | +0.016 |
| long_string_1m / select | 24.312 | 24.328 | 69.016 | 43.766 | +0.016 |
| escaped_1m / rotating | 65.000 | 64.875 | 89.688 | 77.766 | -0.125 |
| escaped_1m / same | 65.000 | 64.875 | 89.688 | 77.766 | -0.125 |
| escaped_1m / select | 23.531 | 23.531 | 68.438 | 43.281 | +0.000 |
| unicode_1m / rotating | 61.562 | 61.453 | 158.625 | 124.922 | -0.109 |
| unicode_1m / same | 22.688 | 22.719 | 212.141 | 149.984 | +0.031 |
| unicode_1m / select | 22.438 | 22.438 | 68.625 | 44.656 | +0.000 |

## Build fingerprints

| Artifact | R26 SHA-256 | R35 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `6b3ab49161780802001557bbc60209524f143212e50287fa3ddd1fbe3376c989` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `7cbd1f09cf0d027b959a86c6d98216b4b57d69731529bd65d7c72d821c327020` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `4e61bc120d203dc7eecc11829510d888e755ee02e4eccf9840777ae3b2836f9a` |

## Diagnostic follow-up

Two additional profile diagnostics accompany this rejected experiment. Each terminal remote window was archived before any later remote action; exact pinned-Node loops, warmups, checksum and all VERIFY/LAST/KEEP bytes match. Their sampled CPU/RSS is not benchmark evidence, and inclusive categories overlap.

Frozen R32 small-record rotating parse (20 million calls, 5,000 warmup) produced 639 workload samples. Object parsing/template work accounts for 50.86% inclusively; string scanning 3.29%, string-value work 7.36%, number parsing 1.56%, construction batching 1.88%, allocation 8.14% and collection 25.82%. Template recording alone has 44 self samples. GC policy remains a separate workstream.

Records-array and escaped-string diagnostics compare frozen R26 with R35 using 4,000 rotating calls and eight warmups. Records have 685/766 workload samples; tape building has 488/586 self samples. This row uses tape construction, so the direct object allocator is not an explanation for its regression. Source inspection shows json_tape::skip_string calls the shared find_string_terminator whose R34/R35 replacement added surrogate-aware scanning. Restoring the generic contract while keeping borrowed-value proof scanning separate is the next controlled hypothesis.

Escaped strings have 757/756 workload samples; parse_string_bytes_slow has 520/569 self samples. The phase analyzer also matches this function under string_scan: these counts do not isolate the new scanner. The slow function remains 846 linked instructions in both arms, with the same shown scalar hot-loop instructions; entry alignment differs (28/40 modulo 64), which is a hypothesis, not established causation. The next proposal copies ordinary decoded spans in word-sized groups within existing scratch capacity.

The linked direct parse_string_value grew from 239 to 702 instructions as the scanned string constructor was inlined; outlining that body is another bounded hypothesis. parse_object_untyped grew 1,405 to 2,089 instructions. This does not change the record-array tape finding.

The regression profile controller initially refused its local reference proof before any SSH because the preserved R26 worker receipt names the original R26 candidate path. The failed controller and exact full-path/source correction are retained; the receipt was not rewritten. Prepared Korean performance fixtures/controllers are unrun. No diagnostic or proposed follow-up qualifies R35 for promotion.

The artifact index records the single terminal raw rotating window, exact CPU/peak-RSS vectors, validation receipts, source patch and independent profile diagnostics. Median peak-RSS changes across the 15 comparisons range from -144 KiB to +32 KiB. Process-level peaks do not establish retained live-heap behavior. The Korean controllers are archived as unrun preparation. R35 has no ready PR or merged-main measurement.
