# GC diagnostics for remaining differences

These four executions enable `PERRY_GC_DIAG=1` and are diagnostic evidence,
not performance standings. Both builds use the same application object, input,
iteration count and warmup count. `provenance.json` records binary hashes;
the quiet gate passed.

For numeric-array parse (96 calls, two warmups), both binaries report seven
`old_reclaim_alloc_point` scans. Their surviving arena blocks diverge. The
reference exits with 3,351,968 live arena bytes and 8,388,608 capacity bytes;
the candidate reports 4,349,512 live bytes and 9,437,184 capacity bytes.
This is evidence of different retention, not an identified leak or a proven
cause of the full process-RSS difference. Parser and GC source are unchanged
between these two builds. A separate [executable-name control](../argv-control/README.md)
rules out differing name lengths as the explanation.

Wide-object parse (36 calls, two warmups) has identical ending arena figures
in both builds: 100,236,416 live bytes and 103,809,024 capacity bytes. The
diagnostic execution does not reproduce the candidate's timing slowdown;
instrumented timing cannot dismiss the concern from the default-GC repeats.

The lazy-array path already retains the original input string. Its tape is a
separate allocation, and its sparse cache/bitmap use arena allocations.
Avoiding a supposed input-string copy would therefore not address this path.
