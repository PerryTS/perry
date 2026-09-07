Six current-worker profiles and the older retained-empty comparison qualify with 11 clean external observations and a passed admission gate. Each profile samples a separate worker for three seconds at one-millisecond intervals; all worker/sample exits are zero. Counts describe this sample, not exact CPU shares or quiet benchmark acceptance. The full CPU/RSS matrix remains the timing authority.

| Workload | Main-thread samples | Findings |
|---|---:|---|
| Heterogeneous stringify | 2,266 | 1,659 in array traversal; 1,605 in the nested-record emitter. Final string allocation records 432 samples, including 429 at its GC trigger; the final buffer copy records another 137. |
| Wide-object parse | 2,213 | 1,763 under the worker loop's moving-GC safepoint; the copied-minor branch has 1,473, including 1,339 in slot rewriting. The parse branch has 449. |
| Small-record stringify | 2,276 | 1,954 in the direct record-output branch, including a 303-sample toJSON-check branch and repeated scalar/string planning and short copies. Only 51 occur under the loop GC safepoint. |
| 1 KB object parse | 2,273 | 903 under direct parsing, 618 at the pre-parse GC trigger, 183 at the depth-preflight branch, 152 at the parse-boundary/debt branch, and 211 under loop GC. |
| Long ASCII string parse | 2,258 | Direct parse has 1,150: 586 copying output, 370 scanning the string, 132 in the string-value branch, and 62 in allocation. Pre-parse GC has 809 and depth preflight 299. |
| Unicode parse | 2,263 | Direct parse has 1,549: 867 counting UTF-16 (707 in UTF-8 validation), 390 copying output, and 262 scanning string syntax. Pre-parse GC has 492 and depth preflight 221. |

The long-string/Unicode fixtures are objects containing an id and a text field, not bare string roots; see generate.py. A root-string-only shortcut would not address these measured rows. The next parse work should reuse valid source-string metadata or combine scans for ordinary string fields while preserving strict JSON validation, WTF-8 behavior and moving-GC rooting. Small-record output needs less per-field planning, short-copy overhead and prototype lookup. Wide-object allocation/layout must reduce work charged to the unchanged collector; skipping GC charges or changing collector policy is not acceptance.
