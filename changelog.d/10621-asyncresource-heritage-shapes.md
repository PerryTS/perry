### Fixed

- **`class X extends AsyncResource` threw at `super()` unless the heritage
  was a bare `import { AsyncResource } from "node:async_hooks"` binding**
  (#10453). A local alias (`const Alias = AsyncResource`), a namespace
  member (`ah.AsyncResource`), and a CJS destructured
  `require('node:async_hooks')` — the exact shape `undici`'s API handlers
  use everywhere (`lib/api/api-request.js` etc.) — all threw `Class
  constructor AsyncResource cannot be invoked without 'new'`. Only the bare
  import shape was recognized statically at HIR-lowering time
  (`canonical_native_parent_name`, `crates/perry-hir/src/lower_decl/class_decl.rs`),
  routing to the dedicated `js_async_resource_subclass_init` codegen; every
  other shape fell through `js_fetch_or_value_super`
  (`crates/perry-runtime/src/object/global_this/fetch_globals.rs`) to a
  plain CALL of the bound `async_hooks` export, which throws by design
  without `new`. `js_fetch_or_value_super` already resolves ANY heritage
  value to its bound native module/method via
  `bound_native_callable_module_and_method` for the WASI case, regardless
  of how the value was reached — this fix adds the same recognition for
  `async_hooks`'s `AsyncResource`, so every aliasing shape now runs the
  same native-backing init the canonical import already used.
  `AsyncLocalStorage` likely has the same gap but isn't fixed here — its
  subclass-init helper lives in `perry-stdlib`, which `perry-runtime`
  cannot depend on.
