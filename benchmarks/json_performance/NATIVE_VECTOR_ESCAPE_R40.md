# Native vector escaping: R40

R40 is not promoted. Dense escaped pretty printing takes 1,775.855 µs versus 897.777 µs in its immediate parent (+97.81%); the replacer case takes 1,793.242 µs versus 924.234 µs (+94.02%). R40 remains about 3.16×/3.14× Bun on those cases. The ASCII and Unicode controls remain much faster than Node and Bun. Peak RSS changes versus the parent are between −16 and +16 KiB.

These are two separately qualified quiet windows, each with seven interleaved R26/Perry/Node/Bun repetitions per case. R39 and R40 were not interleaved against each other. Each window has 168 timed trials and 30 complete-output verification records: 336 timed trials and 60 verifications in total. The six parent measurements are new reference observations for this investigation, additional to the earlier R39 evidence commit.

## Implementation and validation

The candidate adds a bounded ARM vector writer to the existing native stringify buffer path. UTF-8 validation, expansion planning, capacity growth, short-string dispatch and the WTF-8 fallback stay in place. It classifies sixteen source bytes, then emits one escape before continuing. Repeated classification of overlapping blocks is a candidate explanation for the slowdown, not a qualified sampling attribution. No GC policy, threshold, parse boundary, cache admission or managed intermediate allocation changes.

Source 3bde9394e021039603c84801762af4f083b1f9c4 passes 312 serial release JSON tests and 385 string-filter tests (overlapping filters). The guard-page test covers 4,491 source/output cases. All 125 canonical candidate full-output checks pass with positive moving/protected witnesses where required; 30 normalized native/shadow IR files and six worker objects match frozen R26. Known native findings and reference behavioral failures remain explicit. Original-suite checker verdicts are reused from the ten fresh R39 commands only after exact IR, full scripts-tree, source and log hashes match. Supplementary alternate-worktree validation passed 20 large-emitter cases but refused exact IR equality because embedded source paths and anonymous-shape IDs differ; its full logs are preserved. Canonical validation reran those cases at the original source path and passed. Normal three-package production build completed in 390.70197796821594 seconds. Final lint 72/74: public baseline freshness and inherited raw-handle ceiling vs updated main remain; implementation audits, formatting and file cap pass.

The original native finding set, the primitive-key finding, and the large-emitter non-moving string-handle/overflow-store finding remain unsuppressed. Shadow checks and ordinary/callback native checks pass. R26 reference emitter output and moving-GC failures remain explicit; candidate output checks pass. No full, rotating, Korean, changing-object, retained-output or access performance window ran for R40. The seven R39 full-screen regressions are inherited concerns and are not remeasured here.

## Immediate parent comparison

CPU is user plus system time per loop iteration. Peak RSS covers the complete process, including startup. The reference is frozen R26, not current main. Source commits: R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`; parent R39 `e69d1292110cf9abee24f8690c584652805a4167`; candidate R40 `3bde9394e021039603c84801762af4f083b1f9c4`.

| Fixture / operation | R39 µs | R40 µs | Change | R39 peak MiB | R40 peak MiB |
|---|---:|---:|---:|---:|---:|
| long_string_1m / pretty | 87.199219 | 87.687500 | +0.56% | 55.672 | 55.672 |
| long_string_1m / callback | 114.898438 | 114.640625 | -0.22% | 55.781 | 55.766 |
| unicode_1m / pretty | 134.039062 | 133.332031 | -0.53% | 55.109 | 55.094 |
| unicode_1m / callback | 160.210938 | 159.718750 | -0.31% | 55.219 | 55.203 |
| escaped_1m / pretty | 897.777344 | 1775.855469 | +97.81% | 55.562 | 55.578 |
| escaped_1m / callback | 924.234375 | 1793.242188 | +94.02% | 55.641 | 55.625 |

## R40 candidate

Window: 2026-09-11T17:04:29Z to 2026-09-11T17:04:57Z.

| Fixture / operation | R26 µs | Perry µs | Node µs | Bun µs |
|---|---:|---:|---:|---:|
| long_string_1m / pretty | 128.125000 | 87.687500 | 415.250000 | 535.851562 |
| long_string_1m / callback | 155.460938 | 114.640625 | 443.062500 | 546.476562 |
| unicode_1m / pretty | 168.191406 | 133.332031 | 560.609375 | 445.773438 |
| unicode_1m / callback | 196.125000 | 159.718750 | 586.968750 | 455.000000 |
| escaped_1m / pretty | 1793.000000 | 1775.855469 | 1207.980469 | 561.664062 |
| escaped_1m / callback | 1812.242188 | 1793.242188 | 1233.367188 | 571.257812 |

| Fixture / operation | R26 peak MiB | Perry peak MiB | Node peak MiB | Bun peak MiB |
|---|---:|---:|---:|---:|
| long_string_1m / pretty | 55.734 | 55.672 | 89.797 | 87.234 |
| long_string_1m / callback | 55.812 | 55.766 | 89.031 | 75.875 |
| unicode_1m / pretty | 55.141 | 55.094 | 86.766 | 82.766 |
| unicode_1m / callback | 55.250 | 55.203 | 80.375 | 72.828 |
| escaped_1m / pretty | 59.047 | 55.578 | 87.406 | 83.625 |
| escaped_1m / callback | 59.172 | 55.625 | 81.812 | 74.906 |

## R39 immediate parent

Window: 2026-09-11T17:07:17Z to 2026-09-11T17:07:43Z.

| Fixture / operation | R26 µs | Perry µs | Node µs | Bun µs |
|---|---:|---:|---:|---:|
| long_string_1m / pretty | 127.773438 | 87.199219 | 415.691406 | 536.125000 |
| long_string_1m / callback | 156.406250 | 114.898438 | 442.007812 | 546.718750 |
| unicode_1m / pretty | 168.515625 | 134.039062 | 561.011719 | 446.371094 |
| unicode_1m / callback | 196.492188 | 160.210938 | 586.367188 | 455.242188 |
| escaped_1m / pretty | 1793.210938 | 897.777344 | 1208.500000 | 561.597656 |
| escaped_1m / callback | 1809.351562 | 924.234375 | 1233.968750 | 570.203125 |

| Fixture / operation | R26 peak MiB | Perry peak MiB | Node peak MiB | Bun peak MiB |
|---|---:|---:|---:|---:|
| long_string_1m / pretty | 55.703 | 55.672 | 89.875 | 87.203 |
| long_string_1m / callback | 55.812 | 55.781 | 88.984 | 75.875 |
| unicode_1m / pretty | 55.141 | 55.109 | 86.750 | 82.797 |
| unicode_1m / callback | 55.219 | 55.219 | 80.328 | 72.812 |
| escaped_1m / pretty | 59.047 | 55.562 | 87.328 | 83.625 |
| escaped_1m / callback | 59.203 | 55.641 | 81.766 | 74.859 |

## Artifact provenance

The index includes both benchmark windows with their distinct measured source commits, original commands, verification records, complete CPU/RSS vectors and frozen artifact hashes. The supplementary alternate-worktree refusal is separate from the successful canonical run. Standalone correctness includes 509 full outputs matching the pinned Node corpus and is not timing evidence. Every benchmark window was archived before further remote work. This branch is experimental evidence, not a ready PR or a merged-main result.
