### Performance

- **Constructing a `RegExp` no longer allocates its flags string.** A JS regex
  literal evaluates to a fresh `RegExp` object every time it is reached, and
  `js_regexp_new` materialized the canonical flags twice per construction: once
  as a Rust `String` from `validate_and_canonicalize_flags`, and once as a fresh
  GC `StringHeader` for `flags_ptr`. On the claude-code TUI that is **161,897
  constructions per 400-character reply** (`PERRY_REGEX_DIAG`) — ~5.2 MB of
  identical one- and two-byte GC strings per reply, ~44 MB on a 3300-character
  one, and ~1.4 million allocations.

  Neither copy is needed. There are eight legal flags, each may appear once, so
  the canonical form is at most eight ASCII bytes and now lives inline in a
  `CanonicalFlags` value instead of on the heap. And JS strings are immutable
  with no identity semantics, so when the caller's flags text already IS the
  canonical text — which it is for a literal, whose flags the author wrote in
  spec order — the header shares the caller's string rather than duplicating
  it. Only a non-canonical spelling (`/x/ig` → `"gi"`) or a computed
  `new RegExp(p, f)` still materializes one; the new `flags_alloc` counter in
  `PERRY_REGEX_DIAG` reports how often that happens.

  This is a **below-the-line** allocation fix by the campaign's own ~10 % rule:
  at ~2-3 % of arena traffic per turn it cannot change the collection schedule,
  and the cc rig is expected to read flat. It is worth doing because the
  allocation is pure waste, not because it moves a benchmark.
