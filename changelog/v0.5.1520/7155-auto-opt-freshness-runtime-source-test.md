### Fixed

- **Locked in auto-optimize freshness on runtime/stdlib source edits (#7155).**
  The auto-optimize build stamp is keyed on a content fingerprint of every source
  tree that lands in the runtime/stdlib archives (#5930), so editing
  `crates/perry-runtime` / `crates/perry-stdlib` invalidates a cached
  `target/perry-auto-<hash>` archive and forces a rebuild even when mtimes lie
  (`git checkout`, `cp -p`, CI cache restore) — no manual
  `rm libperry_runtime.a`. That guarantee had direct test coverage only for the
  routed `perry-ext-*` crates; this adds regression tests for the runtime/stdlib
  crates themselves — the ones behind the GC-iteration "compile silently reused a
  stale runtime" trap — asserting a content edit rotates both the source
  fingerprint and the build stamp while an identical-bytes rewrite stays a fast
  no-op.
