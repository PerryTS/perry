# Outlined stringify dispatch: rejected experiment

R5 separates the full replacer/spacer serializer from the bounded output entry.
The default entry's ARM64 stack frame shrank from 272 to 144 bytes. Its 278 JSON
runtime tests, matched release build, retained-result checks, and scheduled
moving-GC scan passed. It introduced no intended semantic change.

The longer quiet-host test compared R5, R4, and the merged reference in nine
interleaved repetitions. Node supplied a separate byte-exact output oracle.
Tiny cases ran 50 million calls per trial; Unicode ran 16,384 calls.

| Stringify fixture | Reference ns | R4 ns | R5 ns | R5 / reference |
|---|---:|---:|---:|---:|
| `string_a` | 9.69792 | 9.69826 | 9.69872 | 1.00008 |
| `small_record` | 41.50274 | 41.47910 | 41.71756 | 1.00518 |
| `unicode_1m` | 26,575.684 | 26,974.243 | 26,452.942 | 0.99538 |

Tiny strings did not improve. Small-record samples suggest a small loss rather
than a useful gain. Unicode samples overlap broadly across all three arms.
The smaller stack frame is not sufficient evidence of a performance win, so
R5 was removed from the working source before R6 validation. Its artifacts are
preserved for reproduction; it is not an accepted change.

[Raw trials](results/quiet-dispatch-r5-focus-r9/timing.jsonl),
[medians and samples](results/quiet-dispatch-r5-focus-r9/summary.json),
[quiet admission](results/quiet-dispatch-r5-focus-r9/window.json), and
[source/binary provenance](results/quiet-dispatch-r5-focus-r9/provenance.json).
