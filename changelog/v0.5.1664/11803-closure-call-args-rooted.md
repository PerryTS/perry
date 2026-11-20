Fixed a call to a closure-valued local (`const show = (a, b) => ...;
show(String(x), work())`) passing a stale argument when a later argument
collected (#11789). The closure-call lowering evaluated the arguments into bare
registers, so an evacuating minor inside a later argument left an earlier heap
argument at its retired from-space address: a SIGSEGV under the from-space
quarantine, and an empty string in normal runs under a seeded schedule. Each
argument is now rooted across the arguments after it, as calls to `function`
declarations already were. New seeded-GC witness
`test_gap_gc_11789_closure_call_args`.
