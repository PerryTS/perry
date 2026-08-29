`PERRY_GC_DIAG=1` now reports cumulative mutator-visible collection time and
its wall-clock share at exit:

    [gc-time] wall_us=… step_us=… remark_us=… minor_us=… full_sync_us=… share_permille=…

The existing telemetry answers "how bad is the worst pause" (`step_max_us`,
`final_remark_max_us`); this answers "what fraction of the run went to
collection at all" — the measurement-first gate for any concurrent-marking
work: if the share on real applications is single-digit percent, moving
marking off the mutator thread has nothing worth hiding. Counters are
always-on relaxed atomics (one `fetch_add` per GC event); the wall epoch is
primed in `js_gc_init` so the denominator starts at program start, not at
the first GC event. `share_permille` sums the three disjoint buckets
(budgeted steps, atomic final remarks, copying minors); synchronous
`gc()` calls are timed separately since they can internally drive budgeted
steps.
