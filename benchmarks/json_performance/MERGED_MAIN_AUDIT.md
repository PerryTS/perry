# Merged JSON performance: completion audit

Baseline: squash commit `c82e613d260a80324914b5aba503a31a763ad90e` (PR #10022).
Measured on Apple M1, 8 GiB RAM, Node v26.5.1 and Bun 1.3.14.
Five interleaved repetitions per engine and cell. The dedicated host passed the lock and quiet-window checks before and after the run.

The goal remains open. The 38 repeated-source parse/stringify CPU rows all lead, but they do not cover every original operation or memory metric.

Follow-up experiments: [lazy record construction batches](LAZY_RECORD_BATCHES.md).

| Metric | At or below faster/lower-memory competitor | Total |
|---|---:|---:|
| cpu_us | 46 | 50 |
| peak_rss_mib | 67 | 86 |
| rss_after_mib | 31 | 36 |

All 150 correctness checks passed. CPU uses process user+system time around the operation loop. Peak RSS is process-wide. Retained-output RSS compares one retained result with a live batch; no forced collection or disabled GC is used.

## Remaining CPU gaps

| Fixture | Operation | Perry µs | Best engine | Best µs | Perry / best |
|---|---|---:|---|---:|---:|
| records_array_16k | scan | 292.145 | bun | 35.764 | 8.169 |
| records_array_1m | scan | 4646.377 | bun | 2234.232 | 2.080 |
| records_array_8m | scan | 36761.625 | bun | 21613.875 | 1.701 |
| records_array_20m | roundtrip | 99789.000 | bun | 77233.000 | 1.292 |

## Remaining memory gaps

| Fixture | Operation | Metric | Calls / retained | Perry MiB | Best engine | Best MiB |
|---|---|---|---:|---:|---|---:|
| small_record | parse | peak_rss_mib | 1585900 | 79.97 | node | 59.69 |
| object_1k | parse | peak_rss_mib | 1953341 | 64.89 | node | 61.81 |
| records_array_16k | scan | peak_rss_mib | 3898 | 500.80 | node | 61.89 |
| records_array_16k | roundtrip | peak_rss_mib | 6015 | 68.78 | node | 65.86 |
| records_array_1m | scan | peak_rss_mib | 69 | 160.33 | bun | 83.19 |
| records_array_8m | scan | peak_rss_mib | 8 | 192.27 | bun | 133.55 |
| records_object_8m | parse | peak_rss_mib | 9 | 174.91 | bun | 118.61 |
| records_array_20m | parse | peak_rss_mib | 4 | 255.39 | bun | 220.67 |
| records_array_20m | sparse | peak_rss_mib | 4 | 255.38 | bun | 220.55 |
| records_array_20m | scan | peak_rss_mib | 3 | 224.50 | bun | 190.33 |
| records_array_20m | roundtrip | peak_rss_mib | 2 | 295.38 | bun | 255.89 |
| records_object_20m | parse | peak_rss_mib | 4 | 255.39 | bun | 221.06 |
| wide_1m | parse | peak_rss_mib | 63 | 224.91 | bun | 95.56 |
| records_array_8m | retain-stringify | rss_after_mib | 1 | 77.36 | bun | 73.47 |
| records_array_8m | retain-stringify | peak_rss_mib | 1 | 79.94 | bun | 73.86 |
| records_array_8m | retain-stringify | rss_after_mib | 4 | 98.61 | bun | 94.50 |
| records_array_8m | retain-stringify | peak_rss_mib | 4 | 99.12 | bun | 94.89 |
| records_object_8m | retain-parse | peak_rss_mib | 4 | 94.11 | bun | 92.41 |
| records_object_8m | retain-stringify | rss_after_mib | 1 | 77.34 | bun | 73.48 |
| records_object_8m | retain-stringify | peak_rss_mib | 1 | 79.92 | bun | 73.88 |
| records_object_8m | retain-stringify | rss_after_mib | 4 | 98.62 | bun | 94.52 |
| records_object_8m | retain-stringify | peak_rss_mib | 4 | 99.14 | bun | 94.91 |
| wide_1m | retain-parse | rss_after_mib | 16 | 77.09 | bun | 68.92 |
| wide_1m | retain-parse | peak_rss_mib | 16 | 77.59 | bun | 69.31 |

## Input diversity

`worker.ts` repeatedly parses one input string. The new `rotating-worker.ts` loads eight equal-sized JSON documents before timing and rotates them. Each changes one value without changing the shape or property order. The null and empty-object fixtures have no mutable content; they use separate file reads. This defeats single-source identity caching without charging IO or input creation to the parser.

The same-source control keeps the identical eight-input pool alive and repeatedly parses its first member. The selection-only control measures indexing overhead, reported without subtraction. This is a bounded corpus, not a newly allocated input on every call, and it must not be described as cold-start latency. RSS includes all eight inputs and cannot be compared directly with the original one-input worker.

Verification parses and checks all eight members, plus the final timed value and loop checksum. The TypeScript and JavaScript workers have identical executable statements after erasing type annotations. `summarize_rotating.py` rejects missing fixtures, modes, engines, repetitions, or verification results.

The rotating-input run also passed dedicated-host admission, all 171 output comparisons, and all 855 timing trials. Its remaining CPU gaps are:

| Fixture | Perry µs | Node µs | Bun µs | Perry / best |
|---|---:|---:|---:|---:|
| empty_object | 0.030530 | 0.045530 | 0.025707 | 1.188 |
| tiny_object | 0.047003 | 0.082341 | 0.046774 | 1.005 |
| small_record | 0.498319 | 0.345008 | 0.257362 | 1.936 |
| object_1k | 0.497482 | 0.540332 | 0.240841 | 2.066 |
| long_string_1m | 151.123656 | 372.271889 | 69.545315 | 2.173 |
| unicode_1m | 184.290809 | 438.316872 | 62.547325 | 2.946 |

The tiny-object difference is approximately 0.5%, so parity needs a longer focused run. The selection-only control shows roughly 10 ns/call of Perry indexing/loop overhead versus 3–4 ns for the competitors; this matters for scalars and empty objects, while it is negligible for the larger parsing gaps. Raw selection overhead is not subtracted from the standings.

[All rotating/control rows](results/quiet-merged-rotating-r5/comparison.md) and [quiet admission](results/quiet-merged-rotating-r5/window.json).

Rotating-input RSS also exposes a major reclamation gap hidden by the repeated
source cache:

| Fixture / rotating parse | Perry peak MiB | Node peak MiB | Bun peak MiB |
|---|---:|---:|---:|
| long_string_1m | 1,357.031 | 177.453 | 123.453 |
| unicode_1m | 1,260.688 | 161.203 | 153.406 |

These are whole-process peaks with the same eight-input corpus alive in each
engine. Perry retains only the last result in the worker, so the growth cannot
be explained by intentional retention of every parse result. The subsequent
[large-string investigation](LARGE_STRING_RECLAMATION.md) identifies a completed
malloc-output request being overwritten by the post-parse trigger adjustment.

## Evidence and reproduction

- [Every original CPU/RSS target](results/quiet-merged-all-r5/parity.md)
- [Raw original-operation trials](results/quiet-merged-all-r5/timing.jsonl)
- [Raw retained-output trials](results/quiet-merged-all-r5/memory.jsonl)
- [Quiet-host admission](results/quiet-merged-all-r5/window.json)
- [Host and binary hash](results/quiet-merged-all-r5/host-all.json)
- [Rotating-corpus generator](generate_rotating.py)
- [Rotating controller](run_rotating.py)

Generate fixtures with `generate.py`, then `generate_rotating.py`. Compile `rotating-worker.ts` against the matched Perry runtime. Invoke `run_rotating.py` with explicit `--worker`, `--node`, `--bun`, and a new `--results-dir`; on a staged measurement host also supply `--source-commit`. Run through the token-owned benchmark lock and inspect its before/after admission verdict. The original worker remains the regression baseline; the rotating workload is additional coverage.

Candidate acceptance requires fresh correctness/GC validation, an interleaved candidate/reference comparison with equal iterations, all original parse/stringify and pipeline rows, retained and peak RSS, and rotating inputs. A CPU win does not waive a memory regression.
