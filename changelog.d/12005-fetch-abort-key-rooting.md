Fixed `controller.abort()` sometimes not cancelling an in-flight `fetch`: the
fetch engine keyed each request by its `AbortSignal` address, and a copying
minor collection could move the signal. A root scanner now rewrites those keys.
`scripts/gc_runtime_root_holders.py` no longer counts a field access or a
method name collision as GC-scanner coverage.
