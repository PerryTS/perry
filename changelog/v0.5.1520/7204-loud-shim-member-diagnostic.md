### Fixed

- **An unimplemented member of a bundled npm shim now names itself, and names
  the fix.** When Perry serves a natively-shimmed npm package and the program
  calls a member the shim doesn't implement, the #463 gate defers to a
  throw-on-reach runtime error and records the site for the end-of-compile
  notice. That notice printed only `unimplemented API   <file:line>` — neither
  the module nor the member — so a reader had no way to tell what had gone
  missing, and a caller that swallows the throw (a `try { … } catch {}` around
  config loading, any defensive wrapper) made the notice the *only* evidence
  the program was broken.

  The case that motivated this: `lodash` is in `NATIVE_MODULES`, so Perry
  serves it from `perry-stdlib`'s shim rather than the installed package, and
  that shim implements arrays/strings/math but no object helpers at all — no
  `omit`, `pick`, `get`, `set`, `has`, `merge`, `defaults`. Unlike a
  `perry-ext-*` well-known binding it emits no "serving X from the bundled
  native binding" note either, so nothing in the build said a substitution had
  happened. Socket Firewall's `_.omit(headers, ['host'])` runs on every proxied
  registry request.

  `DeferredEvalSite` now carries `detail` (the dotted symbol) and `remedy` (the
  actionable fix). The notice grows a symbol column, and both the notice and
  the deferred runtime error carry the `perry.compilePackages` escape hatch:

  ```
    - unimplemented API   lodash.omit   app.ts:6   → deferred runtime error (throws only if reached)
    `lodash` is served by Perry's bundled native shim, which implements only part of the
    package's API. To compile the real npm source instead, add it to `perry.compilePackages`
    in package.json: { "perry": { "compilePackages": ["lodash"] } }
  ```

  The remedy is offered only where it can actually be taken: Node builtins
  (`fs.bogus`, `node:fs.bogus`, `process.binding`), Perry-owned surfaces
  (`perry/gc.notReal`), deeper namespaces (`crypto.subtle.digest`), and
  unshimmed modules get the named symbol but no `compilePackages` line, since
  there is no npm source to compile instead. Multiple missing members of the
  same package share one remedy block.

  No change to what compiles. Covered by
  `shimmed_npm_member_carries_the_compile_packages_remedy` and
  `remedy_is_scoped_to_bundled_npm_shims` (both `src/` unit tests, so they run
  in the per-PR `cargo-test` gate).
