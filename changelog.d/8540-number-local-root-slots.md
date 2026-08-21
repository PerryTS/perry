### Performance

- Stop binding and maintaining shadow-root slots for locals proved Number-
  by-construction. The shadow-root map is deliberately conservative because
  it is collected before specialization facts exist; a guarded typed-array
  clone could therefore keep root stores and write barriers in its integer
  hot loop even after the later fact graph proved the local non-pointer.

  The omission is safe at every collection point, not merely at reads. The
  proof considers only function-local `let` bindings, excludes boxed/captured
  locals and module globals, never trusts TypeScript annotations, and checks
  the initializer plus every later write. Its ordinary arm admits only Number
  values. Its specialized undefined-seed arm admits only Number or Perry's
  non-pointer `undefined` sentinel and permanently rejects the local after any
  other write, even when that value is overwritten before the next read.
  Parameters and conditional expression writes stay fail-closed. Generic
  fallback bodies therefore retain their conservative slots when the guarded
  input proof is unavailable.

  On `main` at measurement time (8d837df22) with the 20-row #8496 corpus
  (`PERRY_NO_AUTO_OPTIMIZE=1`, release compiler and matching runtime archives,
  `/usr/bin/time -l`, interleaved medians of 5), `typed_array` retires 56.013 B
  instructions instead of 56.669 B (-1.16%). Peak RSS is 5.489 MB instead of
  5.472 MB (+16 KiB, +0.30%). Heavy host contention makes the wall medians
  non-actionable (25.56 s instead of 13.30 s, with overlapping run ranges).
  Five shorter rows cross 1% in instruction medians amid run-to-run/GC noise:
  `asyncpipe` +2.11%, `cycles` +1.67%, `retain1` -2.11%, `retain_wide1`
  -1.22%, and `shapes` -2.12%; `churn`, `tree`, and `retain` move -0.25%,
  -0.18%, and +0.21%. All 20 outputs remain byte-exact, including
  `typed_array`'s `-821955270` checksum.
