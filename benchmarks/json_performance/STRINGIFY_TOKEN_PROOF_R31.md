# Root callback keys and maintain string escape proofs: R31

**Not promoted.** Large ASCII/Unicode pretty and callback stringify improve19–32%, and pretty printing improves5.7% for small objects and7.8% for16KB records. However small callback stringify regresses1.24% initially and1.45% in an independent11-repetition run. Fresh rotating Unicode parsing regresses22.72%. The extra UTF-8 proof pass is therefore rejected in this form. Small key-list+0.42% separated initially becomes+0.38% overlapping in the recheck; small plain+0.29% becomes+0.07% overlapping. Fresh small parse+0.64% overlaps but remains an adverse trend. No full50, changing-object, access or retained-memory performance run was executed for this unqualified source.

## Scope and reference

The object replacer walk keeps one rewritable key handle across getters, toJSON and replacer callbacks. It reloads key bytes only after callbacks and scopes replacer-pointer use at each call. The same slot is reused for each property. Pretty printing and replacer scalar output now use the existing heap-string provenance and inline-short-string emitters. Concatenation and in-place appends clear escape-free provenance when strings gain arbitrary bytes, while preserving lone-surrogate metadata. Escaped strings retain their existing writer. Borrowed non-ASCII JSON strings must also pass UTF-8 validation before receiving the escape-free flag; valid strings use the existing vector UTF-16 counter, while raw surrogate tokens use the existing normalizing builder. ASCII construction, GC policy, parse boundaries, cache admission and thresholds are unchanged.

Measured source `f0bc3aeb6a2597a941ad4afe1b70e79e2df11a17` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is represented by PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

299 serial release JSON tests and115 string tests pass; these suites overlap. All81 existing candidate behavior/options/GC checks and20 expanded emitter executions pass, including raw unescaped lone surrogates. All20 emitter reference executions are fresh: six tape controls pass and14auto/direct output or callback failures are preserved. Positive moving and protected collections are observed in scheduled arms. Twenty-six emitted IR files match and six worker objects match. Existing native root findings and lazy/getter/fraction limitations remain explicitly unsuppressed. Normal all-three-package production build completed in384.331s from clean committed f0bc3aeb. Lint73/74 executed gates pass; public benchmark freshness fails; filecap passes; compile-tier and CI-only gates skipped. Performance disqualifies this candidate.

All 81 original reference behavior/options receipts are reused from the exact R26 build. All 81 original candidate executions are fresh. The new emitter regression adds 20 fresh executions per arm: the reference has fourteen output/callback failures, while the candidate must pass all 20. Fresh native/shadow checker runs cover the new fixture. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence; existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Stringify option controls

Window: 2026-09-11T12:29:37Z to 2026-09-11T12:30:41Z.

| Fixture / operation | R26 µs | R31 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.044799 | 0.044927 | 0.108837 | 0.119513 | +0.29% | overlap |
| small_record / dynamic-zero | 0.044541 | 0.044590 | 0.247852 | 0.393763 | +0.11% | overlap |
| small_record / zero | 0.044832 | 0.044926 | 0.247648 | 0.393700 | +0.21% | overlap |
| small_record / pretty | 0.484918 | 0.457429 | 0.303144 | 0.525431 | -5.67% | gain |
| small_record / keys | 0.461194 | 0.463133 | 0.594485 | 0.484725 | +0.42% | regression |
| small_record / callback | 0.947840 | 0.959590 | 0.608120 | 0.580935 | +1.24% | regression |
| records_array_16k / pretty | 46.312000 | 42.682000 | 33.490000 | 45.872000 | -7.84% | gain |

| Fixture / operation | R26 MiB | R31 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.391 | 33.391 | 59.609 | 378.547 | +0.000 |
| small_record / dynamic-zero | 33.406 | 33.375 | 59.594 | 378.625 | -0.031 |
| small_record / zero | 33.422 | 33.391 | 59.609 | 378.641 | -0.031 |
| small_record / pretty | 33.766 | 33.734 | 59.656 | 239.359 | -0.031 |
| small_record / keys | 33.719 | 33.703 | 59.672 | 207.781 | -0.016 |
| small_record / callback | 32.500 | 32.469 | 59.594 | 97.688 | -0.031 |
| records_array_16k / pretty | 35.062 | 35.062 | 57.031 | 49.734 | +0.000 |

### Megabyte pretty printing and replacers

Window: 2026-09-11T12:31:46Z to 2026-09-11T12:32:13Z.

| Fixture / operation | R26 µs | R31 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| long_string_1m / pretty | 127.707031 | 86.582031 | 415.589844 | 536.335938 | -32.20% | gain |
| long_string_1m / callback | 156.812500 | 115.242188 | 442.648438 | 546.515625 | -26.51% | gain |
| unicode_1m / pretty | 168.363281 | 133.402344 | 560.597656 | 446.363281 | -20.77% | gain |
| unicode_1m / callback | 196.343750 | 159.718750 | 586.656250 | 455.023438 | -18.65% | gain |
| escaped_1m / pretty | 1793.320312 | 1783.718750 | 1207.320312 | 561.558594 | -0.54% | overlap |
| escaped_1m / callback | 1811.179688 | 1810.679688 | 1234.960938 | 571.117188 | -0.03% | overlap |

| Fixture / operation | R26 MiB | R31 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| long_string_1m / pretty | 55.734 | 55.672 | 89.812 | 87.203 | -0.062 |
| long_string_1m / callback | 55.812 | 55.781 | 88.984 | 75.875 | -0.031 |
| unicode_1m / pretty | 55.141 | 55.125 | 86.734 | 82.766 | -0.016 |
| unicode_1m / callback | 55.250 | 55.203 | 80.375 | 72.781 | -0.047 |
| escaped_1m / pretty | 59.078 | 59.016 | 87.406 | 83.609 | -0.062 |
| escaped_1m / callback | 59.172 | 59.156 | 81.797 | 74.891 | -0.016 |

### Independent small option recheck

Window: 2026-09-11T12:32:42Z to 2026-09-11T12:35:10Z.

| Fixture / operation | R26 µs | R31 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.041226 | 0.041255 | 0.107888 | 0.118196 | +0.07% | overlap |
| small_record / keys | 0.457803 | 0.459545 | 0.598065 | 0.482752 | +0.38% | overlap |
| small_record / callback | 0.910897 | 0.924115 | 0.598685 | 0.561039 | +1.45% | regression |

| Fixture / operation | R26 MiB | R31 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / plain | 33.391 | 33.375 | 59.734 | 394.484 | -0.016 |
| small_record / keys | 33.750 | 33.719 | 59.734 | 378.500 | -0.031 |
| small_record / callback | 33.469 | 33.484 | 59.688 | 98.828 | +0.016 |

### Eight rotating inputs per fixture

Window: 2026-09-11T12:36:59Z to 2026-09-11T12:38:42Z.

| Fixture / operation | R26 µs | R31 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494391 | 0.497536 | 0.340087 | 0.255038 | +0.64% | overlap |
| small_record / same | 0.099095 | 0.099139 | 0.344073 | 0.246330 | +0.04% | overlap |
| small_record / select | 0.009430 | 0.009428 | 0.002658 | 0.003620 | -0.02% | overlap |
| records_array_1m / rotating | 942.436782 | 942.373563 | 2667.310345 | 2161.040230 | -0.01% | overlap |
| records_array_1m / same | 939.896552 | 939.804598 | 2718.798851 | 2151.879310 | -0.01% | overlap |
| records_array_1m / select | 0.011409 | 0.011459 | 0.003108 | 0.003877 | +0.44% | overlap |
| long_string_1m / rotating | 100.179365 | 100.095238 | 373.348413 | 70.100000 | -0.08% | overlap |
| long_string_1m / same | 0.362795 | 0.359285 | 374.009892 | 65.831844 | -0.97% | overlap |
| long_string_1m / select | 0.012008 | 0.011353 | 0.003092 | 0.003840 | -5.45% | overlap |
| escaped_1m / rotating | 1012.207547 | 1012.704403 | 1727.062893 | 2078.081761 | +0.05% | overlap |
| escaped_1m / same | 994.931250 | 993.525000 | 1723.593750 | 2073.731250 | -0.14% | overlap |
| escaped_1m / select | 0.011340 | 0.011529 | 0.003118 | 0.003875 | +1.67% | overlap |
| unicode_1m / rotating | 141.929478 | 174.169958 | 439.717207 | 63.188293 | +22.72% | regression |
| unicode_1m / same | 0.358653 | 0.358295 | 437.026872 | 58.883913 | -0.10% | overlap |
| unicode_1m / select | 0.011376 | 0.011437 | 0.003108 | 0.003881 | +0.53% | overlap |

| Fixture / operation | R26 MiB | R31 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.875 | 59.891 | 80.188 | -0.109 |
| small_record / same | 75.969 | 75.891 | 59.641 | 79.875 | -0.078 |
| small_record / select | 12.812 | 12.766 | 57.891 | 35.266 | -0.047 |
| records_array_1m / rotating | 76.297 | 76.219 | 128.844 | 100.594 | -0.078 |
| records_array_1m / same | 76.297 | 76.219 | 128.844 | 99.969 | -0.078 |
| records_array_1m / select | 22.594 | 22.516 | 67.609 | 42.516 | -0.078 |
| long_string_1m / rotating | 90.234 | 90.141 | 176.703 | 145.500 | -0.094 |
| long_string_1m / same | 24.609 | 24.531 | 197.766 | 150.516 | -0.078 |
| long_string_1m / select | 24.312 | 24.250 | 69.031 | 43.750 | -0.062 |
| escaped_1m / rotating | 65.000 | 64.922 | 89.656 | 77.781 | -0.078 |
| escaped_1m / same | 65.000 | 64.922 | 89.656 | 77.750 | -0.078 |
| escaped_1m / select | 23.531 | 23.438 | 68.438 | 43.281 | -0.094 |
| unicode_1m / rotating | 62.391 | 62.375 | 160.469 | 147.219 | -0.016 |
| unicode_1m / same | 22.703 | 22.672 | 201.234 | 131.156 | -0.031 |
| unicode_1m / select | 22.438 | 22.344 | 68.594 | 44.641 | -0.094 |

## Build fingerprints

| Artifact | R26 SHA-256 | R31 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `790de8abe33c58d2f1e192341d64c11a2c35520c04b0e46828562dfdc5918f4a` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `aaa94cec544e30b427aac800e98a9a5e4eab212cee5fd178565c9cbb9d3034f0` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `649f02e7c750f04a338e9c5a95e03e82227913331d449d8ce82555d291dc97a7` |

The artifact index records archived raw windows, exact sample vectors, validation receipts, source patches and failed attempts. The small-options recheck remote measurement completed and was archived first, but its local controller then failed copying a misnamed local script. Exit1, the failure log, exact controller and reconstruction proof are preserved; the analyzer accepts only the archived remote completion proof and validates every sample. No remeasurement or fabricated exit0 was used. The candidate compiler and both static archives were built in a separate clean worktree; the copy receipt identifies the exact source and hashes. Prior independent profiles and future parser proposals are not R31 timing results.
