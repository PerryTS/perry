Const-initialize per-thread allocator, arena, page-metadata and temporary-root
storage. Allocate their initial backing storage on first use inside the existing
values, retaining thread-exit destructors and initial capacities. This removes
lazy TLS old-value destruction from allocating helper call paths, allowing the
unchanged machine-code GC effect analyzer to prove their production effects.
