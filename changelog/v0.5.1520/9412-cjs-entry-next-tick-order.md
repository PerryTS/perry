### Fixed

- **A `require()` of a builtin no longer demotes `process.nextTick` below
  promise microtasks.**

  ```js
  require("path");                 // delete this line and perry matched node
  const o = [];
  process.nextTick(() => o.push("nextTick"));
  Promise.resolve().then(() => o.push("p1"));
  (async () => { await null; o.push("await"); })();
  setTimeout(() => console.log(JSON.stringify(o)), 20);
  // node:  ["nextTick","p1","await"]
  // perry: ["p1","await","nextTick"]   (5/5 deterministic)
  ```

  The deferral itself is correct, and measurement says so: the same file run
  by node 26 as `.cjs` prints `["nextTick","p1","await"]`, as `.mjs`
  `["p1","await","nextTick"]`. An ES module evaluates inside its module job's
  promise chain, so its first tick drain lands after the promise queue — which
  is exactly what `js_mark_entry_module_esm` (#788) models. It was being
  applied to the wrong module kind.

  Entry codegen decided "is this an ES module?" with
  `!hir.imports.is_empty() || !hir.exports.is_empty() || has_top_level_await`.
  A bare `require(` with no top-level `import` classifies the entry as
  CommonJS, and `cjs_wrap` then rewrites it to ESM — injecting
  `import { createRequire as __perry_cjs_create_require } from 'node:module'`
  and `export default _cjs`. Both halves of that predicate became true for
  every CommonJS program. The `require("path")` call itself contributes no
  import at all; it folds to a native-module reference. Every real bundle
  requires a builtin and every minimal fixture does not, so the ordering was
  right in exactly the programs a test suite contains and wrong in exactly the
  programs users run.

  - `crates/perry-codegen/src/collectors/cjs_scaffolding.rs` —
    `is_cjs_wrapped_module`, keyed on the local name the wrap's synthetic
    `createRequire` import binds. Recognised from the HIR, not from an
    expectation about the template: if the wrap stops emitting it the
    predicate degrades to "not wrapped" (today's behaviour) rather than to a
    wrong answer for hand-written ESM, and a user's own
    `import { createRequire } from 'node:module'` is not mistaken for it
    because the match is on the alias, not the specifier.
  - `crates/perry-codegen/src/codegen/entry.rs` — gate only the
    `js_mark_entry_module_esm` call on that. The `is_esm_entry` below it keeps
    its meaning for GlobalDeclarationInstantiation: a CommonJS module's
    top-level `function` declarations live inside the module wrapper and are
    not global-object properties either, so "not a Script" stays the right
    answer there — and that predicate is mirrored in `perry-hir`'s
    `lower_module_fn`, which runs before the wrap flag is knowable in codegen.
  - `crates/perry/src/commands/compile/cjs_wrap/preamble_canary_tests.rs` —
    a template canary in the same family as #7139/#7152: rename the local in
    `wrap.rs` and every CommonJS entry silently goes back to ES-module tick
    ordering with nothing going red. Plus a negative control, so the fix
    cannot drift the other way and give real ESM entries CommonJS ordering.

  Validation: `test-files/test_gap_9412_require_builtin_tick_order.cts`
  byte-compared against node — ticks first, a tick scheduled from inside a tick
  joining the same drain, a tick scheduled from inside a microtask landing
  after it, and a second event-loop turn where no evaluation checkpoint could
  apply. It has to be a `.cts`: this repo is `"type": "module"`, so a plain
  `.ts` is an ES module for node and perry alike and cannot carry the shape
  (#9418 taught the runner to discover `.cts`).
  `test-files/test_gap_9412_entry_tick_order.ts` pins the ESM side so the fix
  cannot be "stop deferring, always". Demonstrated failing on a compiler built
  from unfixed `origin/main`.
