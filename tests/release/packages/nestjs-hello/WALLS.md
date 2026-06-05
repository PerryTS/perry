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

## Open

### Wall 1 - default-wrapper symbols for re-exported modules

After the resolved walls above, the fixture reaches native linking and then
fails with undefined default-wrapper symbols. Current representative symbols:

```text
undefined reference to `__perry_wrap_perry_fn_node_modules_rxjs_src_index_ts__default'
undefined reference to `perry_fn_node_modules_rxjs_src_index_ts__default'
undefined reference to `perry_fn_node_modules_rxjs_src_operators_index_ts__default'
undefined reference to `perry_fn_node_modules_uid_dist_index_mjs__default'
undefined reference to `__perry_wrap_perry_fn_node_modules__nestjs_core_router_interfaces_routes_interface_js__default'
undefined reference to `__perry_wrap_perry_fn_node_modules__nestjs_platform_express_interfaces_nest_express_application_interface_js__default'
```

These modules are import barrels or type/interface re-export surfaces rather
than concrete default function definitions. Call sites still emit default-call
and default-wrapper references for them, but no object file defines the matching
symbols. The next focused fix should make default-import/default-call lowering
follow re-exported module surfaces back to a concrete binding, or avoid emitting
callable default references for type-only/interface barrels.

## When this fixture flips to PASS

Once the open linker wall is gone and `fixture.sh` succeeds end-to-end, delete
this `WALLS.md`. The fixture driver treats `WALLS.md` as the marker that turns
compile/startup failures into SKIP; removing it converts those into hard FAILs
so regressions past this baseline are visible.
