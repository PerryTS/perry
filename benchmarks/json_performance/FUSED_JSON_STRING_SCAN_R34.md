# Fused JSON token scanning and callback key dispatch: R34

Not promoted. Fresh-input parsing regressed: small records +2.17%, record arrays +15.13%, megabyte ASCII strings +27.39%, and Unicode strings +16.52%; all observed candidate sample ranges were above the reference ranges. Unicode repeated-input parsing also regressed +0.99%. Stringify option gains were small pretty −6.15%, small key filtering −2.28%, small callbacks −5.46%, and 16 KiB pretty output −8.40%. These gains do not qualify the combined source under the no-regressions requirement. Two quiet windows completed with 616 timed trials, 135 oracle records and 60 calibration trials. No full 50-row, large-option, changing-object, access, or retained-performance window was run for R34 after the rotating screen failed.

## Scope and reference

The object replacer walk keeps one rewritable key handle across getters, toJSON and replacer callbacks. It reloads key bytes only after callbacks and scopes replacer-pointer use at each call. The same slot is reused for each property. Pretty printing and replacer scalar output now use the existing heap-string provenance and inline-short-string emitters. Concatenation and in-place appends clear escape-free provenance when strings gain arbitrary bytes, while preserving lone-surrogate metadata. Escaped strings retain their existing writer. A private bounded SIMD scanner stops at raw surrogate prefixes as well as JSON delimiters. Borrowed value tokens use the existing bounded UTF-16 counter without the separate R31 UTF-8 validation pass; surrogate tokens route to the normalizing builder. Decoded wide object keys also use the builder and cannot receive an escape-free proof. Primitive values skip the owned toJSON-key copy while BigInt and pointer-capable values preserve their hooks. GC policy, parse boundaries, cache admission and thresholds are unchanged.

Measured source `9b1bf09c92f7f1bc9f918d1b8c3adb554c3b6792` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

Final source 9b1bf09c92f7f1bc9f918d1b8c3adb554c3b6792 passed 302 serial release JSON tests and 118 tests selected by the string filter (overlapping sets). Normal compiler/runtime-static/stdlib-static release build completed in 382.969710 seconds; terminal controller exit 0 and all three immutable artifacts were copied with source, mtime and SHA checks. All 81 original candidate executions, 20 expanded emitter executions and four primitive-key executions match complete Node output. Scheduled emitter/control cases have positive moved-object and protected-retired-set witnesses. All 28 normalized IR files and six worker objects match the R26 reference. Existing native findings and lazy/getter/fraction outcomes remain recorded; the new primitive fixture has one additional non-moving native global-load finding in both arms, with byte-identical reports and emitted IR, while its shadow checks pass. The strict comparator initially rejected this finding; that failure is preserved and the revised local comparison requires exactly this unsuppressed finding. No checker/allowlist was changed. Fresh final-source lint passed 73 of 74 executed gates: public benchmark freshness remains the sole failure; the file cap passed. Initial source attempts and a source-overlapping lint attempt were rejected and retained; production remained held until final source-consistent unit/lint review.

All 81 original reference behavior/options receipts are reused from the exact R26 build. All 81 original candidate executions are fresh. The new emitter regression adds 20 fresh executions per arm: the reference has fourteen output/callback failures, while the candidate must pass all 20. Fresh native/shadow checker runs cover the emitter and primitive-key fixtures. Four fresh primitive-key executions per arm match complete Node output; the native checker finding described above remains visible. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Stringify option controls

Window: 2026-09-11T13:32:19Z to 2026-09-11T13:33:22Z.

| Fixture / operation | R26 µs | R34 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.044795 | 0.044778 | 0.108266 | 0.119603 | -0.04% | overlap |
| small_record / dynamic-zero | 0.044594 | 0.044473 | 0.247560 | 0.394214 | -0.27% | overlap |
| small_record / zero | 0.044777 | 0.044922 | 0.247431 | 0.394248 | +0.32% | overlap |
| small_record / pretty | 0.485095 | 0.455252 | 0.303152 | 0.525216 | -6.15% | gain |
| small_record / keys | 0.461155 | 0.450662 | 0.599621 | 0.484074 | -2.28% | gain |
| small_record / callback | 0.946105 | 0.894405 | 0.605030 | 0.581485 | -5.46% | gain |
| records_array_16k / pretty | 46.343000 | 42.450000 | 33.556000 | 45.863000 | -8.40% | gain |

| Fixture / operation | R26 MiB | R34 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.391 | 33.391 | 59.625 | 378.578 | +0.000 |
| small_record / dynamic-zero | 33.406 | 33.406 | 59.656 | 378.641 | +0.000 |
| small_record / zero | 33.391 | 33.406 | 59.641 | 378.641 | +0.016 |
| small_record / pretty | 33.750 | 33.750 | 59.641 | 239.391 | +0.000 |
| small_record / keys | 33.719 | 33.719 | 59.750 | 207.781 | +0.000 |
| small_record / callback | 32.469 | 32.438 | 59.688 | 97.750 | -0.031 |
| records_array_16k / pretty | 35.062 | 35.062 | 57.016 | 49.750 | +0.000 |

### Eight rotating inputs per fixture

Window: 2026-09-11T13:30:01Z to 2026-09-11T13:31:45Z.

| Fixture / operation | R26 µs | R34 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494591 | 0.505301 | 0.343994 | 0.255497 | +2.17% | regression |
| small_record / same | 0.099162 | 0.099120 | 0.353378 | 0.246568 | -0.04% | overlap |
| small_record / select | 0.009435 | 0.009435 | 0.002658 | 0.003615 | +0.01% | overlap |
| records_array_1m / rotating | 942.729885 | 1085.356322 | 2791.143678 | 2160.833333 | +15.13% | regression |
| records_array_1m / same | 939.734104 | 1081.982659 | 2707.901734 | 2150.716763 | +15.14% | regression |
| records_array_1m / select | 0.011340 | 0.011345 | 0.003125 | 0.003854 | +0.05% | overlap |
| long_string_1m / rotating | 99.858145 | 127.211224 | 372.831645 | 69.964147 | +27.39% | regression |
| long_string_1m / same | 0.358584 | 0.361892 | 368.846179 | 65.976844 | +0.92% | overlap |
| long_string_1m / select | 0.012006 | 0.011506 | 0.003085 | 0.003880 | -4.16% | overlap |
| escaped_1m / rotating | 1011.949686 | 1012.157233 | 1725.433962 | 2077.710692 | +0.02% | overlap |
| escaped_1m / same | 994.881250 | 994.581250 | 1723.187500 | 2074.225000 | -0.03% | overlap |
| escaped_1m / select | 0.011529 | 0.011553 | 0.003119 | 0.003847 | +0.22% | overlap |
| unicode_1m / rotating | 141.394130 | 164.756115 | 439.440950 | 62.201258 | +16.52% | regression |
| unicode_1m / same | 0.357700 | 0.361249 | 438.146913 | 59.415543 | +0.99% | regression |
| unicode_1m / select | 0.011319 | 0.011281 | 0.003126 | 0.003885 | -0.34% | overlap |

| Fixture / operation | R26 MiB | R34 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.969 | 59.969 | 80.203 | -0.016 |
| small_record / same | 75.156 | 75.156 | 59.594 | 79.875 | +0.000 |
| small_record / select | 12.812 | 12.719 | 57.812 | 35.281 | -0.094 |
| records_array_1m / rotating | 76.297 | 76.250 | 128.875 | 100.516 | -0.047 |
| records_array_1m / same | 76.297 | 76.250 | 128.797 | 98.328 | -0.047 |
| records_array_1m / select | 22.594 | 22.500 | 67.609 | 42.500 | -0.094 |
| long_string_1m / rotating | 90.234 | 90.188 | 176.562 | 148.438 | -0.047 |
| long_string_1m / same | 24.594 | 24.531 | 200.969 | 152.531 | -0.062 |
| long_string_1m / select | 24.328 | 24.234 | 68.984 | 43.781 | -0.094 |
| escaped_1m / rotating | 65.000 | 64.984 | 89.641 | 77.750 | -0.016 |
| escaped_1m / same | 65.000 | 64.984 | 89.656 | 77.766 | -0.016 |
| escaped_1m / select | 23.531 | 23.438 | 68.375 | 43.266 | -0.094 |
| unicode_1m / rotating | 62.375 | 62.359 | 160.438 | 124.984 | -0.016 |
| unicode_1m / same | 22.703 | 22.641 | 220.703 | 147.312 | -0.062 |
| unicode_1m / select | 22.438 | 22.344 | 68.547 | 44.672 | -0.094 |

## Build fingerprints

| Artifact | R26 SHA-256 | R34 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `4240fae9b885c292170c388dd65099c0b7ed5706d7960cb6f73cc72b1a32ce5f` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `9bfe0ff0be8650b3aca5451c8d97d02feb9e5b9dac86849a7af1c8e9db38ed17` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `718c7b660e04f23aa63ea2507d20176f47eeb6c7eb8af9d77290dc22ea4d368a` |

The artifact index records both terminal raw windows, exact CPU/peak-RSS vectors, validation receipts, source patches and failed validation attempts. Both remote controllers exited zero and archived their terminal windows before any subsequent remote action. Median peak-RSS differences across the 22 comparisons ranged from −96 KiB to +16 KiB; these process-level measurements do not establish retained live-heap behavior. The candidate compiler and both static archives were built in a separate clean worktree; the copy receipt identifies the exact source and hashes. An independent R32 access profile is included with its own source and hashes: it is diagnostic evidence for the next investigation, not R34 timing. R35 is a subsequent unmeasured scanner proposal. No ready PR or merged-main measurement is claimed for R34.
