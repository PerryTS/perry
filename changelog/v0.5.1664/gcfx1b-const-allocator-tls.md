Const-initialize per-thread allocator, arena, page-metadata and temporary-root
and blocking-job storage. Allocate their initial backing storage on first use inside the existing
values, retaining thread-exit destructors and initial capacities. This removes
lazy TLS old-value destruction from allocating helper call paths, allowing the
unchanged machine-code GC effect analyzer to prove their production effects.
Resolve page-metadata TLS through the same small address provider as the hot
cache before running registration callbacks. Keep the job table empty until
the first accepted job, preserving its hasher and teardown behavior.
