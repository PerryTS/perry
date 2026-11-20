// Node host runner for the Test262 subset radar (#799, #5346).
//
// `node file.js` executes the file as a CommonJS module: its top-level
// `var`/`function` declarations live in the module wrapper, NOT on the global
// object. That diverges from a real Test262 host (and from Perry), which run
// each assembled case as a *global script*. The difference is invisible for
// most cases but breaks any test whose harness intrinsics must be reachable
// from global scope — notably the Annex B `eval-code/indirect` family, where
// an indirect `(0,eval)(...)` evaluates in the global scope and so cannot see
// a module-scoped `assert` (it throws `ReferenceError: assert is not defined`,
// making Node spuriously "reject" a case Perry runs clean).
//
// Running the assembled program through `vm.runInThisContext` restores true
// script semantics: top-level declarations become globals, top-level `this` is
// `globalThis`, and a syntax error still throws at compile time (so negative
// parse cases keep exiting non-zero). This makes the Node oracle agree with a
// conforming host instead of with the CommonJS wrapper.
//
// The `.cjs` extension is deliberate: the repo's package.json sets
// `"type": "module"`, so a `.js` shim would load as ESM and `require` would be
// undefined. `.cjs` forces CommonJS for the shim itself; the case it runs is
// still evaluated as a global script via vm.runInThisContext.
"use strict";
const vm = require("vm");
const fs = require("fs");

const file = process.argv[2];
const code = fs.readFileSync(file, "utf8");
vm.runInThisContext(code, { filename: file });
