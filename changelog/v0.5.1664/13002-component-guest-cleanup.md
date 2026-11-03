Add deferred cleanup for unreachable owned resources inside Perry-authored
WASI components. Finalizers retain private ownership state, so transferred or
disposed owners cannot release a reused canonical handle. Live borrows retain
their owner and explicit disposal unregisters cleanup.

Synchronous component exports perform a microtask checkpoint while arguments
and results remain rooted, allowing library components to deliver finalizers
without a command event loop. Checkpoints leave timers and immediates pending
and do not wait for I/O or asynchronous WIT results. The guest adapter ABI remains
compatible. Collection timing and cleanup after a trap remain nondeterministic.

Add moving-GC ownership tests and independent Rust-host acceptance coverage
for imported and guest-defined owners, live borrows, transfer, explicit
disposal, return-value rooting and exactly-once destruction. On Linux, all
28 runtime component tests and 55 acceptance phases pass at
`23b5c3aadc18237488d6c978ece554483d774c78`. A negative control reproduces the
old checkpoint dispatching an immediate; the fixed checkpoint leaves it pending
and preserves a returned value across an actual moving collection. Evidence:
`docs/audits/wasi-component-validation-2026-10-21/component-guest-cleanup-20261021.json`.
