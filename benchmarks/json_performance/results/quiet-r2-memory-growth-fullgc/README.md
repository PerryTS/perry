# Corrected R2 memory-growth diagnostic (fullgc)

This is a memory/collection diagnostic, not an acceptance CPU benchmark.
The immutable corrected R2 worker is pinned in metadata.json. Each process
loads the identical eight-input pool before timing, then performs 1, 8, 16 or
32 changing-input parses with zero warmup. Diagnostics are enabled; PERRY_GEN_GC=0 selects the existing full-GC mode for diagnosis only.

All eight processes exit successfully, keep the eight inputs and final result
live, and return finite numeric RESULT fields with the expected parse checksum.
The output schema, native worker and fixture corpus were previously verified
against Node in the complete R2 matrix. This diagnostic does not rerun the
serialized-output oracle; it isolates growth across iteration counts.

Raw stdout/stderr, fixture/worker/runner hashes and the quiet window are preserved.
analysis.json adds full-collection counts, in-place/untraced minor counts and
observed survival ratios to summary.json. These are separate processes, so this
is a progression by matched workload length, not a continuous RSS sampling trace.
The archived run-recorded.py is an exact source record; execute the staged
.work/r2-memory-growth runner (the path is pinned in window.json) to reproduce.
