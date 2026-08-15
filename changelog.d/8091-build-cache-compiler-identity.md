### Fixed

- **The build cache no longer hands back a binary built by a different
  compiler.** The `perry_build_id` check re-fingerprinted the path *recorded in
  the manifest* and compared it to the recorded value — which asks "is the
  binary I recorded still unchanged?", and is trivially true whenever a
  different `perry` runs the second build. The recorded binary is sitting
  exactly where it was, so the check passed, the cache reported
  `"hit": true, "reason": "manifest-match"`, and the build was skipped
  entirely: no relink, output file untouched, nothing printed, exit 0.

  It now compares against the compiler running now, via the same
  `current_perry_fingerprint()` used when the manifest is written.

  `perry_version` did not cover this. During pass development the version
  rarely moves between rebuilds, which is the reason `perry_build_id` exists
  at all (#544) — this restores the guarantee that issue was closed on.

  How to recognise it: a `.ts` probe compiled by a pre-fix compiler kept its
  stale executable when recompiled by a fixed one, so a genuine fix read as not
  working and the phantom was bisected onto an unrelated commit. Touching the
  source does not help, because sources are verified by sha256 rather than
  mtime; only a different output path or a cleared cache does.

  Changed: `crates/perry/src/commands/compile/build_cache.rs` — the
  `perry-build-id` arm of `BuildCacheProbe::probe`.

  Validation: `cargo test -p perry --bins build_cache` (4 passed). Two tests
  cover it — one pins the two comparison expressions in isolation, and
  `a_foreign_build_id_misses_at_the_probe` writes a manifest claiming a
  different compiler's build id and drives the real decision path, asserting
  the miss is `perry-build-id` specifically rather than an incidental later
  check. Sabotage-verified: restoring the old self-comparison at the call site
  turns the probe test red while the expression test stays green, so the
  guarding test is the one that actually holds the fix in place.
