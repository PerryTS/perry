Add deferred cleanup for unreachable owned resources inside Perry-authored
WASI components. Finalizers retain private ownership state, so transferred or
disposed owners cannot release a reused canonical handle. Live borrows retain
their owner and explicit disposal unregisters cleanup.

Synchronous component exports perform a microtask checkpoint while arguments
and results remain rooted, allowing library components to deliver finalizers
without a command event loop. Checkpoints run ready jobs without waiting for
timers, I/O or asynchronous WIT results. The existing guest adapter ABI remains
compatible. Collection timing and cleanup after a trap remain nondeterministic.

Add moving-GC ownership tests and independent Rust-host acceptance coverage
for imported and guest-defined owners, live borrows, transfer, explicit
disposal, return-value rooting and exactly-once destruction. Execution
qualification is pending.
