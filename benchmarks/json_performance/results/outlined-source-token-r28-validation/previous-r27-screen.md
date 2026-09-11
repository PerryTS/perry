# Reuse source UTF-16 length for large JSON tokens: R27

The first fresh-input comparison reduces Unicode parse CPU 47.45%, from 141.715 to 74.476 microseconds. Perry uses 1.17 times Bun CPU, down from about 2.25 times; Node uses 439.660 microseconds. Fresh large ASCII parse improves 12.22%, from 99.836 to 87.635 microseconds, remaining 1.25 times Bun CPU. Fresh 1 MB record-array parse improves 1.84%; the length shortcut is not eligible for those small per-record tokens, so that change must not be attributed directly to avoided UTF-16 counting. Escaped parsing is unchanged within overlapping ranges. Rotating peak RSS medians fall 64–96 KiB on the large cases; no intermediate allocation is added.

This candidate is not promoted for landing. Fresh small-record parsing regresses 0.58% with separated observed ranges, then 0.66% in an independent 11-repetition recheck (11/11 trials slower), approximately 3.3 ns per parse. Same-input and selection controls overlap. The parse_string_value function grows from 239 to 307 static instructions; this suggests a follow-up to outline the large-token proof/allocation arm, but does not prove the regression's cause. The follow-up patch is retained as a proposal outside production.

Both quiet windows are fully archived and verified: 552 timed trials, 120 full-output verification records and 72 calibration trials. The 50-case full matrix, access, options and retained-output performance controls have not been run for R27: broader qualification is deferred while the reproduced small-record regression is addressed. The new correctness/lifetime tests and the complete existing validation corpus have run. No claim is made that general object workloads or all JSON gaps are solved, and these are controlled R26 comparisons rather than merged-main measurements.

## Scope and reference

A large unescaped string token can derive its UTF-16 length from the exact rooted input string when at most 256 surrounding bytes are ASCII. Payload pointer and byte length must both match. A conservative final-byte check rejects sequences that could cross the token boundary under the existing bounded WTF-8 counter. Escaped tokens and every failed proof retain their original counter/allocator. The known-length path uses the identical allocation body, flags, padding, copy and malloc-output accounting. No GC policy, parse-boundary, cache admission or threshold change.

Measured source `decc2d9eed2de5713b800ddaf77ffc75c78fdd0c` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is in ready PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

298 serial release JSON tests pass, including actual source-pointer identity, ASCII-boundary limits, malformed/WTF-8 fallback, and header/byte agreement for small and megabyte tokens. The exact all-three production build completed in 841.162 seconds with frozen hashes and post-start mtimes verified. All 90 candidate behavior/options checks pass freshly; 81 original reference receipts are reused from R26 and nine new token cases run freshly on the reference. Every scheduled subject has positive moved-object/protected-retired-set counters. New token scheduled runs moved 42,408 objects with 2,192 protected sets in auto/direct mode, and 35,925 objects with 2,175 protected sets in tape mode, identically on both arms. All 26 native/shadow IR files and six worker objects match; native normalizes only the ModuleID path comment. New token and zero/retained native checks pass, and all shadow checks pass. The original corpus retains sixteen unsuppressed native findings and changing-record retains four; these baseline matches are not clean native passes. Known lazy stringify crashes/noncanonical output, getter and fractional-spacing differences remain preserved baseline outcomes. Script lint passes 73/74 executed checks; only the existing public benchmark freshness gate fails. File cap passes; compile-tier and CI-only checks remain skipped. No GC policy, parse-boundary or cache-admission changes.

The original 81 reference behavior/options receipts are reused from the exact R26 build; candidate executions are fresh. Nine added source-token cases run freshly on both arms. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence. Added token native/shadow checker runs are fresh on both arms. Existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Eight rotating inputs per fixture

Window: 2026-09-11T10:21:53Z to 2026-09-11T10:23:36Z.

| Fixture / operation | R26 µs | R27 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494515 | 0.497403 | 0.350480 | 0.255634 | +0.58% | regression |
| small_record / same | 0.099111 | 0.099098 | 0.340691 | 0.245468 | -0.01% | overlap |
| small_record / select | 0.009430 | 0.009454 | 0.002665 | 0.003629 | +0.25% | overlap |
| records_array_1m / rotating | 942.384181 | 925.056497 | 2704.237288 | 2156.005650 | -1.84% | gain |
| records_array_1m / same | 937.355932 | 921.870056 | 2661.734463 | 2160.615819 | -1.65% | gain |
| records_array_1m / select | 0.011465 | 0.011565 | 0.003145 | 0.003863 | +0.87% | overlap |
| long_string_1m / rotating | 99.835878 | 87.635115 | 372.705344 | 70.307634 | -12.22% | gain |
| long_string_1m / same | 0.361586 | 0.360296 | 376.619078 | 66.010635 | -0.36% | overlap |
| long_string_1m / select | 0.011671 | 0.011475 | 0.003110 | 0.003893 | -1.68% | overlap |
| escaped_1m / rotating | 1012.396226 | 1012.402516 | 1724.981132 | 2077.798742 | +0.00% | overlap |
| escaped_1m / same | 994.345912 | 994.270440 | 1724.591195 | 2074.194969 | -0.01% | overlap |
| escaped_1m / select | 0.011560 | 0.011298 | 0.003135 | 0.003882 | -2.26% | overlap |
| unicode_1m / rotating | 141.714900 | 74.476361 | 439.659742 | 63.494269 | -47.45% | gain |
| unicode_1m / same | 0.357738 | 0.356289 | 436.942370 | 59.732512 | -0.41% | overlap |
| unicode_1m / select | 0.011425 | 0.011553 | 0.003153 | 0.003903 | +1.12% | overlap |

| Fixture / operation | R26 MiB | R27 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.906 | 59.938 | 80.172 | -0.078 |
| small_record / same | 75.969 | 75.906 | 59.672 | 79.844 | -0.062 |
| small_record / select | 12.812 | 12.766 | 57.859 | 35.250 | -0.047 |
| records_array_1m / rotating | 76.297 | 76.234 | 128.859 | 100.234 | -0.062 |
| records_array_1m / same | 76.297 | 76.234 | 128.891 | 101.781 | -0.062 |
| records_array_1m / select | 22.594 | 22.547 | 67.641 | 42.484 | -0.047 |
| long_string_1m / rotating | 90.203 | 90.141 | 177.562 | 152.516 | -0.062 |
| long_string_1m / same | 24.594 | 24.547 | 204.391 | 150.562 | -0.047 |
| long_string_1m / select | 24.312 | 24.281 | 69.047 | 43.750 | -0.031 |
| escaped_1m / rotating | 65.000 | 64.938 | 89.672 | 77.734 | -0.062 |
| escaped_1m / same | 65.000 | 64.938 | 89.656 | 77.766 | -0.062 |
| escaped_1m / select | 23.531 | 23.484 | 68.453 | 43.266 | -0.047 |
| unicode_1m / rotating | 61.562 | 61.469 | 159.531 | 153.422 | -0.094 |
| unicode_1m / same | 22.703 | 22.641 | 199.875 | 149.969 | -0.062 |
| unicode_1m / select | 22.438 | 22.391 | 68.625 | 44.625 | -0.047 |

### Independent small-record recheck (11 repetitions)

Window: 2026-09-11T10:25:19Z to 2026-09-11T10:25:47Z.

| Fixture / operation | R26 µs | R27 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494494 | 0.497739 | 0.346575 | 0.256591 | +0.66% | regression |
| small_record / same | 0.099561 | 0.099719 | 0.328108 | 0.247129 | +0.16% | overlap |
| small_record / select | 0.009436 | 0.009435 | 0.002669 | 0.003638 | -0.01% | overlap |

| Fixture / operation | R26 MiB | R27 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.906 | 59.938 | 80.156 | -0.078 |
| small_record / same | 78.438 | 78.375 | 59.672 | 79.844 | -0.062 |
| small_record / select | 12.812 | 12.766 | 57.828 | 35.250 | -0.047 |

## Build fingerprints

| Artifact | R26 SHA-256 | R27 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `458a93852da9d3528c0aa12fd51a494430157ba3ff0d054f05d4219ca35ebe44` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `566a2d127f8c48c77eb93a97b4b2abb3f85c43fca88722dceff4bea753c6a9c6` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `45112e80d77f874c3bbcfbd14f055a75e27927111e40c7ad902c4612017489d2` |

The artifact index records archived raw windows, exact sample vectors, validation receipts, source patches and failed attempts. Independent R26 stringify profiles and future scanner/emitter proposals, if included, are diagnostic/proposal evidence and are not R27 performance results.
