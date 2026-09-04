- **The release's ext-library build is one cargo invocation instead of 40.** That
  step *is* the release's duration: in run 33861357826 it took **304 min** on
  macOS x86_64 (killed at GitHub's hard 360-min cap) and **291 min** on aarch64,
  while Windows — which skips ext libs entirely — finished the whole leg in
  35–54 min.

  The cost was structural. The step ran 40 separate `cargo build` invocations,
  each carrying `-p perry -p perry-runtime-static -p perry-stdlib-static -p <ext>`.
  #7358 requires each wrapper be built *alongside* stdlib so their feature unions
  agree — it does not require them to be built one at a time. A different `-p`
  set per iteration is a different feature union, so each of the 40 largely
  rebuilt the compiler, runtime and stdlib: ~7.6 min × 40. Naming all 40 in one
  invocation satisfies the same constraint and builds shared dependencies once.

  If the unified build fails it **falls back to the per-package loop**, keeping
  the old best-effort property that a wrapper which cannot build on a host does
  not fail the release.

- **New release gate: ext archives must share stdlib's tokio (#507/#7629).** This
  is what makes the change above safe to make. rustc names each codegen unit
  `…tokio-<metadata-hash>.tokio.<cgu>…`, so the bundled tokio is readable from an
  archive's member names — the same signal
  `crates/perry/src/commands/compile/shared_tokio.rs` uses at link time. Two
  tokio compilations in one binary means two independent
  `tokio::runtime::context::CONTEXT` thread-locals: stdlib's runtime enters one,
  the wrapper reads the other, and the program aborts at its first socket with
  "there is no reactor running". The release now fails at build time instead.

  The gate asserts its own subject was live: comparing **zero** tokio-using
  archives fails, because Perry ships several (mysql2, http, ws, fastify), so
  seeing none means the ext build produced nothing or the member naming changed —
  either way the comparison verified nothing and must not read green.
  Sabotage-checked on real archives: incoherent → fail, coherent → pass,
  nothing-compared → fail.
