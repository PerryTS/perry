### Added

- **Well-known bindings now carry a `compat` faithfulness marker (#466
  follow-up).** Each `[bindings.X]` in `well_known_bindings.toml` may declare
  `compat = "full"` (an audited complete drop-in for the npm package's public
  API) or `compat = "partial"` (ports only a subset, or not yet audited).
  **Absent ⇒ `partial`**, the conservative default — a binding is never treated
  as faithful by accident. Audited this release: `dotenv`, `nanoid`, `slugify`,
  `uuid` opt in to `full`; the documented subsets `undici` (dispatcher-only),
  `node-forge` (PKI-only), and `lru-cache` (numeric store) are marked `partial`
  explicitly; every other binding inherits the `partial` default.

- **Transparency note when perry auto-prefers a partial binding over your
  installed copy.** When a bare `import 'X'` is served by a bundled
  `perry-ext-X` wrapper that is `partial` **and** you have a `node_modules/X`
  copy on disk, perry now prints a one-line note per package pointing at the
  `perry.compilePackages` escape hatch. The build still succeeds — this is the
  zero-config path — the note just makes the (previously silent) choice visible.

- **`PERRY_REQUIRE_FAITHFUL_BINDINGS=1`** — opt-in strict mode that turns that
  note into a hard error: perry refuses to auto-prefer a `partial` binding over
  an installed `node_modules` copy, telling you to either add the package to
  `perry.compilePackages` (to AOT-compile the real JavaScript) or accept the
  partial binding by unsetting the variable. `full` bindings are unaffected.
  Default off ⇒ existing behavior is byte-identical.

### Changed

- **Auto-compile is now the default — no `perry.compilePackages` allowlist
  required.** A project that does not pin a `perry.compilePackages` value gets
  its whole reachable `node_modules` dependency graph compiled automatically.
  Perry is a compiler, not a supply-chain gate: faithfully compiling code that
  is already installed is not perry's decision to block, and "this package is
  not on a list" is no longer a hard error. A dependency that genuinely cannot
  be compiled surfaces as an ordinary **compile error** with a diagnostic, not
  a policy refusal. Native-shimmed packages still resolve to their bundled
  bindings (the wildcard expansion skips them), so bindings keep winning.

  Supply-chain hygiene (pinning, lockfiles, review, dedicated tooling) is the
  user's responsibility. Opt out to the old "listed packages only" behavior
  with an explicit `perry.compilePackages` array, or `compilePackages: false` /
  `[]` to compile nothing and re-arm the V8-free gate.

### Added

- **`perry.compilePackages: "auto"` (and `perry.allow.compilePackages: true`).**
  Explicit spelling of the new default — sugar for the universal `["*"]`
  wildcard (#3527), useful to document intent or to re-enable auto-compile
  inside a config that would otherwise constrain it. `"auto"` / `"all"` /
  `true` are accepted; an explicit array is unchanged. A universal-trust value
  auto-satisfies `perry.allow.compilePackages`, and native-shimmed packages
  stay on their bindings exactly as with a literal `"*"`.
