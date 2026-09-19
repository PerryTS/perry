// #10735: `require.main` must be the process ENTRY module only, equal to
// `module` there and unequal (or undefined, for an ESM entry — see the
// companion `test_gap_10735_require_main_esm_entry.ts`) everywhere else.
//
// Perry's CJS preamble used to emit `require.main = module;` unconditionally
// in EVERY compiled CommonJS module, so the idiom
// `if (require.main === module) { ...CLI... }` — used by countless packages
// (dotenv among them) to gate CLI behaviour — took its CLI branch whenever
// such a package was merely imported as a library.
//
// `.cts`, deliberately: this repo's package is `"type": "module"`, so a
// plain `.ts` runs as an ES module under Node; `.cts` forces CommonJS goal
// agreement between Node and Perry (see test_gap_9412's header for the full
// rationale). Keep this file free of top-level `import`/`export`.
//
// All `require()` calls come before any `console.log`, matching every
// helper file — see the ordering note in gap_10735_require_main_dep2.cjs.
const depFromEntry = require('./gap_10735_require_main_dep.cjs');
const dep2 = require('./gap_10735_require_main_dep2.cjs');
require('./gap_10735_require_main_cli_guard.cjs');

console.log('entry: require.main === module:', require.main === module);

// Two-levels-deep dependency (entry -> dep2 -> deep) observed the same
// entry module as require.main.
console.log('deep tag:', dep2.deepTag);

// dep.cjs was required once directly by the entry and once transitively by
// dep2 — Node's module cache means both call sites get the SAME exports
// object and the SAME require.main reference back, not a fresh reload.
console.log('dep exports identity cached across require sites:', depFromEntry === dep2.depExportsRef);
console.log('dep require.main identity stable across require sites:', depFromEntry.mainRef === dep2.depMainRefFromHere);
console.log('dep require.main === entry module:', depFromEntry.mainRef === module);

console.log('entry done');
