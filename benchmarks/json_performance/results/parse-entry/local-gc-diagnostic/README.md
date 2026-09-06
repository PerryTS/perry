Six local GC diagnostics with PERRY_GC_DIAG=1; not qualified timing or RSS.
Both arms report the same old-reclaim scan counts and final arena capacity:
record arrays 4 scans/5 MiB, heterogeneous arrays 5 scans/5 MiB, numeric arrays
6 scans/8 MiB. Actual values are retained in summary.json. Candidate final live
arena bytes differ by only 176 bytes on the record/heterogeneous arrays and
are identical on numbers. Equal scan counts and final arena size do not prove
equal GC time, transient arena peaks or Rust allocator RSS. The gc-time fields
report zero here and do not establish that GC took no CPU.

If paired peak RSS reproduces the full-matrix lazy-array increase, isolate the
tape construction/collection ordering next. The new root must stay before the
pending hook. Restoring tape-build-before-trigger would require a raw-input
scratch API, ending byte borrows before a callback can collect, followed by
rederivation from the root. Do not restore the stale input or shared-reference
argument merely to reproduce the old scheduling.
