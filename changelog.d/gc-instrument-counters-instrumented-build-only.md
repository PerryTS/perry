The GC liveness counters (budgeted-step skips, loop polls, incremental steps,
mark-barrier arms, copying-minor counts, step and remark durations, weak-step
records) are stored and counted only in the instrumented runtime build, the
`perry_gc_instruments` cfg that `scripts/build_instr_runtime.sh` and the
debug-profile test builds already set. The default archives carry none of their
state, so the nothing-due budgeted-step poll that every allocating loop and
RegExp capture copy takes no longer pays a locked read-modify-write for a number
nothing reads. The per-thread incremental-completion count that the evacuation
verifier reads stays always on.

Unit tests cover the counter in both builds (zero-sized and inert without the
cfg, counting with it); every test that reads a counter already runs in a
debug-profile build.
