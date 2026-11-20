// #10735 helper: requires `deep.cjs` (two levels deep from the entry) and
// re-requires `dep.cjs` (already loaded by the entry directly) to exercise
// Node's module cache — the SAME dep.cjs instance and the SAME require.main
// object must come back, not a fresh copy.
//
// Requires come BEFORE any side-effecting statement (deliberately): Perry
// transpiles a static `require('./relative')` into a hoisted ESM `import`,
// which runs the imported module's top-level code before THIS module's own
// trailing statements — the reverse of Node's inline evaluation order when a
// require() is preceded by other code in the same file. That reordering is
// an existing, unrelated property of cjs_wrap's require-hoisting (not
// anything #10735 touches), so this fixture avoids it structurally, the way
// real bundled output typically does, to keep the two engines' output
// byte-for-byte comparable and isolate the require.main assertions this
// test exists to check.
const deep = require('./gap_10735_require_main_deep.cjs');
const depAgain = require('./gap_10735_require_main_dep.cjs');
console.log('dep2: require.main === module:', require.main === module);
exports.deepTag = deep.tag;
exports.depExportsRef = depAgain;
exports.depMainRefFromHere = depAgain.mainRef;
