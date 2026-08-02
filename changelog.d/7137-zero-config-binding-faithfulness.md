### Added

- Well-known native bindings may now declare a conservative compatibility
  marker: `compat = "full"` is reserved for an audited complete drop-in,
  while `partial` (and an absent or unknown value) means the wrapper is not
  known to cover the package's complete public API. Existing third-party
  wrappers remain `partial` until an exhaustive audit proves otherwise, and
  aliases inherit their target's marker.

- When Perry auto-prefers a partial bundled binding over an installed
  `node_modules` copy, text-mode builds now print one note per package with
  the `perry.compilePackages` escape hatch. Setting
  `PERRY_REQUIRE_FAITHFUL_BINDINGS=1` turns that case into a hard error;
  false/zero values leave strict mode disabled. Binding policy variables now
  participate in build-cache fingerprints.

- `perry.compilePackages: "auto"` (also `"all"` or `true`) explicitly asks
  Perry to compile every reachable dependency that is not natively shimmed.
  `perry.allow.compilePackages: true` is the corresponding universal allow
  spelling.

### Changed

- Auto-compile is now the default when `perry.compilePackages` is omitted.
  Perry expands the installed dependency graph and compiles reachable package
  source while preserving bundled native bindings. An explicit list, `false`,
  or `[]` opts out. An omitted allow policy is granted automatically for this
  default, but an explicit `perry.allow.compilePackages` policy and
  `PERRY_ALLOW_PERRY_FEATURES=0` remain authoritative fail-closed constraints.

- Compatibility diagnostics resolve registered package subpaths (for example
  `mysql2/promise`) before falling back to the root package binding.
