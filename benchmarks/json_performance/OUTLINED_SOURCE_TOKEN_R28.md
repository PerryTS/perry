# Outline source-length construction from small JSON tokens: R28

Not promoted. The first R28 screen reproduces a separated small-record parse regression: +1.817% (494.702 to 503.691 ns), all 11 paired repetitions slower. Same-input parse (+0.024%) and selection (-0.042%) overlap. Peak process RSS medians change by -80 KiB, -64 KiB and -48 KiB respectively; these are footprint observations, not evidence of lower allocation volume.

The helper reduced parse_string_value from R27's 307 static instructions to 255, still above R26's 239. This did not repair the regression; static instruction count does not establish the cause. R27's large-token gains cannot be assigned to this different build. The other 15-case rotating screen, full 50-case matrix, options, access and retained-output timing controls were not run because the targeted small-object repair failed.

One quiet terminal window was archived first: 132 timed trials, 20 complete-output verification records and 12 calibration trials. This source remains an experimental branch with no PR. The next parser attempt must isolate large-token work from the small-object path.

## Scope and reference

A large unescaped string token can derive its UTF-16 length from the exact rooted input string when at most 256 surrounding bytes are ASCII. Payload pointer and byte length must both match. A conservative final-byte check rejects sequences that could cross the token boundary under the existing bounded WTF-8 counter. Escaped tokens and every failed proof retain their original counter/allocator. The known-length path uses the identical allocation body, flags, padding, copy and malloc-output accounting. The proof and allocation arm are outlined so small-token parsing does not carry their full instruction footprint. No GC policy, parse-boundary, cache admission or threshold change.

Measured source `7d21ef6be54aabb11836fcfc58cfcd9030fa4ee0` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`, both workspace 0.5.1531. The harness calls the reference main/baseline; it is R26, not current main. Earlier implementation is in ready PRs #10050/#10052/#10064 with separate release metadata. This is not a merged-main measurement.

## Validation

298 serial release JSON unit tests pass. The normal production compiler, runtime-static and stdlib-static build completed and all three immutable artifacts have verified SHA-256 hashes and post-start mtimes. All 90 fresh candidate behavior/options checks match the exact frozen R26 reference; all 90 reference receipts are reused. Token, lifetime and scheduled-GC checks assert positive copying and protected retired sets. All 26 corresponding native/shadow IR files and six worker objects agree under the documented normalization. Original native checker findings and known lazy/getter/fraction failure outcomes remain unsuppressed.

73 of 74 executed script lint gates pass; the public benchmark freshness gate fails. File-size cap passes. Compile-tier and two CI-only gates were skipped by the lint controller. This is not an all-CI-green claim.

All 90 reference behavior/options receipts are reused from the exact R26 build, including the nine source-token cases first run for R27. All candidate executions are fresh. The original 18 checker verdicts are reused only after exact emitted-IR and checker-source equivalence. Candidate token native/shadow checker runs are fresh; exact R26 reference verdicts are reused from R27. Existing findings remain unsuppressed.

## Measurement method

Quiet M1/8 GiB host, Node 26.5.1, Bun 1.3.14; fresh interleaved processes. CPU is user+system per loop iteration and peak RSS covers the entire process. Terminal windows are archived first. Repeated-input parsing includes existing caches and lazy construction; rotating inputs and consumption cases provide separate controls. Overlapping observed ranges do not establish equivalence.

### Independent small-record recheck (11 repetitions)

Window: 2026-09-11T11:06:37Z to 2026-09-11T11:07:07Z.

| Fixture / operation | R26 µs | R28 µs | Node µs | Bun µs | CPU change | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494702 | 0.503691 | 0.353753 | 0.255157 | +1.82% | regression |
| small_record / same | 0.099158 | 0.099182 | 0.342463 | 0.245248 | +0.02% | overlap |
| small_record / select | 0.009430 | 0.009427 | 0.002661 | 0.003612 | -0.04% | overlap |

| Fixture / operation | R26 MiB | R28 MiB | Node MiB | Bun MiB | Peak RSS change MiB |
|---|---:|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.906 | 59.922 | 80.172 | -0.078 |
| small_record / same | 75.969 | 75.906 | 59.625 | 79.859 | -0.062 |
| small_record / select | 12.812 | 12.766 | 57.859 | 35.250 | -0.047 |

## Build fingerprints

| Artifact | R26 SHA-256 | R28 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `0642e6dd0560283b5edee2abd8a570a30dbc42f5eaa487ba8bc9e8e5aa34490e` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `4c70301fe1ca78df73f8db793a2f868290a2865a3ca6eb37651ae6db5c82008a` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `cac360e31577ee6c6625751693ed7c14f91a05928c1dfc360acbedba9d852084` |

The artifact index records archived raw windows, exact sample vectors, validation receipts, source patches and failed attempts. Independent R26 stringify profiles and future scanner/emitter proposals, if included, are diagnostic/proposal evidence and are not R28 performance results.
