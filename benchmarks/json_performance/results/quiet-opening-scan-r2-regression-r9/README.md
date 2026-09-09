# Longer original-worker replay: opening scan R2

Candidate includes the wider opening scan plus an outlined empty-object allocator. Baseline is corrected R2 from PR #10034; prior is scanner R1. The quiet M1/8 GiB window is 2026-09-09 20:09:03–20:10:00 UTC. Eight cases, nine randomized repetitions, three Perry arms: all 216 timing trials and 32 output-oracle runs passed. The original worker repeats one source; this is not changing-input acceptance.

| Fixture / operation | Candidate µs | Reference µs | CPU change vs reference | Sample ranges | Change vs scanner R1 | Peak delta KiB |
|---|---:|---:|---:|---|---:|---:|
| null / parse | 0.010635950 | 0.010635100 | +0.008% | overlap | -5.522% | +96 |
| string_a / parse | 0.012830350 | 0.012881900 | -0.400% | separated faster | -4.618% | +96 |
| empty_object / parse | 0.019854000 | 0.020446300 | -2.897% | separated faster | -4.322% | +144 |
| tiny_object / parse | 0.034085200 | 0.034576000 | -1.419% | separated faster | -2.198% | +144 |
| small_record / parse | 0.095800500 | 0.096991500 | -1.228% | separated faster | -1.670% | +112 |
| object_1k / parse | 0.081337000 | 0.082417000 | -1.310% | separated faster | -1.825% | +96 |
| records_array_16k / sparse | 25.098400000 | 25.026800000 | +0.286% | separated slower | +0.124% | +144 |
| numbers_1m / stringify | 993.462000000 | 993.918000000 | -0.046% | overlap | -0.648% | +144 |

The original-worker scalar and tiny-object regressions are repaired here. Empty, tiny, small-record and 1 KB object parse medians are faster than corrected R2 with separated ranges. Null overlaps reference; inline-string parse is slightly faster. The sparse-array row remains +0.286% slower with separated ranges, so this is not a regression-free acceptance result. Full original and changing-input comparisons follow. RSS includes normal process overhead; these small shifts do not resolve large managed-container retention.

Exact drivers, quiet window, raw JSONL, source patch, hashes and GC witnesses are archived beside this report.
