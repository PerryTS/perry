# nestjs-hello — current compilation walls

This fixture is the maintainer's PR #754 ask for an end-to-end smoke test
that boots a real `@nestjs/common` + `@nestjs/core` app through Perry's
legacy decorator metadata path. It is **wired but not yet passing** —
`fixture.sh` reports SKIP and points back here so the release sweep records
the gap without going red.

The fixture's TypeScript surface (`entry.ts`) is intentionally minimal:
one `@Injectable` service, one `@Controller` with a `@Get` route, one
`@Module`, and `NestFactory.create(AppModule)` + `app.listen(port)`. If
this fixture compiles cleanly and the curl assertions pass, the PR
delivers what was pitched. The walls below are what stand in the way.

Run order to reproduce:

```sh
cd tests/release/packages/nestjs-hello
npm install
../../../../target/release/perry entry.ts -o ./out
```

## Resolved by PR #754 (this PR)

- **`util.types` missing from the API manifest.** Caused `\`util.types\`
  is not implemented in Perry` early in the compile. Fixed by declaring
  `property("util", "types")` in
  `crates/perry-api-manifest/src/entries.rs` and shipping a real `types`
  surface (isPromise / isAsyncFunction / isMap / isSet / isUint8Array /
  isProxy → defaults to `false` for the unknown shapes) in the `node:util`
  stub at `crates/perry-jsruntime/src/modules.rs`.
- **`super.<prop>` not implemented.** Caused `Direct super property
  access not yet supported, use super.method()` for rxjs's
  OperatorSubscriber pattern (`this._next = onNext ? wrapper :
  super._next`). Lowered to `this.<prop>` (and `this[expr]` for computed
  access) — correct when the subclass does not override the property,
  which covers the rxjs / NestJS pattern. See the inline comment in
  `crates/perry-hir/src/lower/expr_misc.rs`. Strict-super semantics with
  parent-vtable lookup is still a TODO for a follow-up.
- **`async_hooks.AsyncResource` missing.** Caused
  `\`async_hooks.AsyncResource\` is not implemented in Perry` from
  `@nestjs/core`'s context-bind path. Fixed by adding
  `AsyncResource` + `AsyncLocalStorage` classes (plus
  `executionAsyncId` / `createHook` helpers) to a real `async_hooks`
  stub in `crates/perry-jsruntime/src/modules.rs`. The bind/runInAsyncScope
  shape is enough for the NestJS code path; no real async-context
  tracking yet.

## Open

### Wall 4 — cross-module method symbol mangling for re-exported classes

After the three resolved walls, the compile and codegen succeed but
linking fails:

```
Undefined symbols for architecture arm64:
  "_perry_method_node_modules_rxjs_src_index_ts__Observable__subscribe", …
  "_perry_method_node_modules_rxjs_src_index_ts__Subject__error", …
  "_perry_method_node_modules_rxjs_src_index_ts__Subscriber__next", …
  "_perry_method_node_modules_rxjs_src_index_ts__Subscription__unsubscribe", …
```

The class definitions live in
`node_modules/rxjs/src/internal/<Subscription|Subscriber|Observable|Subject>.ts`,
so the **defining** module's prefix is
`node_modules_rxjs_src_internal_Subscription_ts` (etc.) — that's the
emitted symbol. But callers that import via `rxjs`'s barrel
`src/index.ts` reference the symbol with the **importing** module's
prefix (`node_modules_rxjs_src_index_ts__Subscription__unsubscribe`),
which never gets emitted by anyone, so the linker fails.

The same mangling site that emits the `extern` declaration at the call
site needs to follow the re-export chain back to the defining module's
prefix. The hono / fastify fixtures don't hit this because they import
flat classes from a single file, not barrel-re-exported class
hierarchies.

This is a Perry codegen surgery (probably in
`crates/perry-codegen/src/codegen.rs` around the
`emit_string_pool` / class-vtable-registration loop or the cross-module
method-call lowering). Out of scope for PR #754; a focused follow-up.

### Probable walls after #4

Even with the mangling fix in, the NestJS bootstrap likely surfaces a
few more — these are educated guesses based on what NestJS does at
container build time, ranked in order of likelihood:

1. **Express adapter** — `@nestjs/platform-express` instantiates an
   express app, attaches middleware, calls `.listen(port)`. The
   compile already showed `_perry_method_node_modules__nestjs_platform_express_adapters_express_adapter_js__ExpressAdapter__initHttpServer` as missing, which is the same wall #4 hitting from a different angle.
2. **`process` / `events` surfaces NestJS Logger reaches into.** The
   `EventEmitter` stub already covers the dominant calls, but `setImmediate`
   / `process.nextTick` may need a closer look.
3. **`@nestjs/common` reflection helpers** — the container does a
   prototype walk via `Object.getOwnPropertyNames` (already working
   thanks to the PR's `js_object_get_own_property_names` extension on
   class refs).

## When this fixture flips to PASS

Once the open walls are gone and `fixture.sh` succeeds end-to-end,
delete this `WALLS.md`. The fixture driver treats `WALLS.md`'s presence
as the marker that turns compile / startup failures into SKIP; removing
it converts those into hard FAILs so a regression past this baseline
becomes impossible.
