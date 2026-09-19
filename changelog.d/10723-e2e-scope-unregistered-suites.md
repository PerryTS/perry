### Fixed

- **CI: `e2e-scoped` was red on every PR since train 218.** `scripts/ci_e2e_scope.py`
  asserts that every `crates/perry-codegen/tests/*.rs` suite is either in
  `SOURCE_SUITE_MAP` or in `SUITE_EXCLUSIONS` (the property #7708 added, so that a
  suite nobody classified fails instead of being silently invisible to per-PR CI).
  Two suites arrived unclassified in `6925754a7d` (#10443/#10446) and the step has
  failed in ~16s on every PR since, regardless of content — including PRs whose diff
  is a single Python script or `.ts` fixture.

  Both are now mapped rather than excluded: `SUITE_EXCLUSIONS` is for a suite held
  out with a named failing test and an issue number, and these have neither. They
  are the cheap in-process shape the map exists for, and both passed when train 218
  ran them as diff-named suites (`error_subclass_field_init` 2 tests,
  `typed_collection_receiver_guard` 3 tests, 0.01s each).

  Verified discriminating rather than merely green: deleting either entry makes
  `--self-test` exit 1 naming the suite, and restoring it returns exit 0.
