### Fixed

- **The panic-profile contract now audits every workspace in the repo, not
  just the main one — and found a fourth instance of the defect it exists to
  prevent** (#8147). Perry's invoke/landingpad exception transport requires
  the runtime to be built `panic = "abort"`: under `unwind`, rustc plants
  RFC-2945 abort-on-unwind guards in every `extern "C"` fn with an interior
  Rust call — `js_throw` included — so a JS throw crossing such a helper
  aborts the process instead of reaching its handler. The crash reads `panic
  in a function that cannot unwind` directly below `_js_throw`, *with a
  handler already armed* (`try_depth` > 0).

  `crates/perry/src/panic_profile_contract.rs` was written to stop exactly
  that, but it read exactly one file — `CARGO_MANIFEST_DIR/../../Cargo.toml`.
  Any *separate* workspace in the tree that builds a runtime archive was
  invisible to it, which is how the defect shipped a third time: the #8034
  fixture at `tests/release/packages/next-app-route/provider/Cargo.toml` is
  its own workspace whose `[profile.release]` sets `codegen-units`/`lto`/
  `strip` and never mentions `panic`, silently taking cargo's `unwind`
  default. A compiled production Next.js App Route aborted during startup
  with `try_depth=7`; diagnosing it cost most of a session.

  The audit now walks every `Cargo.toml` in the repository and mirrors
  cargo's real semantics rather than grepping for a string. Only a workspace
  ROOT's profiles are read (a `[profile.*]` in a non-root member is ignored
  by cargo, so it may neither be trusted nor blamed); members are attributed
  by walking up to the nearest `[workspace]` that does not `exclude` them,
  honouring `package.workspace`; `inherits` chains are resolved, so an
  override sitting under an innocuous `inherits = "release"` is judged by
  what it actually resolves to. A root is in scope only when its member graph
  reaches `perry-runtime` or `perry-stdlib` through a **path** dependency —
  transitively, and including a renamed
  `{ package = "perry-runtime", path = ... }` entry, which is how the
  provider fixtures spell it. `dev-dependencies` are not an edge: they are
  linked only into test/bench harnesses, for which cargo ignores `panic`
  outright.

  An absent `panic` key now fails exactly like a wrong one — cargo's default
  is `unwind`, so silence is the bug. The failure names the manifest, the
  witness chain that put it in scope, and the one-line fix.

  Six negative tests keep this from becoming gate theatre: the missing-key
  shape, the explicit-`unwind` shape, the inherits-then-override shape (that
  was the second instance), a member-level profile that must not excuse a bad
  root, and two false-alarm guards — a workspace that cannot reach the
  runtime (`benchmarks/json_polyglot` is a real one, with a `panic`-less
  `[profile.release]`), and a dev-dependency-only edge. The positive test
  additionally asserts its own subject was live, so a discovery regression
  cannot make it pass vacuously.

  Running the generalized audit found a fourth instance already on `main`:
  `tests/fixtures/issue_8075_provider_gc/stdlib-provider/Cargo.toml` declares
  a correct `[profile.provider]` — what `scripts/gc_provider_dylib_gate.sh`
  actually builds with, so that gate was never wrong — but no
  `[profile.release]` at all, leaving a bare `cargo build --release` there to
  produce an unwind runtime. Fixed with the one line the message asks for.
