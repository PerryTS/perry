# Opening-container scan diagnostic

This measures only the allocation-free ARM64 byte search used by nesting
preflight, not JSON.parse or a linked Perry runtime. It provides no acceptance
claim for parser CPU or RSS. Actual runtime validation and full workload
regression checks are still required before adopting the prototype.

The old scan searches 16 bytes per iteration. The proposed scan retains an
initial 16-byte early-positive check, then tests 64 bytes per reduction. Folding
bit 0x20 maps exactly '[' and '{' to '{', removing one vector comparison.
Both routines receive the fixture bytes after the root opening, as in preflight.
Inputs are loaded before timing. Each arm executes 8192 calls per trial, nine
trials in shuffled order. CPU is process user plus system time around the Rust
loop; Python dispatch and two usage queries are included. Both arms reside in
one library and use the same buffer; repeated calls use black_box. RSS is not
measured in this diagnostic.

The M1 quiet window is 2026-09-09 19:07:32–19:07:39 UTC. All result checks pass.
The exact helper source, runner, fixture/library hashes and raw timings are
preserved. property-probe.rs separately compares old and new across all byte
values, lengths 0..259, unaligned offsets, every opening position and seeded
arbitrary buffers. Both tests pass.

See summary.json for the measured per-scan CPU and sample ranges. The next step
is an isolated runtime candidate after the source-length constructor experiment
has finished; the production parser has not been changed by this probe.
