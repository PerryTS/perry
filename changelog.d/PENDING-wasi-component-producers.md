Implement component-bound JavaScript future/stream factories and writable
handles. Expose structural WIT metadata while capturing the original nominal
schema privately. Validate typed writes, preserve owned payload roots until
native ownership receipts, and return fresh owned remainders after partial or
cancelled writes. Preserve byte arrays, wide integers, Unicode, nested futures,
and dense undefined unit-stream counts.

Add stale-safe writer collection: close streams locally, recheck completed
future ownership before cleanup, and use Store abort for abandoned unfinished
canonical futures. Intentional collection retires quietly without suppressing
genuine callback/destructor failures. Require matching component-host/runtime
ABI 11. Share the
native 64-pending-call admission limit so collectors defer under pressure while
allowing returned invocation contexts to remain retained separately.

Qualify actual compiled production Wasm round trips, guest resource consumption,
cancellation/retry, GC and component reuse; add producer acceptance to component
checks. Async guest adapters, concurrent persistent HTTP, platform/package
qualification and the previously recorded security debt remain outstanding.
See `docs/validation/wasi-component-producers-20261026.json` for exact evidence.
