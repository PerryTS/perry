# Opening-container scan R1: full original matrix

Candidate source `29236271f61a6cfb24de810ff4fe1f884c774c5e`, version 0.5.1530.
Reference is the immutable corrected R2 build from PR #10034, not unfixed main.
All four engines ran five repetitions under the quiet-host gate on the M1/8 GiB
host, 2026-09-09 19:43:07–19:48:14 UTC. The original suite repeats one source;
changing-input results belong to the separate rotating suite. It includes
parse, stringify, scan, sparse, roundtrip, peak RSS and retained-output RSS.

This candidate is not accepted as regression-free. Eight timing rows have every
candidate sample slower than every reference sample in this run:

| Fixture | Operation | Candidate CPU change |
|---|---|---:|
| empty_object | parse | +1.114% |
| null | parse | +5.934% |
| numbers_1m | stringify | +0.504% |
| object_1k | parse | +0.396% |
| records_array_16k | sparse | +0.308% |
| small_record | parse | +0.289% |
| string_a | parse | +4.411% |
| tiny_object | parse | +0.689% |

The maximum median peak-RSS increase is 176 KiB. Most RSS rows shift by roughly
one hundred KiB; this alone is not evidence of managed-heap growth.

The `null` and inline-string paths never execute the changed opening scan.
Both generated parse entries have a 96-byte frame and save twelve registers,
because empty-object allocation is inlined into the entry. A follow-up tests
outlining that allocator. The scan change itself does not change GC scheduling.

See [all target comparisons](parity.md), [all summaries](summary.json),
[every candidate/reference row and sample](reference-screen.json), and
[quiet-window metadata](window.json). Raw JSONL, the exact runner and worker
sources, source patch, immutable artifact hashes and GC witnesses are retained
here. Correctness/GC validation is in `../opening-scan-r1-validation`.
