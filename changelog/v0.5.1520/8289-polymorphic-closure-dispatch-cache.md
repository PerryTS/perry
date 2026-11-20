Improved dynamic closure dispatch for small polymorphic call sites. The runtime
now remembers the four most recently resolved closure strategies instead of
only the last one, so stage pipelines that rotate among a few closure bodies no
longer perform a thread-local hash-table lookup on every call. Monomorphic call
sites retain their one-comparison first-slot path, and late closure-arity/rest
registration still invalidates every matching cached entry.
