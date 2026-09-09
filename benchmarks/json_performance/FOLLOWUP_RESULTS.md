# JSON construction and reclamation follow-up

Latest: [fresh merged-main 0.5.1529 measurements](MERGED_MAIN_EEE.md), after
[the escaped-record correction](ESCAPED_RECORD_CORRECTION.md) landed through
#10035, and [the opening-scan follow-up](OPENING_SCAN.md) in PR #10036.
The earlier measurements below retain their original revisions and provenance.

The 0.5.1528 release implementation makes record-array traversal 30–78% faster
and fixes discarded large parsed strings accumulating beyond a GiB. Construction
uses the existing batch and suppression scope. Completed outputs publish
reclamation requests; collection runs at later rooted boundaries. No background
GC thread, output-identity shortcut, or new GC environment knob is introduced.

The release binaries completed the original and rotating-input matrices
on the Apple M1 / 8 GiB benchmark Mac, against an interleaved immutable PR #10022
merge (`c82e613d260a80324914b5aba503a31a763ad90e`), Node 26.5.1, and Bun 1.3.14.
All engines perform identical work within each comparison. These are five-trial
medians from fresh processes; quiet admission passed before and after each run.

## Measured gains

| Workload | Reference CPU µs | Candidate CPU µs | CPU reduction | Reference peak MiB | Candidate peak MiB |
|---|---:|---:|---:|---:|---:|
| 13 KiB record-array scan (`records_array_16k`) | 291.945 | 63.228 | 78.3% | 440.953 | 207.391 |
| 1 MiB record-array scan | 4,613.855 | 3,232.420 | 29.9% | 160.328 | 158.313 |
| 8 MiB record-array scan | 36,481.625 | 24,987.125 | 31.5% | 192.266 | 186.563 |
| Rotating 1 MiB ASCII-string parse | 152.577 | 107.969 | 29.2% | 1,352.969 | 90.297 |
| Rotating Unicode-string parse | 185.748 | 148.443 | 20.1% | 1,189.656 | 62.438 |

Rotating input means eight preloaded equal-size, same-shape strings whose
contents differ in one value. It defeats the single-source parse caches without
timing file reads or allocating a fresh input string on every call. The worker
retains the eight inputs and only its last result. This is a bounded corpus,
not a claim about every cold or changing object workload. Stringify is measured
separately on already-materialized inputs in the original matrix.

## Regressions and correctness

The original matrix passed all 200 output checks and validated 344 measurement
groups. Two large-stringify CPU medians rose by 3.9% (ASCII) and 3.6% (Unicode),
triggering longer replication. No other CPU timing median rose by more than
2.5%, and no peak-RSS median rose by more than 0.75 MiB. The rotating matrix
passed 380 output checks and 1,140 timing trials; its JSON modes had no flags
at those thresholds. These are investigation thresholds, not statistical proof
that every difference is zero. Three selection-only controls rose 3.7–5.5%;
they execute no JSON and remain reported without subtraction.

The subsequent longer stringify check ran nine interleaved trials per engine,
each with 32,768 calls. ASCII's median was 0.6% below the reference. Unicode's
was 2.2% above (mean +1.7%), with overlapping trial ranges. The original flags
did not repeat at their earlier magnitude, but the remaining positive Unicode
difference is retained as uncertainty. An earlier 25-repetition roundtrip check
also found no repeatable RSS increase; the release matrix has no such RSS flag.
All raw samples are preserved, including the preliminary flags.

All 279 JSON runtime tests passed, including the upstream template-ownership
test and new output-debt, blocked-boundary, collection-cadence, and relocated
lazy-record tests. Compiled retained results match Node under normal, full, and
scheduled moving GC. The scan witness ran 1,323 copying minors with protected
retired pages; the retained-output witness ran 59. The large-stringify cadence
matches the reference's recurring malloc trigger counts. Collection is asserted
to have happened; these are not zero-collection stress passes.

## Remaining Node/Bun gaps

The candidate has the lowest CPU median on the original 38 basic parse/stringify
rows, and on 46 of the complete 50 operations. It has the lowest peak-RSS median
on 67 of 86 targets and the lowest retained-output RSS median on 31 of 36.
Repeated-source caching contributes to those results; the rotating rows are
necessary to assess changing inputs.

| CPU gap | Perry µs | Best Node/Bun µs | Perry / best |
|---|---:|---:|---:|
| 13 KiB record-array scan | 63.228 | 35.639 | 1.77× |
| 1 MiB record-array scan | 3,232.420 | 2,218.928 | 1.46× |
| 8 MiB record-array scan | 24,987.125 | 21,391.375 | 1.17× |
| 20 MiB record-array roundtrip | 98,809.500 | 76,436.500 | 1.29× |
| Rotating small-record parse | 0.498 | 0.255 | 1.95× |
| Rotating 1 KiB-object parse | 0.497 | 0.240 | 2.07× |
| Rotating ASCII-string parse | 107.969 | 70.837 | 1.52× |
| Rotating Unicode-string parse | 148.443 | 63.461 | 2.34× |

Rotating empty-object CPU also trails by 1.19×; the tiny-object median is 1.0004×
and too close to distinguish confidently here. Both include the input-selection
loop. Important memory gaps remain: the 13 KiB scan peaks at 207 MiB versus
Node's 62 MiB, and wide-object parse peaks at 228 MiB versus Bun's 95 MiB.
The overall parity goal remains open.

## Evidence and release status

- [Every original CPU and RSS row, all four engines](results/quiet-release1528-all-r5/comparison.md),
  [parity inventory](results/quiet-release1528-all-r5/parity.md),
  [quiet window](results/quiet-release1528-all-r5/window.json).
- [Every rotating-input and control row](results/quiet-release1528-rotating-r5/comparison.md),
  [quiet window](results/quiet-release1528-rotating-r5/window.json).
- [Longer release stringify comparison](results/quiet-release1528-stringify-focus/comparison.md)
  and [earlier roundtrip replication](results/quiet-integrated-r6-focus/comparison.md).
- [Source/build provenance](results/quiet-release1528-all-r5/provenance.json)
  and [moving-GC witnesses](results/quiet-release1528-all-r5/gc-witness.json).
- [Original merge audit](MERGED_MAIN_AUDIT.md),
  [record-batch experiments](LAZY_RECORD_BATCHES.md),
  [reclamation root cause and rejected candidates](LARGE_STRING_RECLAMATION.md),
  [rejected stringify outline](STRINGIFY_DISPATCH.md), and
  [next investigations](NEXT_PARSE_TARGETS.md).

The exact measured release is 0.5.1528, built after integrating upstream
`d342c816b`'s reusable-template ownership fix and `f2dc03582`'s release metadata.
Its matched compiler/runtime/stdlib build and compiled lifetime/cadence checks
pass. Earlier 0.5.1527 integrated runs retain their original hashes and reports.
The final CI follow-up changes only test handle access and benchmark reporting;
production JSON/GC code is identical to the measured release.

[PR #10032](https://github.com/PerryTS/perry/pull/10032) carries this work.
Two inherited main failures currently prevent a green merge gate: the public
benchmark artifact needs refreshing, and the Linux thread-stack test fails on
both main and this PR. [CI evidence and disposition](results/pr10032-ci/README.md)
records the matching failures and input fingerprints. The JSON tests' four
new raw-handle lint findings have been corrected without raising any ceiling.
A merged-main remeasurement requires the PR to merge first; these results are
explicitly measurements of the recorded release candidate, not of a future merge.
