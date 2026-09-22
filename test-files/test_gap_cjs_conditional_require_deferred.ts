// #10437: CommonJS `require()` outside a function is hoisted and run
// unconditionally at module init, whatever the surrounding control flow.
// Perry loaded every `require('<literal>')` in a CJS file before the
// file's first statement ran, so a branch that never runs (`if (false)`, a
// false env check, `&&`, `?:`, `switch`, a loop that never iterates) still
// loaded its module, and a module reached via a taken branch loaded before
// the statements preceding it.
//
// The crash form is `pg` 8.22.0: `lib/index.js` guards its optional native
// binding behind `if (forceNative) { require('./native') }`, and `./native`
// requires the optional, often-uninstalled `pg-native`. Every program using
// `pg` crashed at init with `Cannot find module 'pg-native'` even though
// `forceNative` was false. `./_helpers/gap10437_cjs_lazy_require.cjs`
// reproduces the full variant matrix (A-H from the issue, plus require
// caching and a swallowed try/catch around a genuinely missing module) in
// one file so the expected interleaving with its own `console.log` calls is
// unambiguous.
import mod from "./_helpers/gap10437_cjs_lazy_require.cjs";

console.log("typeof never=" + typeof mod.never);
