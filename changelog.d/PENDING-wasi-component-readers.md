Implement native component future and stream readers with typed payloads,
nonthenable owned handles, bounded chunks, byte arrays and idempotent disposal.
Preserve observed temporary reads through moving GC while allowing unobserved
reads and unread handles to trigger soft cancellation without retiring the Store.
Invalidate transferred aliases and collect owned native results refused by hooks
or left without an observer.

Qualify Linux production hosts with nested futures, owned stream chunks and EOF,
Unicode and large byte payloads, suspended-producer cancellation and component
reuse. Run persistent, async-resource and reader acceptance in component checks,
alongside the Promise and async-hook regression selections. JavaScript producers,
concurrent HTTP handoff, async guest adapters and platform/package qualification
remain outstanding. Record exact evidence in
`docs/validation/wasi-component-readers-20261026.json`.
