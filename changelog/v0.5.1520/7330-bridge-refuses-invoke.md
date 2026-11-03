The explicit statepoint bridge emitted no statepoint on `invoke`, so every call
inside a `try` carried no GC roots.

`is_call` matched only `call`/`tail call`, so an `invoke` skipped both the
statepoint conversion *and* the fail-closed panic below it, and passed through as
an ordinary line. Since #7302 moved exception lowering to `invoke`/`landingpad`,
that is every call inside a `try`. Measured on one program: 58 invokes, 0
carrying `gc.statepoint`, with allocating callees among them
(`js_object_alloc_class_inline_keys`, `js_array_push_f64`,
`js_native_call_method_by_id`).

`--statepoint-report` was silent about it too, because it counts only the lines
it recognises — "0 parser fallbacks" said nothing about any call inside a `try`,
including in #7314's headline census.

Forming a statepoint from an invoke is real work: the statepoint must itself
become an invoke, with `gc.result` and the relocates in the normal successor.
Until the bridge does that, it now refuses — the same fail-closed rule the plain
stack-map fallback was deleted for (#7314). Invokes are also counted by the
report, so the census stops overstating its coverage.

Note this leaves no working statepoint path for try-carrying code on a default
toolchain: RS4GC handles invokes but requires `PERRY_LLVM_CLANG` pointing at a
version-matched LLVM 22, since Apple clang rejects the IR it emits.
