**Classify #9717's `forwarded_stub_recoveries` diagnostic counter.** The
`gc_runtime_root_holders` gate requires a written verdict for every new
core `perry_thread_local!` declaration; `FORWARDED_STUB_MEMBERSHIP_RECOVERIES`
is a `Cell<u64>` tally reported on the `PERRY_GC_DIAG` `[gc-incremental]` line
and holds no address, so it records as `not_a_gc_pointer`.
