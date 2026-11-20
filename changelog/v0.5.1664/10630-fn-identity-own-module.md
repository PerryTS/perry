### Fixed

- **A function referenced inside its own module is a different object from
  the same function imported elsewhere (#10554).** `function f(){}; export
  function isSame(x){ return x === f; }; export { f };` gave `isSame(f)`
  `false` for an importer's own `f` — identity checks, registries/caches
  keyed by function, `removeEventListener`/`off(fn)`, and memoization all
  silently took the wrong branch.

  Root cause: the cross-module *function* inliner in
  `perry-transform`'s `inline/cross_module.rs` harvests an exported
  function's whole value-dependency graph — every function it transitively
  references, including by value (`x === f`), not just as a call target —
  and clones the entire graph into every importing module under fresh
  `__perry_xmod_inline_<id>_<name>` symbols. When the referenced function
  (`f`) is *also* independently exported, it got cloned alongside the
  candidate instead of resolved through its own canonical wrapper. Every
  function value materializes into a heap closure keyed by its wrapper
  *symbol* (`js_closure_alloc_singleton`), so the clone's `f` and the
  canonical `f` every importer resolves through produced two distinct
  closures — an in-module identity check comparing them disagreed with
  every importer's own view.

  Fix: `gather_cross_module_functions` now refuses a candidate whose
  dependency graph would need to bundle a *separately exported* sibling
  function referenced by value — it falls back to an ordinary cross-module
  call instead, which resolves through the shared canonical wrapper.
  Self-recursion is unaffected. The directly-affected shape actually gets
  **faster**, not slower: the unsound inline was paying for an extra
  closure materialization on every call.

  Validation: new `test_gap_10554_fn_identity_own_module` (function
  declaration, function expression, arrow-in-const, named export, a barrel
  re-export, a default export referencing an exported sibling, `Set`
  membership, both identity directions) fails on the baseline and matches
  Node on the fix; the existing `test_gap_10434`/export/import/cross-module/
  inline/module gap-test families (16 tests) are unaffected.
