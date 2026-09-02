### Fixed

- **ES module top-level code is now lowered as strict code, which it always is.**

  ```js
  // any .mts / .ts under "type": "module" -- an ES module, strict with no directive
  console.log(this === undefined);                          // node: true   Perry: false (an object)

  const a = [1, 2]; Object.freeze(a);
  for (a[0] of [7]) {}                                      // node: TypeError   Perry: silent
  ```

  ES2024 §11.2.2: a Module *is* strict mode code, with no `"use strict"`
  prologue needed. Lowering already knows this —
  `LoweringContext::module_strict` is computed from the file's module goal and
  feeds `current_strict`, so every HIR node that carries its own `strict` flag
  (`PutValueSet`, `PropertyUpdate`, `IndexUpdate`) was already right, which is
  why a plain `frozenObject.x = 9` at module top level threw correctly and this
  stayed hidden.

  Codegen could not see it. Module init is lowered as a synthetic function, and
  `FnCtx::is_strict_fn` was hardcoded `false` for it at both
  `codegen/entry.rs` sites (entry module and per-module `__init`), and again for
  every outlined entry chunk in `codegen/entry_outline.rs` — whose comment said
  so and asked the next person to match it. So every lane keyed on the
  *context's* strictness rather than on a node-carried flag ran module top-level
  code sloppy:

  - `Expr::IndexSet` (`expr/dispatch.rs` passes `ctx.is_strict_fn` straight into
    `index_set::lower`) — the node a `for` head or a destructuring target with a
    computed member lowers to. A rejected `for (frozenArray[0] of …)` was a
    silent no-op.
  - `Expr::This` (`expr/this_super_call.rs`) — module top-level `this` took
    `js_implicit_this_get_sloppy` and read the global object instead of
    `undefined`.
  - `delete obj.prop` and `delete proxy.key`
    (`expr/instance_misc1.rs`, `expr/proxy_reflect.rs`), which route their
    `[[Delete]]` boolean through `js_delete_result(strict)`.

  The module's strictness now rides on the HIR module as `Module::init_is_strict`,
  set next to `ctx.module_strict` at the top of lowering, and read by both
  `entry.rs` sites and threaded into `entry_outline.rs`'s chunk functions — a
  chunk is module top-level code that merely moved into a function, so relaxing
  its mode would reopen the same hole. It also joins the module's stable hash:
  it changes emitted code, so a cached object from a sloppy compile must not be
  reused for a strict module.

  `test-files/test_gap_9423_module_init_strictness.ts` is a plain `.ts`, which
  under this repo's `"type": "module"` package is strict-mode ESM in **both**
  runtimes, so every write in it sits at module top level where the spec says
  strict. It covers module `this`, an undeclared-name assignment, and rejected
  writes through each lowering that reaches a store at module top level — static
  name, computed key, `for`-of head (named and computed), destructuring target
  (named and computed), array element, and `arr.length` — plus the over-throw
  controls that must still succeed (`sealed`/`preventExtensions` writes to an
  existing property, and the same `for`-of head and destructure on an unfrozen
  receiver). Byte-compared against node 26.5.1. The sloppy control for the same
  shapes is #9422's `.cts` fixture, which is a CommonJS script in both runtimes.
