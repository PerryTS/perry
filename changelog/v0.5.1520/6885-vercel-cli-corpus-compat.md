### Fixed

- **CJS→ESM wrap: comma-first `require` declarator lists no longer produce a
  parse error.** The alias regex ends at `(?m)$`, so in the pre-ES6 style

  ```js
  var compileSchema = require('./compile')
    , resolve = require('./compile/resolve')
    , Cache = require('./cache');
  ```

  only declarator #0 matched. Blanking that range left `, resolve = …` at
  statement position — `TS1109 (Expression expected)`, the same failure shape
  as the `.EventEmitter;` case in #845, reached via a comma instead of a member
  access. A multi-declarator statement is now skipped entirely: nothing is
  blanked, the body keeps the valid original, and the IIFE-bound `require`
  resolves each specifier at runtime (the same fallback the wrap already takes
  when it refuses an adoption). Hit by ajv 6.x (`lib/ajv.js`,
  `lib/compile/index.js`).

- **`enum` declared inside a function body now compiles.** Valid TypeScript
  that Perry rejected with "declare it at module scope". Enum member access is
  materialized from `ctx.lookup_enum`, but codegen resolves `Expr::EnumMember`
  against `Module::enums`, which body-local declarations had no route to. They
  are now registered at their declaration site and drained into the module via
  `LoweringContext::pending_body_enums` once every function is lowered. Two
  same-named body-local enums with *different* members still raise a
  diagnostic rather than silently reusing the first registration.

- **`D005` (dynamic import) no longer fires on static specifiers.** The check
  was a per-line `!line.contains("import('")` test, so it was sensitive to
  formatting rather than to the argument: a prettier-wrapped
  `await import(\n  './x'\n)` was reported as a variable path while the
  identical single-line call was not — both shapes occur in the same file in
  the Vercel CLI. It also scanned `import(` inside string literals, flagging a
  loader script that is built as a template literal and written to disk for a
  *child Node process*. The specifier is now resolved across newlines on a
  comment/string-masked copy of the source.

- **`T002` (missing types) now honors the canonical `types`/`typings` field.**
  The probe checked three hardcoded paths (`index.d.ts`, `dist/index.d.ts`,
  `types/`) and ignored package.json entirely, so `date-fns`
  (`./typings.d.ts`), `tldts` (`dist/types/index.d.ts`) and `@inquirer/*`
  (`./dist/cjs/types/index.d.ts`) were all reported as untyped. Resolution now
  checks `types`/`typings` (including the extensionless form), a `types`
  condition anywhere in an `exports` map, then the legacy layouts, then a
  bounded `.d.ts` scan of the package root and `dist/`.

- **A `.node` addon reached as a module now reports what it is.** Both module
  read paths call `fs::read_to_string`, so a compiled N-API addon surfaced as
  `stream did not contain valid UTF-8`, naming neither the constraint nor the
  package. `refuse_compile_package_native_addon` only covers addons whose
  package root resolved to a `compilePackages` entry, which misses the napi-rs
  sidecar layout (`@napi-rs/keyring` → `@napi-rs/keyring-darwin-arm64`,
  containing nothing but the `.node` file and a package.json). The read itself
  is now guarded, so the diagnostic is the same either way.

### Internal

- **Test env guard is now process-wide.** `optimized_libs::tests` swaps `PATH`
  for a fake, `sh`-less directory behind a module-private mutex, but
  `install::lifecycle`'s `run_lifecycle_executes_script` reads `PATH` (via
  `augment_path`) to resolve `sh` and did not share that lock — so it could
  spawn against the fake PATH and fail with `No such file or directory`. The
  guard moved to `crate::test_env_lock` and both sides take it. Latent before
  this branch; the added tests shifted scheduling enough to surface it.

All five were found by compiling the Vercel CLI (`real-apps/vercel`,
1137 TS files / 168k LOC) end to end. On that corpus `perry check --check-deps`
goes from 4 errors / 20 `T002` warnings to 0 errors / 5 warnings, and the five
remaining are genuine — four are packages the Vercel repo itself hand-writes
ambient declarations for.
