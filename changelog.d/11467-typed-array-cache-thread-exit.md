Fix stale typed-array admission caches after worker-thread exit. Arena teardown
now invalidates the kind cache and owning-Uint32Array cache before freeing each
block, without touching thread-local registries or clearing another thread's
replacement entry. Previously a reused address could make a fresh Promise look
like a Uint8Array, causing the asynchronous stream-abort test in #11463 to reject
early. Add deterministic regressions for both caches and preservation of entries
outside the retiring range.
