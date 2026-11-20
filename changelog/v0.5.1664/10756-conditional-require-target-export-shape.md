### Fixed

**A conditional CommonJS `require()` of a side-effect-only module never ran it (#10754).**

A top-level `require()` in a conditional position loaded nothing when its target
carried no CommonJS export marker — the branch was taken, the shim was reached,
and the module body never ran. Silent: no crash, no diagnostic, just a
dependency whose side effects never happened.

The issue reports the failure as syntactic (`if` works, `&&` / `?:` / `try` /
`switch` / loop-body do not). That is a confound in the reproducer: its `if`
case required a module with `module.exports = 1` while the other five required
side-effect-only modules. Crossing the two axes against Node 26.5.1 on a
release build of `main` @ 91a566c8af shows the discriminator is the TARGET's
export shape, not the call-site shape — all six shapes fail with a
side-effect-only target and all six pass with a value-returning one.

Root cause. #10674 defers a conditional require correctly: the target stays
`ModuleInitKind::Deferred` and the CJS shim returns the `_lazyreq_N` import
binding, with codegen firing `<S>__init()` at the binding read
(`perry-codegen/src/expr/dyn_extern_i18n.rs`). That init call is gated on the
binding being a known imported FUNCTION (`ctx.import_function_prefixes`). A
target with no default export has no such entry, so nothing fires. The `if`
cases that "worked" worked because their targets had a default export to read.

The fix (PR #10285's implementation, rebased, plus a registry-miss fallback):

- `cjs_wrap/deferred_requires.rs` — an AST visitor replaces the text-scanning
  deferral classifier. The scanner missed `if (cond) x = require('S')` (still
  eagerly hoisted on `main`, a residual #10437 shape), concise arrows, the
  ternary ALTERNATE arm, both halves of a `do`/`while`, and `&&=`/`||=`/`??=`.
  `extract_requires::function_local_specs` remains the parse-failure fallback.
- `cjs_wrap/wrap.rs` — a deferred specifier resolves through the path registry
  (`__perry_require_path_module`) rather than its import binding, so
  initialization no longer depends on the target's export shape. The registry
  record is memoized per call site once `loaded === true`: re-entering the
  registry on every call measured 3.4x on a hot require.
- `cjs_wrap/wrap.rs`, new on top of #10285 — the registry only holds EXPORTS for
  a target that publishes them, which is every CJS-wrapped module and no other.
  A file with no CommonJS marker at all is not CJS-wrapped, so it registers an
  initializer and never any exports: the registry ran its body and returned
  `undefined` where Node returns `{}`, so
  `const v = cond ? require('./side-effect-only.cjs') : 0` came back
  `undefined`. The arm now falls back to the import binding on a genuine
  registry miss, discriminated by `__perry_has_path_module` (a real module may
  export `undefined`) — the same guard the generic runtime-`require(path)` arm
  in the same wrapper already uses.
- `perry-codegen/src/expr/dyn_extern_i18n.rs` — fire the deferred `__init()`
  before the imported-class and namespace fast paths, which can themselves
  depend on module initialization.

Validation. `test-files/test_gap_10754_cjs_conditional_require_shapes.cts`
crosses all six shapes with three cells each — taken/side-effect-only,
taken/value-returning, and not-taken — because a fix that loads the module
unconditionally is #10437 again, not a fix. It fails on unfixed `main` —
12 differing lines against the Node oracle: six side-effect-only targets never
load at all, and three value-returning targets load before the program's first
statement instead of at their call site (`if (cond) x = require('S')`,
`cond && (x = require('S'))` and the same in a `for` body, the shapes the text
scanner could not see) — and matches Node byte-for-byte with the fix. The
harness agrees: `parity_fail` on unfixed `main`, `PASS` with the change. The existing #10437
fixture covers the not-taken direction with value-returning targets only.
