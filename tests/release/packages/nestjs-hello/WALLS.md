# nestjs-hello - current compilation wall

This fixture is an end-to-end NestJS smoke test that boots a real
`@nestjs/common` + `@nestjs/core` + `@nestjs/platform-express` app through
Perry's legacy decorator metadata path. It remains wired but skipped:
`fixture.sh` reports SKIP while this file is present so the release sweep
records the current gap without going red.

Run order to reproduce:

```sh
cd tests/release/packages/nestjs-hello
npm install
../../../../target/release/perry entry.ts -o ./out
```

## Resolved by the current package-compat cut

- `npm install` now resolves normally with NestJS 11-compatible dependency
  versions.
- The fixture trust metadata covers the current NestJS, Express, readable-stream,
  RxJS, and small transitive helper package graph that Perry must compile
  ahead-of-time.
- `depd` and `function-bind` no longer stop compilation with dynamic
  `Function(...)` wrappers; their package-specific CommonJS rewrites compile
  arity-erased closures instead.
- `safer-buffer` no longer probes private `process.binding('buffer')` for
  `kStringMaxLength`.
- `safe-buffer` no longer routes its fallback through deprecated
  `buffer.SlowBuffer`.
- Legacy CommonJS inheritance patterns that call `Stream.call(this)` or
  `EventEmitter.call(this)` now lower to the same receiver initialization shape
  this fixture needs from Express/readable-stream.
- **Wall 1 (resolved, #4872)** — undefined default-wrapper symbols for
  re-exported barrel modules (`__perry_wrap_perry_fn_..._rxjs_src_index_ts__default`,
  the nestjs `*.interface.js` family, `uid_dist_index_mjs__default`,
  `perry_fn_..._common_index_js__Controller`, …). Four coordinated fixes:
  a default import of a compiled module with no `default` export now binds
  the module namespace (Node `require(esm)` semantics) instead of a phantom
  callable; `__exportStar(require("./x"), exports)` in CJS-wrapped sources
  now also emits a real `export * from './x'` so multi-level tsc barrels
  resolve named imports to the defining module; `export *` propagation no
  longer leaks `default` across hops; and tsc-emitted type-only modules
  whose only statement is `Object.defineProperty(exports, "__esModule", …)`
  are now detected as CJS (previously they compiled as zero-export ESM and
  threw `ReferenceError: exports is not defined` at init). A TS constructor
  overload-signature miscount (rxjs `Notification` rejected with "may only
  have one constructor") was fixed alongside. The fixture now **links**:
  `Wrote executable` (~41 MB).
- **Wall 2 (resolved, #4949)** — `.prototype` on a capturing class expression
  (`ClassExprFresh`) now resolves to a live declared-class prototype object.
  This unblocks the original `@nestjs/common/services/logger.service.js`
  decorator shape where tsc/tslib calls
  `Object.getOwnPropertyDescriptor(Logger.prototype, "error")`; the previous
  failure was `TypeError: Cannot convert undefined or null to object` because
  `Logger.prototype` read as `undefined`.

## Open

### Wall 3 - decorator descriptor read now reaches an undefined descriptor value

The binary still links and now gets past the missing-`.prototype` wall, but the
server dies during module init with:

```text
TypeError: Cannot read properties of undefined (reading 'value')
    at <anonymous>
```

Reproduce with:

```sh
cd tests/release/packages/nestjs-hello
npm install
PERRY_BIN=../../../../target/release/perry ./fixture.sh
```

The next focused investigation should locate which decorated member now
produces an undefined descriptor/value after `Logger.prototype` itself is an
object. This is a later NestJS bootstrap wall, not the original #4949
`ClassExprFresh.prototype === undefined` failure.

## When this fixture flips to PASS

Once the open runtime wall is gone and `fixture.sh` succeeds end-to-end, delete
this `WALLS.md`. The fixture driver treats `WALLS.md` as the marker that turns
compile/startup failures into SKIP; removing it converts those into hard FAILs
so regressions past this baseline are visible.
