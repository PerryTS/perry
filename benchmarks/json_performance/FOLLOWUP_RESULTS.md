# JSON construction and reclamation follow-up

The integrated R6 implementation makes record-array traversal 30–78% faster
and fixes discarded large parsed strings accumulating beyond a GiB. Construction
uses the existing batch and suppression scope. Completed outputs publish
reclamation requests; collection runs at later rooted boundaries. No background
GC thread, output-identity shortcut, or new GC environment knob is introduced.

The integrated implementation passed the original and rotating-input matrices
on the Apple M1 / 8 GiB benchmark Mac, against an interleaved immutable PR #10022
merge (`c82e613d260a80324914b5aba503a31a763ad90e`), Node 26.5.1, and Bun 1.3.14.
All engines perform identical work within each comparison. These are five-trial
medians from fresh processes; quiet admission passed before and after each run.

## Measured gains

| Workload | Reference CPU µs | Candidate CPU µs | CPU reduction | Reference peak MiB | Candidate peak MiB |
|---|---:|---:|---:|---:|---:|
| 13 KiB record-array scan (`records_array_16k`) | 290.272 | 63.109 | 78.3% | 443.688 | 210.141 |
| 1 MiB record-array scan | 4,616.971 | 3,234.217 | 29.9% | 160.328 | 158.328 |
| 8 MiB record-array scan | 36,525.125 | 24,948.750 | 31.7% | 192.266 | 186.578 |
| Rotating 1 MiB ASCII-string parse | 152.413 | 108.114 | 29.1% | 1,330.563 | 90.250 |
| Rotating Unicode-string parse | 186.499 | 148.497 | 20.4% | 1,243.766 | 62.422 |

Rotating input means eight preloaded equal-size, same-shape strings whose
contents differ in one value. It defeats the single-source parse caches without
timing file reads or allocating a fresh input string on every call. The worker
retains the eight inputs and only its last result. This is a bounded corpus,
not a claim about every cold or changing object workload. Stringify is measured
separately on already-materialized inputs in the original matrix.

## Regressions and correctness

The original matrix passed all 200 output checks and validated 344 measurement
groups. No CPU timing median exceeded the reference by more than 2.5%; no
peak-RSS median exceeded it by more than 0.75 MiB. The rotating matrix passed
380 output checks and 1,140 timing trials; its JSON modes also had no flags at
those thresholds. These are investigation thresholds, not statistical proof
that every difference is zero. The 20 MiB selection-only control rose about
0.53 ns/call (4.1%); it executes no JSON and is reported without subtraction.

The longer regression check used 25 repetitions of the preliminary roundtrip
RSS concern, plus nine trials at a larger fixed call count. The RSS increase
did not reproduce. ASCII stringify's longer median rose 1.8%, with broad trial
overlap; the full integrated matrix showed 1.1%. The raw samples preserve this
remaining measurement uncertainty.

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
| 13 KiB record-array scan | 63.109 | 35.679 | 1.77× |
| 1 MiB record-array scan | 3,234.217 | 2,222.783 | 1.46× |
| 8 MiB record-array scan | 24,948.750 | 21,419.875 | 1.16× |
| 20 MiB record-array roundtrip | 98,585.500 | 76,395.500 | 1.29× |
| Rotating small-record parse | 0.499 | 0.255 | 1.96× |
| Rotating 1 KiB-object parse | 0.497 | 0.240 | 2.07× |
| Rotating ASCII-string parse | 108.114 | 70.421 | 1.54× |
| Rotating Unicode-string parse | 148.497 | 62.077 | 2.39× |

Rotating empty-object CPU also trails by 1.20×; the tiny-object median is 1.006×
and too close to distinguish confidently here. Both include the input-selection
loop. Important memory gaps remain: the 13 KiB scan peaks at 210 MiB versus
Node's 62 MiB, and wide-object parse peaks at 228 MiB versus Bun's 95 MiB.
The overall parity goal remains open.

## Evidence and release status

- [Every original CPU and RSS row, all four engines](results/quiet-integrated-r6-all-r5/comparison.md),
  [parity inventory](results/quiet-integrated-r6-all-r5/parity.md),
  [quiet window](results/quiet-integrated-r6-all-r5/window.json).
- [Every rotating-input and control row](results/quiet-integrated-r6-rotating-r5/comparison.md),
  [quiet window](results/quiet-integrated-r6-rotating-r5/window.json).
- [Longer regression comparison](results/quiet-integrated-r6-focus/comparison.md).
- [Source/build provenance](results/quiet-integrated-r6-all-r5/provenance.json)
  and [moving-GC witnesses](results/quiet-integrated-r6-all-r5/gc-witness.json).
- [Original merge audit](MERGED_MAIN_AUDIT.md),
  [record-batch experiments](LAZY_RECORD_BATCHES.md),
  [reclamation root cause and rejected candidates](LARGE_STRING_RECLAMATION.md),
  [rejected stringify outline](STRINGIFY_DISPATCH.md), and
  [next investigations](NEXT_PARSE_TARGETS.md).

The measured integrated binaries identify themselves as 0.5.1527. They include
upstream `d342c816b`'s reusable-template ownership fix. Upstream then recorded
that fix's release metadata at `f2dc03582`; this follow-up is rebased there and
uses 0.5.1528. Its matched build and compiled lifetime/cadence checks pass, and
its runtime sources match the tested implementation. Its executable layout
changed during rebuilding, so measurements of those exact release artifacts
are in progress before marking the PR ready. Earlier measurements retain their
original hashes and are not relabelled as later builds.
