### Fixed

- **`class X extends AsyncLocalStorage` threw at `super()` unless the
  heritage was a bare `import { AsyncLocalStorage } from
  "node:async_hooks"` binding** (#10625), the same defect #10621 fixed for
  `AsyncResource` (#10453). A local alias (`const Alias =
  AsyncLocalStorage`), a namespace member (`ah.AsyncLocalStorage`), a
  default import (`import ahDefault from "node:async_hooks";
  ahDefault.AsyncLocalStorage`), and a CJS destructured
  `require('node:async_hooks')` all threw `Class constructor
  AsyncLocalStorage cannot be invoked without 'new'`. Only the bare import
  shape was recognized statically at HIR-lowering time
  (`canonical_native_parent_name`,
  `crates/perry-hir/src/lower_decl/class_decl.rs`), routing to
  perry-stdlib's `js_async_local_storage_subclass_init` via a
  codegen-declared extern symbol; every other shape fell through
  `js_fetch_or_value_super`
  (`crates/perry-runtime/src/object/global_this/fetch_globals.rs`) to a
  plain CALL of the bound `async_hooks` export, which throws by design
  without `new`. Unlike `AsyncResource`, whose implementation lives
  entirely in perry-runtime, `AsyncLocalStorage`'s subclass-init helper
  lives in perry-stdlib (it needs the stdlib `Handle` registry), and
  perry-runtime cannot depend on perry-stdlib. Fixed by adding
  `JS_NATIVE_ASYNC_LOCAL_STORAGE_SUBCLASS_INIT` /
  `js_set_native_async_local_storage_subclass_init`
  (`crates/perry-runtime/src/value/{tags,handle}.rs`), a registration hook
  perry-stdlib installs at startup, matching the existing
  `JS_NATIVE_ASYNC_HOOKS_CONSTRUCT` / `JS_NATIVE_EVENTS_CONSTRUCT` pattern
  already used for this exact kind of cross-crate reach.
  `bound_native_callable_module_and_method` needed no changes — it already
  generically resolves any bound native export; only the per-consumer
  match arm in `js_fetch_or_value_super` was missing for
  `AsyncLocalStorage`.
