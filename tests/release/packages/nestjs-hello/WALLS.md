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

## Open

### Wall 2 - `.prototype` of a capturing class expression is undefined (tslib `__decorate`)

The binary now links but the server dies during module init with
`TypeError: Cannot convert undefined or null to object`. Root cause, bisected
to `@nestjs/common/services/logger.service.js`: tsc's class-decorator output

```js
let Logger = Logger_1 = class Logger { ... };
tslib_1.__decorate([Logger.WrapBuffer, ...], Logger.prototype, "error", null);
```

calls `Object.getOwnPropertyDescriptor(Logger.prototype, "error")`, and
`Logger.prototype` reads as `undefined` (while `typeof Logger` is
`"function"`). Minimal repro — no CJS wrap needed; the trigger is a class
EXPRESSION inside a closure whose **getter captures an outer variable**,
which routes the class through the `ClassExprFresh` lowering
(`captured_args: [LocalGet(..)]` in HIR), and `.prototype` on a
`ClassExprFresh` value is not wired:

```ts
const r = (function() {
  var L_1: any;
  let L: any = L_1 = class L {
    get gi() { return L_1.staticRef; }  // getter capture → ClassExprFresh
    e(m: any) { return m; }
  };
  return [typeof L, typeof L.prototype];
})();
// Perry: ["function", "undefined"] — node: ["function", "object"]
```

Without the capturing getter the same shape keeps a real `.prototype`. The
next focused fix should give `ClassExprFresh` values a live `.prototype`
object (tslib `__decorate` then mutates it via `defineProperty`, so instances
must observe the decorated methods).

## When this fixture flips to PASS

Once the open runtime wall is gone and `fixture.sh` succeeds end-to-end, delete
this `WALLS.md`. The fixture driver treats `WALLS.md` as the marker that turns
compile/startup failures into SKIP; removing it converts those into hard FAILs
so regressions past this baseline are visible.
