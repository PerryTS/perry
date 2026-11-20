Restore proof-aware lowering for tracked buffer-view reads after escapes,
disposal and backing-buffer exposure. The numeric typed-array read tier added
in `2724261ced` intercepted demoted tracked views before their fallback records
were emitted, breaking 15 `native_proof_buffer_views` tests. Its managed-storage
guards still rejected native arenas and reloaded managed backing; this was
missing proof evidence rather than a demonstrated stale-pointer runtime read.

Keep tracked receivers on their existing native/fallback paths, including their
invalidation reasons, while retaining the new read tier for untracked locals
and parameters. The proof assertions remain unchanged. Add Node parity coverage
for a spread call that mutates a view's backing and a call that detaches it
before subsequent element reads.

Validation: parent 47/47, introducing commit 32/47, restored proof suite 47/47;
full codegen and runtime suites have no failures, and 52 gap cases match Node.
Temporarily removing recursive escape downgrading makes ten proof tests fail.
