### Fixed

- **CommonJS `require()` outside a function is no longer hoisted past the
  control flow that guards it.** Perry's CJS→ESM wrap turned every
  literal `require('S')` in a wrapped file into a static `import` at the
  top of the module and eager-initialized the target — even when the call
  sat inside `if (false)`, a false env check, `&&`/`??`, a ternary arm, a
  `switch` case, or a loop that never iterates. A module reached only
  through such a branch loaded (and could throw) at program start,
  regardless of whether the branch ever ran; a module reached through a
  taken branch loaded before the statements preceding it. This was the
  sole remaining blocker compiling `pg` from source: `lib/index.js` guards
  its optional native binding behind `if (forceNative) { require('./native') }`,
  and `./native` requires the often-uninstalled `pg-native` — every
  program using `pg` crashed at init with `Cannot find module 'pg-native'`
  even though `forceNative` was false.
  `cjs_wrap::extract_requires::function_local_specs` now classifies a
  `require()` call site as deferred (Node's actual "loads only when
  control flow reaches it" semantics) whenever it sits inside a
  control-flow block (`if`/`for`/`while`/`switch`/`catch`/`try`/`else`/
  `do`/`finally`) or a braceless/operator equivalent (`cond &&
  require(...)`, `cond ? require(...) : x`, `for (...) require(...)` with
  no block) — not only inside a function body as before. An ordinary
  object literal or class body still does not count, so the common
  `module.exports = { fs: require('fs'), path: require('path') }` barrel
  shape stays eager. A `process.platform === '<literal>'` guard (the
  node-pty Windows/Unix terminal split) is exempted from the broader
  reclassification and keeps its existing eager treatment — the platform
  is a compile-time-known build target, not a runtime unknown, and
  `wrap_commonjs_for_target`'s dead-branch pruning already resolves it.

Verified end-to-end: `pg` now compiles, links, and runs from real source
under `perry.compilePackages`, reaching a real TCP connect attempt with no
`pg-native` crash.

Fixes #10437.
